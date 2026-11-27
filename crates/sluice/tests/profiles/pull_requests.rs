//! Pull requests into a branch on GitHub (C.5 pull request, C.7
//! `pull_requests`), end to end through the real binaries: `sluice step`
//! and `wt submit` against a bare repository standing for GitHub's git
//! and a fake of GitHub's API answering from a file, on a port the system
//! picks. One test per scenario of the change's specs: intake, status and
//! submit. Needs git and wt, so it is in the check-tools tier.

#![cfg(feature = "tools")]

#[path = "../support/mod.rs"]
mod support;

#[path = "forge/github.rs"]
mod github;

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use sluice::profiles::github::forge::{Forge as _, Posting, Status};
use support::{Repo, Scratch};

const KEY: &[u8] = include_bytes!("forge/test-key.pem");

/// The check: fails while a file named `broken` exists, printing a
/// token-shaped line and a last line, and takes five seconds while one
/// named `slow` does. While one named `hang` names a file that does not
/// exist, it writes its process's id there and goes on until it is ended.
const CHECK: &str = "if test -e hang && ! test -e \"$(cat hang)\"; then \
                     echo $$ > \"$(cat hang)\"; exec sleep 300; fi\n\
                     if test -e slow; then sleep 5; fi\n\
                     if test -e broken; then echo GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123; \
                     echo it broke; fi\ntest ! -e broken\n";

/// The policy's table for `bolt/*`, branches on GitHub sluice lands on
/// that take no pull requests.
const BOLTS: &str = "\n[branches.\"bolt/*\"]\nremote = \"origin\"\ntimeout = \"1m\"\n";

/// `main` on GitHub, taking pull requests with `conditions`, the lines of
/// its `pull_requests` table.
struct OnGitHub {
    repo: Repo,
    bare: Scratch,
    config: Scratch,
    state: Scratch,
    gh: github::GitHub,
    _files: Scratch,
    /// The `[signing]` table of host `b`, whose signer the policy names
    /// beside this host's `a` (C.6; design D28).
    b_signing: String,
}

/// Its tests end while a run may go: its host process is stopped as it
/// ends, which ends the runs (`a-leaked-child-fails-its-test@9a82a5147ba6`).
impl Drop for OnGitHub {
    fn drop(&mut self) {
        support::guard::stop_host_processes(self.state.path());
    }
}

/// `policy` naming signers `a` and `b` by `keys`, and each branch's own
/// table trusting both, so every branch with a remote keeps its records
/// there (C.6; 78; design D28). A policy that names signers already is
/// left as it is.
fn signed(policy: &str, keys: &[ssh_key::PrivateKey; 2]) -> String {
    if policy.contains("[signers.") {
        return policy.to_string();
    }
    let line =
        |p: &str, k: &ssh_key::PrivateKey| format!("{p} {}", k.public_key().to_openssh().unwrap());
    let signers = format!(
        "[signers.a]\nssh = \"{}\"\n\n[signers.b]\nssh = \"{}\"\n\n",
        line("a", &keys[0]),
        line("b", &keys[1])
    );
    let lines: Vec<&str> = policy.lines().collect();
    let mut out = String::new();
    let mut named = false;
    for (i, l) in lines.iter().enumerate() {
        let table = l.starts_with("[branches.") && !l.ends_with(".pull_requests]");
        if l.starts_with("[branches.") && !named {
            out.push_str(&signers);
            named = true;
        }
        out.push_str(l);
        out.push('\n');
        if table {
            let own = lines[i + 1..]
                .iter()
                .take_while(|n| !n.starts_with('['))
                .any(|n| n.starts_with("trust"));
            if !own {
                out.push_str("trust = [\"a\", \"b\"]\n");
            }
        }
    }
    out
}

/// The `[signing]` table of signer `name`, its key `key` written to `dir`.
fn signing(name: &str, key: &ssh_key::PrivateKey, dir: &Path) -> String {
    let path = dir.join(format!("{name}.signing"));
    key.write_openssh_file(&path, ssh_key::LineEnding::LF)
        .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    format!(
        "[signing]\nsigner = \"{name}\"\nkey = {:?}\n",
        path.display().to_string()
    )
}

impl OnGitHub {
    fn new(name: &str, conditions: &str) -> OnGitHub {
        OnGitHub::checked(name, conditions, "1m", "")
    }

    /// `main` on GitHub, and `bolt/x` beside it, a branch on GitHub sluice
    /// lands on, made from `main` and taking no pull requests.
    fn with_bolt(name: &str) -> OnGitHub {
        let g = OnGitHub::checked_policy(name, "", "1m", "", BOLTS);
        g.repo.git(&["branch", "bolt/x", "main"]);
        g.repo
            .git(&["push", "-q", "origin", "bolt/x:refs/heads/bolt/x"]);
        g
    }

    /// `main` checked by `check.sh` with the time limit `timeout`, and
    /// `lines` in its table beside its remote and time limit.
    fn checked(name: &str, conditions: &str, timeout: &str, lines: &str) -> OnGitHub {
        OnGitHub::checked_policy(name, conditions, timeout, lines, "")
    }

    /// [`OnGitHub::checked`], with `extra` after its tables in the policy.
    fn checked_policy(
        name: &str,
        conditions: &str,
        timeout: &str,
        lines: &str,
        extra: &str,
    ) -> OnGitHub {
        OnGitHub::with(
            name,
            &format!(
                "[tests]\nrunner = \"command\"\ncommand = \"sh check.sh\"\n\n\
                 [branches.main]\nremote = \"origin\"\ntimeout = \"{timeout}\"\n{lines}\n\
                 [branches.main.pull_requests]\n{conditions}\n{extra}"
            ),
            &[("check.sh", CHECK)],
        )
    }

    /// `main` on GitHub under `policy`, its tree holding `files` beside it.
    /// The policy names signers `a` and `b`, with keys made for the
    /// fixture, and this host signs as `a`, so every branch with a remote
    /// keeps its records there and this host keeps `main` (C.6; 137;
    /// design D28).
    fn with(name: &str, policy: &str, files: &[(&str, &str)]) -> OnGitHub {
        OnGitHub::kept_as(name, policy, files, true)
    }

    /// `main` on GitHub taking pull requests under a policy that names no
    /// signers, so its records are each host's own and main is refused
    /// alone (137; design D22).
    fn unkept(name: &str, extra: &str) -> OnGitHub {
        OnGitHub::kept_as(
            name,
            &format!(
                "[tests]\nrunner = \"command\"\ncommand = \"sh check.sh\"\n\n\
                 [branches.main]\nremote = \"origin\"\ntimeout = \"1m\"\n\n\
                 [branches.main.pull_requests]\n{extra}"
            ),
            &[("check.sh", CHECK)],
            false,
        )
    }

    /// [`OnGitHub::with`], the policy naming signers where `signers` says.
    fn kept_as(name: &str, policy: &str, files: &[(&str, &str)], signers: bool) -> OnGitHub {
        use ssh_key::{Algorithm, PrivateKey};
        let keys = [
            PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap(),
            PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap(),
        ];
        let repo = Repo::new(name);
        let policy = match signers {
            true => signed(policy, &keys),
            false => policy.to_string(),
        };
        repo.write(".config/sluice.toml", &policy);
        for (path, content) in files {
            repo.write(path, content);
        }
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "sluice's policy"]);
        let bare = repo.bare_origin();
        repo.git(&["remote", "set-url", "origin", "https://github.com/o/r.git"]);
        repo.git(&[
            "config",
            &format!("url.{}.insteadOf", bare.path().display()),
            "https://github.com/o/r.git",
        ]);
        let config = Scratch::new(&format!("{name}-config"));
        let dir = config.path().join("sluice");
        std::fs::create_dir_all(&dir).unwrap();
        let key = dir.join("app.pem");
        std::fs::write(&key, KEY).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        std::fs::write(
            dir.join("config.toml"),
            format!(
                "{}\n[runs]\nmax = 2\n\n[forge]\napp_id = {}\nkey = {:?}\n",
                signing("a", &keys[0], &dir),
                github::APP,
                key.display().to_string()
            ),
        )
        .unwrap();
        let b_signing = signing("b", &keys[1], &dir);
        let files = Scratch::new(&format!("{name}-github"));
        let gh = github::GitHub::start(&github::file(files.path()), Some(bare.path()));
        OnGitHub {
            repo,
            bare,
            config,
            state: Scratch::new(&format!("{name}-state")),
            gh,
            _files: files,
            b_signing,
        }
    }

    /// `program` in `dir`, this setup's host. The host process its
    /// commands start passes only when asked, so the rounds that run are
    /// the ones the test's commands ask for (design D14).
    fn command(&self, program: &str, dir: &Path) -> Command {
        let bin = Path::new(env!("CARGO_BIN_EXE_sluice")).parent().unwrap();
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
        let mut c = Command::new(program);
        support::isolated(&mut c)
            .current_dir(dir)
            .env("PATH", path)
            .env("SLUICE_ROUND_SLEEP", "record")
            .env("XDG_CONFIG_HOME", self.config.path())
            .env("XDG_STATE_HOME", self.state.path())
            .env("SLUICE_FORGE_API", self.gh.url())
            .env_remove("SLUICE_AGENT")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null());
        c
    }

    fn sluice(&self, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_sluice"), self.repo.dir())
            .args(args)
            .output()
            .unwrap()
    }

    /// One round of `main`, its pull requests taken in first.
    fn step(&self) {
        let said = self.round();
        assert!(!said.contains("warning"), "{said}");
    }

    /// One round of `main`, its pull requests taken in first: what it
    /// said, warnings and all. The host's interval has passed since it
    /// last asked GitHub, as it has for a test that changes GitHub as a
    /// person would and then steps.
    fn round(&self) -> String {
        self.discovery_passes();
        let out = self.sluice(&["step", "main"]);
        assert!(out.status.success(), "{}", text(&out));
        text(&out)
    }

    /// The host's `[forge] discovery` has passed since it last asked
    /// GitHub for any branch's open pull requests (136): its next pass
    /// asks.
    fn discovery_passes(&self) {
        let dir = self.state.path().join("sluice/forge/requests/o/r");
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let mut v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            v["asked"] = Value::Null;
            std::fs::write(&path, v.to_string()).unwrap();
        }
    }

    /// `main`'s log, a document a line, as its records on the remote hold
    /// it (C.6).
    fn log(&self) -> Vec<Value> {
        let out = Command::new("git")
            .args(["--git-dir", self.bare.path().to_str().unwrap()])
            .args(["show", "refs/sluice/state/main:log.jsonl"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// `main`'s submissions.
    fn subs(&self) -> Vec<Value> {
        let out = self.sluice(&["status", "main", "--json", "--all"]);
        assert!(out.status.success(), "{}", text(&out));
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        v["submissions"].as_array().cloned().unwrap_or_default()
    }

    /// 51, D13: submission `id`'s `delivery`, as `sluice status <id>
    /// --json` gives it; null where it has none.
    fn delivery(&self, id: &Value) -> Value {
        let out = self.sluice(&["status", &id.to_string(), "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        v["delivery"].clone()
    }

    /// The log's lines of `event` naming submission `id`.
    fn logged(&self, event: &str, id: &Value) -> Vec<Value> {
        self.log()
            .into_iter()
            .filter(|l| l["event"] == event && l["id"] == *id)
            .collect()
    }

    /// The submissions from pull request `n`.
    fn of(&self, n: u64) -> Vec<Value> {
        let place = format!("pull/{n}");
        self.subs()
            .into_iter()
            .filter(|s| s["from"]["worktree"] == place.as_str())
            .collect()
    }

    /// A branch of the repository off `main` with one commit of `changes`,
    /// pushed to GitHub; returns its head.
    fn push_branch(&self, name: &str, changes: &[(&str, Option<&str>)]) -> String {
        let head = self.repo.branch(name, "main", changes);
        self.repo
            .git(&["push", "-q", "origin", &format!("{name}:refs/heads/{name}")]);
        head
    }

    /// A person opens a pull request into `main` from `name`, a branch of
    /// the repository with one commit of `changes`; returns its number and
    /// head.
    fn by_hand(&self, name: &str, changes: &[(&str, Option<&str>)], draft: bool) -> (u64, String) {
        let head = self.push_branch(name, changes);
        let n = self
            .gh
            .forge
            .open_by("person", "main", name, &head, draft, None)
            .unwrap();
        (n, head)
    }

    /// GitHub serves no head for pull request `n`: its branch is gone,
    /// and `refs/pull/<n>/head` with it.
    fn no_head(&self, n: u64) {
        let pull = self.gh.forge.pull(&github::repo(), n).unwrap().unwrap();
        github::delete_ref(self.bare.path(), &format!("refs/heads/{}", pull.head_ref));
        github::delete_ref(self.bare.path(), &format!("refs/pull/{n}/head"));
    }

    /// The report comment on pull request `n`.
    fn report(&self, n: u64) -> Option<String> {
        self.comments(n)
            .into_iter()
            .find(|c| c.starts_with("<!-- sluice-report: main -->"))
    }

    fn comments(&self, n: u64) -> Vec<String> {
        self.gh
            .forge
            .held()
            .unwrap()
            .comments
            .get(&n)
            .into_iter()
            .flatten()
            .map(|c| c.body.clone())
            .collect()
    }

    /// Step until `done` holds, at most a minute.
    fn step_until(&self, what: &str, done: impl Fn(&OnGitHub) -> bool) {
        let until = Instant::now() + Duration::from_secs(60);
        while !done(self) {
            assert!(Instant::now() < until, "never {what}: {:?}", self.subs());
            self.step();
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// GitHub's `main`.
    fn main_on_github(&self) -> String {
        github::rev(self.bare.path(), "refs/heads/main").unwrap()
    }

    /// A session's worktree on `unit/<name>` off `main`, with one commit
    /// of `changes`.
    fn session(&self, name: &str, changes: &[(&str, &str)]) -> PathBuf {
        let dir = self.worktree(name);
        self.commit_in(&dir, changes);
        dir
    }

    /// A session's worktree on `unit/<name>` off `main`, with no commit.
    fn worktree(&self, name: &str) -> PathBuf {
        self.worktree_on(name, &format!("unit/{name}"))
    }

    /// A worktree named for `name` on the new branch `branch` off `main`,
    /// with no commit.
    fn worktree_on(&self, name: &str, branch: &str) -> PathBuf {
        let dir = self.repo.dir().with_extension(format!("s-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        self.repo.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            branch,
            dir.to_str().unwrap(),
            "main",
        ]);
        dir
    }

    fn commit_in(&self, dir: &Path, changes: &[(&str, &str)]) -> String {
        for (p, c) in changes {
            std::fs::write(dir.join(p), c).unwrap();
        }
        let git = |args: &[&str]| {
            let mut a = vec!["-C", dir.to_str().unwrap()];
            a.extend(args);
            self.repo.git(&a)
        };
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", &format!("Change {}", changes[0].0)]);
        git(&["rev-parse", "HEAD"])
    }

    fn wt_submit(&self, dir: &Path, args: &[&str]) -> Output {
        self.command("wt", dir)
            .args(["submit", "main"])
            .args(args)
            .output()
            .unwrap()
    }

    /// The pull request from `head_ref`, as GitHub holds it now.
    fn pull_from(&self, head_ref: &str) -> sluice::profiles::github::forge::Pull {
        self.gh
            .forge
            .by_ref(head_ref)
            .unwrap()
            .unwrap_or_else(|| panic!("no pull request from {head_ref}"))
    }

    /// GitHub's `bolt/x`.
    fn bolt_on_github(&self) -> String {
        github::rev(self.bare.path(), "refs/heads/bolt/x").unwrap()
    }

    /// One submission of `changes`, from a worktree of its own off
    /// `bolt/x`, landed on `bolt/x` by its rounds; returns GitHub's
    /// `bolt/x` after it.
    fn land_on_bolt(&self, name: &str, changes: &[(&str, &str)]) -> String {
        let dir = self.repo.dir().with_extension(format!("b-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        let before = self.bolt_on_github();
        self.repo.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            &format!("unit/{name}"),
            dir.to_str().unwrap(),
            &before,
        ]);
        self.commit_in(&dir, changes);
        let out = self
            .command(env!("CARGO_BIN_EXE_sluice"), &dir)
            .args(["submit", "bolt/x", "--no-wait"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        let until = Instant::now() + Duration::from_secs(60);
        while self.bolt_on_github() == before {
            assert!(Instant::now() < until, "never landed on bolt/x");
            let out = self.sluice(&["step", "bolt/x"]);
            assert!(out.status.success(), "{}", text(&out));
            std::thread::sleep(Duration::from_millis(50));
        }
        self.bolt_on_github()
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

// ---------------------------------------------------------------------------
// pull-request-intake
// ---------------------------------------------------------------------------

/// C.7: a policy with `pull_requests` and no `remote` is refused, naming
/// the branch and `remote`.
#[test]
fn pull_requests_need_a_remote() {
    let repo = Repo::new("pr-no-remote");
    repo.write(
        ".config/sluice.toml",
        "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\n\
         [branches.main]\n[branches.main.pull_requests]\nreviews = 1\n",
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    let config = Scratch::new("pr-no-remote-config");
    let out = support::isolated(&mut Command::new(env!("CARGO_BIN_EXE_sluice")))
        .current_dir(repo.dir())
        .args(["step", "main"])
        .env("XDG_CONFIG_HOME", config.path())
        .env("XDG_STATE_HOME", config.path())
        .env_remove("GIT_DIR")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let said = text(&out);
    assert!(
        said.contains("main") && said.contains("needs remote"),
        "{said}"
    );
}

/// Another queue of `main` lands `head`: a merge commit onto main on
/// GitHub, as a second copy of the repository on the host lands it, which
/// GitHub reads as the pull request merged. Returns the landing.
fn landed_elsewhere(g: &OnGitHub, head: &str) -> String {
    let git = |args: &[&str]| g.on_github(args);
    let main = g.main_on_github();
    let tree = git(&["merge-tree", "--write-tree", &main, head]);
    let landing = git(&[
        "commit-tree",
        &tree,
        "-p",
        &main,
        "-p",
        head,
        "-m",
        "chore(land): elsewhere",
    ]);
    git(&["update-ref", "refs/heads/main", &landing, &main]);
    landing
}

/// pull-request-status, output as asked (ADR 0044): the last lines of a
/// failed check's output, scrubbed.
#[test]
fn output_posted_as_asked_and_scrubbed() {
    let g = OnGitHub::new("pr-report-output", "log_lines = 2");
    let (n, _) = g.by_hand("unit/broken", &[("broken", Some("x\n"))], false);
    g.step_until("blamed", |g| {
        g.of(n).first().is_some_and(|s| s["state"] == "failed")
    });
    g.step();
    let r = g.report(n).unwrap();
    assert!(r.contains("it broke"), "{r}");
    assert!(r.contains("[redacted]") && !r.contains("ghp_"), "{r}");
}

impl OnGitHub {
    /// What sluice's host process said in this host's state: its log and
    /// the log of the process before it.
    fn host_log(&self) -> String {
        let host = self.state.path().join("sluice/host");
        ["log.1", "log"]
            .iter()
            .map(|f| std::fs::read_to_string(host.join(f)).unwrap_or_default())
            .collect()
    }
}

/// 133, rule core.28 (design D27): GitHub serves no head for a pull
/// request; the host process's passes say the cause once in its log, and
/// main's log holds its `read-refused` once, marked `intake`; a host
/// process started afresh says nothing of it again.
#[test]
fn a_standing_intake_failure_is_said_once_for_each_cause_and_host() {
    let g = OnGitHub::new("pr-intake-once", "");
    g.named("a", "");
    let (n, _) = g.by_hand("unit/headless", &[("h.txt", Some("h\n"))], false);
    g.no_head(n);
    g.on_main();
    let line = format!("sluice: warning: taking in main's pull requests: pull/{n}: ");
    let logged = |g: &OnGitHub| -> usize {
        g.log()
            .into_iter()
            .filter(|l| l["event"] == "read-refused" && l["intake"] == true)
            .filter(|l| l["asked"] == format!("pull/{n}").as_str())
            .count()
    };
    let mut said = String::new();
    for _ in 0..2 {
        g.discovery_passes();
        let out = g.skipping(g.repo.dir(), &["run", "--rounds", "3"]);
        assert!(out.status.success(), "{}", text(&out));
        said.push_str(&text(&out));
    }
    assert_eq!(
        g.host_log().matches(&line).count(),
        1,
        "{}\n{said}\n{:?}",
        g.host_log(),
        g.log()
    );
    assert_eq!(logged(&g), 1);
    support::kill_host_process(&mut g.command(env!("CARGO_BIN_EXE_sluice"), g.repo.dir()));
    g.discovery_passes();
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "2"]);
    assert!(out.status.success(), "{}", text(&out));
    // The host process started afresh writes `log`, and the start keeps
    // only the log before it as `log.1`. A host process can end idle
    // between the two runs above, so the log that says the cause can be
    // gone: this reads the new host process's own log alone.
    let own = std::fs::read_to_string(g.state.path().join("sluice/host/log")).unwrap();
    assert_eq!(own.matches(&line).count(), 0, "{own}");
    assert_eq!(logged(&g), 1);
}

/// 133 (design D27): while this host's passes cannot take in one of
/// main's pull requests, `sluice status main` says so above the queue, and
/// `--json` carries it as `intake_refused`; once a pass asks GitHub and
/// meets nothing, it says nothing of it.
#[test]
fn status_shows_this_hosts_standing_intake_refusal() {
    let g = OnGitHub::new("pr-intake-status", "");
    g.named("a", "");
    let (n, _) = g.by_hand("unit/headless", &[("h.txt", Some("h\n"))], false);
    g.no_head(n);
    g.on_main();
    for _ in 0..2 {
        g.discovery_passes();
        let out = g.skipping(g.repo.dir(), &["run", "--rounds", "1"]);
        assert!(out.status.success(), "{}", text(&out));
    }
    let words = g.status_words();
    let line = "this host could not take in main's pull requests since ";
    assert!(words.contains(line), "{words}");
    assert!(
        words.contains(&format!(": pull/{n}: pull/{n} has no head")),
        "{words}"
    );
    let doc = g.status_document();
    let refused = &doc["intake_refused"];
    assert_eq!(refused["remote"], "origin", "{doc}");
    assert_eq!(refused["asked"], format!("pull/{n}").as_str(), "{doc}");
    assert!(refused["since"].is_string(), "{doc}");
    assert!(refused["rounds"].as_u64().is_some_and(|r| r >= 2), "{doc}");
    // Closed, it is asked for no more, and the next pass meets nothing.
    g.gh.forge.close(n).unwrap();
    g.discovery_passes();
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "1"]);
    assert!(out.status.success(), "{}", text(&out));
    let doc = g.status_document();
    assert!(doc.get("intake_refused").is_none(), "{doc}");
    assert!(!g.status_words().contains(line));
}

/// 128 (design D26): GitHub serves no head for a pull request; `sluice
/// step` says on stderr what its intake met, `sluice: warning: taking in
/// main's pull requests: pull/<n>: ...`, every step, its own intake
/// printing nothing beside it.
#[test]
fn a_step_prints_what_its_intake_met() {
    let g = OnGitHub::new("pr-step-warnings", "");
    let (n, _) = g.by_hand("unit/headless", &[("h.txt", Some("h\n"))], false);
    g.no_head(n);
    let line = format!("sluice: warning: taking in main's pull requests: pull/{n}: ");
    for _ in 0..2 {
        g.discovery_passes();
        let out = g.sluice(&["step", "main"]);
        assert!(out.status.success(), "{}", text(&out));
        let err = String::from_utf8_lossy(&out.stderr);
        let said: Vec<&str> = err.lines().filter(|l| l.starts_with(&line)).collect();
        assert_eq!(said.len(), 1, "{err}");
        assert!(said[0].contains(&format!("pull/{n} has no head")), "{err}");
    }
}

/// A failure that is no one pull request's: GitHub answers an error for the
/// list of `main`'s pull requests on every try the retry policy makes (134).
/// Each round warns that taking in `main`'s pull requests failed, after how
/// many tries, nothing is submitted or withdrawn, and the submission
/// already queued is tested and lands.
#[test]
fn a_failed_list_ends_intake_and_the_queue_still_lands() {
    let g = OnGitHub::new("pr-no-list", "");
    let (n, head) = g.by_hand("unit/listed", &[("slow", Some("x\n"))], false);
    g.step();
    assert_eq!(g.of(n).len(), 1, "{:?}", g.subs());
    g.gh.fail_pulls(true);
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        let said = g.round();
        assert!(
            said.contains(
                "sluice: warning: taking in main's pull requests: main's pull requests: tried "
            ) && said.contains(": GET /repos/o/r/pulls?state=open")
                && said.contains("500 Server Error")
                && !said.contains(&format!("pull/{n}: ")),
            "{said}"
        );
        let subs = g.of(n);
        assert_eq!(subs.len(), 1, "{subs:?}");
        assert_ne!(subs[0]["state"], "withdrawn", "{subs:?}");
        if subs[0]["state"] == "landed" {
            break;
        }
        assert!(Instant::now() < until, "never landed: {subs:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
    let landed = g.main_on_github();
    assert_eq!(
        github::rev(g.bare.path(), &format!("{landed}^2")).unwrap(),
        head
    );
}

/// forge-identity, "The host process gets a repository's token once for
/// its hour": a host process started afresh, held open with nothing in
/// play, looks at the idle repository every few seconds, each look with a
/// binding of its own that fetches the records from GitHub as the app;
/// over four looks it buys one token for o/r (record 0108 as amended
/// 2026-11-19).
#[test]
fn the_host_process_buys_a_repositorys_token_once_across_its_looks() {
    use sluice::host_process::client::Client;
    let g = OnGitHub::new("pr-one-token", "");
    g.settle();
    support::kill_host_process(&mut g.command(env!("CARGO_BIN_EXE_sluice"), g.repo.dir()));
    let doc: Value = serde_json::from_slice(&g.sluice(&["overview", "--json"]).stdout).unwrap();
    let repository = PathBuf::from(doc["repository"].as_str().unwrap());
    // The host process the overview started ends, so the client's own,
    // which counts its git, holds the repository.
    support::kill_host_process(&mut g.command(env!("CARGO_BIN_EXE_sluice"), g.repo.dir()));
    let bought = || {
        g.gh.calls(
            "POST",
            &format!("/app/installations/{}/access_tokens", github::INSTALLATION),
        )
    };
    let before = bought();
    let git = support::CountingGit::new();
    let client = Client::new(
        Path::new(env!("CARGO_BIN_EXE_sluice")),
        support::env_of(git.on(&mut g.command(env!("CARGO_BIN_EXE_sluice"), g.repo.dir()))),
    )
    .unwrap();
    // A subscription of no folder holds it open, and reads nothing.
    let held = client
        .subscribe(Vec::new())
        .expect("the host process starts");
    // The host process looks at the repository as it starts, and at each
    // pass asked of it, each reading fetching every
    // branch's records from GitHub as the app once, and main's again.
    let looks = || {
        git.starts()
            .iter()
            .filter(|a| a.contains(" fetch ") && a.contains("+refs/sluice/hosts/*"))
            .count()
    };
    let until = Instant::now() + Duration::from_secs(30);
    while looks() < 4 {
        assert!(
            Instant::now() < until,
            "four looks: {:?}\n{}",
            git.starts(),
            g.host_log()
        );
        // A reading that asks the remote: a pass of the repository.
        client.pass(&repository, None).unwrap();
    }
    drop(held);
    assert_eq!(bought() - before, 1, "{:?}", git.starts());
}

/// 133, rule core.28 (design D27): GitHub no longer finds the app's
/// installation on the repository, as when it was renamed; the host
/// process's passes say the round's refusal once, with what to do, and
/// once main's records can be written again its log holds the intake's
/// cause once, marked `intake`, beside the round's own `read-refused`.
#[test]
fn a_repository_github_no_longer_finds_is_said_once_with_what_to_do() {
    let g = OnGitHub::new("pr-renamed", "");
    let (n, _) = g.by_hand("unit/renamed", &[("slow", Some("x\n"))], false);
    g.step();
    assert_eq!(g.of(n).len(), 1, "{:?}", g.subs());
    g.gh.trouble(
        "GET",
        "/repos/o/r/installation",
        usize::MAX,
        github::Trouble::Status(404),
    );
    // A host process started afresh, which holds no token bought before.
    support::kill_host_process(&mut g.command(env!("CARGO_BIN_EXE_sluice"), g.repo.dir()));
    g.discovery_passes();
    g.on_main();
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "3"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let said = "GET /repos/o/r/installation: 404 Not Found";
    let todo = format!(
        "GitHub finds no installation of app {} on o/r: if the repository was renamed or \
         transferred, point the remote that names it at its new address",
        github::APP
    );
    let passed = "main could not be read, passed by: ";
    assert_eq!(
        stdout.matches(passed).count(),
        1,
        "{}\n{}\n{:?}",
        text(&out),
        g.host_log(),
        g.log()
    );
    assert_eq!(stdout.matches(&todo).count(), 1, "{}", text(&out));
    // Main's records are on its remote, which cannot be written while
    // GitHub gives no token: both refusals are kept on the host until it
    // can be, and then written once each.
    g.gh.calm();
    g.discovery_passes();
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "2"]);
    assert!(out.status.success(), "{}", text(&out));
    let refused: Vec<Value> = g
        .log()
        .into_iter()
        .filter(|l| l["event"] == "read-refused")
        .collect();
    let (theirs, ours): (Vec<&Value>, Vec<&Value>) =
        refused.iter().partition(|l| l["intake"] == true);
    assert_eq!(theirs.len(), 1, "{refused:?}");
    assert_eq!(theirs[0]["asked"], "main's pull requests", "{refused:?}");
    let words = theirs[0]["said"].as_str().unwrap();
    assert!(words.contains(said) && words.contains(&todo), "{words}");
    assert_eq!(ours.len(), 1, "{refused:?}");
}

// ---------------------------------------------------------------------------
// pull-request-status
// ---------------------------------------------------------------------------

/// forge-identity spec, "A landing and its records pushed as the app"
/// (record 0019 as amended 2026-11-18; rule core.43): a request landed on
/// `main`, which sluice pushes to GitHub as its app, is a landing commit
/// whose author and committer are the app's bot, and so is the commit of
/// `main`'s records on `refs/sluice/state/main`.
#[test]
fn a_landing_and_its_records_name_the_apps_bot() {
    let g = OnGitHub::new("pr-landed-by-bot", "");
    let (n, _) = g.by_hand("unit/bot", &[("b.txt", Some("b\n"))], false);
    g.step_until("landed", |g| {
        g.of(n).first().is_some_and(|s| s["state"] == "landed")
    });
    let bot = format!(
        "{}[bot] <{}+{}[bot]@users.noreply.github.com> ",
        github::SLUG,
        github::BOT,
        github::SLUG
    );
    for name in ["refs/heads/main", "refs/sluice/state/main"] {
        let out = support::git_command()
            .arg("--git-dir")
            .arg(g.bare.path())
            .args(["cat-file", "-p", name])
            .output()
            .unwrap();
        let raw = String::from_utf8_lossy(&out.stdout);
        assert!(raw.contains(&format!("\nauthor {bot}")), "{name}: {raw}");
        assert!(raw.contains(&format!("\ncommitter {bot}")), "{name}: {raw}");
    }
}

/// A check that goes on until the test ends it, the first time it runs:
/// `file` is the `hang` file's content, naming `pid`, where the check
/// writes its process. Dropped, it ends that process, passed or failed,
/// so no check a test started outlives it.
struct Hang {
    pid: PathBuf,
}

impl Hang {
    fn in_repo(g: &OnGitHub) -> Hang {
        Hang {
            pid: g.repo.dir().join(".git/hang.pid"),
        }
    }

    fn file(&self) -> String {
        format!("{}\n", self.pid.display())
    }
}

impl Drop for Hang {
    fn drop(&mut self) {
        if let Ok(process) = std::fs::read_to_string(&self.pid) {
            let _ = Command::new("kill").arg(process.trim()).status();
        }
    }
}

// ---------------------------------------------------------------------------
// pull-request-submit
// ---------------------------------------------------------------------------

/// A waiting submit follows its pull request through review: it prints
/// waiting, then queued or testing, then landed, and exits 0; the agent's
/// worktree is removed on landing.
#[test]
fn a_waiting_submit_follows_its_pull_request() {
    let g = OnGitHub::new("pr-wait", "reviews = 1");
    let w = g.session("wait", &[("w.txt", "w\n")]);
    let mut child = g
        .command("wt", &w)
        .args(["submit", "main"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // What the submit has said so far, both streams, read as it goes.
    let heard = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let readers: Vec<_> = [
        Box::new(child.stdout.take().unwrap()) as Box<dyn std::io::Read + Send>,
        Box::new(child.stderr.take().unwrap()),
    ]
    .into_iter()
    .map(|mut stream| {
        let heard = heard.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n @ 1..) = stream.read(&mut buf) {
                heard
                    .lock()
                    .unwrap()
                    .push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        })
    })
    .collect();
    let until = Instant::now() + Duration::from_secs(60);
    let pull = loop {
        if let Some(p) = g.gh.forge.by_ref("unit/wait").unwrap() {
            if !p.head.is_empty() {
                break p;
            }
        }
        assert!(Instant::now() < until, "no pull request opened");
        std::thread::sleep(Duration::from_millis(100));
    };
    // Reviewed once the submit has said it waits, however long the
    // rounds take on a loaded host: a backstop of its own.
    let until = Instant::now() + Duration::from_secs(60);
    while !heard.lock().unwrap().contains("waiting: needs 1 review") {
        assert!(
            Instant::now() < until,
            "never said it waits: {}",
            heard.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    g.gh.forge
        .review(pull.number, "reviewer", "APPROVED", &pull.head)
        .unwrap();
    let status = child.wait().unwrap();
    for r in readers {
        r.join().unwrap();
    }
    let said = heard.lock().unwrap().clone();
    assert_eq!(status.code(), Some(0), "{said}");
    let waiting = said.find("waiting: needs 1 review").expect(&said);
    let queued = said
        .find("queued")
        .or_else(|| said.find("testing"))
        .expect(&said);
    let landed = said.find("landed as").expect(&said);
    assert!(waiting < queued && queued < landed, "{said}");
    // Every submission is named by its pull request's number.
    assert!(
        said.contains(&format!("testing main + #{} ·", pull.number)),
        "{said}"
    );
    assert!(!w.exists(), "the worktree is removed on landing: {said}");
    // 51: the request's outcome was posted on it, which confirmed it on
    // receipt, and the wait that reported it left that as it stood.
    let id = g.of(pull.number)[0]["id"].clone();
    assert_eq!(g.delivery(&id)["state"], "done");
    let handed = g.logged("handed", &id);
    assert_eq!(handed.len(), 1, "{handed:?}");
    assert_eq!(handed[0]["by"], "pull-request");
    assert_eq!(
        handed[0]["to"],
        format!("pull/{}", pull.number).as_str(),
        "{handed:?}"
    );
}

/// GitHub answering 503 to a read of the pull request a waiting submit
/// follows is no outcome: the submit reads it again, a pause apart, and
/// follows it to its landing.
#[test]
fn a_waiting_submit_reads_again_through_githubs_503() {
    let g = OnGitHub::new("pr-503", "");
    let w = g.session("unanswered", &[("u.txt", "u\n")]);
    g.gh.unavailable(2);
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "main"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    assert_eq!(g.gh.unavailable_left(), 0);
    let main = g.main_on_github();
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .ends_with(&format!("passed · landed as {}\n", &main[..7])),
        "{}",
        text(&out)
    );
}

// ---------------------------------------------------------------------------
// group-submit, under pull requests (125)
// ---------------------------------------------------------------------------

impl OnGitHub {
    /// The open pull request from `unit/<name>`.
    fn pull_of(&self, name: &str) -> sluice::profiles::github::forge::Pull {
        self.gh
            .forge
            .by_ref(&format!("unit/{name}"))
            .unwrap()
            .unwrap_or_else(|| panic!("no pull request from unit/{name}"))
    }
}

/// group-submit, "Stopped while both are open": a waiting act of two pull
/// requests, every condition holding, one combination at a time, the
/// first's check going on until the test ends it. Stopped with Ctrl-C
/// before either is decided, it says each one still open with the wait
/// that follows it by its number, and exits 130 (record 0130 D4).
#[test]
fn a_group_act_stopped_names_each_open_pull_requests_wait() {
    let g = OnGitHub::checked("pr-group-stopped", "", "1m", "parallel = 1\n");
    let hang = Hang::in_repo(&g);
    let ws = [
        g.session("o1", &[("hang", hang.file().as_str())]),
        g.session("o2", &[("o2.txt", "x\n")]),
    ];
    let mut args = vec!["submit", "main"];
    for w in &ws {
        args.extend(["--worktree", w.to_str().unwrap()]);
    }
    let act = g
        .command(env!("CARGO_BIN_EXE_sluice"), g.repo.dir())
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    started(&hang.pid);
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(act.id() as i32),
        nix::sys::signal::Signal::SIGINT,
    )
    .unwrap();
    let out = act.wait_with_output().unwrap();
    let said = text(&out);
    assert_eq!(out.status.code(), Some(sluice::STOPPED), "{said}");
    let lines: Vec<&str> = said.lines().collect();
    for name in ["o1", "o2"] {
        let n = g.pull_of(name).number;
        let still = format!("still open: #{n}; sluice wait main --pull-request {n}");
        assert!(lines.contains(&still.as_str()), "{still:?} in {said}");
    }
}

// ---------------------------------------------------------------------------
// pull-request-intake, a group's pull requests (125)
// ---------------------------------------------------------------------------

/// S58, 124, 1: a worktree that has checked out `bolt/x`, this host's copy
/// two landings behind GitHub's, submits to `main`: nothing is pushed, the
/// pull request is from `bolt/x` as GitHub holds it and ready at GitHub's
/// revision, and the output names both revisions. A hand's reset of the
/// copy is refused and the bolt's round levels it. Submitted again after
/// another landing on the bolt, the same pull request is ready at it.
#[test]
fn a_bolts_landing_worktree_submits_the_bolt_as_github_holds_it() {
    let g = OnGitHub::with_bolt("pr-bolt");
    let made = g.bolt_on_github();
    g.land_on_bolt("one", &[("one.txt", "1\n")]);
    let remote = g.land_on_bolt("two", &[("two.txt", "2\n")]);
    // This host's bolt/x stands where it was before both landings, as an
    // older sluice left it, past the protection, and a worktree has it
    // checked out.
    g.repo.git(&[
        "-c",
        "core.hooksPath=/dev/null",
        "update-ref",
        "refs/heads/bolt/x",
        &made,
    ]);
    let w = g.repo.dir().with_extension("landing");
    let _ = std::fs::remove_dir_all(&w);
    g.repo
        .git(&["worktree", "add", "-q", w.to_str().unwrap(), "bolt/x"]);

    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let said = text(&out);
    assert_eq!(
        g.bolt_on_github(),
        remote,
        "nothing pushed to GitHub's bolt/x"
    );
    assert_eq!(g.repo.rev("bolt/x"), made, "this host's bolt/x unmoved");
    let pull = g.pull_from("bolt/x");
    assert!(pull.open && !pull.draft && pull.base == "main", "{pull:?}");
    assert_eq!(pull.head, remote);
    assert!(!said.contains("pushed"), "{said}");
    assert!(
        said.contains(&format!(
            "this host's bolt/x is at {}, behind origin's {}; submitted {}",
            &made[..7],
            &remote[..7],
            &remote[..7]
        )),
        "{said}"
    );
    // Under --json, the pull request's document carries the same: a
    // submit's, since `wt submit` run again answers with the request open
    // and ready at what it would send (record 0065).
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "main", "--no-wait", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["pull_request"], pull.number, "{v}");
    assert_eq!(
        v["copy"],
        serde_json::json!({
            "branch": "bolt/x", "copy": made, "remote": "origin", "at": remote, "how": "behind",
        }),
        "{v}"
    );
    assert_eq!(g.bolt_on_github(), remote);

    // A tool's reset of this host's bolt/x to GitHub's revision is
    // refused, naming the command and both revisions; the bolt's next round
    // levels it there.
    let out = g
        .repo
        .git_raw(&["-C", w.to_str().unwrap(), "reset", "-q", "--hard", &remote]);
    assert_eq!(out.status.code(), Some(128), "{}", text(&out));
    let refused = text(&out);
    assert!(
        refused.contains(&format!(
            "reset -q --hard {remote}` would move it from {} to {}",
            &made[..7],
            &remote[..7]
        )),
        "{refused}"
    );
    assert_eq!(g.repo.rev("bolt/x"), made);
    let out = g.sluice(&["step", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(g.repo.rev("bolt/x"), remote);

    // Another submission lands on the bolt; the worktree is brought level
    // with this host's bolt/x, which the rounds set, and submits again.
    let again = g.land_on_bolt("three", &[("three.txt", "3\n")]);
    assert_eq!(g.repo.rev("bolt/x"), again);
    // GitHub's request follows its branch there, which this host finds at
    // its next asking (136).
    g.discovery_passes();
    g.repo
        .git(&["-C", w.to_str().unwrap(), "reset", "-q", "--hard"]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let said = text(&out);
    assert!(
        said.contains(&format!("updated #{}", pull.number)),
        "{said}"
    );
    assert!(
        !said.contains("pushed") && !said.contains("this host's"),
        "{said}"
    );
    let held = g.gh.forge.held().unwrap();
    assert_eq!(held.pulls.len(), 1);
    assert_eq!(held.pulls[0].head, again);
    assert!(held.pulls[0].open && !held.pulls[0].draft);
}

// ---------------------------------------------------------------------------
// submit-by-name: a branch sluice lands on, named from no worktree, to a
// branch that takes pull requests (124, C.5; record 0090; rule core.50)
// ---------------------------------------------------------------------------

impl OnGitHub {
    /// [`OnGitHub::submit_by_name`], waiting, spawned: it goes on beside
    /// the test until [`ended`].
    fn spawn_by_name(&self, branch: &str, args: &[&str]) -> std::process::Child {
        self.command(env!("CARGO_BIN_EXE_sluice"), &self.repo.dir().join(".git"))
            .args(["submit", "main", "--head", branch])
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    /// `git` in GitHub's own repository, as another queue, or a person
    /// pushing there, writes it: what it printed.
    fn on_github(&self, args: &[&str]) -> String {
        let out = crate::support::git_command()
            .arg("--git-dir")
            .arg(self.bare.path())
            .args(args)
            .env("GIT_AUTHOR_NAME", "another queue")
            .env("GIT_AUTHOR_EMAIL", "queue@example.com")
            .env("GIT_COMMITTER_NAME", "another queue")
            .env("GIT_COMMITTER_EMAIL", "queue@example.com")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }
}

/// `child` once it has ended, within two minutes, with what it said.
fn ended(mut child: std::process::Child) -> Output {
    let until = Instant::now() + Duration::from_secs(120);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > until {
            let _ = child.kill();
            let out = child.wait_with_output().unwrap();
            panic!("never ended: {}", text(&out));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.wait_with_output().unwrap()
}

/// `main` taking pull requests, beside `bolt/x` on GitHub, a branch sluice
/// lands on whose one commit holds a check that goes on until it is ended
/// the first time it runs, and `pid`, where that check writes its process.
fn with_a_hanging_bolt(name: &str) -> (OnGitHub, PathBuf) {
    let g = OnGitHub::checked_policy(name, "", "1m", "", BOLTS);
    let pid = g.repo.dir().join(".git/hang.pid");
    let hang = format!("{}\n", pid.display());
    g.repo.branch("bolt/x", "main", &[("hang", Some(&hang))]);
    g.repo
        .git(&["push", "-q", "origin", "bolt/x:refs/heads/bolt/x"]);
    (g, pid)
}

/// Until `pid` exists: the check of the submission first taken in has
/// started. It waits on the file events of `pid`'s folder, and on no
/// clock: sluice names no time within which a check starts, since a run
/// waits for room under the host's bound, which other runs on the host may
/// hold (150, rule core.49). The run's own time limit ends a check that
/// never starts (66).
fn started(pid: &Path) {
    use notify_debouncer_full::notify::{recommended_watcher, RecursiveMode, Watcher};
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = recommended_watcher(tx).expect("a watch of the check's folder");
    watcher
        .watch(pid.parent().unwrap(), RecursiveMode::NonRecursive)
        .expect("a watch of the check's folder");
    while !pid.exists() {
        let _ = rx.recv().expect("the watch of the check's folder ended");
    }
}

/// 122, 135; the signal `agentplot/crew-state:agentplot/main
/// signals/2026-11-07-agentplot-ops-d4947942/01-sluice-exits-4-after-its-own-landing`:
/// a waiting submit by name in the repository's git directory, whose
/// request another queue of `main` lands once this host took it in, exits
/// 0 and says `landed as <rev>`, never `withdrawn`, and its submission's
/// record reads landed, `landed_elsewhere`, once a round read the branch.
/// The wait answers from the branch (rule core.48), so it can end before
/// any round of a host that passes only when asked moved the record.
#[test]
fn a_waiting_submit_by_name_in_a_bare_clone_reports_another_queues_landing() {
    let (g, pid) = with_a_hanging_bolt("pr-by-name-elsewhere");
    let child = g.spawn_by_name("bolt/x", &[]);
    started(&pid);
    let pull = g.pull_from("bolt/x");
    let first = g.of(pull.number)[0]["id"].as_u64().unwrap();
    let landing = landed_elsewhere(&g, &pull.head);
    g.discovery_passes();

    let out = ended(child);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{said}");
    assert!(
        said.contains(&format!("landed as {}", &landing[..7])),
        "{said}"
    );
    assert!(!said.contains("withdrawn"), "{said}");
    let _ = g.round();
    let sub = g
        .of(pull.number)
        .into_iter()
        .find(|x| x["id"] == first)
        .unwrap();
    assert_eq!(sub["state"], "landed", "{sub}");
    assert_eq!(sub["landed_elsewhere"], true, "{sub}");
}

// ---------------------------------------------------------------------------
// forge-allowance: a command waits the pause out within its own limit, a
// round passes the forge's part by (134 as amended 2026-10-28; ADR 0071)
// ---------------------------------------------------------------------------

/// GitHub's words in the pause the tests leave on the host.
const SPENT: &str = "API rate limit exceeded for installation ID 7.";

impl OnGitHub {
    /// The host's state says GitHub's REST allowance for the app's
    /// installation is spent for `minutes` more, as another process on the
    /// host found: until when. The pause is kept under the account the
    /// repository is on, which names the app's one installation there
    /// (134; record 0108).
    fn spent_for(&self, minutes: i64) -> chrono::DateTime<chrono::Utc> {
        let until = chrono::Utc::now() + chrono::TimeDelta::minutes(minutes);
        let dir = self.state.path().join("sluice/forge/pauses");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!(
                "{}-{}-core.json",
                github::APP,
                github::repo().owner
            )),
            serde_json::json!({ "until": until, "words": SPENT, "streak": 1 }).to_string(),
        )
        .unwrap();
        until
    }

    /// How many calls the fake took under the installation's allowance:
    /// every call to the repository but finding the installation, which
    /// is the app's own.
    fn repo_calls(&self) -> usize {
        self.gh
            .fake
            .requests()
            .iter()
            .filter(|r| r.path.starts_with("/repos/o/r/") && r.path != "/repos/o/r/installation")
            .count()
    }
}

/// `until` as the host's clock shows it.
fn hhmm(until: chrono::DateTime<chrono::Utc>) -> String {
    until
        .with_timezone(&chrono::Local)
        .format("%H:%M")
        .to_string()
}

/// The forge-allowance spec's "A waiting submit meets the pause": stderr
/// says once until when GitHub's allowance is spent, the submit calls
/// GitHub again once it has passed, and goes on to its outcome.
#[test]
fn a_waiting_submit_waits_out_the_pause() {
    let g = OnGitHub::new("pr-spent-submit", "");
    let w = g.session("paused", &[("p.txt", "p\n")]);
    let until = g.spent_for(20);
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "main"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!(
        "sluice: GitHub's allowance of calls is spent until {}; waiting: {SPENT}",
        hhmm(until)
    );
    assert_eq!(stderr.matches(&said).count(), 1, "{stderr}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("allowance"));
    // 135 (design D9): the host process landed what the submit opened
    // once the pause was waited out; it asks GitHub nothing while the
    // pause stands, so the request reads merged at its next call.
    let n = g.pull_from("unit/paused").number;
    assert_eq!(g.of(n)[0]["state"], "landed", "{:?}", g.subs());
}

/// The forge-allowance spec's "A wait whose timeout comes first": `sluice
/// wait main --timeout 5m`, following the pull request its worktree's
/// submit opened, beside a pause twenty minutes long exits `5` with the
/// request as it stands, having made no call to GitHub. A wait runs no
/// intake and follows the host process (135; design D9): the request it
/// reads of GitHub is where it meets the pause.
#[test]
fn a_wait_whose_timeout_comes_first_calls_nothing() {
    let g = OnGitHub::new("pr-spent-wait", "");
    let w = g.session("slow", &[("slow", "x\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    g.spent_for(20);
    let before = g.repo_calls();
    let out = g.skipping(&w, &["wait", "main", "--timeout", "5m"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    assert_eq!(g.repo_calls(), before, "no call to GitHub: {}", text(&out));
}

/// The user, 2026-10-28: a submit that does not wait fails at once on a
/// spent allowance, exit 3, naming when it ends, and calls nothing.
#[test]
fn a_submit_that_does_not_wait_fails_at_once_on_the_pause() {
    let g = OnGitHub::new("pr-spent-no-wait", "");
    let w = g.session("hurried", &[("h.txt", "h\n")]);
    let until = g.spent_for(20);
    let before = g.repo_calls();
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "main", "--no-wait"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    let said = text(&out);
    assert!(
        said.contains(&format!(
            "GitHub's allowance of calls for app {} is spent until {}: {SPENT}",
            github::APP,
            hhmm(until)
        )),
        "{said}"
    );
    assert!(!said.contains("waiting"), "{said}");
    assert_eq!(g.repo_calls(), before);
    assert!(g.gh.forge.by_ref("unit/hurried").unwrap().is_none());
}

/// The forge-allowance spec's "A landing during the pause" and "Said
/// once": `sluice run` under a pause lands a combination that passed,
/// calls nothing of the installation's allowance, and says the pause once.
#[test]
fn a_round_lands_through_the_pause_and_says_it_once() {
    let g = OnGitHub::new("pr-spent-run", "");
    let (n, head) = g.by_hand("unit/through", &[("t.txt", Some("t\n"))], false);
    g.step();
    assert_eq!(g.of(n).len(), 1, "{:?}", g.subs());
    let until = g.spent_for(20);
    let before = g.repo_calls();
    let out = g.sluice(&["run", "--rounds", "6"]);
    assert!(out.status.success(), "{}", text(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let said = format!(
        "main's pull requests passed by: GitHub's allowance of calls for app {} is spent until {}: {SPENT}",
        github::APP,
        hhmm(until)
    );
    assert_eq!(stdout.matches(&said).count(), 1, "{}", text(&out));
    assert_eq!(g.of(n)[0]["state"], "landed", "{:?}", g.subs());
    let landed = g.main_on_github();
    assert_eq!(
        github::rev(g.bare.path(), &format!("{landed}^2")).unwrap(),
        head
    );
    assert_eq!(g.repo_calls(), before, "no call under the allowance");
}

// ---------------------------------------------------------------------------
// forge-reads: the open pull requests asked for at the host's interval, a
// pull request read again only once it moved, and posted on only when what
// sluice says changed (136)
// ---------------------------------------------------------------------------

impl OnGitHub {
    /// `sluice <args>` in the repository, its pauses between rounds
    /// skipped and its clock moved on by each.
    fn skipping(&self, dir: &Path, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_sluice"), dir)
            .env("SLUICE_ROUND_SLEEP", "record")
            .args(args)
            .output()
            .unwrap()
    }

    /// The repository's own HEAD on `main`, so `sluice run` serves the
    /// branches its policy turns pull requests on for, as it reads them
    /// from the default branch.
    fn on_main(&self) {
        self.repo.git(&["symbolic-ref", "HEAD", "refs/heads/main"]);
    }

    /// Two rounds of `main`: the first writes the branch's records, which
    /// the second's asking reads as a move; after them nothing on the host
    /// moves.
    fn settle(&self) {
        self.step();
        self.step();
    }

    /// How many times main's open pull requests were asked for.
    fn lists(&self) -> usize {
        self.gh.calls("GET", "/repos/o/r/pulls?")
    }

    /// How many times pull request `n` itself was read.
    fn pull_reads(&self, n: u64) -> usize {
        let path = format!("/repos/o/r/pulls/{n}");
        self.gh
            .fake
            .requests()
            .iter()
            .filter(|r| r.method == "GET" && r.path == path)
            .count()
    }

    /// How many times sluice's own check runs on `head` were read back.
    fn posted_reads(&self, head: &str) -> usize {
        self.gh.calls(
            "GET",
            &format!("/repos/o/r/commits/{head}/check-runs?app_id"),
        )
    }
}

impl OnGitHub {
    /// `main` on GitHub taking pull requests, and `bolt/x` and `bolt/y`
    /// beside it, branches on GitHub that take pull requests too, each
    /// stepped until nothing on the host moves.
    fn with_bolts_taking_requests(name: &str) -> OnGitHub {
        let g = OnGitHub::checked_policy(
            name,
            "",
            "1m",
            "",
            &format!("{BOLTS}\n[branches.\"bolt/*\".pull_requests]\n"),
        );
        for bolt in ["bolt/x", "bolt/y"] {
            g.repo.git(&["branch", bolt, "main"]);
            g.repo
                .git(&["push", "-q", "origin", &format!("{bolt}:refs/heads/{bolt}")]);
        }
        for branch in ["main", "bolt/x", "bolt/y"] {
            for _ in 0..2 {
                let out = g.sluice(&["step", branch]);
                assert!(out.status.success(), "{}", text(&out));
            }
        }
        g
    }
}

/// The fault "A `sluice submit --no-wait` exits 3 when the GitHub App's
/// hourly requests are spent": main and two bolts take pull requests,
/// and the installation's allowance holds what one list of the
/// repository in each interval and a submit need, with room for the
/// order the submit's calls and the host's pass fall in. A hundred and
/// sixty passes of `sluice run` over eight minutes, with nothing on the
/// host moving, leave the allowance for the submit, which opens its pull
/// request and exits 0
/// (`one-forge-read-per-repository-per-interval@b3b6757cea2d`).
#[test]
fn idle_branches_leave_the_allowance_for_a_submit() {
    let g = OnGitHub::with_bolts_taking_requests("pr-allowance");
    g.on_main();
    g.discovery_passes();
    let w = g.session("allowed", &[("a.txt", "a\n")]);
    let before = g.repo_calls();
    let asked = g.gh.fake.requests().len();
    g.gh.allow(ALLOWED);
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "160"]);
    assert!(out.status.success(), "{}", text(&out));
    let idle = g.repo_calls() - before;
    // The run's host process ends, so no pass of its own, a real minute
    // on, takes a call before the submit's: the submit's host process
    // starts afresh and reads the last asking the host keeps.
    support::kill_host_process(&mut g.command(env!("CARGO_BIN_EXE_sluice"), g.repo.dir()));
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "main", "--no-wait"])
        .output()
        .unwrap();
    let calls: Vec<String> = g.gh.fake.requests()[asked..]
        .iter()
        .map(|r| format!("{} {}", r.method, r.path))
        .collect();
    assert_eq!(
        out.status.code(),
        Some(0),
        "idle {idle}, calls {calls:#?}: {}",
        text(&out)
    );
    assert_eq!(idle, 8, "one list in each minute: {calls:#?}");
    assert!(g.gh.forge.by_ref("unit/allowed").unwrap().is_some());
}

/// The calls the installation's allowance holds in
/// [`idle_branches_leave_the_allowance_for_a_submit`]: the repository's
/// one list in each of eight minutes, and ten for the submit and the
/// pass it moves, which take three to six as they fall. A list of each
/// branch in each minute spends the allowance before the submit, at its
/// eighteenth list, and the submit exits 3.
const ALLOWED: usize = 18;

/// reader-tick, "A branch that takes pull requests between discoveries":
/// none open and the host's interval a minute, six passes of `sluice run`
/// within it start git in the run exactly as often as one, and ask GitHub
/// nothing after the first (129, 136). The passes are the host process's,
/// which the run follows (135): its starts are counted apart, whichever
/// command started it.
#[test]
fn passes_between_discoveries_start_no_git_and_ask_github_nothing() {
    let g = OnGitHub::new("pr-idle-ticks", "");
    g.settle();
    g.on_main();
    let git = support::CountingGit::new();
    let run = |rounds: &str| -> (Vec<String>, Vec<(String, String)>) {
        // Each run follows a host process of its own, which gets its token
        // as it starts: whether the last one lived on between two runs is a
        // matter of time, and its token no pass's ask.
        support::kill_host_process(&mut g.command(env!("CARGO_BIN_EXE_sluice"), g.repo.dir()));
        g.discovery_passes();
        let (starts, calls) = (git.count(), g.gh.fake.requests().len());
        let child = git
            .on(&mut g.command(env!("CARGO_BIN_EXE_sluice"), g.repo.dir()))
            .env("SLUICE_ROUND_SLEEP", "record")
            .args(["run", "--rounds", rounds])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{}", text(&out));
        let asked = g.gh.fake.requests()[calls..]
            .iter()
            .map(|r| (r.method.clone(), r.path.clone()))
            .collect();
        let own = git.parented()[starts..]
            .iter()
            .filter(|(parent, _)| *parent == pid)
            .map(|(_, args)| args.clone())
            .collect();
        (own, asked)
    };
    run("1");
    let lists = g.lists();
    let (one, asked_one) = run("1");
    let (six, asked_six) = run("6");
    // Six of the host process's passes start no git in the run beyond
    // what one does (129, 135).
    let fetches = |s: &[String]| s.iter().filter(|a| a.contains(" fetch ")).count();
    assert_eq!(six.len(), one.len(), "one: {one:?}; six: {six:?}");
    assert_eq!(fetches(&six), fetches(&one), "{six:?}");
    assert_eq!(g.lists(), lists + 2, "asked at each run's first pass alone");
    for call in &asked_six {
        assert!(
            asked_one.contains(call),
            "{call:?} beyond one pass's: {asked_six:?}"
        );
    }
    assert!(asked_six.len() <= asked_one.len(), "{asked_six:?}");
}

/// 136: a waiting `sluice wait main` that follows its pull request for a
/// minute of a clock its pauses move, its twenty rounds three seconds
/// apart, nothing on the host moving, reads the request from GitHub no
/// more often than once in a quarter of the host's interval: four times,
/// at most. The host's interval has passed as it begins, so its first
/// pass asks for main's pull requests at the wait's own time, and its
/// last, fifty-seven seconds on, finds nothing due however long the
/// commands before it took. Asked last by the step before it, at that
/// step's time, it had three seconds of the host's minute to begin in:
/// deneb, 2026-11-18, main at 15b5cf173, its last passes asking again.
#[test]
fn a_wait_reads_its_pull_request_once_in_a_quarter_of_the_interval() {
    let g = OnGitHub::new("pr-discovery-follow", "reviews = 1");
    let w = g.session("followed", &[("f.txt", "f\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let n = g.pull_from("unit/followed").number;
    g.settle();
    let before = g.pull_reads(n);
    let lists = g.lists();
    g.discovery_passes();
    let out = g.skipping(&w, &["wait", "main", "--timeout", "57s"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let reads = g.pull_reads(n) - before;
    assert!((1..=4).contains(&reads), "{reads} reads: {}", text(&out));
    // Its own asking asked at its first pass and found nothing due after
    // it; beside that, only the wait's finding of it.
    assert_eq!(g.lists(), lists + 2, "{}", text(&out));
}

/// Another host over `main`'s remote and GitHub: its own clone, config and
/// state, its name among its peers given (137).
struct Other<'a> {
    g: &'a OnGitHub,
    repo: Repo,
    config: Scratch,
    state: Scratch,
}

/// Another host's tests end while its run may go: its host process is
/// stopped as it ends, which ends the runs.
impl Drop for Other<'_> {
    fn drop(&mut self) {
        support::guard::stop_host_processes(self.state.path());
    }
}

impl OnGitHub {
    /// This host is called `name` among its peers, with `lines` more in
    /// its `[peers]` table, in place of a `[peers]` table it has.
    fn named(&self, name: &str, lines: &str) {
        let path = self.config.path().join("sluice/config.toml");
        let text = std::fs::read_to_string(&path).unwrap();
        let text = match text.find("[peers]\n") {
            Some(at) => {
                let rest = &text[at + "[peers]\n".len()..];
                let end = rest
                    .find("\n[")
                    .map_or(text.len(), |e| at + "[peers]\n".len() + e + 1);
                format!("{}{}", &text[..at], &text[end..])
            }
            None => text,
        };
        std::fs::write(
            &path,
            format!("[peers]\nname = \"{name}\"\n{lines}\n{text}"),
        )
        .unwrap();
    }

    /// Another host, called `name` among its peers with `lines` more in its
    /// `[peers]` table, and `config` first in its config, over the same
    /// remote and GitHub. Host `b` signs as the policy's signer `b` where
    /// `config` names no `[signing]` (design D28).
    fn other(&self, name: &str, lines: &str, config: &str) -> Other<'_> {
        let config = match (name, config.contains("[signing]")) {
            ("b", false) => format!("{}\n{config}", self.b_signing),
            _ => config.to_string(),
        };
        let config = config.as_str();
        let repo = Repo::clone_of(self.bare.path(), &format!("{name}-host"));
        repo.git(&["remote", "set-url", "origin", "https://github.com/o/r.git"]);
        repo.git(&[
            "config",
            &format!("url.{}.insteadOf", self.bare.path().display()),
            "https://github.com/o/r.git",
        ]);
        let theirs = Scratch::new(&format!("{name}-host-config"));
        let dir = theirs.path().join("sluice");
        std::fs::create_dir_all(&dir).unwrap();
        let ours = std::fs::read_to_string(self.config.path().join("sluice/config.toml")).unwrap();
        // This host's [forge] table, its key among them, and nothing of its
        // [peers] or [signing] tables.
        let forge = &ours[ours.find("[runs]").unwrap()..];
        std::fs::write(
            dir.join("config.toml"),
            format!("{config}\n[peers]\nname = \"{name}\"\n{lines}\n{forge}"),
        )
        .unwrap();
        Other {
            g: self,
            repo,
            config: theirs,
            state: Scratch::new(&format!("{name}-host-state")),
        }
    }
}

impl Other<'_> {
    /// As [`OnGitHub::command`], on this host.
    fn command(&self, program: &str, dir: &Path) -> Command {
        let bin = Path::new(env!("CARGO_BIN_EXE_sluice")).parent().unwrap();
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
        let mut c = Command::new(program);
        support::isolated(&mut c)
            .current_dir(dir)
            .env("PATH", path)
            .env("SLUICE_ROUND_SLEEP", "record")
            .env("XDG_CONFIG_HOME", self.config.path())
            .env("XDG_STATE_HOME", self.state.path())
            .env("SLUICE_FORGE_API", self.g.gh.url())
            .env_remove("SLUICE_AGENT")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null());
        c
    }

    fn sluice(&self, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_sluice"), self.repo.dir())
            .args(args)
            .output()
            .unwrap()
    }

    /// `sluice <args>` in `dir`, its pauses skipped and its clock moved on
    /// by each.
    fn skipping(&self, dir: &Path, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_sluice"), dir)
            .env("SLUICE_ROUND_SLEEP", "record")
            .args(args)
            .output()
            .unwrap()
    }

    /// `sluice run` of `rounds` passes, its HEAD on `main` so it serves the
    /// branches the policy turns pull requests on for.
    fn run(&self, rounds: &str) -> String {
        self.repo.git(&["symbolic-ref", "HEAD", "refs/heads/main"]);
        let out = self.skipping(self.repo.dir(), &["run", "--rounds", rounds]);
        assert!(out.status.success(), "{}", text(&out));
        text(&out)
    }

    /// This host's `[forge] discovery` has passed since it last asked
    /// GitHub for main's open pull requests (136).
    fn discovery_passes(&self) {
        let path = self
            .state
            .path()
            .join("sluice/forge/requests/o/r/main.json");
        if let Ok(bytes) = std::fs::read(&path) {
            let mut v: Value = serde_json::from_slice(&bytes).unwrap();
            v["asked"] = Value::Null;
            std::fs::write(&path, v.to_string()).unwrap();
        }
    }

    /// The submissions from pull request `n` in this host's records.
    fn of(&self, n: u64) -> Vec<Value> {
        let out = self.sluice(&["status", "main", "--json", "--all"]);
        assert!(out.status.success(), "{}", text(&out));
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        let place = format!("pull/{n}");
        v["submissions"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s["from"]["worktree"] == place.as_str())
            .collect()
    }
}

/// pull-request-intake, "Records each host keeps" (137, 56, S64; design
/// D22): main takes pull requests and the policy names no signers, so main
/// is refused alone, in the refusal's words: a submit to it exits 2 with
/// nothing pushed and nothing opened, a step and a status say it, and
/// GitHub is asked nothing of main's pull requests. Main has no records
/// here, so the overview lists nothing of it.
#[test]
fn pull_requests_need_records_on_the_remote() {
    let g = OnGitHub::unkept("intake-unkept", "");
    let words = "main takes pull requests, which only the host that keeps it on origin takes \
                 in, and its state is each host's own: name [signers] in .config/sluice.toml so \
                 its state is kept on origin, as `sluice init actions` names a pool's signer (137)";
    g.by_hand("unit/each", &[("e.txt", Some("e\n"))], false);
    let w = g.session("refused", &[("r.txt", "r\n")]);
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "main", "--no-wait"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::CONFLICT), "{}", text(&out));
    assert!(text(&out).contains(words), "{}", text(&out));
    assert!(
        g.gh.forge.by_ref("unit/refused").unwrap().is_none(),
        "nothing opened"
    );
    assert_eq!(
        github::rev(g.bare.path(), "refs/heads/unit/refused"),
        None,
        "nothing pushed"
    );
    let said = g.sluice(&["step", "main"]);
    assert!(text(&said).contains(words), "{}", text(&said));
    let status = g.sluice(&["status", "main"]);
    assert!(text(&status).contains(words), "{}", text(&status));
    assert_eq!(g.lists(), 0, "GitHub was asked nothing of main's list");
}

/// request-intake-host, "The host that keeps it takes them in": main's
/// records name no host, and host A's pass keeps main and takes its pull
/// requests in (137; design D21).
#[test]
fn the_host_that_keeps_main_takes_its_requests_in() {
    let g = OnGitHub::new("intake-keeper", "");
    g.named("a", "");
    let (n, _) = g.by_hand("unit/shown", &[("s.txt", Some("s\n"))], false);
    g.on_main();
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "1"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(g.lead().as_deref(), Some("a"), "{}", text(&out));
    assert_eq!(g.of(n).len(), 1, "{}", text(&out));
}

/// satisfies: 137, record 0093 as amended 2026-11-08, record 0122
/// (design D8): host c signs as no signer main's policy names, so main does
/// not accept it: its pass takes none of main's pull requests in, writes
/// no lead and follows the request as a host that does not keep main;
/// host a, which main accepts, takes it in at its next pass.
#[test]
fn a_host_the_branch_does_not_accept_takes_no_request_in() {
    let g = OnGitHub::new("intake-unaccepted", "");
    g.named("a", "");
    let (n, _) = g.by_hand("unit/unaccepted", &[("u.txt", Some("u\n"))], false);
    let c = g.other("c", "", "");
    c.run("1");
    assert_eq!(g.lead(), None, "c writes no lead");
    assert!(c.of(n).is_empty(), "c takes nothing in");
    g.on_main();
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "1"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(g.lead().as_deref(), Some("a"), "{}", text(&out));
    assert_eq!(g.of(n).len(), 1, "{}", text(&out));
}

/// request-intake-host, "An older mark": a mark an older sluice left on
/// the remote names host C, and the host that keeps main takes `#12` in
/// all the same, reading nothing of the mark and fetching it nowhere
/// (design D23).
#[test]
fn an_older_mark_is_read_by_nothing() {
    let g = OnGitHub::new("intake-older-mark", "");
    g.named("a", "");
    let tree = g
        .repo
        .git(&["hash-object", "-t", "tree", "-w", "/dev/null"]);
    let said = "{\"host\":\"c\",\"holder\":\"driver\",\"sluice\":\"0.90.0\"}";
    let commit = g.repo.git(&["commit-tree", &tree, "-m", said]);
    g.repo.git(&[
        "push",
        "-q",
        "origin",
        &format!("{commit}:refs/sluice/intake/main"),
    ]);
    let (n, _) = g.by_hand("unit/older", &[("o.txt", Some("o\n"))], false);
    g.step();
    assert_eq!(g.lead().as_deref(), Some("a"));
    assert_eq!(g.of(n).len(), 1, "taken in");
    assert_eq!(g.of(n)[0]["from"]["worktree"], format!("pull/{n}"));
    let fetched = g
        .repo
        .git(&["for-each-ref", "refs/sluice-fetched/origin/intake/"]);
    assert_eq!(fetched, "", "the mark is fetched nowhere");
    assert_eq!(
        github::rev(g.bare.path(), "refs/sluice/intake/main").as_deref(),
        Some(commit.as_str()),
        "nor deleted"
    );
}

impl OnGitHub {
    /// Rewrite the JSON file `path` of main's records on the remote by `f`,
    /// as an older sluice wrote it, with a commit on
    /// `refs/sluice/state/main` over the last, its version one more.
    fn rewrite_on_remote(&self, path: &str, f: impl FnOnce(Value) -> Value) {
        let bare = self.bare.path();
        let git = |args: &[&str], input: Option<&str>, index: Option<&Path>| -> String {
            let mut c = Command::new("git");
            c.args(["--git-dir", bare.to_str().unwrap()])
                .args(args)
                .env("GIT_AUTHOR_NAME", "sluice")
                .env("GIT_AUTHOR_EMAIL", "sluice@localhost")
                .env("GIT_COMMITTER_NAME", "sluice")
                .env("GIT_COMMITTER_EMAIL", "sluice@localhost")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if let Some(i) = index {
                c.env("GIT_INDEX_FILE", i);
            }
            let mut child = c.spawn().unwrap();
            if let Some(text) = input {
                use std::io::Write as _;
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(text.as_bytes())
                    .unwrap();
            }
            drop(child.stdin.take());
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success(), "git {args:?}: {}", text(&out));
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let state = "refs/sluice/state/main";
        let old = git(&["rev-parse", state], None, None);
        let doc: Value =
            serde_json::from_str(&git(&["show", &format!("{old}:{path}")], None, None)).unwrap();
        let blob = git(
            &["hash-object", "-w", "--stdin"],
            Some(&serde_json::to_string_pretty(&f(doc)).unwrap()),
            None,
        );
        let version: u64 = git(&["show", &format!("{old}:version")], None, None)
            .parse()
            .unwrap();
        let next = git(
            &["hash-object", "-w", "--stdin"],
            Some(&format!("{}\n", version + 1)),
            None,
        );
        let scratch = Scratch::new("rewrite-index");
        let index = scratch.path().join("index");
        git(&["read-tree", &old], None, Some(&index));
        for (p, b) in [(path, &blob), ("version", &next)] {
            git(
                &["update-index", "--cacheinfo", &format!("100644,{b},{p}")],
                None,
                Some(&index),
            );
        }
        let tree = git(&["write-tree"], None, Some(&index));
        let commit = git(
            &["commit-tree", &tree, "-p", &old, "-m", "an older sluice"],
            None,
            None,
        );
        git(&["update-ref", state, &commit, &old], None, None);
    }

    /// Host A, called `a`, keeps main and took pull request `<name>` in;
    /// main's records on the remote are then rewritten as an older sluice
    /// left them when its hand-over to `b` withdrew that submission: its
    /// record withdrawn in the hand-over's words, naming `b`, and its
    /// combination gone. The request's number and head, and the
    /// submission's id.
    fn handed_over_before(&self, name: &str) -> (u64, String, u64) {
        self.named("a", "");
        let (n, head) = self.by_hand(name, &[("slow", Some("o\n"))], false);
        self.step();
        let id = self.of(n)[0]["id"].as_u64().expect("taken in");
        self.rewrite_on_remote(&format!("submissions/{id}.json"), |mut sub| {
            sub["state"] = "withdrawn".into();
            sub["withdrawn"] = serde_json::json!({
                "by": "b takes main's pull requests in",
                "at": "2026-11-06T12:00:00Z",
                "handed_over_to": "b",
            });
            sub
        });
        self.rewrite_on_remote("branch.json", |mut doc| {
            doc["candidates"] = serde_json::json!([]);
            doc
        });
        (n, head, id)
    }
}

/// request-intake-host, "An older hand-over" (120; record 0088 as amended;
/// design D24): main's records on the remote hold a submission an older
/// sluice withdrew in a hand-over to host B; the host that keeps main reads
/// it as holding no request and takes the pull request in again at its
/// head, with no draft made.
#[test]
fn an_older_hand_over_carried_onto_the_remote_is_taken_in_again() {
    let g = OnGitHub::new("intake-older-hand-over", "");
    let (n, head, id) = g.handed_over_before("unit/older");
    g.discovery_passes();
    let said = g.round();
    let subs = g.of(n);
    assert_eq!(subs.len(), 2, "{said}{subs:?}");
    assert_eq!(subs[0]["id"], id, "{subs:?}");
    assert_eq!(subs[1]["head"], head.as_str(), "{subs:?}");
    assert_ne!(subs[1]["state"], "withdrawn", "{subs:?}");
    assert!(!g.pull_from("unit/older").draft, "no draft made");
}

/// request-intake-host, "A wait on an older hand-over" (120; core.48;
/// record 0100; design D24): `sluice wait <id>` on a submission an older
/// sluice withdrew in a hand-over follows its pull request rather than
/// ending withdrawn.
#[test]
fn a_wait_on_an_older_hand_over_follows_its_pull_request() {
    let g = OnGitHub::new("wait-older-hand-over", "");
    let (n, _, id) = g.handed_over_before("unit/followed");
    let out = g.skipping(
        g.repo.dir(),
        &["wait", &id.to_string(), "--timeout", "2s", "--json"],
    );
    assert_ne!(out.status.code(), Some(sluice::WITHDRAWN), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    // The request taken in again at its head carries its work.
    let carrier = doc["carried_by"]["id"]
        .as_u64()
        .unwrap_or_else(|| panic!("{doc}"));
    assert!(carrier > id, "{doc}");
    assert_eq!(g.of(n).last().unwrap()["id"], carrier, "{doc}");
}

/// request-intake-host, "A host that stopped": host A kept main and took
/// a pull request in, then stopped; host B, having seen main's records
/// unchanged for its lease with work in them, keeps main from then on and
/// takes its pull requests in (72, 73, 137; design D21).
#[test]
fn a_host_that_stopped_passes_main_on_after_its_lease() {
    let g = OnGitHub::new("intake-lapsed", "");
    g.named("a", "");
    let (n, _) = g.by_hand("unit/lapsed", &[("slow", Some("l\n"))], false);
    g.step();
    assert_eq!(g.lead().as_deref(), Some("a"));
    assert_eq!(g.of(n).len(), 1, "A took it in");
    let b = g.other("b", "lease = \"30s\"\nalive = \"10s\"", "");
    let said = b.run("3");
    assert_eq!(g.lead().as_deref(), Some("a"), "within the lease: {said}");
    let said = b.run("20");
    assert_eq!(g.lead().as_deref(), Some("b"), "{said}");
    let (m, _) = g.by_hand("unit/after", &[("a.txt", Some("a\n"))], false);
    b.discovery_passes();
    let said = b.run("1");
    assert_eq!(b.of(m).len(), 1, "B takes main's pull requests in: {said}");
}

/// pull-request-intake, "A refused main" (137, 56, S64; design D22): main
/// is refused for taking pull requests on records each host keeps, and a
/// worktree's submission to `bolt/x`, which takes none, lands as before.
#[test]
fn a_refused_main_leaves_a_bolt_landing() {
    let g = OnGitHub::unkept("intake-bolt", BOLTS);
    g.repo.git(&["branch", "bolt/x", "main"]);
    g.repo
        .git(&["push", "-q", "origin", "bolt/x:refs/heads/bolt/x"]);
    let w = g.session("bolted", &[("b.txt", "b\n")]);
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "bolt/x", "--no-wait", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        let out = g.sluice(&["step", "bolt/x"]);
        assert!(out.status.success(), "{}", text(&out));
        let out = g.sluice(&["status", "bolt/x", "--json", "--all"]);
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        if v["submissions"][0]["state"] == "landed" {
            break;
        }
        assert!(Instant::now() < until, "never landed: {v}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

impl OnGitHub {
    /// The host main's records on the remote name as keeping it (72, 137;
    /// design D25); `None` where none does.
    fn lead(&self) -> Option<String> {
        let out = Command::new("git")
            .args(["--git-dir", self.bare.path().to_str().unwrap()])
            .args(["show", "refs/sluice/state/main:branch.json"])
            .output()
            .unwrap();
        let doc: Value = serde_json::from_slice(&out.stdout).ok()?;
        doc["lead"].as_str().map(str::to_string)
    }
}

/// request-intake-host, "Two waits and a status": host A follows its pull
/// request, which host B takes in, with `discovery` at sixty seconds; two
/// waits on it for a minute, and statuses of main beside them, read it and
/// its check from GitHub no more than four times each in all.
#[test]
fn two_waits_and_a_status_read_at_one_pace() {
    let g = OnGitHub::new("follows-pace", "reviews = 1");
    g.named("a", "");
    let b = g.other("b", "", "");
    b.run("1");
    let w = g.session("paced", &[("p.txt", "p\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/paced");
    let (before, checks) = (g.pull_reads(pull.number), g.posted_reads(&pull.head));
    let wait = || {
        g.command(env!("CARGO_BIN_EXE_sluice"), &w)
            .env("SLUICE_ROUND_SLEEP", "record")
            .args(["wait", "main", "--timeout", "57s"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let waits = [wait(), wait()];
    for _ in 0..6 {
        let out = g.sluice(&["status", "main"]);
        assert!(out.status.success(), "{}", text(&out));
    }
    let outs: Vec<Output> = waits
        .into_iter()
        .map(|w| w.wait_with_output().unwrap())
        .collect();
    let shown: Vec<String> = outs.iter().map(text).collect();
    for out in &outs {
        assert_eq!(out.status.code(), Some(sluice::WAITING), "{shown:?}");
    }
    let reads = g.pull_reads(pull.number) - before;
    assert!(reads <= 4, "{reads} reads of the request: {shown:?}");
    let reads = g.posted_reads(&pull.head) - checks;
    assert!(reads <= 4, "{reads} reads of its check: {shown:?}");
    assert!(g.of(pull.number).is_empty(), "no intake, no round");
}

impl OnGitHub {
    /// This host's next follow of pull request `n` into `main` reads it
    /// from GitHub, as if no process of the host had read it within a
    /// quarter of `discovery` (D4).
    fn follow_due(&self, n: u64) {
        let path = self.state.path().join("sluice/forge/follows/o/r/main.json");
        let mut v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        v["requests"][n.to_string()]["read_at"] = Value::Null;
        std::fs::write(&path, v.to_string()).unwrap();
    }

    /// sluice's check on `head` as another host's sluice left it, on
    /// GitHub alone: `status`, `title` and `summary`. Host A's next follow
    /// of pull request `n` reads it.
    fn posted_elsewhere(&self, n: u64, head: &str, status: Status, title: &str, summary: &str) {
        let posting = Posting {
            status,
            title: title.into(),
            summary: summary.into(),
            text: None,
        };
        self.gh
            .forge
            .post_named(&github::repo(), head, "sluice", None, &posting)
            .unwrap();
        self.follow_due(n);
    }
}

// ---------------------------------------------------------------------------
// submission-wait: a wait across hosts names where its pull request stands
// (137, design D2 and D5)
// ---------------------------------------------------------------------------

impl OnGitHub {
    /// Host B keeps main, its pass having run first, and this host, called
    /// `a`, opened a pull request from its session `name` by `wt submit
    /// --no-wait`: the session's worktree, and the pull request.
    fn opened_here_held_by_b(
        &self,
        name: &str,
    ) -> (PathBuf, sluice::profiles::github::forge::Pull) {
        self.named("a", "");
        self.other("b", "", "").run("1");
        let file = format!("{name}.txt");
        let w = self.session(name, &[(file.as_str(), "x\n")]);
        let out = self.wt_submit(&w, &["--no-wait"]);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out));
        (w, self.pull_from(&format!("unit/{name}")))
    }
}

/// sluice's check on a pull request's head as host B posts it while B
/// holds the request as `submission`, next on main.
fn queued_on_b(submission: u64) -> Posting {
    let block = serde_json::json!({
        "host": "b", "taken_in": true, "submission": submission, "state": "queued",
        "ahead": [], "waits_for": [],
    });
    Posting {
        status: Status::Queued,
        title: "queued: next on main".into(),
        summary: format!(
            "Submission {submission} on b.\n\n\
             In main's queue; it lands once its combination passes.\n\n\
             <!-- sluice:queue {block} -->"
        ),
        text: None,
    }
}

impl OnGitHub {
    /// sluice's check on `head`, pull request `n`'s, as host B posts it
    /// while B holds the request as `submission`; host A's next follow of
    /// it reads it.
    fn held_there(&self, n: u64, head: &str, submission: u64) {
        let p = queued_on_b(submission);
        self.posted_elsewhere(n, head, p.status, &p.title, &p.summary);
    }
}

// ---------------------------------------------------------------------------
// request-intake-host: status shows each followed request as the taking
// host's (137, design D2 and D5)
// ---------------------------------------------------------------------------

impl OnGitHub {
    /// `sluice status main --json` on this host, which succeeds: its
    /// document.
    fn status_document(&self) -> Value {
        let out = self.sluice(&["status", "main", "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)))
    }

    /// `sluice status main` on this host, which succeeds: what it says.
    fn status_words(&self) -> String {
        let out = self.sluice(&["status", "main"]);
        assert!(out.status.success(), "{}", text(&out));
        text(&out)
    }
}

/// The one `pull_requests` row of a status document, pull request `n`'s.
fn row_of(doc: &Value, n: u64) -> &Value {
    let rows = doc["pull_requests"]
        .as_array()
        .unwrap_or_else(|| panic!("{doc}"));
    assert_eq!(rows.len(), 1, "{doc}");
    assert_eq!(rows[0]["pull_request"], n, "{doc}");
    &rows[0]
}

// ---------------------------------------------------------------------------
// overview: a branch lists the pull requests another host takes in that
// this host follows (137, design D5)
// ---------------------------------------------------------------------------

/// `main`'s part of an overview document.
fn main_of(doc: &Value) -> &Value {
    doc["branches"]
        .as_array()
        .and_then(|b| b.iter().find(|b| b["branch"] == "main"))
        .unwrap_or_else(|| panic!("{doc}"))
}

/// overview, "A one-shot overview": host B's `sluice run` holds main's
/// mark and holds host A's pull request as submission 1406, and `sluice
/// overview --json` on A, which follows it, gives main `taken_in_by`
/// naming B and a row for the request whose queue names 1406, read from
/// GitHub as status reads it, with its `read_at`; its words list the same
/// row under main.
#[test]
fn a_one_shot_overview_lists_requests_taken_in_elsewhere() {
    let g = OnGitHub::new("overview-there", "");
    let (_w, pull) = g.opened_here_held_by_b("listed");
    let n = pull.number;
    g.held_there(n, &pull.head, 1406);
    let out = g.sluice(&["overview", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    let main = main_of(&doc);
    assert_eq!(
        main["taken_in_by"],
        serde_json::json!({ "host": "b" }),
        "{doc}"
    );
    let row = row_of(main, n);
    assert_eq!(
        (&row["queue"]["host"], &row["queue"]["submission"]),
        (&"b".into(), &1406.into()),
        "{doc}"
    );
    assert_eq!(
        (&row["state"], &row["from"], &row["head"]),
        (
            &"queued: next on main".into(),
            &"unit/listed".into(),
            &pull.head.as_str().into()
        ),
        "{doc}"
    );
    assert!(row["read_at"].is_string(), "{doc}");
    let out = g.sluice(&["overview"]);
    assert!(out.status.success(), "{}", text(&out));
    // Under main's line, `main at …` or `main · no records` where this
    // host has no records of it.
    let said = String::from_utf8_lossy(&out.stdout).to_string();
    let under: Vec<&str> = said
        .lines()
        .skip_while(|l| !l.starts_with("main "))
        .collect();
    let rows = format!(
        "\n  pull requests b takes in:\n    \
         #{n} {} · submission 1406 there · queued: next on main",
        pull.title
    );
    assert!(under.join("\n").contains(&rows), "{said}");
    assert!(g.of(n).is_empty(), "A took nothing in");
}

/// overview, "A watch reads none of them itself": `sluice overview --json
/// --watch` is host A's reader while A follows its pull request, which
/// host B holds, and nothing else on A runs. Its documents carry the
/// request's row as A last kept it, and carry it again once what A keeps
/// of it moves; the watch reads nothing from GitHub itself, and the host
/// process's reading reads the row that fell due at the host's one pace,
/// no more than four times a minute (core.31; design D25).
#[test]
fn a_watch_reads_no_followed_request_itself() {
    let g = OnGitHub::new("overview-watch-there", "");
    let (_w, pull) = g.opened_here_held_by_b("watched");
    let n = pull.number;
    g.held_there(n, &pull.head, 1406);
    // A status reads it from GitHub, so A keeps its check; then the row is
    // due again, so a process that asked GitHub would ask at once.
    assert_eq!(row_of(&g.status_document(), n)["queue"]["submission"], 1406);
    g.follow_due(n);
    let (pulls, checks) = (g.pull_reads(n), g.posted_reads(&pull.head));
    let mut watch = g
        .command(env!("CARGO_BIN_EXE_sluice"), g.repo.dir())
        .args(["overview", "--json", "--watch"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = std::io::BufReader::new(watch.stdout.take().unwrap());
    let (send, lines) = std::sync::mpsc::channel::<Value>();
    std::thread::spawn(move || {
        use std::io::BufRead as _;
        for line in stdout.lines().map_while(Result::ok) {
            let Ok(doc) = serde_json::from_str(&line) else {
                continue;
            };
            if send.send(doc).is_err() {
                break;
            }
        }
    });
    // The first document whose row for the request `holds` says so.
    let row_until = |holds: &dyn Fn(&Value) -> bool| -> Value {
        let until = Instant::now() + Duration::from_secs(30);
        let mut seen = Vec::new();
        while let Some(left) = until.checked_duration_since(Instant::now()) {
            let Ok(doc) = lines.recv_timeout(left) else {
                break;
            };
            let row = doc["branches"]
                .as_array()
                .and_then(|b| b.iter().find(|b| b["branch"] == "main"))
                .and_then(|m| m["pull_requests"].as_array())
                .and_then(|rows| rows.iter().find(|r| r["pull_request"] == n))
                .cloned();
            match row {
                Some(row) if holds(&row) => return row,
                _ => seen.push(doc),
            }
        }
        panic!("no such row: {seen:?}");
    };
    let row = row_until(&|r| r["queue"]["submission"] == 1406);
    assert_eq!(row["state"], "queued: next on main", "{row}");
    // What A keeps of it moves, as another process's read would move it:
    // the next document carries it.
    let path = g.state.path().join("sluice/forge/follows/o/r/main.json");
    let mut kept: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    kept["requests"][n.to_string()]["title"] = "Renamed while watched".into();
    std::fs::write(&path, kept.to_string()).unwrap();
    row_until(&|r| r["title"] == "Renamed while watched");
    // A few ticks more, the row still due.
    std::thread::sleep(Duration::from_secs(3));
    let _ = watch.kill();
    let out = watch.wait_with_output().unwrap();
    assert!(g.pull_reads(n) - pulls <= 4, "{}", text(&out));
    assert!(g.posted_reads(&pull.head) - checks <= 4, "{}", text(&out));
    assert!(g.of(n).is_empty(), "A took nothing in");
}

// ---------------------------------------------------------------------------
// forge-reads: one query reads a branch's open pull requests whole, and a
// refused one falls back to the reads per pull request (136; rule core.32)
// ---------------------------------------------------------------------------

/// An address that refuses each call: a command run with it as
/// `SLUICE_FORGE_API` that read the forge itself would fail.
const REFUSING: &str = "http://127.0.0.1:9";

/// resident-changes, "A submit to a branch that takes pull requests" (the
/// change `sluice-host-makes-each-change`, design D2): with `sluice host`
/// running on the test's forge, `sluice submit main --no-wait` run with
/// `SLUICE_FORGE_API` naming an address that refuses each call exits 0 as
/// it did, and the forge holds an open request from the worktree's branch,
/// marked ready, the remote the worktree's head on that branch.
#[test]
fn a_submit_to_a_branch_that_takes_pull_requests_reads_no_forge() {
    let g = OnGitHub::new("pr-submit-no-forge", "");
    let w = g.session("nf", &[("nf.txt", "nf\n")]);
    // `sluice host`, started on the test's forge and kept running by a
    // watch's connection, so the submit meets it and starts none of its
    // own on the refusing address however long the submit takes to come.
    let mut watch = g
        .command(env!("CARGO_BIN_EXE_sluice"), g.repo.dir())
        .args(["overview", "--json", "--watch"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut first = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(watch.stdout.as_mut().unwrap()),
        &mut first,
    )
    .unwrap();
    assert!(!first.is_empty(), "the watch saw no host process");
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .env("SLUICE_FORGE_API", REFUSING)
        .args(["submit", "main", "--no-wait", "--json"])
        .output()
        .unwrap();
    let _ = watch.kill();
    let _ = watch.wait();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    let pull = g.pull_from("unit/nf");
    assert_eq!(doc["pull_request"], pull.number, "{doc}");
    assert!(pull.open, "{pull:?}");
    assert!(!pull.draft, "{pull:?}");
    let head = g
        .repo
        .git(&["-C", w.to_str().unwrap(), "rev-parse", "HEAD"]);
    assert_eq!(pull.head, head);
    assert_eq!(
        github::rev(g.bare.path(), "refs/heads/unit/nf").as_deref(),
        Some(head.as_str())
    );
}

/// resident-changes, "A wait on a request that lands" (design D2): with
/// request `#n` open into `main`, `sluice wait main --pull-request <n>` run
/// with `SLUICE_FORGE_API` naming an address that refuses each call prints
/// the landing, as a wait printed it, once `sluice host` lands the request,
/// and exits 0.
#[test]
fn a_wait_on_a_request_that_lands_reads_no_forge() {
    let g = OnGitHub::new("pr-wait-no-forge", "");
    let w = g.session("wl", &[("wl.txt", "wl\n")]);
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "main", "--no-wait"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let n = g.pull_from("unit/wl").number.to_string();
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), g.repo.dir())
        .env("SLUICE_FORGE_API", REFUSING)
        .args(["wait", "main", "--pull-request", &n, "--timeout", "2m"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    let landed = g.main_on_github();
    assert!(
        text(&out).contains(&format!("landed as {}", &landed[..7])),
        "{}",
        text(&out)
    );
}
