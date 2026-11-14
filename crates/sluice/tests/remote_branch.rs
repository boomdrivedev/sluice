//! A branch whose shared copy lives on a remote, end to end through
//! `sluice submit`, `sluice step` and `sluice run` (C.4 pushed refs, C.7
//! `remote`). A bare repository on this host is the remote: git leases,
//! refuses and fetches against it as against a forge. A `receivepack`
//! wrapper on the remote times what happens around one push: another
//! clone's push, a stop after it, an unreachable remote. Needs git, so it
//! is in the check-tools tier.

#![cfg(feature = "tools")]

#[path = "support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use support::{Repo, Scratch};

/// Each run counts itself in `runs`, then waits for `go` when the test
/// holds it.
fn check(seen: &Path) -> String {
    let seen = seen.display();
    format!(
        "echo run >> {seen}/runs\n\
         while [ -e {seen}/hold ] && [ ! -e {seen}/go ]; do sleep 0.1; done\n"
    )
}

fn policy(remote: &str) -> String {
    format!(
        "[tests]\nrunner = \"command\"\ncommand = \"sh check.sh\"\n\n\
         [branches.main]\ntimeout = \"1m\"\nremote = \"{remote}\"\n\n\
         [branches.\"bolt/*\"]\ntimeout = \"1m\"\nremote = \"{remote}\"\n"
    )
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

struct Setup {
    repo: Repo,
    origin: Scratch,
    seen: Scratch,
    sessions: Scratch,
    config: Scratch,
    /// The host's state, the setup's own, so another agent's runs on this
    /// host never take the bound its rounds start under (71).
    state: Scratch,
    /// The git every command it runs starts, counting its starts (129).
    git: support::CountingGit,
}

impl Setup {
    /// A repository whose `main` lives on `origin`, a bare repository,
    /// under a policy naming `remote`.
    fn new(name: &str, remote: &str) -> Setup {
        Setup::with(name, &policy(remote))
    }

    /// As [`Setup::new`], with every branch in local refs: the policy
    /// names no remote, though the repository has one.
    fn local(name: &str) -> Setup {
        Setup::with(name, &policy("").replace("remote = \"\"\n", ""))
    }

    /// As [`Setup::new`] on `origin`, with `main`'s state on the remote: a
    /// policy naming one signer, whose key this host signs with as the
    /// host `name` (C.6).
    fn signed(name: &str) -> Setup {
        Setup::signing(name, &["main"])
    }

    /// As [`Setup::signed`], each branch of `trusting`, `main` or
    /// `bolt/*`, trusting the signer, so its results count (78).
    fn signing(name: &str, trusting: &[&str]) -> Setup {
        use ssh_key::{Algorithm, LineEnding, PrivateKey};
        let key = PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap();
        let line = format!("a {}", key.public_key().to_openssh().unwrap());
        let mut policy = policy("origin").replace(
            "[branches.main]\n",
            &format!("[signers.a]\nssh = \"{line}\"\n\n[branches.main]\n"),
        );
        for b in trusting {
            let table = match b.contains('*') {
                true => format!("[branches.\"{b}\"]\n"),
                false => format!("[branches.{b}]\n"),
            };
            policy = policy.replace(&table, &format!("{table}trust = [\"a\"]\n"));
        }
        let s = Setup::with(name, &policy);
        let path = s.config.path().join("signing");
        key.write_openssh_file(&path, LineEnding::LF).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::create_dir_all(s.config.path().join("sluice")).unwrap();
        std::fs::write(
            s.config.path().join("sluice/config.toml"),
            format!(
                "[signing]\nsigner = \"a\"\nkey = {:?}\n\n[peers]\nname = \"{name}\"\n",
                path.display().to_string()
            ),
        )
        .unwrap();
        s
    }

    /// The file `path` of `main`'s state on the remote, as another host
    /// reads it.
    fn remote_state(&self, path: &str) -> String {
        let out = support::git_command()
            .arg("--git-dir")
            .arg(self.origin.path())
            .args(["show", &format!("refs/sluice/state/main:{path}")])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn with(name: &str, policy: &str) -> Setup {
        let repo = Repo::new(name);
        let seen = Scratch::new(&format!("{name}-seen"));
        repo.write(".config/sluice.toml", policy);
        repo.write("check.sh", &check(seen.path()));
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "sluice's policy"]);
        let origin = repo.bare_origin();
        Setup {
            repo,
            origin,
            seen,
            sessions: Scratch::new(&format!("{name}-sessions")),
            config: Scratch::new(&format!("{name}-config")),
            state: Scratch::new(&format!("{name}-state")),
            git: support::CountingGit::new(),
        }
    }

    /// The binary in `dir`, this setup's host. The host process its
    /// commands start passes only when asked, so the rounds that run are
    /// the ones the test's commands ask for (design D14).
    fn command(&self, dir: &Path) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_sluice"));
        support::isolated(&mut c)
            .current_dir(dir)
            .env("SLUICE_ROUND_SLEEP", "record")
            .env("XDG_CONFIG_HOME", self.config.path())
            .env("XDG_STATE_HOME", self.state.path())
            .env_remove("SLUICE_AGENT")
            .env_remove("SLUICE_OTEL")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null());
        self.git.on(&mut c);
        c
    }

    fn sluice(&self, args: &[&str]) -> Output {
        self.command(self.repo.dir()).args(args).output().unwrap()
    }

    /// A session on `unit/<name>` off the local `main`, with one commit
    /// of its own file, submitted without waiting.
    fn submit(&self, name: &str) {
        let out = self.try_submit(name);
        assert!(out.status.success(), "{}", text(&out));
    }

    fn try_submit(&self, name: &str) -> Output {
        self.submit_to(name, "main", "main")
    }

    /// A session on `unit/<name>` cut from `base`, with one commit of its
    /// own file, submitted to `branch` without waiting.
    fn submit_to(&self, name: &str, base: &str, branch: &str) -> Output {
        self.submit_files(name, base, branch, &[])
    }

    /// As [`Setup::submit_to`], the commit also writing `files`.
    fn submit_files(&self, name: &str, base: &str, branch: &str, files: &[(&str, &str)]) -> Output {
        let dir: PathBuf = self.sessions.path().join(name);
        self.repo.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            &format!("unit/{name}"),
            dir.to_str().unwrap(),
            base,
        ]);
        std::fs::write(dir.join(format!("{name}.txt")), name).unwrap();
        for (path, content) in files {
            std::fs::write(dir.join(path), content).unwrap();
        }
        let git = |args: &[&str]| {
            let mut a = vec!["-C", dir.to_str().unwrap()];
            a.extend(args);
            self.repo.git(&a)
        };
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", name]);
        self.command(&dir)
            .args(["submit", branch, "--no-wait", "--json"])
            .output()
            .unwrap()
    }

    fn status(&self) -> serde_json::Value {
        let out = self.sluice(&["status", "main", "--json", "--all"]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    fn states(&self) -> Vec<String> {
        self.status()["submissions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["state"].as_str().unwrap().to_string())
            .collect()
    }

    /// Step `main` until every submission is out of the queue.
    fn step_until_out(&self) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let out = self.sluice(&["step", "main"]);
            assert!(out.status.success(), "{}", text(&out));
            if self
                .states()
                .iter()
                .all(|s| ["landed", "failed", "withdrawn"].contains(&s.as_str()))
            {
                return;
            }
            assert!(Instant::now() < deadline, "{}", self.status());
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// The remote's refs, name and revision, as another host sees them.
    fn remote_refs(&self) -> Vec<String> {
        let out = support::git_command()
            .arg("--git-dir")
            .arg(self.origin.path())
            .args(["for-each-ref", "--format=%(refname) %(objectname)"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(String::from)
            .collect()
    }

    fn remote_main(&self) -> String {
        let out = support::git_command()
            .arg("--git-dir")
            .arg(self.origin.path())
            .args(["rev-parse", "refs/heads/main"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn runs(&self) -> usize {
        std::fs::read_to_string(self.seen.path().join("runs"))
            .unwrap_or_default()
            .lines()
            .count()
    }

    /// Run `script` on the remote's side of the next push only, before
    /// the push is received unless `after` is set.
    fn on_next_push(&self, script: &str, after: bool) {
        let mark = self.seen.path().join("pushed-once");
        let wrapper = self.seen.path().join("receive-pack");
        let body = if after {
            format!("git-receive-pack \"$@\"\ncode=$?\n{script}\nexit $code\n")
        } else {
            format!("{script}\nexec git-receive-pack \"$@\"\n")
        };
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\nunset GIT_DIR GIT_WORK_TREE\n\
                 if [ -e {mark} ]; then exec git-receive-pack \"$@\"; fi\n\
                 touch {mark}\n{body}",
                mark = mark.display(),
            ),
        )
        .unwrap();
        self.repo.git(&[
            "config",
            "remote.origin.receivepack",
            &format!("sh {}", wrapper.display()),
        ]);
    }

    /// `main`'s records, where the remote's side of a push can copy them.
    fn records(&self) -> PathBuf {
        self.repo.dir().join(".git/sluice/main")
    }

    /// Step `main` until a step fails; what it said.
    fn step_until_failed(&self) -> String {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let out = self.sluice(&["step", "main"]);
            if !out.status.success() {
                return text(&out);
            }
            assert!(Instant::now() < deadline, "{}", self.status());
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// A records file of `main` holds `word`.
    fn logged(&self, word: &str) -> bool {
        let dir = self.repo.dir().join(".git/sluice/main");
        std::fs::read_dir(&dir).unwrap().any(|e| {
            std::fs::read_to_string(e.unwrap().path())
                .map(|t| t.contains(word))
                .unwrap_or(false)
        })
    }
}

/// C.4: three submissions land on the remote's `main` through `sluice
/// run`; the local `main` follows, and nothing else reaches the remote.
#[test]
fn submissions_land_on_the_remote_through_run() {
    let s = Setup::new("remote-run", "origin");
    let before: Vec<String> = s
        .remote_refs()
        .into_iter()
        .filter(|r| !r.starts_with("refs/heads/main "))
        .collect();
    for n in ["a", "b", "c"] {
        s.submit(n);
    }
    let out = s.sluice(&["run", "--rounds", "8"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(s.states(), ["landed"; 3], "{}", s.status());
    let main = s.remote_main();
    assert_eq!(s.repo.rev("main"), main);
    for n in ["a", "b", "c"] {
        s.repo.git(&["cat-file", "-e", &format!("{main}:{n}.txt")]);
    }
    let after: Vec<String> = s
        .remote_refs()
        .into_iter()
        .filter(|r| !r.starts_with("refs/heads/main "))
        .collect();
    assert_eq!(after, before);
}

/// 24, S8: another clone pushes to the remote's `main` after the
/// combination was formed; the landing is refused and reported, and the
/// next round tests the submission on the new revision and lands it.
#[test]
fn a_push_from_elsewhere_refuses_the_landing_and_the_next_round_lands() {
    let s = Setup::new("remote-moved", "origin");
    let other = Repo::clone_of(s.origin.path(), "remote-moved-other");
    other.write("other.txt", "other\n");
    other.git(&["add", "-A"]);
    other.git(&["commit", "-q", "-m", "other"]);
    let theirs = other.rev("HEAD");
    s.on_next_push(
        &format!("git -C {} push -q origin HEAD:main", other.dir().display()),
        false,
    );
    s.submit("a");
    s.step_until_out();
    assert!(s.logged("refused"), "the refusal is not reported");
    assert_eq!(s.states(), ["landed"]);
    assert_eq!(s.runs(), 2, "tested once on each revision");
    let main = s.remote_main();
    assert_eq!(s.repo.rev("main"), main);
    assert_eq!(s.repo.rev(&format!("{main}^1")), theirs);
}

/// 24: the push was accepted and sluice stopped before recording it; the
/// next round finds the landing on the remote and records it, with no new
/// run.
#[test]
fn a_landing_pushed_but_unrecorded_is_recorded_by_the_next_round() {
    let s = Setup::new("remote-stopped", "origin");
    let pushed = s.seen.path().join("pushed");
    s.on_next_push(&format!("touch {}\nsleep 5", pushed.display()), true);
    s.submit("a");
    // Step until the push is under way, then stop the step that made it.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let mut step = s
            .command(s.repo.dir())
            .args(["step", "main"])
            .spawn()
            .unwrap();
        loop {
            if pushed.exists() {
                step.kill().unwrap();
                step.wait().unwrap();
                break;
            }
            if step.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if pushed.exists() {
            break;
        }
        assert!(Instant::now() < deadline, "{}", s.status());
        std::thread::sleep(Duration::from_millis(200));
    }
    let landed = s.remote_main();
    assert_ne!(s.states(), ["landed"]);
    s.step_until_out();
    assert_eq!(s.states(), ["landed"]);
    assert_eq!(s.runs(), 1, "no new run");
    assert_eq!(s.remote_main(), landed);
}

/// 24: the remote could not be reached during the push; the next round
/// finds the remote unchanged and lands then, with no new run.
#[test]
fn a_push_that_never_reached_the_remote_lands_on_the_next_round() {
    let s = Setup::new("remote-unreachable", "origin");
    let aside = s.origin.path().with_extension("aside");
    s.on_next_push(
        &format!("mv {} {}", s.origin.path().display(), aside.display()),
        false,
    );
    let before = s.remote_main();
    s.submit("a");
    let failed = s.step_until_failed();
    assert!(failed.contains("pushing main to origin failed"), "{failed}");
    assert_eq!(s.states(), ["landing"], "not landed");
    std::fs::rename(&aside, s.origin.path()).unwrap();
    assert_eq!(s.remote_main(), before);
    s.step_until_out();
    assert_eq!(s.states(), ["landed"]);
    assert_eq!(s.runs(), 1, "no new run");
    assert_eq!(s.repo.rev("main"), s.remote_main());
}

/// 22, 122, 124, I4: when the remote's side of the landing's push has
/// received it, the remote holds the landing and the records still say
/// the submission is landing: landed is written only once the remote has
/// moved, and the host's copy is levelled to it after.
#[test]
fn landed_is_written_only_once_the_remote_holds_the_landing() {
    let s = Setup::new("remote-landed-after", "origin");
    let at_push = s.seen.path().join("at-push");
    s.on_next_push(
        &format!(
            "mkdir -p {at}/submissions\n\
             git --git-dir {origin} rev-parse refs/heads/main > {at}/remote\n\
             cp {records}/submissions/*.json {at}/submissions/\n\
             cp {records}/log.jsonl {at}/log.jsonl\n",
            at = at_push.display(),
            origin = s.origin.path().display(),
            records = s.records().display(),
        ),
        true,
    );
    let before = s.remote_main();
    s.submit("a");
    s.step_until_out();
    assert_eq!(s.states(), ["landed"]);
    let landed = s.remote_main();
    assert_ne!(landed, before);
    assert_eq!(s.status()["submissions"][0]["landed_as"], landed.as_str());

    let remote = std::fs::read_to_string(at_push.join("remote")).unwrap();
    assert_eq!(remote.trim(), landed, "the remote held the landing");
    let subs: Vec<serde_json::Value> = std::fs::read_dir(at_push.join("submissions"))
        .unwrap()
        .map(|e| serde_json::from_slice(&std::fs::read(e.unwrap().path()).unwrap()).unwrap())
        .collect();
    let states: Vec<&str> = subs.iter().map(|x| x["state"].as_str().unwrap()).collect();
    assert_eq!(
        states,
        ["landing"],
        "and the records did not yet say landed"
    );
    let log = std::fs::read_to_string(at_push.join("log.jsonl")).unwrap();
    assert!(!log.contains("\"event\":\"landed\""), "{log}");
    assert!(s.logged("\"event\":\"landed\""));
    assert_eq!(s.repo.rev("main"), landed, "the copy is levelled to it");
}

/// 22, 122, 124, I4: a push the remote refuses by a rule of its own lands
/// nothing: the round says the remote's own words, the submission is left
/// landing and never told landed, and neither the remote nor the host's
/// copy moves. Once the remote takes the push, the next round lands it,
/// with no new run.
#[test]
fn a_push_the_remote_refuses_leaves_the_submission_unlanded_with_its_words() {
    use std::os::unix::fs::PermissionsExt;
    let s = Setup::new("remote-rule", "origin");
    let words = "main takes no push but the queue's identity";
    let hook = s.origin.path().join("hooks/pre-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, format!("#!/bin/sh\necho \"{words}\" >&2\nexit 1\n")).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let before = s.remote_main();
    s.submit("a");
    let failed = s.step_until_failed();
    assert!(failed.contains("pushing main to origin failed"), "{failed}");
    assert!(failed.contains(words), "{failed}");
    assert!(failed.contains("pre-receive hook declined"), "{failed}");
    assert_eq!(s.states(), ["landing"]);
    assert!(s.status()["submissions"][0]["landed_as"].is_null());
    assert!(!s.logged("\"event\":\"landed\""));
    assert_eq!(s.remote_main(), before);
    assert_eq!(
        s.repo.rev("main"),
        before,
        "the copy is written at no other time"
    );

    // A waiter is told the words, and not that it landed.
    let id = s.status()["submissions"][0]["id"].to_string();
    let out = s.sluice(&["wait", &id, "--timeout", "30s", "--json"]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains(words), "{}", text(&out));
    assert_eq!(s.states(), ["landing"]);

    std::fs::remove_file(&hook).unwrap();
    s.step_until_out();
    assert_eq!(s.states(), ["landed"]);
    assert_eq!(s.runs(), 1, "no new run");
    let landed = s.remote_main();
    assert_eq!(s.repo.rev(&format!("{landed}^1")), before);
    assert_eq!(s.repo.rev("main"), landed);
}

/// 124, 6, rule core.6: after a landing, this host's copy of `main`
/// stands where it was before it, and git's lock keeps any round from
/// levelling it (S64). A session cut from the copy submits; its candidate
/// is formed on the remote's revision, which holds the first landing, and
/// lands there, and the copy is written only once its lock is gone, to
/// the landing.
#[test]
fn a_submit_with_the_copy_behind_is_formed_on_the_remotes_revision() {
    let s = Setup::new("remote-copy-behind", "origin");
    let made = s.repo.rev("main");
    s.submit("a");
    s.step_until_out();
    let first = s.remote_main();
    assert_eq!(s.repo.rev("main"), first);
    // As an older sluice left it, past the protection.
    s.repo.git(&[
        "-c",
        "core.hooksPath=/dev/null",
        "update-ref",
        "refs/heads/main",
        &made,
    ]);
    let lock = s.repo.dir().join(".git/refs/heads/main.lock");
    std::fs::write(&lock, "").unwrap();

    s.submit("b");
    s.step_until_out();
    assert_eq!(s.states(), ["landed", "landed"], "{}", s.status());
    let second = s.remote_main();
    assert_eq!(
        s.repo.rev(&format!("{second}^1")),
        first,
        "formed on origin's"
    );
    for n in ["a", "b"] {
        s.repo
            .git(&["cat-file", "-e", &format!("{second}:{n}.txt")]);
    }
    assert_eq!(s.runs(), 2);
    assert_eq!(s.repo.rev("main"), made, "the copy is left behind");
    assert!(s.logged("copy-behind"));

    std::fs::remove_file(&lock).unwrap();
    let out = s.sluice(&["step", "main"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(s.repo.rev("main"), second, "levelled to the landing");
    assert!(s.logged("copy-levelled"));
}

/// A failed fetch lands nothing and says so; the checks already running
/// keep running, and their verdict lands the submission once the remote
/// is back.
#[test]
fn a_failed_fetch_lands_nothing_and_says_so() {
    let s = Setup::new("remote-fetch", "origin");
    std::fs::write(s.seen.path().join("hold"), "").unwrap();
    s.submit("a");
    let deadline = Instant::now() + Duration::from_secs(60);
    while s.runs() == 0 {
        let out = s.sluice(&["step", "main"]);
        assert!(out.status.success(), "{}", text(&out));
        assert!(Instant::now() < deadline, "{}", s.status());
        std::thread::sleep(Duration::from_millis(200));
    }
    let before = s.remote_main();
    let aside = s.origin.path().with_extension("aside");
    std::fs::rename(s.origin.path(), &aside).unwrap();
    let out = s.sluice(&["step", "main"]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("fetching main from origin failed"),
        "{}",
        text(&out)
    );
    std::fs::write(s.seen.path().join("go"), "").unwrap();
    std::fs::rename(&aside, s.origin.path()).unwrap();
    assert_eq!(s.remote_main(), before);
    s.step_until_out();
    assert_eq!(s.states(), ["landed"]);
    assert_eq!(s.runs(), 1, "the run kept running and its verdict stood");
}

/// `main`'s log lines of `event`, as JSON.
fn events(log: &str, event: &str) -> Vec<serde_json::Value> {
    log.lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .filter(|l| l["event"] == event)
        .collect()
}

/// 133: with the remote moved away, a waiting submit says once on stderr
/// why the host process's round could not fetch, and goes on following,
/// so it is ended once it has (135; design D9); `main`'s log holds one
/// `read-refused` line however many rounds are refused; once the remote
/// is back, one round logs `read-again` with the rounds it counted.
#[test]
fn a_refused_fetch_is_logged_once_and_its_end_with_the_rounds() {
    let s = Setup::new("remote-refused", "origin");
    std::fs::write(s.seen.path().join("hold"), "").unwrap();
    let dir = s.sessions.path().join("a");
    s.repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "unit/a",
        dir.to_str().unwrap(),
        "main",
    ]);
    std::fs::write(dir.join("a.txt"), "a").unwrap();
    s.repo.git(&["-C", dir.to_str().unwrap(), "add", "-A"]);
    s.repo
        .git(&["-C", dir.to_str().unwrap(), "commit", "-q", "-m", "a"]);
    let mut waiting = s
        .command(&dir)
        .args(["submit", "main"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = std::io::BufReader::new(waiting.stderr.take().unwrap());
    let (send, said) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufRead::lines(stderr).map_while(Result::ok) {
            if send.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(60);
    while s.runs() == 0 {
        assert!(Instant::now() < deadline, "the check never started");
        std::thread::sleep(Duration::from_millis(100));
    }
    let aside = s.origin.path().with_extension("aside");
    std::fs::rename(s.origin.path(), &aside).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut seen: Vec<String> = Vec::new();
    while !seen
        .iter()
        .any(|l| l.contains("fetching main from origin failed"))
    {
        let left = deadline.saturating_duration_since(Instant::now());
        match said.recv_timeout(left) {
            Ok(line) => seen.push(line),
            Err(_) => panic!("the waiting submit never said the round's failure: {seen:?}"),
        }
    }
    let _ = waiting.kill();
    let _ = waiting.wait();
    for _ in 0..3 {
        let out = s.sluice(&["step", "main"]);
        assert!(!out.status.success());
        assert!(
            text(&out).contains("fetching main from origin failed"),
            "{}",
            text(&out)
        );
    }
    let log = || std::fs::read_to_string(s.records().join("log.jsonl")).unwrap();
    let refused = events(&log(), "read-refused");
    assert_eq!(refused.len(), 1, "{}", log());
    assert_eq!(refused[0]["remote"], "origin");
    assert_eq!(refused[0]["asked"], "refs/heads/main");
    let said = refused[0]["said"].as_str().unwrap();
    assert!(
        said.contains("does not appear to be a git repository"),
        "{said}"
    );
    assert!(!said.starts_with("fetching"), "git's words alone: {said}");
    let kept: serde_json::Value = serde_json::from_slice(
        &std::fs::read(s.repo.dir().join(".git/sluice/.refused/main.json")).unwrap(),
    )
    .unwrap();
    let rounds = kept["rounds"].as_u64().unwrap();
    assert!(rounds >= 3, "{kept}");

    std::fs::rename(&aside, s.origin.path()).unwrap();
    let out = s.sluice(&["step", "main"]);
    assert!(out.status.success(), "{}", text(&out));
    let again = events(&log(), "read-again");
    assert_eq!(again.len(), 1, "{}", log());
    assert_eq!(again[0]["host"], refused[0]["host"]);
    assert_eq!(again[0]["since"], refused[0]["at"]);
    assert_eq!(again[0]["rounds"], rounds);
    assert!(!s.repo.dir().join(".git/sluice/.refused/main.json").exists());
    std::fs::write(s.seen.path().join("go"), "").unwrap();
    s.step_until_out();
    assert_eq!(s.states(), ["landed"]);
    assert_eq!(events(&log(), "read-refused").len(), 1);
}

/// 133, 80, 73: with `main`'s state on the remote too, the rounds while it
/// is away write nothing, and both lines land in the first write after it
/// is back, each timed when it happened.
#[test]
fn with_the_state_on_the_remote_both_lines_land_in_the_first_write_after() {
    let s = Setup::signed("remote-refused-state");
    s.submit("a");
    s.step_until_out();
    assert_eq!(s.states(), ["landed"]);
    // Rounds until one writes nothing more.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let before = s.remote_state("version");
        let out = s.sluice(&["step", "main"]);
        assert!(out.status.success(), "{}", text(&out));
        if s.remote_state("version") == before {
            break;
        }
        assert!(Instant::now() < deadline, "main never settled");
    }
    let version: u64 = s.remote_state("version").trim().parse().unwrap();
    let aside = s.origin.path().with_extension("aside");
    std::fs::rename(s.origin.path(), &aside).unwrap();
    for _ in 0..3 {
        let out = s.sluice(&["step", "main"]);
        assert!(!out.status.success());
        assert!(
            text(&out).contains("fetching refs/sluice/state/main from origin failed"),
            "{}",
            text(&out)
        );
    }
    let kept: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            s.repo
                .dir()
                .join(".git/sluice-remote/origin/refused/main.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(kept["rounds"], 3, "{kept}");
    std::fs::rename(&aside, s.origin.path()).unwrap();
    assert_eq!(
        s.remote_state("version").trim(),
        version.to_string(),
        "nothing was written while the remote was away"
    );
    let out = s.sluice(&["step", "main"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(
        s.remote_state("version").trim(),
        (version + 1).to_string(),
        "one write"
    );
    let log = s.remote_state("log.jsonl");
    let refused = events(&log, "read-refused");
    let again = events(&log, "read-again");
    assert_eq!((refused.len(), again.len()), (1, 1), "{log}");
    assert_eq!(refused[0]["host"], "remote-refused-state");
    assert_eq!(refused[0]["asked"], "refs/sluice/state/main");
    assert_eq!(refused[0]["at"], kept["since"], "timed when it happened");
    assert_eq!(again[0]["since"], kept["since"]);
    assert_eq!(again[0]["rounds"], 3);
    assert!(again[0]["at"].as_str() >= refused[0]["at"].as_str());
    let out = s.sluice(&["step", "main"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(
        events(&s.remote_state("log.jsonl"), "read-refused").len(),
        1,
        "no line is written twice"
    );
}

/// C.7: a `remote` the repository does not have lands nothing, and both
/// the submission and the round name it.
#[test]
fn an_unknown_remote_lands_nothing_and_is_named() {
    let s = Setup::new("remote-unknown", "nowhere");
    let before = s.repo.rev("main");
    let out = s.try_submit("a");
    assert!(!out.status.success());
    assert!(text(&out).contains("\"nowhere\""), "{}", text(&out));
    let out = s.sluice(&["step", "main"]);
    assert!(!out.status.success());
    assert!(text(&out).contains("\"nowhere\""), "{}", text(&out));
    assert_eq!(s.repo.rev("main"), before);
    assert_eq!(s.runs(), 0);
}

/// A bolt whose policy names a remote that has no such branch is refused,
/// saying how to make it; sluice makes it there, a submission lands on it,
/// and once the bolt has landed on `main` sluice deletes it there
/// (branches-1 104, 105).
#[test]
fn a_bolt_is_made_landed_on_and_deleted_on_its_remote() {
    let s = Setup::new("remote-bolt-life", "origin");
    s.repo.git(&["branch", "bolt/x", "main"]);
    let out = s.submit_to("a", "bolt/x", "bolt/x");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("bolt/x is not on origin; make it with sluice branch create bolt/x"),
        "{}",
        text(&out)
    );
    let out = s.sluice(&["branch", "create", "bolt/x", "--from", "main", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let made: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(made["created"], s.remote_main());
    assert_eq!(remote_rev(&s, "refs/heads/bolt/x"), s.remote_main());
    let out = s.sluice(&["branch", "create", "bolt/x", "--from", "main"]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("bolt/x is already there"),
        "{}",
        text(&out)
    );

    let out = s
        .command(&s.sessions.path().join("a"))
        .args(["submit", "bolt/x", "--no-wait", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let out = s.sluice(&["run", "--rounds", "8"]);
    assert!(out.status.success(), "{}", text(&out));
    let bolt = remote_rev(&s, "refs/heads/bolt/x");
    s.repo.git(&["cat-file", "-e", &format!("{bolt}:a.txt")]);

    let out = s.sluice(&["branch", "delete", "bolt/x", "--into", "main"]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("has not landed into main"),
        "{}",
        text(&out)
    );

    // The bolt lands on main, submitted from a branch at its revision.
    let out = s.submit_to("land", "origin/bolt/x", "main");
    assert!(out.status.success(), "{}", text(&out));
    let out = s.sluice(&["run", "--rounds", "8"]);
    assert!(out.status.success(), "{}", text(&out));

    let out = s.sluice(&["branch", "delete", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!s
        .remote_refs()
        .iter()
        .any(|r| r.starts_with("refs/heads/bolt/x ")));
    for gone in ["refs/heads/bolt/x", "refs/remotes/origin/bolt/x"] {
        let out = s.repo.git_raw(&["rev-parse", "--verify", "-q", gone]);
        assert!(!out.status.success(), "{gone} is still here");
    }
}

/// With no remote, sluice makes and deletes a bolt in local refs under
/// the same rules (branches-1 104, 105; C.4 local refs).
#[test]
fn a_bolt_is_made_and_deleted_with_no_remote() {
    let s = Setup::local("local-bolt-life");
    let main = s.repo.rev("main");
    let out = s.sluice(&["branch", "create", "bolt/x", "--from", "main"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(s.repo.rev("bolt/x"), main);
    let out = s.sluice(&["branch", "create", "bolt/x", "--from", "main"]);
    assert!(!out.status.success(), "{}", text(&out));

    let out = s.submit_to("a", "bolt/x", "bolt/x");
    assert!(out.status.success(), "{}", text(&out));
    let out = s.sluice(&["run", "--rounds", "8"]);
    assert!(out.status.success(), "{}", text(&out));
    let bolt = s.repo.rev("bolt/x");
    assert_ne!(bolt, main);
    let out = s.sluice(&["branch", "delete", "bolt/x"]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("has not landed into main"),
        "{}",
        text(&out)
    );

    let out = s.submit_to("land", "bolt/x", "main");
    assert!(out.status.success(), "{}", text(&out));
    let out = s.sluice(&["run", "--rounds", "8"]);
    assert!(out.status.success(), "{}", text(&out));
    let out = s.sluice(&["branch", "delete", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    let out = s
        .repo
        .git_raw(&["rev-parse", "--verify", "-q", "refs/heads/bolt/x"]);
    assert!(!out.status.success(), "bolt/x is still here");
    assert_eq!(s.remote_main(), main, "nothing reached the remote");

    // 105: its records stay and answer its status, which says it is gone,
    // and the overview shows it with its landing (record 0091).
    let out = s.sluice(&["status", "bolt/x", "--all", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["gone"], true, "{doc}");
    assert_eq!(doc["submissions"][0]["state"], "landed", "{doc}");
    let gone = format!("bolt/x is gone, last at {}", &bolt[..7]);
    let out = s.sluice(&["status", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).starts_with(&gone), "{}", text(&out));
    let out = s.sluice(&["overview"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains(&gone), "{}", text(&out));
    assert!(text(&out).contains("landed #1"), "{}", text(&out));
    let out = s.sluice(&["overview", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let named = |b: &str| {
        doc["branches"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["branch"] == b)
            .unwrap_or_else(|| panic!("the overview names {b}: {doc}"))
            .clone()
    };
    assert_eq!(named("bolt/x")["gone"], true, "{doc}");
    assert!(named("main").get("gone").is_none(), "{doc}");
    let out = s.sluice(&["status", "main", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["gone"], false, "{doc}");
}

/// A bolt made here and deleted with nothing submitted to it here, as
/// when every submission to it was made in another copy of the
/// repository, so its records hold the revision it was made at alone:
/// `sluice status` answers it, plain and with `--all`, saying it is gone
/// and its queue empty, and the overview names it gone, apart from the
/// branches it lands on. A branch with neither records nor a revision is
/// still no branch (26, 61, 105; record 0091).
#[test]
fn a_bolt_deleted_with_nothing_submitted_here_answers_its_status() {
    for s in [
        Setup::local("gone-bolt-local"),
        Setup::new("gone-bolt-remote", "origin"),
    ] {
        let out = s.sluice(&["branch", "create", "bolt/x", "--from", "main"]);
        assert!(out.status.success(), "{}", text(&out));
        let out = s.sluice(&["branch", "delete", "bolt/x"]);
        assert!(out.status.success(), "{}", text(&out));

        let out = s.sluice(&["status", "bolt/x"]);
        assert!(out.status.success(), "{}", text(&out));
        assert!(text(&out).starts_with("bolt/x is gone\n"), "{}", text(&out));
        assert!(
            text(&out).contains("nothing in the queue"),
            "{}",
            text(&out)
        );
        for args in [
            &["status", "bolt/x", "--json"][..],
            &["status", "bolt/x", "--all", "--json"],
        ] {
            let out = s.sluice(args);
            assert!(out.status.success(), "{}", text(&out));
            let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(doc["gone"], true, "{doc}");
            assert_eq!(doc["submissions"], serde_json::json!([]), "{doc}");
        }

        let out = s.sluice(&["overview", "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let bolt = doc["branches"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["branch"] == "bolt/x")
            .unwrap_or_else(|| panic!("the overview names bolt/x: {doc}"));
        assert_eq!(bolt["gone"], true, "{doc}");
        let out = s.sluice(&["overview"]);
        assert!(out.status.success(), "{}", text(&out));
        assert!(
            text(&out).contains("gone, nothing queued or recent: bolt/x\n"),
            "{}",
            text(&out)
        );
        assert!(!text(&out).contains("bolt/x at"), "{}", text(&out));

        let out = s.sluice(&["status", "bolt/never"]);
        assert_eq!(out.status.code(), Some(1), "{}", text(&out));
        assert!(
            text(&out).contains("no branch bolt/never; branches with records: bolt/x"),
            "{}",
            text(&out)
        );
    }
}

/// A bolt another host made is on the remote and, here, only as its
/// remote-tracking ref: a session cut from it submits to it, and the
/// landing goes to the remote.
#[test]
fn a_bolt_only_fetched_here_lands_on_its_remote() {
    let s = Setup::new("remote-bolt-fetched", "origin");
    s.repo
        .git(&["push", "-q", "origin", "origin/main:refs/heads/bolt/y"]);
    s.repo.git(&["fetch", "-q", "origin"]);
    let out = s.submit_to("b", "origin/bolt/y", "bolt/y");
    assert!(out.status.success(), "{}", text(&out));
    let out = s.sluice(&["run", "--rounds", "8"]);
    assert!(out.status.success(), "{}", text(&out));
    let bolt = remote_rev(&s, "refs/heads/bolt/y");
    s.repo.git(&["cat-file", "-e", &format!("{bolt}:b.txt")]);
}

/// 124, C.7 pushed refs: this host's `main` is from before
/// `.config/sluice.toml`, while `origin/main` as fetched holds the policy
/// naming origin; `sluice branch create` reads `main` as origin's and
/// makes the bolt there at origin's `main`, leaving this host's `main`
/// where it was.
#[test]
fn a_bolt_is_made_on_origin_from_a_main_this_host_holds_from_before_the_policy() {
    let s = Setup::new("remote-bolt-stale-main", "origin");
    let root = s.repo.rev("main~1");
    s.repo.git(&["reset", "-q", "--hard", &root]);
    let origins = s.remote_main();
    assert_eq!(s.repo.rev("refs/remotes/origin/main"), origins);
    let out = s.sluice(&["branch", "create", "bolt/x", "--from", "main", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let made: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(made["created"], origins);
    assert_eq!(remote_rev(&s, "refs/heads/bolt/x"), origins);
    assert_eq!(s.repo.rev("main"), root);
}

/// 124: this host's `main` names origin and `upstream/main`, a second
/// remote's as fetched, names upstream; nothing the host holds says which
/// `main` is on, so a round lands nothing, runs nothing, moves neither
/// remote's `main`, and names each remote with the ref that named it.
#[test]
fn two_remotes_named_for_main_land_nothing_and_are_named() {
    let s = Setup::new("remote-two-remotes", "origin");
    s.submit("a");
    let upstream = Scratch::new("remote-two-remotes-upstream.git");
    let at = upstream.path().to_str().unwrap();
    s.repo.git(&["clone", "-q", "--bare", ".", at]);
    s.repo.git(&["remote", "add", "upstream", at]);
    let theirs = policy("upstream");
    let named = s.repo.branch(
        "upstream-policy",
        "main",
        &[(".config/sluice.toml", Some(theirs.as_str()))],
    );
    s.repo.git(&[
        "push",
        "-q",
        "upstream",
        &format!("{named}:refs/heads/main"),
    ]);
    s.repo.git(&["fetch", "-q", "upstream"]);
    let origins = s.remote_main();

    let out = s.sluice(&["step", "main"]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("origin on main ("), "{}", text(&out));
    assert!(
        text(&out).contains("upstream on upstream/main ("),
        "{}",
        text(&out)
    );
    assert_eq!(s.remote_main(), origins);
    let upstreams = support::git_command()
        .arg("--git-dir")
        .arg(upstream.path())
        .args(["rev-parse", "refs/heads/main"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&upstreams.stdout).trim(), named);
    assert_eq!(s.runs(), 0);
}

fn remote_rev(s: &Setup, name: &str) -> String {
    let out = support::git_command()
        .arg("--git-dir")
        .arg(s.origin.path())
        .args(["rev-parse", name])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// S57's start: `bolt/x` in local refs lands `a`, then `b`, which brings
/// the policy naming origin as its remote, while origin's `bolt/x` is the
/// older revision `bolt/x` was made at; `c` is then queued on it. Returns
/// the setup, origin's revision and the second landing.
fn landed_locally_then_remote_older(name: &str) -> (Setup, String, String) {
    let s = Setup::local(name);
    let out = s.sluice(&["branch", "create", "bolt/x", "--from", "main"]);
    assert!(out.status.success(), "{}", text(&out));
    let older = s.repo.rev("bolt/x");
    // A refused landing left origin's copy at the revision it was made at.
    set_remote(&s, "refs/heads/bolt/x", "refs/heads/bolt/x");
    let out = s.submit_to("a", "bolt/x", "bolt/x");
    assert!(out.status.success(), "{}", text(&out));
    step_bolt_until_out(&s);
    let remote = policy("origin");
    let out = s.submit_files("b", "bolt/x", "bolt/x", &[(".config/sluice.toml", &remote)]);
    assert!(out.status.success(), "{}", text(&out));
    step_bolt_until_out(&s);
    assert_eq!(bolt_states(&s), ["landed", "landed"]);
    let second = s.repo.rev("bolt/x");
    s.repo.git(&["cat-file", "-e", &format!("{second}:b.txt")]);
    assert_eq!(remote_rev(&s, "refs/heads/bolt/x"), older);

    let out = s.submit_to("c", "bolt/x", "bolt/x");
    assert!(out.status.success(), "{}", text(&out));
    (s, older, second)
}

/// Set origin's `to` to this repository's `from`, as another host's push
/// would, writing nothing here.
fn set_remote(s: &Setup, from: &str, to: &str) {
    let out = support::git_command()
        .arg("--git-dir")
        .arg(s.origin.path())
        .args(["fetch", "-q", "--force"])
        .arg(s.repo.dir())
        .arg(format!("{from}:{to}"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
}

fn bolt_states(s: &Setup) -> Vec<String> {
    let out = s.sluice(&["status", "bolt/x", "--json", "--all"]);
    assert!(out.status.success(), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    v["submissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["state"].as_str().unwrap().to_string())
        .collect()
}

/// Step `bolt/x` until every submission on it is out of the queue.
fn step_bolt_until_out(s: &Setup) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let out = s.sluice(&["step", "bolt/x"]);
        assert!(out.status.success(), "{}", text(&out));
        if bolt_states(s)
            .iter()
            .all(|s| ["landed", "failed", "withdrawn"].contains(&s.as_str()))
        {
            return;
        }
        assert!(Instant::now() < deadline, "{:?}", bolt_states(s));
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The held round: it forms and lands nothing, names both revisions, and
/// moves neither origin's `bolt/x` nor this host's.
fn step_holds(s: &Setup, older: &str, second: &str) {
    let runs = s.runs();
    let out = s.sluice(&["step", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!(
            "held: bolt/x reads {}, which lacks the landing {}; put the landing back on \
             bolt/x, or run sluice branch accept bolt/x {}\n",
            &older[..7],
            &second[..7],
            &older[..7]
        )
    );
    assert_eq!(bolt_states(s), ["landed", "landed", "queued"]);
    assert_eq!(s.runs(), runs, "nothing tested");
    assert_eq!(remote_rev(s, "refs/heads/bolt/x"), older);
    assert_eq!(s.repo.rev("bolt/x"), second);
    let out = s.sluice(&["status", "bolt/x", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        (v["held"]["read"].as_str(), v["held"]["lacks"].as_str()),
        (Some(older), Some(second))
    );
}

/// S57, 121: the round that first reads `bolt/x` from origin, older than
/// what landed here, holds it; once the operator pushes the second
/// landing to origin, the third submission is formed on it and lands.
#[test]
fn s57_a_remote_older_than_the_local_landings_holds_until_the_landing_is_pushed() {
    let (s, older, second) = landed_locally_then_remote_older("remote-s57-pushed");
    step_holds(&s, &older, &second);
    set_remote(&s, "refs/heads/bolt/x", "refs/heads/bolt/x");
    step_bolt_until_out(&s);
    assert_eq!(bolt_states(&s), ["landed"; 3]);
    let bolt = remote_rev(&s, "refs/heads/bolt/x");
    assert_eq!(s.repo.rev(&format!("{bolt}^1")), second);
    s.repo.git(&["cat-file", "-e", &format!("{bolt}:c.txt")]);
}

/// S57, 121, C.5, 124: the operator accepts origin's older revision
/// instead; the third submission is formed on it and lands. The round
/// after the acceptance levels this host's `bolt/x` to that revision,
/// whose policy names no remote: from then on `bolt/x` is a branch in this
/// host's own refs, and the landing is made there (C.7).
#[test]
fn s57_a_remote_older_than_the_local_landings_holds_until_its_revision_is_accepted() {
    let (s, older, second) = landed_locally_then_remote_older("remote-s57-accepted");
    step_holds(&s, &older, &second);
    let out = s.sluice(&["branch", "accept", "bolt/x", &older[..7], "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        (v["accepted"].as_str(), v["lacked"].as_str()),
        (Some(older.as_str()), Some(second.as_str()))
    );
    step_bolt_until_out(&s);
    assert_eq!(bolt_states(&s), ["landed"; 3]);
    let bolt = s.repo.rev("bolt/x");
    assert_eq!(s.repo.rev(&format!("{bolt}^1")), older);
    s.repo.git(&["cat-file", "-e", &format!("{bolt}:c.txt")]);
    assert_eq!(remote_rev(&s, "refs/heads/bolt/x"), older);
}

/// 124, S58, 1, rule core.20: from a worktree that has checked out
/// `bolt/x`, whose copy here is behind origin's, `sluice submit main`
/// submits origin's revision, writes no ref of `bolt/x` here or on
/// origin, and says the copy is behind; `sluice step bolt/x` then levels
/// the copy and leaves the worktree's files and index as they were.
#[test]
fn a_worktree_on_a_bolt_behind_submits_origins_revision_and_a_round_levels_it() {
    let s = Setup::new("remote-bolt-copy", "origin");
    s.repo.git(&["branch", "bolt/x", "main"]);
    s.repo
        .git(&["push", "-q", "origin", "bolt/x:refs/heads/bolt/x"]);
    let made = s.repo.rev("bolt/x");
    for n in ["a", "b"] {
        let out = s.submit_to(n, "bolt/x", "bolt/x");
        assert!(out.status.success(), "{}", text(&out));
        step_bolt_until_out(&s);
    }
    let remote = remote_rev(&s, "refs/heads/bolt/x");
    assert_eq!(
        s.repo.rev("bolt/x"),
        remote,
        "each landing's round levels it"
    );
    // This host's bolt/x stands where it was before both landings, as an
    // older sluice left it, past the protection, and a worktree has it
    // checked out.
    s.repo.git(&[
        "-c",
        "core.hooksPath=/dev/null",
        "update-ref",
        "refs/heads/bolt/x",
        &made,
    ]);
    let w = s.sessions.path().join("landing");
    s.repo
        .git(&["worktree", "add", "-q", w.to_str().unwrap(), "bolt/x"]);
    let bolt_refs = |s: &Setup| -> Vec<String> {
        s.remote_refs()
            .into_iter()
            .filter(|r| r.starts_with("refs/heads/bolt/"))
            .collect()
    };
    let before = bolt_refs(&s);

    let out = s
        .command(&w)
        .args(["submit", "main", "--no-wait"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(&format!(
            "this host's bolt/x is at {}, behind origin's {}; submitted {}\n",
            &made[..7],
            &remote[..7],
            &remote[..7]
        )),
        "{}",
        text(&out)
    );
    let subs = s.status()["submissions"].clone();
    assert_eq!(subs[0]["head"], remote.as_str(), "{subs}");
    assert_eq!(bolt_refs(&s), before, "nothing of bolt/x on origin written");
    assert_eq!(s.repo.rev("bolt/x"), made, "nor here");

    let in_worktree = |args: &[&str]| {
        let mut a = vec!["-C", w.to_str().unwrap()];
        a.extend(args);
        s.repo.git(&a)
    };
    let index = in_worktree(&["ls-files", "--stage"]);
    let files: Vec<String> = std::fs::read_dir(&w)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let out = s.sluice(&["step", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(s.repo.rev("bolt/x"), remote);
    assert_eq!(in_worktree(&["ls-files", "--stage"]), index);
    let after: Vec<String> = std::fs::read_dir(&w)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(after, files);
    assert!(!w.join("a.txt").exists() && !w.join("b.txt").exists());
}

/// 124, D2, 1, rule core.4: `bolt/x`, named from the repository with no
/// worktree on it, is submitted to `main` at origin's revision, two
/// landings ahead of this host's copy: no branch is pushed, no ref of it
/// is written here or on origin, and nothing is said of this host's copy.
/// The forecast of it names the same head (64).
#[test]
fn a_branch_by_name_is_submitted_at_its_remotes_revision() {
    let s = Setup::new("remote-by-name", "origin");
    s.repo.git(&["branch", "bolt/x", "main"]);
    s.repo
        .git(&["push", "-q", "origin", "bolt/x:refs/heads/bolt/x"]);
    let made = s.repo.rev("bolt/x");
    for n in ["a", "b"] {
        let out = s.submit_to(n, "bolt/x", "bolt/x");
        assert!(out.status.success(), "{}", text(&out));
        step_bolt_until_out(&s);
    }
    let remote = remote_rev(&s, "refs/heads/bolt/x");
    assert_ne!(remote, made);
    // This host's bolt/x stands where it was before both landings, as an
    // older sluice left it, past the protection.
    set_copy(&s, "bolt/x", &made);
    let main = s.repo.rev("main");
    let refs = |s: &Setup| -> Vec<String> {
        s.remote_refs()
            .into_iter()
            .filter(|r| r.starts_with("refs/heads/"))
            .collect()
    };
    let before = refs(&s);

    let out = s.sluice(&["submit", "main", "--check", "--head", "bolt/x", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let forecast: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(forecast["head"], remote.as_str(), "{forecast}");

    let out = s.sluice(&["submit", "main", "--head", "bolt/x", "--no-wait"]);
    assert!(out.status.success(), "{}", text(&out));
    let said = String::from_utf8_lossy(&out.stdout);
    let line = said
        .strip_prefix("submission ")
        .and_then(|l| l.strip_suffix(" to main from bolt/x · nothing queued ahead\n"));
    assert!(line.is_some(), "{}", text(&out));
    assert!(!text(&out).contains("this host's"), "{}", text(&out));
    let subs = s.status()["submissions"].clone();
    assert_eq!(subs[0]["head"], remote.as_str(), "{subs}");
    assert_eq!(subs[0]["from"]["head"], "bolt/x", "{subs}");

    let out = s.sluice(&["submit", "main", "--head", "bolt/x", "--no-wait", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        v,
        serde_json::json!({ "id": v["id"], "branch": "main", "ahead": [] })
    );
    assert_eq!(refs(&s), before, "nothing pushed to origin");
    assert_eq!(s.repo.rev("bolt/x"), made, "nor written here");
    assert_eq!(s.repo.rev("main"), main);
}

/// A `[[checkout.cache]]` table, which sluice no longer keeps and whose
/// policy it refuses whole (ADR 0049).
const CACHE: &str = "\n[[checkout.cache]]\nkey = [\"Cargo.lock\"]\npaths = [\"target\"]\n";

/// A commit on `from` whose policy is `policy`, made on the scratch
/// branch `name`, which no policy protects; returns it.
fn with_policy(s: &Setup, name: &str, from: &str, policy: &str) -> String {
    s.repo
        .branch(name, from, &[(".config/sluice.toml", Some(policy))])
}

/// Set this host's own `branch` to `to`, as an older sluice left it,
/// past the protection.
fn set_copy(s: &Setup, branch: &str, to: &str) {
    s.repo.git(&[
        "-c",
        "core.hooksPath=/dev/null",
        "update-ref",
        &format!("refs/heads/{branch}"),
        to,
    ]);
}

/// S64, 124; record 0069: `bolt/x` is made on origin under a clean
/// policy, and this host's own `bolt/x` moved past the protection to a
/// commit adding a `[[checkout.cache]]` table, which sluice refuses. A
/// submission to `bolt/x` waits; the bolt's own rounds read origin's
/// revision, land the submission there, and level this host's `bolt/x`
/// to the landing.
#[test]
fn s64_the_bolts_round_reads_past_a_copy_that_lags() {
    let s = Setup::new("remote-s64-lags", "origin");
    let out = s.sluice(&["branch", "create", "bolt/x", "--from", "main"]);
    assert!(out.status.success(), "{}", text(&out));
    s.repo.git(&["fetch", "-q", "origin"]);
    let made = remote_rev(&s, "refs/heads/bolt/x");
    let cached = with_policy(&s, "cached", &made, &format!("{}{CACHE}", policy("origin")));
    set_copy(&s, "bolt/x", &cached);

    let out = s.submit_to("a", "origin/bolt/x", "bolt/x");
    assert!(out.status.success(), "{}", text(&out));
    step_bolt_until_out(&s);
    assert_eq!(bolt_states(&s), ["landed"]);
    let bolt = remote_rev(&s, "refs/heads/bolt/x");
    assert_eq!(s.repo.rev(&format!("{bolt}^1")), made, "formed on origin's");
    s.repo.git(&["cat-file", "-e", &format!("{bolt}:a.txt")]);
    assert_eq!(s.repo.rev("bolt/x"), bolt, "this host's copy is levelled");
}

/// 124, 56; record 0069: origin's `bolt/x` holds a `[[checkout.cache]]`
/// table while a submission waits; a round lands nothing, runs nothing,
/// moves nothing, and names `bolt/x`, origin's revision and the table. A
/// commit removing the table is pushed to origin's `bolt/x` by hand, this
/// host's own `bolt/x` left holding it, and the next rounds land the
/// submission on origin.
#[test]
fn a_policy_refused_on_the_remote_is_cleared_by_a_push() {
    let s = Setup::new("remote-refused-pushed", "origin");
    let out = s.sluice(&["branch", "create", "bolt/x", "--from", "main"]);
    assert!(out.status.success(), "{}", text(&out));
    s.repo.git(&["fetch", "-q", "origin"]);
    let out = s.submit_to("a", "origin/bolt/x", "bolt/x");
    assert!(out.status.success(), "{}", text(&out));
    let made = remote_rev(&s, "refs/heads/bolt/x");
    let cached = with_policy(&s, "cached", &made, &format!("{}{CACHE}", policy("origin")));
    set_remote(&s, "refs/heads/cached", "refs/heads/bolt/x");
    set_copy(&s, "bolt/x", &cached);

    let out = s.sluice(&["step", "bolt/x"]);
    assert!(!out.status.success(), "{}", text(&out));
    let said = text(&out);
    for words in ["bolt/x", &cached[..7], "[[checkout.cache]]"] {
        assert!(said.contains(words), "{words}: {said}");
    }
    assert_eq!(s.runs(), 0, "nothing ran");
    assert_eq!(remote_rev(&s, "refs/heads/bolt/x"), cached);
    assert_eq!(s.repo.rev("bolt/x"), cached);

    let fixed = with_policy(&s, "fixed", &cached, &policy("origin"));
    set_remote(&s, "refs/heads/fixed", "refs/heads/bolt/x");
    assert_eq!(
        s.repo.rev("bolt/x"),
        cached,
        "this host's copy still refused"
    );
    step_bolt_until_out(&s);
    assert_eq!(bolt_states(&s), ["landed"]);
    let bolt = remote_rev(&s, "refs/heads/bolt/x");
    assert_eq!(
        s.repo.rev(&format!("{bolt}^1")),
        fixed,
        "formed on the push"
    );
    s.repo.git(&["cat-file", "-e", &format!("{bolt}:a.txt")]);
}

/// `branch`'s part of `sluice overview --json` as `s` reads it.
fn overview_of(s: &Setup, branch: &str) -> serde_json::Value {
    let out = s.sluice(&["overview", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    v["branches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["branch"] == branch)
        .cloned()
        .unwrap_or_else(|| panic!("{branch} in {v}"))
}

/// 64, 124: with `main`'s records on its remote and nothing queued, the
/// overview names the revision its read found of `main` there, a commit
/// pushed to it by hand, beside the revision its records last saw.
#[test]
fn the_overview_names_a_hand_pushed_revision() {
    let s = Setup::signed("overview-hand-pushed");
    s.submit("a");
    s.step_until_out();
    assert_eq!(s.states(), ["landed"]);
    let before = overview_of(&s, "main");
    assert_eq!(before["revision"], s.remote_main().as_str(), "{before}");
    let other = Repo::clone_of(s.origin.path(), "overview-hand-pushed-b");
    let pushed = other.push_change("by-hand.txt", "pushed by hand\n");
    let after = overview_of(&s, "main");
    assert_eq!(after["revision"], pushed.as_str(), "{after}");
    assert_eq!(after["at"], before["at"], "{after}");
    assert_eq!(after["depth"], 0, "{after}");
}

/// A quiet branch mended outside sluice (124, 129; record 0046; rule
/// core.15): its records last saw a revision whose policy holds a
/// `[[checkout.cache]]` table, and a clean one is pushed to its remote by
/// another host. Nothing is queued on it, so no round runs.
mod quiet {
    use super::*;

    /// Another host's clone of origin, `branch` checked out.
    fn elsewhere(s: &Setup, name: &str, branch: &str) -> Repo {
        let other = Repo::clone_of(s.origin.path(), name);
        other.git(&["switch", "-q", branch]);
        other
    }

    /// `other`'s commit of `policy` on `branch`, pushed to origin: its
    /// revision.
    fn push_policy(other: &Repo, branch: &str, policy: &str) -> String {
        other.write(".config/sluice.toml", policy);
        other.git(&["commit", "-q", "-am", "the policy, by hand"]);
        other.git(&["push", "-q", "origin", &format!("HEAD:{branch}")]);
        other.rev("HEAD")
    }

    /// A records document of `branch`, as `doc` is written and with the
    /// version a write moves to.
    fn rewritten(doc: &str, base: &str) -> String {
        let mut v: serde_json::Value = serde_json::from_str(doc).unwrap();
        v["base"] = base.into();
        serde_json::to_string_pretty(&v).unwrap()
    }

    /// `branch`'s records in this host's files as a sluice that read
    /// `at`'s policy left them: a round stood on `at` last.
    fn last_saw_here(s: &Setup, branch: &str, at: &str) {
        let dir = s.repo.dir().join(".git/sluice").join(branch);
        let doc = std::fs::read_to_string(dir.join("branch.json")).unwrap();
        let version: u64 = std::fs::read_to_string(dir.join("version"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        std::fs::write(dir.join("branch.json"), rewritten(&doc, at)).unwrap();
        std::fs::write(dir.join("version"), format!("{}\n", version + 1)).unwrap();
    }

    /// `doc`'s `base`, and how many `moved` lines `log` holds.
    fn stood(doc: &str, log: &str) -> (String, usize) {
        let v: serde_json::Value = serde_json::from_str(doc).unwrap();
        let moved = log
            .lines()
            .filter(|l| l.contains("\"event\":\"moved\""))
            .count();
        (v["base"].as_str().unwrap_or_default().to_string(), moved)
    }

    /// `branch`'s records in this host's files: its `base`, and its log's
    /// `moved` lines.
    fn stood_here(s: &Setup, branch: &str) -> (String, usize) {
        let dir = s.repo.dir().join(".git/sluice").join(branch);
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap_or_default();
        stood(&read("branch.json"), &read("log.jsonl"))
    }

    /// The reflog of this host's `ref`, newest first.
    fn reflog(s: &Setup, name: &str) -> String {
        s.repo.git(&["reflog", "show", "--format=%H %gs", name])
    }

    /// remote-branch's "A quiet branch this host fetched is read whole by
    /// status" (124, 129; rule core.15): `main` on origin, its state in
    /// this host's files last seeing `r1`, whose policy holds a
    /// `[[checkout.cache]]` table, and this host's `origin/main` at `r2`,
    /// which takes it out. Status and the overview read `main` whole,
    /// nothing is fetched, nothing is written, and no round runs.
    #[test]
    fn a_quiet_branch_this_host_fetched_is_read_whole_by_status() {
        let s = Setup::new("quiet-fetched", "origin");
        s.submit("a");
        s.step_until_out();
        assert_eq!(s.states(), ["landed"]);
        let other = elsewhere(&s, "quiet-fetched-b", "main");
        let clean = std::fs::read_to_string(other.dir().join(".config/sluice.toml")).unwrap();
        let r1 = push_policy(&other, "main", &format!("{clean}{CACHE}"));
        s.repo.git(&["fetch", "-q", "origin"]);
        last_saw_here(&s, "main", &r1);
        let r2 = push_policy(&other, "main", &clean);
        s.repo.git(&["fetch", "-q", "origin"]);
        assert_eq!(s.repo.rev("origin/main"), r2);
        let fetched = reflog(&s, "refs/remotes/origin/main");
        let records = s.records();
        let files = |dir: &Path| -> Vec<(PathBuf, Vec<u8>)> {
            let mut out: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.is_file())
                .map(|p| (p.clone(), std::fs::read(&p).unwrap()))
                .collect();
            out.sort();
            out
        };
        let written = files(&records);
        let before = stood_here(&s, "main");
        assert_eq!(before.0, r1);

        let status = s.status();
        assert_eq!(status["at"], r1.as_str(), "{status}");
        let main = overview_of(&s, "main");
        assert!(main.get("unread").is_none(), "{main}");
        assert_eq!(main["at"], r1.as_str(), "{main}");
        assert_eq!(main["revision"], r2.as_str(), "{main}");

        assert_eq!(reflog(&s, "refs/remotes/origin/main"), fetched, "fetched");
        assert!(files(&records) == written, "the records were written");
        assert_eq!(stood_here(&s, "main"), before, "a round ran");
    }

    /// remote-branch's "A quiet branch this host never fetched takes a
    /// submit" (124, 129): `bolt/x` on origin, its state in this host's
    /// files last seeing `r1`, whose policy holds a `[[checkout.cache]]`
    /// table, origin's `bolt/x` at `r2`, which takes it out, and this
    /// host's `origin/bolt/x` still `r1`. Status refuses `bolt/x` naming
    /// `r1`; a submit fetches it and takes the submission; status after it
    /// reads `bolt/x` whole, the submission queued, and no round runs.
    #[test]
    fn a_quiet_branch_this_host_never_fetched_takes_a_submit() {
        let s = Setup::new("quiet-never-fetched", "origin");
        let out = s.sluice(&["branch", "create", "bolt/x", "--from", "main"]);
        assert!(out.status.success(), "{}", text(&out));
        s.repo.git(&["fetch", "-q", "origin"]);
        let out = s.submit_to("a", "origin/bolt/x", "bolt/x");
        assert!(out.status.success(), "{}", text(&out));
        step_bolt_until_out(&s);
        assert_eq!(bolt_states(&s), ["landed"]);
        let other = elsewhere(&s, "quiet-never-fetched-b", "bolt/x");
        let clean = std::fs::read_to_string(other.dir().join(".config/sluice.toml")).unwrap();
        let r1 = push_policy(&other, "bolt/x", &format!("{clean}{CACHE}"));
        s.repo.git(&["fetch", "-q", "origin"]);
        assert_eq!(s.repo.rev("origin/bolt/x"), r1);
        last_saw_here(&s, "bolt/x", &r1);
        let r2 = push_policy(&other, "bolt/x", &clean);
        assert_eq!(
            s.repo.rev("origin/bolt/x"),
            r1,
            "this host never fetched r2"
        );
        let before = stood_here(&s, "bolt/x");
        assert_eq!(before.0, r1);

        let out = s.sluice(&["status", "bolt/x"]);
        assert!(!out.status.success(), "{}", text(&out));
        let said = text(&out);
        assert!(
            said.contains(&r1) && said.contains("[[checkout.cache]]"),
            "{said}"
        );
        assert!(!said.contains(&r2), "{said}");

        let out = s.submit_to("b", "bolt/x", "bolt/x");
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(s.repo.rev("origin/bolt/x"), r2, "the submit fetched it");
        assert_eq!(bolt_states(&s), ["landed", "queued"]);
        assert_eq!(stood_here(&s, "bolt/x"), before, "a round ran");
    }
}

/// A worktree on `unit/<name>` cut from `base`, with one commit writing
/// `files`, submitted nowhere: its directory.
fn worktree(s: &Setup, name: &str, base: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = s.sessions.path().join(name);
    s.repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        &format!("unit/{name}"),
        dir.to_str().unwrap(),
        base,
    ]);
    for (path, content) in files {
        std::fs::write(dir.join(path), content).unwrap();
    }
    let at = dir.to_str().unwrap();
    s.repo.git(&["-C", at, "add", "-A"]);
    s.repo.git(&["-C", at, "commit", "-q", "-m", name]);
    dir
}

/// Every line of `trace`, a `GIT_TRACE` file, that runs a git command
/// asking a remote anything.
fn remote_calls(trace: &Path) -> Vec<String> {
    std::fs::read_to_string(trace)
        .unwrap_or_default()
        .lines()
        .filter(|l| {
            ["git fetch", "git ls-remote", "git push", "git-upload-pack"]
                .iter()
                .any(|c| l.contains(c))
        })
        .map(String::from)
        .collect()
}

/// 64, 129: beside a live reader keeping its tick, whose last read holds
/// `main`, with `#1` queued on it, and `bolt/y`, both on a remote, a
/// forecast of `bolt/y` onto `main` takes both from that read and asks
/// the remote nothing: it forecasts as the read says with the remote gone.
#[test]
fn a_forecast_beside_a_current_reader_asks_the_remote_nothing() {
    let s = Setup::new("forecast-reader", "origin");
    s.repo.git(&["branch", "bolt/y", "main"]);
    s.repo
        .git(&["push", "-q", "origin", "bolt/y:refs/heads/bolt/y"]);
    s.repo.git(&["fetch", "-q", "origin"]);
    let out = s.submit_files("one", "main", "main", &[("shared.txt", "one\n")]);
    assert!(out.status.success(), "{}", text(&out));
    let out = s.submit_files("y", "bolt/y", "bolt/y", &[("shared.txt", "two\n")]);
    assert!(out.status.success(), "{}", text(&out));
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let out = s.sluice(&["step", "bolt/y"]);
        assert!(out.status.success(), "{}", text(&out));
        let out = s.sluice(&["status", "bolt/y", "--json", "--all"]);
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        if v["submissions"][0]["state"] == "landed" {
            break;
        }
        assert!(Instant::now() < deadline, "{v}");
        std::thread::sleep(Duration::from_millis(200));
    }

    let out = s.sluice(&["overview", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let mut doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    doc["tick"] = 3600.into();
    let part = |name: &str| {
        doc["branches"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["branch"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("{name} in {doc}"))
    };
    let (main, bolt) = (part("main"), part("bolt/y"));
    assert!(
        main["revision"].is_string() && bolt["revision"].is_string(),
        "{doc}"
    );
    let repository = doc["repository"].as_str().unwrap().to_string();
    let readers = s.state.path().join("sluice/readers");
    let mut reader =
        match sluice::reader::Reader::take(&readers, &repository, sluice::VERSION).unwrap() {
            sluice::reader::Reader::Lead(lead) => lead,
            sluice::reader::Reader::Follow(_) => panic!("a reader of {repository} already"),
        };
    reader.write(&doc).unwrap();
    let aside = s.origin.path().with_extension("aside");
    std::fs::rename(s.origin.path(), &aside).unwrap();

    let trace = s.seen.path().join("trace");
    let out = s
        .command(s.repo.dir())
        .args(["submit", "main", "--check", "--head", "bolt/y", "--json"])
        .env("GIT_TRACE", &trace)
        .output()
        .unwrap();
    std::fs::rename(&aside, s.origin.path()).unwrap();
    assert_eq!(out.status.code(), Some(sluice::CONFLICT), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["onto"], main["revision"], "{v}");
    assert_eq!(v["head"], bolt["revision"], "{v}");
    assert_eq!(v["ahead"], serde_json::json!([1]), "{v}");
    assert_eq!(
        v["conflict"]["paths"],
        serde_json::json!(["shared.txt"]),
        "{v}"
    );
    assert_eq!(remote_calls(&trace), Vec::<String>::new());
}

/// 64, 124: with no reader on this host, a forecast reads `main` as a
/// round does: the work another host submitted to it, which this host
/// does not hold, is fetched, and the forecast is formed against it.
#[test]
fn a_forecast_with_no_reader_fetches_another_hosts_work() {
    let s = Setup::signed("forecast-other-host");
    let other = Repo::clone_of(s.origin.path(), "forecast-other-host-b");
    let config = Scratch::new("forecast-other-host-b-config");
    let state = Scratch::new("forecast-other-host-b-state");
    std::fs::create_dir_all(config.path().join("sluice")).unwrap();
    std::fs::write(
        config.path().join("sluice/config.toml"),
        format!(
            "[signing]\nsigner = \"a\"\nkey = {:?}\n\n[peers]\nname = \"b\"\n",
            s.config.path().join("signing").display().to_string()
        ),
    )
    .unwrap();
    let sessions = Scratch::new("forecast-other-host-b-sessions");
    let dir = sessions.path().join("b");
    other.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "unit/b",
        dir.to_str().unwrap(),
        "main",
    ]);
    std::fs::write(dir.join("shared.txt"), "b's\n").unwrap();
    let at = dir.to_str().unwrap();
    other.git(&["-C", at, "add", "-A"]);
    other.git(&["-C", at, "commit", "-q", "-m", "b"]);
    let out = s
        .command(&dir)
        .env("XDG_CONFIG_HOME", config.path())
        .env("XDG_STATE_HOME", state.path())
        .args(["submit", "main", "--no-wait", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let theirs: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = theirs["id"].as_u64().unwrap();
    let head = other.rev("unit/b");
    assert!(
        !s.repo
            .git_raw(&["cat-file", "-e", &format!("{head}^{{commit}}")])
            .status
            .success(),
        "b's work is on this host already"
    );

    let mine = worktree(&s, "a", "main", &[("shared.txt", "a's\n")]);
    let out = s
        .command(&mine)
        .args(["submit", "main", "--check", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::CONFLICT), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ahead"], serde_json::json!([id]), "{v}");
    assert_eq!(v["conflict"]["with"][0]["id"], id, "{v}");
    assert_eq!(
        v["conflict"]["paths"],
        serde_json::json!(["shared.txt"]),
        "{v}"
    );
}

/// A branch whose policy this sluice refuses is refused alone (26, S64).
mod refused_alone {
    use super::*;

    /// The id a `submit --no-wait --json` answered with.
    fn id(out: &Output) -> String {
        assert!(out.status.success(), "{}", text(out));
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        v["id"].to_string()
    }

    /// The host's own `branch` moved, past the protection, to a commit
    /// whose policy is `edit` of the one it holds. The commit is made in a
    /// worktree of its own, so the repository's HEAD still names its
    /// default branch.
    fn own_policy(s: &Setup, branch: &str, edit: impl Fn(&str) -> String) {
        let dir = s
            .sessions
            .path()
            .join(format!("refused-{}", branch.replace('/', "-")));
        let dir = dir.to_str().unwrap();
        s.repo
            .git(&["worktree", "add", "-q", "--detach", dir, branch]);
        let policy = Path::new(dir).join(".config/sluice.toml");
        let text = std::fs::read_to_string(&policy).unwrap();
        std::fs::write(&policy, edit(&text)).unwrap();
        s.repo
            .git(&["-C", dir, "commit", "-q", "-am", "a refused policy"]);
        let at = s.repo.git(&["-C", dir, "rev-parse", "HEAD"]);
        s.repo.git(&[
            "-c",
            "core.hooksPath=/dev/null",
            "update-ref",
            &format!("refs/heads/{branch}"),
            &at,
        ]);
    }

    /// The host's own `branch` and origin's moved, past the protection, to
    /// a commit whose policy adds a `[[checkout.cache]]` table, which this
    /// sluice refuses: the host's own copy holds it as on deneb (S64), and
    /// origin's, since the routing passes over the own copy's other fields
    /// and reads the policy whole on the remote (record 0069), so the
    /// branch is refused whichever copy a read takes.
    fn refuse(s: &Setup, branch: &str) {
        own_policy(s, branch, |p| format!("{p}{CACHE}"));
        s.repo.git(&[
            "-c",
            "core.hooksPath=/dev/null",
            "push",
            "-q",
            "origin",
            &format!("{branch}:refs/heads/{branch}"),
        ]);
    }

    /// The record a `sluice wait --json` ended with.
    fn waited(out: &Output) -> serde_json::Value {
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", text(out)))
    }

    /// 26, S64, rule core.11: the deneb case. The host's own ref of
    /// `bolt/b`, a branch with state, holds a policy this sluice refuses,
    /// and origin's as well; `sluice wait` on a submission to `main` finds
    /// it past `bolt/b` and follows it to its landing.
    #[test]
    fn a_wait_on_main_passes_a_bolt_whose_own_ref_is_refused() {
        let s = Setup::new("refused-alone-wait", "origin");
        s.repo.git(&["branch", "bolt/b", "main"]);
        s.repo
            .git(&["push", "-q", "origin", "bolt/b:refs/heads/bolt/b"]);
        s.repo.git(&["fetch", "-q", "origin"]);
        id(&s.submit_to("b", "bolt/b", "bolt/b"));
        let on_main = id(&s.submit_to("a", "main", "main"));
        refuse(&s, "bolt/b");
        let out = s.sluice(&["status", "bolt/b"]);
        assert!(!out.status.success(), "bolt/b still reads: {}", text(&out));

        let out = s.sluice(&["wait", &on_main, "--json", "--timeout", "2m"]);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out));
        let v = waited(&out);
        assert_eq!(v["id"].to_string(), on_main, "{v}");
        assert_eq!(v["state"], "landed", "{v}");
    }

    /// 26, S64: `bolt/b`'s policy refused on the host's own ref and on
    /// origin's, so whichever copy a round reads refuses it. A bounded
    /// `sluice run` lands the submission on `main`, and, its last round
    /// having passed `bolt/b` by, exits 3 as a step of `bolt/b` would,
    /// naming it on stderr with the refusal's words.
    #[test]
    fn a_bounded_run_lands_main_and_exits_3_naming_the_refused_bolt() {
        let s = Setup::new("refused-alone-run", "origin");
        s.repo.git(&["branch", "bolt/b", "main"]);
        s.repo
            .git(&["push", "-q", "origin", "bolt/b:refs/heads/bolt/b"]);
        s.repo.git(&["fetch", "-q", "origin"]);
        id(&s.submit_to("b", "bolt/b", "bolt/b"));
        let on_main = id(&s.submit_to("a", "main", "main"));
        refuse(&s, "bolt/b");
        let out = s.sluice(&["status", "bolt/b"]);
        assert!(!out.status.success(), "bolt/b still reads: {}", text(&out));

        let out = s.sluice(&["run", "--rounds", "2"]);
        assert_eq!(out.status.code(), Some(3), "{}", text(&out));
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.lines().any(|l| l.starts_with("sluice: bolt/b: ")
                && l.contains("Remove [[checkout.cache]] from .config/sluice.toml")),
            "{}",
            text(&out)
        );
        let out = s.sluice(&["status", &on_main, "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["state"], "landed", "{v}");
    }

    /// 26: the host's own `main`, the default branch, holds a policy this
    /// sluice refuses where the routing reads it, `main`'s remote no
    /// remote's name (record 0069), while the state is on origin and this
    /// host has read it before. The branches are listed all the same:
    /// `sluice wait` on a submission to `bolt/a` follows it to its
    /// landing, and the overview shows `main` with its refusal and `bolt/a`
    /// whole.
    #[test]
    fn a_refused_main_hides_no_branch_this_host_has_read() {
        let s = Setup::signing("refused-alone-main", &["main", "bolt/*"]);
        s.submit("m");
        s.step_until_out();
        assert_eq!(s.states(), ["landed"]);
        s.repo.git(&["branch", "bolt/a", "main"]);
        s.repo
            .git(&["push", "-q", "origin", "bolt/a:refs/heads/bolt/a"]);
        s.repo.git(&["fetch", "-q", "origin"]);
        let on_bolt = id(&s.submit_to("a", "bolt/a", "bolt/a"));
        own_policy(&s, "main", |p| {
            p.replacen("remote = \"origin\"", "remote = 1", 1)
        });
        let out = s.sluice(&["status", "main"]);
        assert!(!out.status.success(), "main still reads: {}", text(&out));

        let out = s.sluice(&["wait", &on_bolt, "--json", "--timeout", "2m"]);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out));
        let v = waited(&out);
        assert_eq!(v["id"].to_string(), on_bolt, "{v}");
        assert_eq!(v["state"], "landed", "{v}");

        let out = s.sluice(&["overview", "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let part = |name: &str| {
            v["branches"]
                .as_array()
                .unwrap()
                .iter()
                .find(|b| b["branch"] == name)
                .cloned()
                .unwrap_or_else(|| panic!("{name} in {v}"))
        };
        assert!(
            part("main")["unread"]
                .as_str()
                .is_some_and(|w| w.contains("remote must be the name of a git remote")),
            "{v}"
        );
        assert!(part("bolt/a").get("unread").is_none(), "{v}");
    }
}

/// The git `s`'s binary started for `args`, run once in the repository
/// with its pauses between rounds skipped.
fn started(s: &Setup, args: &[&str]) -> Vec<String> {
    let before = s.git.count();
    let out = s
        .command(s.repo.dir())
        .env("SLUICE_ROUND_SLEEP", "record")
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}: {}", text(&out));
    s.git.starts()[before..].to_vec()
}

/// reader-tick, "A branch on a remote with its records on the host":
/// with nothing queued, `sluice run` starts git no more over six passes
/// than over one, and fetches nothing after its first (129).
#[test]
fn an_idle_branch_on_a_remote_is_passed_over_with_no_git() {
    let s = Setup::new("idle-remote", "origin");
    s.submit("a");
    s.step_until_out();
    let one = started(&s, &["run", "--rounds", "1"]);
    let six = started(&s, &["run", "--rounds", "6"]);
    assert_eq!(six.len(), one.len(), "one: {one:?}; six: {six:?}");
    let fetches = |starts: &[String]| starts.iter().filter(|l| l.contains("fetch")).count();
    assert_eq!(fetches(&six), fetches(&one), "{six:?}");
}

/// reader-tick, "A branch on a remote moved there and nothing in play":
/// another host lands on `origin`'s `main`; a watch shows `main` at its
/// revision as last fetched, fetching nothing, and the next submission's
/// round fetches `main` before it forms anything (121, 129).
#[test]
fn a_branch_moved_on_its_remote_is_shown_as_last_fetched_until_a_round() {
    let s = Setup::new("moved-remote", "origin");
    s.submit("a");
    s.step_until_out();
    let landed = s.remote_main();
    let pushed = Repo::clone_of(s.origin.path(), "moved-elsewhere").push_change("b.txt", "b\n");
    assert_eq!(s.remote_main(), pushed);

    let mut watch = s
        .command(s.repo.dir())
        .args(["overview", "--json", "--watch"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut first = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(watch.stdout.take().unwrap()),
        &mut first,
    )
    .unwrap();
    let _ = watch.kill();
    let _ = watch.wait();
    let doc: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(doc["branches"][0]["branch"], "main", "{doc}");
    assert_eq!(doc["branches"][0]["at"], landed.as_str(), "{doc}");
    assert_eq!(
        s.repo.rev("refs/remotes/origin/main"),
        landed,
        "the watch fetched"
    );

    s.submit("c");
    s.step_until_out();
    let out = support::git_command()
        .arg("--git-dir")
        .arg(s.origin.path())
        .args(["merge-base", "--is-ancestor", &pushed, "refs/heads/main"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{} is not on the landing",
        &pushed[..7]
    );
    assert_eq!(s.states(), ["landed", "landed"]);
}
