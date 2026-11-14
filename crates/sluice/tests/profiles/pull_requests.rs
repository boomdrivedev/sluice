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
use sluice::profiles::github::forge::{Forge as _, Posting, Standing, Status};
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

    /// `main` on GitHub whose runs the time limit `timeout` ends: one
    /// shorter than five seconds ends the run of a head with `slow`.
    fn with_timeout(name: &str, conditions: &str, timeout: &str) -> OnGitHub {
        OnGitHub::checked(name, conditions, timeout, "")
    }

    /// `main` testing only its longest combination (C.7 `batch`).
    fn batched(name: &str, conditions: &str) -> OnGitHub {
        OnGitHub::checked(name, conditions, "1m", "batch = true\n")
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
        use ssh_key::{Algorithm, PrivateKey};
        let keys = [
            PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap(),
            PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap(),
        ];
        let repo = Repo::new(name);
        repo.write(".config/sluice.toml", &signed(policy, &keys));
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

    /// A commit on top of `name`'s head, pushed; returns the new head.
    fn push_more(&self, name: &str, path: &str) -> String {
        self.repo.git(&["switch", "-q", name]);
        self.repo.write(path, "more\n");
        self.repo.git(&["add", "-A"]);
        self.repo.git(&["commit", "-q", "-m", path]);
        let head = self.repo.rev("HEAD");
        self.repo.git(&["switch", "-q", "--detach", "main"]);
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

    /// What sluice's check on `head` reads.
    fn posted(&self, head: &str) -> Option<(Status, String, String)> {
        let held = self.gh.forge.held().unwrap();
        let p = held.posted.get(head)?.iter().find(|p| p.name == "sluice")?;
        Some((
            p.posting.status,
            p.posting.title.clone(),
            p.posting.summary.clone(),
        ))
    }

    /// The report comment on pull request `n`.
    fn report(&self, n: u64) -> Option<String> {
        self.comments(n)
            .into_iter()
            .find(|c| c.starts_with("<!-- sluice-report: main -->"))
    }

    /// sluice's check named `name` on `head`: its status and title.
    fn check(&self, head: &str, name: &str) -> Option<(Status, String)> {
        let held = self.gh.forge.held().unwrap();
        let p = held.posted.get(head)?.iter().find(|p| p.name == name)?;
        Some((p.posting.status, p.posting.title.clone()))
    }

    /// The text of sluice's check named `name` on `head`.
    fn text(&self, head: &str, name: &str) -> Option<String> {
        let held = self.gh.forge.held().unwrap();
        let p = held.posted.get(head)?.iter().find(|p| p.name == name)?;
        p.posting.text.clone()
    }

    fn posts(&self) -> usize {
        self.gh.forge.held().unwrap().posts.len()
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
            std::thread::sleep(Duration::from_millis(200));
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

    /// A commit in the worktree `dir` writing `path`, its message
    /// `message`. Returns the new head.
    fn commit_message(&self, dir: &Path, path: &str, message: &str) -> String {
        std::fs::write(dir.join(path), format!("{path}\n")).unwrap();
        let dir = dir.to_str().unwrap();
        self.repo.git(&["-C", dir, "add", "-A"]);
        self.repo.git(&["-C", dir, "commit", "-q", "-m", message]);
        self.repo.git(&["-C", dir, "rev-parse", "HEAD"])
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

    /// A commit of `changes` in the worktree `dir`, pushed to GitHub by git
    /// alone, as a fix another submission lands on its branch would be:
    /// no submit pushes it. Returns the new head.
    fn push_from(&self, dir: &Path, changes: &[(&str, &str)]) -> String {
        let head = self.commit_in(dir, changes);
        let branch = self.repo.git(&[
            "-C",
            dir.to_str().unwrap(),
            "rev-parse",
            "--abbrev-ref",
            "HEAD",
        ]);
        self.repo.git(&[
            "push",
            "-q",
            "origin",
            &format!("{branch}:refs/heads/{branch}"),
        ]);
        head
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
            std::thread::sleep(Duration::from_millis(200));
        }
        self.bolt_on_github()
    }

    /// The submissions from pull request `n`, after `steps` more rounds.
    fn of_after(&self, n: u64, steps: usize) -> Vec<Value> {
        for _ in 0..steps {
            self.step();
        }
        self.of(n)
    }

    /// The repository's driver on the host, the test's own, until
    /// dropped: a `sluice run` living beside the commands it starts (135).
    fn driver(&self) -> sluice::reader::Driver {
        let readers = self.state.path().join("sluice").join("readers");
        let repository = std::fs::canonicalize(self.repo.dir().join(".git")).unwrap();
        sluice::reader::Driver::take(&readers, repository.to_str().unwrap())
            .unwrap()
            .expect("no driver of the repository yet")
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

/// S21, waiting for QA: approved and out of draft, it waits while
/// `qa/approved` has not passed on its head; once it passes, the next
/// round submits it.
#[test]
fn waiting_for_qa() {
    let g = OnGitHub::new("pr-qa", "reviews = 1\nchecks = [\"qa/approved\"]");
    let (n, head) = g.by_hand("unit/qa", &[("qa.txt", Some("qa\n"))], false);
    g.gh.forge.review(n, "reviewer", "APPROVED", &head).unwrap();
    g.gh.forge
        .check(&head, "qa/approved", Standing::Pending)
        .unwrap();
    g.step();
    assert!(g.of(n).is_empty(), "{:?}", g.subs());
    g.gh.forge
        .check(&head, "qa/approved", Standing::Passed)
        .unwrap();
    g.step();
    let subs = g.of(n);
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0]["head"], head.as_str());
    // The agent is the pull request's author, with no marker in its body.
    assert_eq!(subs[0]["from"]["agent"], "person");
}

/// A draft waits, every condition holding, until it is marked ready.
#[test]
fn a_draft_waits() {
    let g = OnGitHub::new("pr-draft", "");
    let (n, head) = g.by_hand("unit/draft", &[("d.txt", Some("d\n"))], true);
    g.step();
    g.step();
    assert!(g.of(n).is_empty(), "{:?}", g.subs());
    assert_eq!(g.posted(&head).unwrap().1, "waiting: a draft");
    g.gh.forge.set_draft(n, false).unwrap();
    g.step();
    assert_eq!(g.of(n).len(), 1, "{:?}", g.subs());
}

/// S23: a pull request opened by hand, ready, is submitted by the next
/// round with no `sluice submit` run for it.
#[test]
fn a_pull_request_opened_by_hand() {
    let g = OnGitHub::new("pr-by-hand", "reviews = 0");
    let (n, head) = g.by_hand("unit/hand", &[("h.txt", Some("h\n"))], false);
    // Its body names the agent whose work it is.
    g.gh.forge
        .edit(
            &github::repo(),
            n,
            None,
            "Hand work.\n\n<!-- sluice: agent=crw-1-unit-1 -->",
        )
        .unwrap();
    g.step();
    let subs = g.of(n);
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0]["head"], head.as_str());
    assert_eq!(subs[0]["from"]["agent"], "crw-1-unit-1");
    assert_eq!(subs[0]["from"]["worktree"], format!("pull/{n}"));
    // The branch it was opened from, for the home view (ADR 0025).
    assert_eq!(subs[0]["from"]["head"], "unit/hand");
    assert!(subs[0]["from"].get("path").is_none());
}

/// C.7 `reviews`: an approval given on an earlier head does not count.
#[test]
fn reviews_on_an_older_head_do_not_count() {
    let g = OnGitHub::new("pr-stale", "reviews = 1");
    let (n, old) = g.by_hand("unit/stale", &[("s.txt", Some("s\n"))], false);
    g.gh.forge.review(n, "reviewer", "APPROVED", &old).unwrap();
    let new = g.push_more("unit/stale", "s2.txt");
    g.step();
    assert!(g.of(n).is_empty(), "{:?}", g.subs());
    let (_, title, _) = g.posted(&new).unwrap();
    assert_eq!(title, "waiting: needs 1 review");
}

/// A fork's pull request is never submitted, whatever its conditions.
#[test]
fn a_forks_pull_request_is_never_submitted() {
    let g = OnGitHub::new("pr-fork", "reviews = 0");
    let head = g.repo.branch("fork", "main", &[("f.txt", Some("f\n"))]);
    let n =
        g.gh.forge
            .open_by("stranger", "main", "fork", &head, false, Some("stranger/r"))
            .unwrap();
    // GitHub keeps a fork's head in the repository it is into.
    g.repo.git(&[
        "push",
        "-q",
        "origin",
        &format!("{head}:refs/pull/{n}/head"),
    ]);
    g.step();
    g.step();
    assert!(g.of(n).is_empty(), "{:?}", g.subs());
    assert_eq!(g.posted(&head), None);
}

/// S22: a commit pushed to a submitted pull request withdraws its
/// submission; it waits for its conditions on the new head, and is
/// submitted again once they hold.
#[test]
fn a_new_commit_while_queued() {
    let g = OnGitHub::new("pr-new-commit", "reviews = 1");
    let (n, old) = g.by_hand("unit/moving", &[("slow", Some("x\n"))], false);
    g.gh.forge.review(n, "reviewer", "APPROVED", &old).unwrap();
    g.step();
    assert_eq!(g.of(n).len(), 1);
    let new = g.push_more("unit/moving", "m.txt");
    g.step();
    let subs = g.of(n);
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0]["state"], "withdrawn");
    assert_eq!(g.posted(&new).unwrap().1, "waiting: needs 1 review");
    g.gh.forge.review(n, "reviewer", "APPROVED", &new).unwrap();
    g.step();
    let subs = g.of(n);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], new.as_str());
    assert_ne!(subs[1]["state"], "withdrawn");
}

/// S24: a submitted pull request closed is withdrawn.
#[test]
fn closed_while_queued() {
    let g = OnGitHub::new("pr-closed", "");
    let (n, _) = g.by_hand("unit/closed", &[("slow", Some("x\n"))], false);
    g.step();
    assert_eq!(g.of(n).len(), 1);
    g.gh.forge.close(n).unwrap();
    g.step();
    assert_eq!(g.of(n)[0]["state"], "withdrawn", "{:?}", g.subs());
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

/// 122, 5; pull-request-intake, "Landed by another queue": a submitted pull
/// request another queue of `main` lands, under test here, leaves GitHub's
/// open list as merged; its submission here is landed as that landing,
/// never withdrawn, its run ended, and a wait on it exits 0.
#[test]
fn landed_by_another_queue_while_queued() {
    let g = OnGitHub::new("pr-landed-elsewhere", "");
    let (n, head) = g.by_hand("unit/elsewhere", &[("slow", Some("x\n"))], false);
    g.step();
    let id = g.of(n)[0]["id"].as_u64().unwrap();
    assert_eq!(g.of(n)[0]["state"], "testing", "{:?}", g.of(n));
    let landing = landed_elsewhere(&g, &head);
    g.discovery_passes();
    g.step();
    let a = g.of(n);
    assert_eq!(a.len(), 1, "{a:?}");
    assert_eq!(a[0]["state"], "landed", "{a:?}");
    assert_eq!(a[0]["landed_as"], landing.as_str());
    assert_eq!(a[0]["landed_elsewhere"], true);
    let log = g.log();
    assert!(!log.iter().any(|l| l["event"] == "withdrawn"), "{log:?}");
    assert!(
        log.iter()
            .any(|l| l["event"] == "found_landed" && l["to"] == landing.as_str()),
        "{log:?}"
    );
    assert_eq!(g.main_on_github(), landing, "nothing more is landed of it");
    let out = g.sluice(&["wait", &id.to_string(), "--json"]);
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["state"], "landed");
    assert_eq!(doc["landed_as"], landing.as_str());
}

/// 122: a submission withdrawn whose work the branch then took, as a copy
/// whose intake withdrew a request another queue landed left its records,
/// is reported landed as the commit that landed it, by `sluice wait <id>`
/// and by `sluice wait main` from its session, never as withdrawn.
#[test]
fn a_withdrawn_submission_whose_work_landed_is_reported_landed() {
    let g = OnGitHub::new("pr-withdrawn-landed", "");
    let w = g.session("withdrawn-landed", &[("slow", "x\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/withdrawn-landed");
    let id = g.of(pull.number)[0]["id"].as_u64().unwrap();
    let out = g.sluice(&["withdraw", &id.to_string()]);
    assert!(out.status.success(), "{}", text(&out));
    let landing = landed_elsewhere(&g, &pull.head);
    let out = g.sluice(&["wait", &id.to_string(), "--json"]);
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["state"], "landed", "{doc}");
    assert_eq!(doc["landed_as"], landing.as_str());
    assert_eq!(
        g.of(pull.number)[0]["state"],
        "withdrawn",
        "its record stands"
    );
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["wait", "main"])
        .output()
        .unwrap();
    let said = text(&out);
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{said}");
    assert!(
        said.contains(&format!("landed as {}", &landing[..7])),
        "{said}"
    );
    assert!(!said.contains("closed without landing"), "{said}");
}

/// 122; record 0100 D5: beside a live driver, whose tick is the only fetch
/// of `main`, `sluice wait main` follows a pull request this host took in;
/// another queue then lands it, and GitHub reads it closed, with no fetch
/// of `main` on this host since the wait began. The wait reads the closed
/// request against `main` as a round reads it, from its remote: it says
/// landed as the commit that landed it and exits 0, never closed without
/// landing.
#[test]
fn a_wait_beside_a_driver_reads_a_request_another_queue_landed_from_the_branch() {
    let g = OnGitHub::new("pr-closed-beside-driver", "");
    // The wait reads the request again every quarter of it.
    g.discovery("10s");
    let w = g.session("beside", &[("slow", "x\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/beside");
    assert_eq!(g.of(pull.number).len(), 1, "{:?}", g.subs());
    let _driver = g.driver();
    let mut child = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["wait", "main", "--timeout", "60s"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let heard = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let reader = {
        let heard = heard.clone();
        let mut stdout = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n @ 1..) = stdout.read(&mut buf) {
                heard
                    .lock()
                    .unwrap()
                    .push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        })
    };
    // Landed elsewhere once the wait has read the request open.
    let until = Instant::now() + Duration::from_secs(60);
    while heard.lock().unwrap().is_empty() {
        assert!(
            Instant::now() < until,
            "the wait never said where it stands"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let landing = landed_elsewhere(&g, &pull.head);
    let status = ended(child).status;
    reader.join().unwrap();
    let said = heard.lock().unwrap().clone();
    assert_eq!(status.code(), Some(sluice::LANDED), "{said}");
    assert!(
        said.contains(&format!("landed as {}", &landing[..7])),
        "{said}"
    );
    assert!(!said.contains("closed without landing"), "{said}");
}

/// 5, 120: the operator's withdrawal of a hand-opened pull request's
/// submission rests it on its head, open and ready: no work of it goes
/// on, so a wait on it exits 4 at once.
#[test]
fn a_wait_on_a_request_the_operator_withdrew_exits_withdrawn() {
    let g = OnGitHub::new("pr-operator-withdrew", "");
    let (n, _) = g.by_hand("unit/rested", &[("slow", Some("x\n"))], false);
    g.step();
    let id = g.of(n)[0]["id"].as_u64().unwrap();
    let out = g.sluice(&["withdraw", &id.to_string()]);
    assert!(out.status.success(), "{}", text(&out));
    let out = g.sluice(&["wait", &id.to_string(), "--timeout", "20s", "--json"]);
    assert_eq!(out.status.code(), Some(sluice::WITHDRAWN), "{}", text(&out));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["state"], "withdrawn", "{doc}");
    assert_eq!(doc["id"], id, "{doc}");
}

/// 67, 122; record 0100 D2, D4: a wait on a pull request's submission its
/// own submit withdrew for a new head follows the request to the
/// submission of that head, and ends with its own submission landed by
/// that one's landing, naming it in `carried_by`.
#[test]
fn a_wait_follows_the_new_head_its_submit_withdrew_it_for() {
    let g = OnGitHub::new("pr-follow-new-head", "");
    let w = g.session("follow", &[("slow", "x\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let n = g.pull_from("unit/follow").number;
    let first = g.of(n)[0]["id"].as_u64().unwrap();
    let new = g.commit_in(&w, &[("more.txt", "more\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["wait", &first.to_string(), "--timeout", "60s", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    let subs = g.of(n);
    let second = subs
        .iter()
        .find(|x| x["head"] == new.as_str())
        .unwrap_or_else(|| panic!("{subs:?}"));
    assert_eq!(second["state"], "landed", "{second}");
    assert_eq!(doc["id"], first, "{doc}");
    assert_eq!(doc["state"], "landed", "{doc}");
    assert_eq!(doc["landed_elsewhere"], true, "{doc}");
    assert_eq!(doc["landed_as"], second["landed_as"], "{doc}");
    assert_eq!(
        doc["carried_by"],
        serde_json::json!({ "id": second["id"], "state": "landed" }),
        "{doc}"
    );
    assert_eq!(g.of(n)[0]["state"], "withdrawn");
}

/// 67, 120; record 0100 D2, D3: a pull request closed on the new head that
/// carries a withdrawn submission's work, before that head lands, ends the
/// withdrawn submission's wait withdrawn, exit 4, its own record naming the
/// request closed in `carried_by`.
#[test]
fn a_wait_exits_withdrawn_when_the_request_carrying_its_work_closes() {
    let g = OnGitHub::new("pr-follow-closed", "");
    let w = g.session("follow-closed", &[("slow", "x\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let n = g.pull_from("unit/follow-closed").number;
    let first = g.of(n)[0]["id"].as_u64().unwrap();
    g.commit_in(&w, &[("more.txt", "more\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(g.of(n).len(), 2, "{:?}", g.of(n));
    g.gh.forge.close(n).unwrap();
    // The wait's first intake asks GitHub, as it does once the host's
    // interval has passed.
    g.discovery_passes();
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["wait", &first.to_string(), "--timeout", "60s", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::WITHDRAWN), "{}", text(&out));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["id"], first, "{doc}");
    assert_eq!(doc["state"], "withdrawn", "{doc}");
    assert_eq!(
        doc["carried_by"],
        serde_json::json!({ "pull_request": n, "state": "closed" }),
        "{doc}"
    );
}

/// A blamed pull request whose author marks it a draft and then ready
/// again, with no new commits, is submitted again on the same head.
#[test]
fn blamed_then_retried() {
    let g = OnGitHub::new("pr-retried", "");
    let (n, head) = g.by_hand("unit/retried", &[("broken", Some("x\n"))], false);
    g.step_until("blamed", |g| {
        g.of(n).first().is_some_and(|s| s["state"] == "failed")
    });
    // The ready-for-review event must be later than the blame, to the
    // second GitHub and the records keep.
    std::thread::sleep(Duration::from_millis(1100));
    g.gh.forge.set_draft(n, true).unwrap();
    g.gh.forge.set_draft(n, false).unwrap();
    g.step();
    let subs = g.of(n);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], head.as_str());
}

/// A blamed pull request left alone is never submitted again, and is
/// commented on once.
#[test]
fn blamed_and_left_alone() {
    let g = OnGitHub::new("pr-left-alone", "");
    let (n, head) = g.by_hand("unit/alone", &[("broken", Some("x\n"))], false);
    g.step_until("blamed", |g| {
        g.of(n).first().is_some_and(|s| s["state"] == "failed")
    });
    for _ in 0..3 {
        g.step();
    }
    assert_eq!(g.of(n).len(), 1, "{:?}", g.subs());
    let (status, title, _) = g.posted(&head).unwrap();
    assert_eq!(
        (status, title.as_str()),
        (Status::Failure, "failed: sh check.sh")
    );
    // pull-request-status, blamed: one comment naming the failed check,
    // its log, and how to submit again, pointing at the report; and the
    // report, one comment of its own (ADR 0044).
    let comments = g.comments(n);
    assert_eq!(comments.len(), 2, "{comments:?}");
    let c = comments.iter().find(|c| c.contains("told=")).unwrap();
    assert!(c.contains("`sh check.sh` failed"), "{c}");
    assert!(c.contains("The logs, on the host"), "{c}");
    assert!(
        c.contains("Push a fix") && c.contains("draft and then ready"),
        "{c}"
    );
    assert!(c.contains("[sluice's report]("), "{c}");
    let r = g.report(n).unwrap();
    assert!(
        r.contains("## sluice: failed on main, #1 is to blame"),
        "{r}"
    );
    assert!(r.contains("| 🟥 | `sh check.sh` |"), "{r}");
    // The `sluice` check carries the same report, and the check has its
    // own check.
    assert_eq!(g.text(&head, "sluice").as_deref(), Some(r.as_str()));
    let own = g.check(&head, "sluice / sh check.sh").unwrap();
    assert_eq!(own.0, Status::Failure);
    // Output is posted only when the policy asks for it.
    assert!(!r.contains("it broke"), "{r}");
}

/// A pull request opened by hand, approved and submitted, whose
/// submission the operator withdraws with `sluice withdraw`; returns its
/// number and head.
fn withdrawn_by_hand(g: &OnGitHub, name: &str) -> (u64, String) {
    let (n, head) = g.by_hand(name, &[("slow", Some("x\n"))], false);
    g.gh.forge.review(n, "reviewer", "APPROVED", &head).unwrap();
    g.step();
    let id = g.of(n)[0]["id"].as_u64().unwrap().to_string();
    let out = g.sluice(&["withdraw", &id, "--as", "operator"]);
    assert!(out.status.success(), "{}", text(&out));
    (n, head)
}

/// Withdrawn by the operator, opened by hand (120, C.5): its conditions
/// holding, no later round submits it; its check names the operator and
/// how to bring it back, and no comment is left.
#[test]
fn withdrawn_by_the_operator_opened_by_hand() {
    let g = OnGitHub::new("pr-rests", "reviews = 1");
    let (n, head) = withdrawn_by_hand(&g, "unit/rests");
    let subs = g.of_after(n, 4);
    assert_eq!(subs.len(), 1, "never submitted again: {subs:?}");
    assert_eq!(subs[0]["state"], "withdrawn");
    assert_eq!(subs[0]["withdrawn"]["by"], "operator");
    assert!(!g.pull_from("unit/rests").draft);
    let (status, title, summary) = g.posted(&head).unwrap();
    assert_eq!(
        (status, title.as_str()),
        (Status::Queued, "waiting: withdrawn by the operator")
    );
    assert!(
        summary.contains("by the operator")
            && summary.contains("New commits, or marking it a draft and then ready for review")
            && summary.contains("Forecast:"),
        "{summary}"
    );
    assert!(
        !g.comments(n).iter().any(|c| c.contains("told=")),
        "{:?}",
        g.comments(n)
    );
}

/// Withdrawn by the operator, then retried by its author: marked draft and
/// then ready, with no new commits, the same head is submitted again.
#[test]
fn withdrawn_by_the_operator_then_retried_by_its_author() {
    let g = OnGitHub::new("pr-rests-retried", "reviews = 1");
    let (n, head) = withdrawn_by_hand(&g, "unit/rests-retried");
    assert_eq!(g.of_after(n, 2).len(), 1, "{:?}", g.subs());
    // The ready-for-review event must be later than the withdrawal, to the
    // second GitHub and the records keep.
    std::thread::sleep(Duration::from_millis(1100));
    g.gh.forge.set_draft(n, true).unwrap();
    g.step();
    assert_eq!(g.of(n).len(), 1, "a draft waits: {:?}", g.subs());
    g.gh.forge.set_draft(n, false).unwrap();
    let subs = g.of_after(n, 1);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], head.as_str());
    assert_ne!(subs[1]["state"], "withdrawn", "{subs:?}");
}

/// Withdrawn by the operator, then a new commit: the new head is
/// submitted once its conditions hold on it.
#[test]
fn withdrawn_by_the_operator_then_a_new_commit() {
    let g = OnGitHub::new("pr-rests-new-head", "reviews = 1");
    let (n, _) = withdrawn_by_hand(&g, "unit/rests-moved");
    assert_eq!(g.of_after(n, 2).len(), 1, "{:?}", g.subs());
    let new = g.push_more("unit/rests-moved", "m.txt");
    g.step();
    assert_eq!(g.of(n).len(), 1, "waits for its review: {:?}", g.subs());
    assert_eq!(g.posted(&new).unwrap().1, "waiting: needs 1 review");
    g.gh.forge.review(n, "reviewer", "APPROVED", &new).unwrap();
    let subs = g.of_after(n, 1);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], new.as_str());
    assert_ne!(subs[1]["state"], "withdrawn", "{subs:?}");
}

/// The crew #127/#128 case (120, S56): a pull request `wt submit` opened
/// is queued, and a commit no submit pushed moves its head. Its
/// submission is withdrawn, sluice marks it a draft, and no round submits
/// it in its agent's name, whatever its conditions.
#[test]
fn a_new_head_no_submit_pushed() {
    let g = OnGitHub::new("pr-held-new-head", "");
    let w = g.session("held", &[("slow", "x\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let n = g.pull_from("unit/held").number;
    assert_eq!(g.of(n).len(), 1, "queued: {:?}", g.subs());
    let main = g.main_on_github();
    let new = g.push_from(&w, &[("fix.txt", "a fix\n")]);
    let subs = g.of_after(n, 4);
    assert_eq!(subs.len(), 1, "never submitted again: {subs:?}");
    assert_eq!(subs[0]["state"], "withdrawn", "{subs:?}");
    let pull = g.pull_from("unit/held");
    assert!(pull.open && pull.draft, "{pull:?}");
    assert_eq!(pull.head, new);
    assert_eq!(g.main_on_github(), main, "nothing landed");
    assert_eq!(
        g.posted(&new).map(|p| (p.0, p.1)),
        Some((Status::Queued, "waiting: the next submit".into()))
    );
}

/// Withdrawn by the operator: a pull request `wt submit` opened, its
/// submission withdrawn with `sluice withdraw`, is marked a draft, and no
/// later round submits it.
#[test]
fn withdrawn_by_the_operator() {
    let g = OnGitHub::new("pr-held-withdrawn", "");
    let w = g.session("pulled", &[("slow", "x\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/pulled");
    let id = g.of(pull.number)[0]["id"].as_u64().unwrap().to_string();
    let out = g.sluice(&["withdraw", &id, "--as", "operator"]);
    assert!(out.status.success(), "{}", text(&out));
    let subs = g.of_after(pull.number, 3);
    assert_eq!(subs.len(), 1, "never submitted again: {subs:?}");
    assert_eq!(subs[0]["state"], "withdrawn");
    assert!(g.pull_from("unit/pulled").draft);
    assert_eq!(
        g.posted(&pull.head).unwrap().1,
        "waiting: the next submit",
        "{:?}",
        g.posted(&pull.head)
    );
}

/// Marked ready by a person: a pull request `wt submit` opened, held and
/// marked a draft after its head moved, is submitted at its new head once
/// a person marks it ready for review on GitHub.
#[test]
fn held_then_marked_ready_by_a_person() {
    let g = OnGitHub::new("pr-held-ready", "");
    let w = g.session("readied", &[("slow", "x\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let n = g.pull_from("unit/readied").number;
    let new = g.push_from(&w, &[("fix.txt", "a fix\n")]);
    assert_eq!(g.of_after(n, 2).len(), 1);
    assert!(g.pull_from("unit/readied").draft);
    // The ready-for-review event must be later than the withdrawal, to the
    // second GitHub and the records keep: one in the second the submission
    // was submitted and withdrawn in is the submit's own.
    std::thread::sleep(Duration::from_millis(1100));
    g.gh.forge.set_draft(n, false).unwrap();
    let subs = g.of_after(n, 1);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], new.as_str());
    assert_ne!(subs[1]["state"], "withdrawn", "{subs:?}");
}

/// Held when it cannot be marked a draft: GitHub refuses the draft, and
/// no round submits the pull request; its check says it waits for the
/// next submit, and each asking tries the draft again, until one is taken.
#[test]
fn held_when_github_refuses_the_draft() {
    let g = OnGitHub::new("pr-held-refused", "");
    let w = g.session("refused", &[("slow", "x\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let n = g.pull_from("unit/refused").number;
    g.gh.refuse_drafts(true);
    let new = g.push_from(&w, &[("fix.txt", "a fix\n")]);
    for _ in 0..3 {
        let out = g.round();
        assert!(
            out.contains(&format!(
                "#{n} waits for the next submit, and GitHub would not mark it a draft"
            )) && out.contains("Resource not accessible by integration"),
            "{out}"
        );
    }
    let subs = g.of(n);
    assert_eq!(subs.len(), 1, "never submitted again: {subs:?}");
    assert_eq!(subs[0]["state"], "withdrawn");
    assert!(!g.pull_from("unit/refused").draft);
    assert_eq!(g.posted(&new).unwrap().1, "waiting: the next submit");
    g.gh.refuse_drafts(false);
    g.step();
    assert!(g.pull_from("unit/refused").draft);
    assert_eq!(g.of(n).len(), 1, "{:?}", g.subs());
}

/// pull-request-status, edited not repeated (ADR 0044): one report
/// comment, edited from testing to landed.
#[test]
fn one_report_comment_edited_until_landed() {
    let g = OnGitHub::new("pr-report-edited", "");
    let (n, head) = g.by_hand("unit/fine", &[("slow", Some("x\n"))], false);
    g.step_until("landed", |g| {
        g.of(n).first().is_some_and(|s| s["state"] == "landed")
    });
    for _ in 0..2 {
        g.step();
    }
    let reports: Vec<String> = g
        .comments(n)
        .into_iter()
        .filter(|c| c.starts_with("<!-- sluice-report: main -->"))
        .collect();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(
        reports[0].contains("## sluice: landed on main as "),
        "{}",
        reports[0]
    );
    assert!(
        reports[0].contains("| 🟩 | `sh check.sh` |"),
        "{}",
        reports[0]
    );
    let own = g.check(&head, "sluice / sh check.sh").unwrap();
    assert_eq!(own.0, Status::Success);
    // Nothing reads differently, so a further round posts nothing.
    let before = g.posts();
    let edits = g.gh.forge.held().unwrap().edits.len();
    g.step();
    assert_eq!(g.posts(), before);
    assert_eq!(g.gh.forge.held().unwrap().edits.len(), edits);
}

/// S50 (118, 20): behind a blamed pull request, nothing of the
/// failed combination is shown.
#[test]
fn behind_a_blamed_pull_request_no_failure_is_shown() {
    let g = OnGitHub::new("pr-report-behind", "");
    let (one, _) = g.by_hand("unit/broken", &[("broken", Some("x\n"))], false);
    let (two, head) = g.by_hand("unit/fine", &[("fine", Some("x\n"))], false);
    g.step_until("blamed", |g| {
        g.of(one).first().is_some_and(|s| s["state"] == "failed")
    });
    let shown = g.report(two).unwrap_or_default();
    assert!(!shown.contains("#1") && !shown.contains("🟥"), "{shown}");
    if let Some((status, _, _)) = g.posted(&head) {
        assert_ne!(status, Status::Failure);
    }
    if let Some(own) = g.check(&head, "sluice / sh check.sh") {
        assert_ne!(own.0, Status::Failure, "{own:?}");
    }
    g.step_until("landed", |g| {
        g.of(two).iter().any(|s| s["state"] == "landed")
    });
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

/// S63 (128): GitHub serves no head for one of two pull requests opened by
/// hand, the one it lists first, whose branch is gone. Each round names it
/// and that it has no head, and the other is submitted; once its branch is
/// pushed again, the next round submits it. Closed, one with no head is
/// named no more.
#[test]
fn s63_a_request_sluice_cannot_fetch_holds_no_other() {
    let g = OnGitHub::new("pr-s63", "reviews = 0");
    let (gone, gone_head) = g.by_hand("unit/gone", &[("gone.txt", Some("g\n"))], false);
    let (fine, fine_head) = g.by_hand("unit/fine", &[("slow", Some("x\n"))], false);
    g.no_head(gone);
    let named = |said: &str, n: u64| {
        said.contains(&format!("pull/{n}: ")) && said.contains(&format!("pull/{n} has no head"))
    };
    let said = g.round();
    assert!(named(&said, gone), "{said}");
    assert!(g.of(gone).is_empty(), "{:?}", g.subs());
    assert_eq!(g.of(fine).len(), 1, "{:?}", g.subs());
    let (_, title, _) = g.posted(&fine_head).unwrap();
    assert!(
        title.starts_with("queued") || title.starts_with("testing"),
        "{title}"
    );
    // Read again each round, until GitHub serves its head.
    for _ in 0..2 {
        let said = g.round();
        assert!(named(&said, gone), "{said}");
        assert!(g.of(gone).is_empty(), "{:?}", g.subs());
    }
    g.repo
        .git(&["push", "-q", "origin", "unit/gone:refs/heads/unit/gone"]);
    g.step();
    let subs = g.of(gone);
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0]["head"], gone_head.as_str());
    // Closed, it is no longer read.
    let (third, _) = g.by_hand("unit/third", &[("third.txt", Some("t\n"))], false);
    g.no_head(third);
    let said = g.round();
    assert!(named(&said, third), "{said}");
    g.gh.forge.close(third).unwrap();
    g.step();
    assert!(g.of(third).is_empty(), "{:?}", g.subs());
}

/// 128: GitHub answers an error for one pull request's reviews. The round
/// names it and its reviews, and submits the other, whose review holds;
/// once its reviews can be read, the next round submits it.
#[test]
fn reviews_that_cannot_be_read_hold_no_other() {
    let g = OnGitHub::new("pr-no-reviews", "reviews = 1");
    let (one, one_head) = g.by_hand("unit/one", &[("one.txt", Some("1\n"))], false);
    let (two, two_head) = g.by_hand("unit/two", &[("two.txt", Some("2\n"))], false);
    g.gh.forge
        .review(one, "reviewer", "APPROVED", &one_head)
        .unwrap();
    g.gh.forge
        .review(two, "reviewer", "APPROVED", &two_head)
        .unwrap();
    g.gh.fail_reviews(one, true);
    let said = g.round();
    assert!(
        said.contains(&format!("pull/{one}: reading pull/{one}'s reviews")),
        "{said}"
    );
    assert!(g.of(one).is_empty(), "{:?}", g.subs());
    assert_eq!(g.of(two).len(), 1, "{:?}", g.subs());
    g.gh.fail_reviews(one, false);
    g.step();
    let subs = g.of(one);
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0]["head"], one_head.as_str());
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
            said.contains("sluice: warning: taking in main's pull requests: tried ")
                && said.contains(": GET /repos/o/r/pulls?state=open&base=main")
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
        std::thread::sleep(Duration::from_millis(200));
    }
    let landed = g.main_on_github();
    assert_eq!(
        github::rev(g.bare.path(), &format!("{landed}^2")).unwrap(),
        head
    );
}

/// deneb, 2026-11-05: GitHub renamed the repository, and its old address,
/// which the remote still names, finds no installation of the app. A
/// running driver says so with what to do once, never every pass: its
/// intake on stderr, `main`'s round on stdout, and the refused read once
/// in `main`'s log, in GitHub's words (133; rules core.28, core.29).
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
    g.discovery_passes();
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "3"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = "GET /repos/o/r/installation: 404 Not Found";
    let todo = format!(
        "GitHub finds no installation of app {} on o/r: if the repository was renamed or \
         transferred, point the remote that names it at its new address",
        github::APP
    );
    let intake = "sluice: warning: taking in main's pull requests: ";
    assert_eq!(stderr.matches(intake).count(), 1, "{}", text(&out));
    let warned = &stderr[stderr.find(intake).unwrap()..];
    assert!(
        warned.contains(&format!("{said}\n{todo}")),
        "{}",
        text(&out)
    );
    let passed = "main could not be read, passed by: ";
    assert_eq!(stdout.matches(passed).count(), 1, "{}", text(&out));
    assert_eq!(stdout.matches(&todo).count(), 1, "{}", text(&out));
    let refused: Vec<Value> = g
        .log()
        .into_iter()
        .filter(|l| l["event"] == "read-refused")
        .collect();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(refused[0]["asked"], "refs/heads/main");
    assert_eq!(refused[0]["said"], said);
}

// ---------------------------------------------------------------------------
// pull-request-status
// ---------------------------------------------------------------------------

/// What is missing: a pull request out of draft needing a review and
/// `qa/approved` shows both, queued.
#[test]
fn the_check_names_what_is_missing() {
    let g = OnGitHub::new("pr-missing", "reviews = 1\nchecks = [\"qa/approved\"]");
    let (_, head) = g.by_hand("unit/missing", &[("m.txt", Some("m\n"))], false);
    g.step();
    let (status, title, summary) = g.posted(&head).unwrap();
    assert_eq!(status, Status::Queued);
    assert_eq!(title, "waiting: needs 1 review, qa/approved");
    assert!(
        summary.contains("1 review") && summary.contains("`qa/approved`"),
        "{summary}"
    );
    assert!(
        summary.contains("Forecast: merges onto main without conflict"),
        "{summary}"
    );
}

/// A pull request held for its submitter says so: one `wt submit` opened,
/// blamed, is a draft whose check reads "waiting: the next submit",
/// naming the next submit from its branch and the ready mark; its one
/// comment names the failed check, links to the report, and says sluice
/// marked it a draft and what submits it again.
#[test]
fn a_submits_pull_request_sent_back_says_so() {
    let g = OnGitHub::new("pr-held-blamed", "");
    let w = g.session("blamed", &[("broken", "x\n")]);
    let out = g.wt_submit(&w, &[]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    let pull = g.pull_from("unit/blamed");
    assert!(pull.draft, "{pull:?}");
    assert_eq!(g.of_after(pull.number, 2).len(), 1, "{:?}", g.subs());
    // Record 0096 (D3): GitHub keeps the check failed once it was, so the
    // held state is read by its title, as any host reads it now; what
    // sluice sent for it is queued.
    let (_, title, summary) = g.posted(&pull.head).unwrap();
    assert_eq!(title, "waiting: the next submit");
    let held = g.gh.forge.held().unwrap();
    let sent = held
        .posts
        .iter()
        .rfind(|(rev, name, _)| *rev == pull.head && name == "sluice")
        .unwrap();
    assert_eq!(
        (sent.2.status, sent.2.title.as_str()),
        (Status::Queued, "waiting: the next submit")
    );
    assert!(
        summary.contains(
            "The next `sluice submit main` from `unit/blamed`, or marking it ready for review, \
             submits it again."
        ),
        "{summary}"
    );
    let delivered: Vec<String> = g
        .comments(pull.number)
        .into_iter()
        .filter(|c| c.contains("told="))
        .collect();
    assert_eq!(delivered.len(), 1, "{delivered:?}");
    let c = &delivered[0];
    assert!(c.contains("`sh check.sh` failed"), "{c}");
    assert!(c.contains("[sluice's report]("), "{c}");
    assert!(
        c.contains(
            "sluice marked it a draft. The next `sluice submit main` from `unit/blamed`, or \
             marking it ready for review, submits it again."
        ),
        "{c}"
    );
    assert!(!c.contains("Push a fix"), "{c}");
}

/// Landed, merged by landing: the check is success and reads landed;
/// GitHub's `main` is sluice's landing commit, whose second parent is the
/// pull request's head, and GitHub shows it merged.
#[test]
fn landed_and_merged_by_landing() {
    let g = OnGitHub::new("pr-landed", "");
    let (n, head) = g.by_hand("unit/lands", &[("l.txt", Some("l\n"))], false);
    g.step_until("landed", |g| {
        g.of(n).first().is_some_and(|s| s["state"] == "landed")
    });
    let landed = g.main_on_github();
    assert_eq!(
        github::rev(g.bare.path(), &format!("{landed}^2")).unwrap(),
        head
    );
    let (status, title, _) = g.posted(&head).unwrap();
    assert_eq!(status, Status::Success);
    assert!(title.starts_with("landed as "), "{title}");
    g.step();
    let pull = g.gh.forge.held().unwrap().pulls[0].clone();
    assert!(!pull.open, "GitHub shows it merged");
    // No sent-back comment; only the report (ADR 0044).
    let comments = g.comments(n);
    assert!(
        comments.iter().all(|c| !c.contains("told=")),
        "{comments:?}"
    );
    assert!(g
        .report(n)
        .unwrap()
        .contains("## sluice: landed on main as "));
}

/// 51, C.5; outcome-delivery spec, "A request's landing is posted" and "A
/// post that fails": the driver's post of a request's outcome is a
/// hand-over by `pull-request` to `pull/<n>` that confirms on receipt, so a
/// landing posted is done, with its hand-over and its confirmation logged,
/// and no inbox in a worktree on the request's head branch gives it; a
/// blame whose post GitHub refuses stays owed to the request with no
/// hand-over (record 0102, design D5).
#[test]
fn a_requests_post_confirms_its_outcome_on_receipt() {
    let g = OnGitHub::new("pr-delivered", "");
    let (n, _) = g.by_hand("unit/delivered", &[("t.txt", Some("t\n"))], false);
    g.step_until("landed", |g| {
        g.of(n).first().is_some_and(|s| s["state"] == "landed")
    });
    let id = g.of(n)[0]["id"].clone();
    let key = format!("{id}:landed");
    let reader = format!("pull/{n}");
    g.step_until("posted", |g| !g.logged("handed", &id).is_empty());
    let delivery = g.delivery(&id);
    assert_eq!(delivery["key"], key.as_str(), "{delivery}");
    assert_eq!(delivery["state"], "done", "{delivery}");
    let handed = g.logged("handed", &id);
    assert_eq!(handed.len(), 1, "{handed:?}");
    assert_eq!(handed[0]["by"], "pull-request");
    assert_eq!(handed[0]["to"], reader.as_str());
    let confirmed = g.logged("confirmed", &id);
    assert_eq!(confirmed.len(), 1, "{confirmed:?}");
    assert_eq!(confirmed[0]["reader"], reader.as_str());
    assert_eq!(confirmed[0]["by"], "pull-request");
    let w = g.repo.dir().with_extension("on-delivered");
    let _ = std::fs::remove_dir_all(&w);
    g.repo.git(&[
        "worktree",
        "add",
        "-q",
        w.to_str().unwrap(),
        "unit/delivered",
    ]);
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["inbox", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    assert_eq!(doc, serde_json::json!({ "outcomes": [] }));

    // A post that fails.
    let (m, _) = g.by_hand("unit/broken", &[("broken", Some("x\n"))], false);
    for (method, path) in [
        ("POST", "/repos/o/r/check-runs"),
        ("PATCH", "/repos/o/r/check-runs"),
        ("POST", "/repos/o/r/issues"),
        ("PATCH", "/repos/o/r/issues"),
    ] {
        g.gh.trouble(method, path, usize::MAX, github::Trouble::Status(422));
    }
    let until = Instant::now() + Duration::from_secs(60);
    while g.of(m).iter().all(|s| s["state"] != "failed") {
        assert!(Instant::now() < until, "never failed: {:?}", g.subs());
        g.discovery_passes();
        g.sluice(&["step", "main"]);
        std::thread::sleep(Duration::from_millis(200));
    }
    for _ in 0..2 {
        g.sluice(&["step", "main"]);
    }
    let id = g.of(m)[0]["id"].clone();
    let delivery = g.delivery(&id);
    assert_eq!(
        delivery["reader"],
        format!("pull/{m}").as_str(),
        "{delivery}"
    );
    assert_eq!(delivery["state"], "owed", "{delivery}");
    assert_eq!(delivery["handed"], serde_json::json!([]), "{delivery}");
    assert!(g.logged("handed", &id).is_empty());
}

/// D1: the queue a check's summary carries in its block, as a program
/// reads it.
fn block(summary: &str) -> Value {
    let q = sluice::profiles::github::pulls::queue_of(summary)
        .unwrap_or_else(|| panic!("no queue block: {summary}"));
    serde_json::to_value(q).unwrap()
}

/// A submission's number on this host.
fn id_of(subs: &[Value]) -> u64 {
    subs.first()
        .and_then(|s| s["id"].as_u64())
        .unwrap_or_else(|| panic!("not taken in: {subs:?}"))
}

impl OnGitHub {
    /// How many times sluice's check on `head` was posted.
    fn posts_on(&self, head: &str) -> usize {
        let held = self.gh.forge.held().unwrap();
        held.posts
            .iter()
            .filter(|(at, name, _)| at == head && name == "sluice")
            .count()
    }
}

/// pull-request-status, "Queued behind another", "A queued submission's
/// block", "Waiting for a review" and "Nothing in it changed": host `b`
/// takes main's pull requests in, one at a time. The check on the second
/// request's head names `b`, its submission there and the one ahead of it
/// with its pull request, in its words and its block; the check of one
/// needing a review says it waits to be taken in on `b`, and for what.
/// Once the second round has posted the first as under test and the
/// forecast behind both, a round where neither moved posts nothing on
/// either; the first's check goes on until the test ends it, so nothing
/// lands between them however long a round takes on a loaded host (D1;
/// rule core.32). A check of five seconds left room for one: deneb,
/// 2026-11-12, the third round posting the second's check again as
/// `queued: next on main`.
#[test]
fn the_check_names_its_host_and_queue() {
    let g = OnGitHub::checked("pr-host-queue", "reviews = 1", "1m", "parallel = 1\n");
    g.named("b", "");
    let hang = Hang::in_repo(&g);
    let (a, a_head) = g.by_hand("unit/ahead", &[("hang", Some(&hang.file()))], false);
    let (b, b_head) = g.by_hand("unit/behind", &[("b.txt", Some("b\n"))], false);
    let (_, c_head) = g.by_hand("unit/unreviewed", &[("c.txt", Some("c\n"))], false);
    for (n, head) in [(a, &a_head), (b, &b_head)] {
        g.gh.forge.review(n, "reviewer", "APPROVED", head).unwrap();
    }
    g.step();
    started(&hang.pid);
    let (ahead, behind) = (id_of(&g.of(a)), id_of(&g.of(b)));

    let (status, title, summary) = g.posted(&b_head).unwrap();
    assert_eq!(status, Status::Queued);
    assert_eq!(title, format!("queued behind #{a}"));
    assert!(
        summary.starts_with(&format!("Submission {behind} on b.\n\n")),
        "{summary}"
    );
    assert_eq!(
        block(&summary),
        serde_json::json!({
            "host": "b", "taken_in": true, "submission": behind, "state": "queued",
            "ahead": [{ "submission": ahead, "pull_request": a }], "waits_for": [],
        })
    );

    let (_, title, summary) = g.posted(&c_head).unwrap();
    assert_eq!(title, "waiting: needs 1 review");
    assert!(
        summary.starts_with("Waiting to be taken in on b.\n\n"),
        "{summary}"
    );
    assert_eq!(
        block(&summary),
        serde_json::json!({
            "host": "b", "taken_in": false, "submission": null, "state": null,
            "ahead": [], "waits_for": ["1 review"],
        })
    );

    g.step();
    let (on_b, on_c) = (g.posts_on(&b_head), g.posts_on(&c_head));
    g.step();
    assert_eq!(g.posts_on(&b_head), on_b, "{:?}", g.posted(&b_head));
    assert_eq!(g.posts_on(&c_head), on_c, "{:?}", g.posted(&c_head));
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

/// pull-request-status, "The queue ahead moves": once the submission ahead
/// lands, host `b` posts the check of the one behind it again, the same
/// submission with nothing ahead of it. Its own check goes on until the
/// test ends it, so the round after the landing posts it before it could
/// land, however long a round takes on a loaded host (D1).
#[test]
fn the_queue_block_moves_with_the_queue_ahead() {
    let g = OnGitHub::checked("pr-host-queue-moves", "", "1m", "parallel = 1\n");
    g.named("b", "");
    let hang = Hang::in_repo(&g);
    let (a, _) = g.by_hand("unit/first", &[("f.txt", Some("f\n"))], false);
    let (b, b_head) = g.by_hand("unit/second", &[("hang", Some(&hang.file()))], false);
    g.step();
    let (ahead, behind) = (id_of(&g.of(a)), id_of(&g.of(b)));
    let before = block(&g.posted(&b_head).unwrap().2);
    assert_eq!(
        before["ahead"],
        serde_json::json!([{ "submission": ahead, "pull_request": a }])
    );

    g.step_until("the first landed", |g| {
        g.of(a).first().is_some_and(|s| s["state"] == "landed")
    });
    g.step();
    let (_, title, summary) = g.posted(&b_head).unwrap();
    assert!(
        title == "queued: next on main" || title == format!("testing main + #{b}"),
        "{title}"
    );
    assert!(
        summary.starts_with(&format!("Submission {behind} on b.\n\n")),
        "{summary}"
    );
    let after = block(&summary);
    assert_eq!(after["submission"], behind, "{after}");
    assert_eq!(after["ahead"], serde_json::json!([]), "{after}");
}

/// pull-request-status, "each submission ahead of it": the check's block
/// and title name what is ahead in the order host `b`'s rounds take its
/// queue, not the order of submission. With the first request under test,
/// a planner gives the third priority 5, and the second's check names it
/// ahead; then the third lands after the second, and the second's check
/// names only the first again, the third's both (6, 7; D1).
#[test]
fn the_queue_block_follows_the_queues_order() {
    let g = OnGitHub::checked("pr-host-queue-order", "", "1m", "parallel = 1\n");
    g.named("b", "");
    let pid = g.repo.dir().join(".git/hang.pid");
    let hang = format!("{}\n", pid.display());
    let (a, _) = g.by_hand("unit/tested", &[("hang", Some(&hang))], false);
    let (b, b_head) = g.by_hand("unit/second", &[("b.txt", Some("b\n"))], false);
    let (c, c_head) = g.by_hand("unit/third", &[("c.txt", Some("c\n"))], false);
    g.step();
    let (first, second, third) = (id_of(&g.of(a)), id_of(&g.of(b)), id_of(&g.of(c)));
    let until = Instant::now() + Duration::from_secs(60);
    while !pid.exists() {
        assert!(Instant::now() < until, "the check never started");
        std::thread::sleep(Duration::from_millis(100));
    }
    let process = std::fs::read_to_string(&pid).unwrap().trim().to_string();
    let amend = |args: &[&str]| {
        let out = g.sluice(&[&["amend", &third.to_string()], args].concat());
        assert!(out.status.success(), "{}", text(&out));
    };
    let shown = |head: &str| {
        let (_, title, summary) = g.posted(head).unwrap();
        (title, block(&summary)["ahead"].clone())
    };
    let entry = |submission: u64, n: u64| serde_json::json!({ "submission": submission, "pull_request": n });

    amend(&["--priority", "5"]);
    g.step();
    assert_eq!(
        shown(&b_head),
        (
            format!("queued behind #{a}, #{c}"),
            serde_json::json!([entry(first, a), entry(third, c)])
        )
    );
    assert_eq!(
        shown(&c_head),
        (
            format!("queued behind #{a}"),
            serde_json::json!([entry(first, a)])
        )
    );

    amend(&["--after", &second.to_string()]);
    g.step();
    assert_eq!(
        shown(&b_head),
        (
            format!("queued behind #{a}"),
            serde_json::json!([entry(first, a)])
        )
    );
    assert_eq!(
        shown(&c_head),
        (
            format!("queued behind #{a}, #{b}"),
            serde_json::json!([entry(first, a), entry(second, b)])
        )
    );
    assert!(Command::new("kill")
        .arg(&process)
        .status()
        .unwrap()
        .success());
}

/// Two rounds with nothing changed between them post nothing the second
/// time.
#[test]
fn no_repeated_posts() {
    let g = OnGitHub::new("pr-quiet", "reviews = 1");
    g.by_hand("unit/quiet", &[("q.txt", Some("q\n"))], true);
    g.by_hand("unit/still", &[("r.txt", Some("r\n"))], false);
    g.step();
    let posts = g.posts();
    assert_eq!(posts, 2);
    g.step();
    assert_eq!(g.posts(), posts);
}

/// A conflict seen early: a waiting pull request changing a file a queued
/// one changes otherwise names that one and the file, and runs nothing.
#[test]
fn a_conflict_seen_early() {
    let g = OnGitHub::new("pr-early", "reviews = 1");
    let (a, a_head) = g.by_hand("unit/a", &[("plan.py", Some("a\n"))], false);
    g.gh.forge
        .review(a, "reviewer", "APPROVED", &a_head)
        .unwrap();
    let (b, b_head) = g.by_hand("unit/b", &[("plan.py", Some("b\n"))], false);
    g.step();
    assert_eq!(g.of(a).len(), 1);
    assert!(g.of(b).is_empty());
    let (_, _, summary) = g.posted(&b_head).unwrap();
    assert!(
        summary.contains(&format!("conflicts with #{a} in plan.py")),
        "{summary}"
    );
}

/// 128: a draft whose head GitHub serves none of is only a draft. The
/// round names it, submits the pull request that is ready, and posts the
/// draft's check as a draft's, with no forecast.
#[test]
fn a_draft_whose_head_cannot_be_fetched_is_only_a_draft() {
    let g = OnGitHub::new("pr-draft-no-head", "");
    let (draft, draft_head) = g.by_hand("unit/drafted", &[("d.txt", Some("d\n"))], true);
    let (ready, _) = g.by_hand("unit/ready", &[("r.txt", Some("r\n"))], false);
    g.no_head(draft);
    let said = g.round();
    assert!(
        said.contains(&format!("pull/{draft}: pull/{draft} has no head")),
        "{said}"
    );
    assert!(g.of(draft).is_empty(), "{:?}", g.subs());
    assert_eq!(g.of(ready).len(), 1, "{:?}", g.subs());
    let (status, title, summary) = g.posted(&draft_head).unwrap();
    assert_eq!(
        (status, title.as_str()),
        (Status::Queued, "waiting: a draft")
    );
    assert!(!summary.contains("Forecast:"), "{summary}");
}

// ---------------------------------------------------------------------------
// pull-request-submit
// ---------------------------------------------------------------------------

/// First submit: the agent's branch is on GitHub, a pull request from it
/// into `main` is open, and the output names it and what it waits for.
/// Submit again: the same pull request has the new head, and no second
/// is opened.
#[test]
fn submit_opens_then_updates_a_pull_request() {
    let g = OnGitHub::new("pr-submit", "reviews = 1");
    let w = g.session("review", &[("r.txt", "r\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let said = text(&out);
    let head = github::rev(g.bare.path(), "refs/heads/unit/review").unwrap();
    let pull = g.gh.forge.by_ref("unit/review").unwrap().unwrap();
    assert!(pull.open && !pull.draft && pull.base == "main");
    assert_eq!(pull.head, head);
    assert!(
        pull.body.contains("<!-- sluice: agent=unit/review -->"),
        "{}",
        pull.body
    );
    // C.5 pull request: titled by the branch's name, given no subject.
    assert_eq!(pull.title, "unit/review");
    assert!(
        said.contains(&format!(
            "pushed unit/review · opened #{}: unit/review",
            pull.number
        )),
        "{said}"
    );
    assert!(said.contains("waiting: needs 1 review"), "{said}");
    assert!(g.of(pull.number).is_empty());

    let again = g.commit_in(&w, &[("r2.txt", "r2\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(
        text(&out).contains(&format!("updated #{}", pull.number)),
        "{}",
        text(&out)
    );
    let held = g.gh.forge.held().unwrap();
    assert_eq!(held.pulls.len(), 1);
    assert_eq!(held.pulls[0].head, again);
    assert!(w.exists());
}

/// C.5 pull request: a bolt's branch submitted with no subject is titled
/// by its name, never by its first commit, its proposal's.
#[test]
fn a_bolt_submitted_with_no_subject_is_titled_by_its_branch() {
    let g = OnGitHub::new("pr-bolt-title", "reviews = 1");
    let w = g.worktree_on("bolt-title", "bolt/review-cards");
    g.commit_message(&w, "proposal.md", "docs(openspec): propose review-cards");
    g.commit_message(&w, "cards.txt", "feat(cards): batch review cards");
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("bolt/review-cards");
    assert_eq!(pull.title, "bolt/review-cards");
    assert!(
        text(&out).contains(&format!("opened #{}: bolt/review-cards", pull.number)),
        "{}",
        text(&out)
    );
}

/// C.5 pull request: a session submitted with a subject is titled by it,
/// as written.
#[test]
fn a_session_submitted_with_a_subject_is_titled_by_it() {
    let g = OnGitHub::new("pr-subject", "reviews = 1");
    let w = g.session("review", &[("r.txt", "r\n")]);
    let out = g.wt_submit(&w, &["--no-wait", "--subject", "Batch review cards"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/review");
    assert_eq!(pull.title, "Batch review cards");
    assert!(
        text(&out).contains(&format!(
            "pushed unit/review · opened #{}: Batch review cards",
            pull.number
        )),
        "{}",
        text(&out)
    );
}

/// C.5 pull request: whatever titles it, the description holds every
/// commit's message whole, oldest first, and the title is neither's.
#[test]
fn the_description_still_carries_the_commits() {
    let g = OnGitHub::new("pr-subject-body", "reviews = 1");
    let w = g.worktree("cards");
    g.commit_message(&w, "one.txt", "Add the first card\n\nWith its reasons.");
    g.commit_message(&w, "two.txt", "Add the second card");
    let out = g.wt_submit(&w, &["--no-wait", "--subject", "Batch review cards"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/cards");
    assert_eq!(
        pull.body,
        "Add the first card\n\nWith its reasons.\n\nAdd the second card\n\n\
         <!-- sluice: agent=unit/cards -->"
    );
    assert_eq!(pull.title, "Batch review cards");
}

/// The calls that read the app's login.
fn logins(g: &OnGitHub) -> usize {
    g.gh.fake
        .requests()
        .iter()
        .filter(|r| r.path == "/app")
        .count()
}

/// C.5 pull request: sluice opened the request titled by its branch; a
/// new head submitted with a subject retitles it.
#[test]
fn a_new_head_with_a_subject_retitles_it() {
    let g = OnGitHub::new("pr-retitle", "reviews = 1");
    let w = g.session("review", &[("r.txt", "r\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let n = g.pull_from("unit/review").number;
    assert_eq!(g.pull_from("unit/review").title, "unit/review");
    let head = g.commit_in(&w, &[("r2.txt", "r2\n")]);
    let out = g.wt_submit(&w, &["--no-wait", "--subject", "Batch review cards"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/review");
    assert_eq!((pull.number, pull.head.as_str()), (n, head.as_str()));
    assert_eq!(pull.title, "Batch review cards");
    assert!(
        text(&out).contains(&format!("updated #{n}: Batch review cards")),
        "{}",
        text(&out)
    );
    assert_eq!(g.gh.forge.held().unwrap().pulls.len(), 1);
}

/// C.5 pull request: a new head submitted with no subject leaves the
/// title as it is, and asks nothing of who opened it.
#[test]
fn a_new_head_with_no_subject_keeps_the_title() {
    let g = OnGitHub::new("pr-keep-title", "reviews = 1");
    let w = g.session("review", &[("r.txt", "r\n")]);
    let out = g.wt_submit(&w, &["--no-wait", "--subject", "Batch review cards"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let head = g.commit_in(&w, &[("r2.txt", "r2\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/review");
    assert_eq!(pull.head, head);
    assert_eq!(pull.title, "Batch review cards");
    assert!(
        text(&out).contains(&format!("updated #{}: Batch review cards", pull.number)),
        "{}",
        text(&out)
    );
    assert_eq!(logins(&g), 0);
}

/// A worktree on `unit/<name>` with one commit of `message`, pushed to
/// GitHub, and a pull request from it opened by `author` and titled
/// `title`, a draft; returns the worktree and the request's number.
fn opened_by(g: &OnGitHub, author: &str, name: &str, message: &str, title: &str) -> (PathBuf, u64) {
    let w = g.worktree(name);
    let head = g.commit_message(&w, &format!("{name}.txt"), message);
    let branch = format!("unit/{name}");
    g.repo.git(&[
        "push",
        "-q",
        "origin",
        &format!("{branch}:refs/heads/{branch}"),
    ]);
    let n =
        g.gh.forge
            .open_by(author, "main", &branch, &head, true, None)
            .unwrap();
    g.gh.forge
        .edit(&github::repo(), n, Some(title), "")
        .unwrap();
    (w, n)
}

/// C.5 pull request: a request sluice opened while it titled them by the
/// first commit is retitled by the next subject.
#[test]
fn a_request_opened_before_titles_changed_is_retitled() {
    let g = OnGitHub::new("pr-old-title", "reviews = 1");
    let proposal = "docs(openspec): propose review";
    let (w, n) = opened_by(&g, "sluice", "review", proposal, proposal);
    let out = g.wt_submit(&w, &["--no-wait", "--subject", "Batch review cards"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/review");
    assert_eq!(pull.number, n);
    assert_eq!(pull.title, "Batch review cards");
    assert!(pull.body.starts_with(proposal), "{}", pull.body);
    assert_eq!(logins(&g), 1);
}

/// 120: a request a person opened keeps their title, whatever the subject;
/// the submit still writes its description and marks it ready at the head
/// submitted.
#[test]
fn a_subject_on_a_request_opened_by_hand_leaves_its_title() {
    let g = OnGitHub::new("pr-hand-title", "reviews = 1");
    let (w, n) = opened_by(
        &g,
        "person",
        "review",
        "Change r.txt",
        "Review cards, first pass",
    );
    let head = github::rev(g.bare.path(), "refs/heads/unit/review").unwrap();
    let out = g.wt_submit(&w, &["--no-wait", "--subject", "Batch review cards"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/review");
    assert_eq!(pull.number, n);
    assert_eq!(pull.title, "Review cards, first pass");
    assert!(
        pull.body.starts_with("Change r.txt\n\n<!-- sluice: agent="),
        "{}",
        pull.body
    );
    assert!(!pull.draft && pull.head == head, "{pull:?}");
    assert!(
        text(&out).contains(&format!("updated #{n}: Review cards, first pass")),
        "{}",
        text(&out)
    );
}

/// The message of the commit `sub`, a submission of `main`, landed as on
/// GitHub.
fn landing_message(g: &OnGitHub, sub: &Value) -> String {
    let landed = sub["landed_as"].as_str().expect("landed");
    let out = support::git_command()
        .arg("--git-dir")
        .arg(g.bare.path())
        .args(["log", "-1", "--format=%B", landed])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// Step until pull request `n`'s submission lands; returns it.
fn landed(g: &OnGitHub, n: u64) -> Value {
    g.step_until("landed", |g| g.of(n).iter().any(|s| s["state"] == "landed"));
    g.of(n)
        .into_iter()
        .find(|s| s["state"] == "landed")
        .unwrap()
}

/// 23, 118: a request a submit opened with a subject is taken with its
/// title as the submission's subject, so its landing commit names the
/// work by it, its trailers still naming the request's place.
#[test]
fn a_request_sluice_opened_with_a_subject_lands_named_by_it() {
    let g = OnGitHub::new("pr-land-subject", "");
    let w = g.session("review", &[("r.txt", "r\n")]);
    let out = g.wt_submit(&w, &["--no-wait", "--subject", "Batch review cards"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let n = g.pull_from("unit/review").number;
    let sub = landed(&g, n);
    let m = landing_message(&g, &sub);
    let id = &sub["id"];
    assert_eq!(
        m.lines().next(),
        Some(format!("chore(land): #{id} Batch review cards").as_str()),
        "{m}"
    );
    let from = format!(
        "\nSluice-From: {} (pull/{n})\n",
        sub["from"]["agent"].as_str().unwrap()
    );
    assert!(m.contains(&from), "{m}");
}

/// 23, 120: a request a person opened lands named by the title they
/// gave it.
#[test]
fn a_request_a_person_opened_lands_named_by_its_title() {
    let g = OnGitHub::new("pr-land-title", "");
    let (n, _) = g.by_hand("unit/hand", &[("h.txt", Some("h\n"))], false);
    g.gh.forge
        .edit(
            &github::repo(),
            n,
            Some("Review cards, first pass"),
            "Hand work.",
        )
        .unwrap();
    let sub = landed(&g, n);
    let m = landing_message(&g, &sub);
    assert_eq!(
        m.lines().next(),
        Some(format!("chore(land): #{} Review cards, first pass", sub["id"]).as_str()),
        "{m}"
    );
}

/// A message of `chars` characters: `subject`, a blank line, a body.
fn long_message(subject: &str, chars: usize) -> String {
    let body = "x".repeat(chars - subject.chars().count() - 2);
    format!("{subject}\n\n{body}")
}

/// The line a description gives the commits it holds by subject alone.
fn more_commits(r: usize, name: &str) -> String {
    let s = if r == 1 { "" } else { "s" };
    format!(
        "{r} more commit{s}, each by its subject; all of them in full: \
         https://github.com/o/r/compare/main...unit/{name}"
    )
}

/// A bolt too long for one description: GitHub takes the request, its
/// description the first two messages whole, the line counting the third
/// with the compare view, the third's subject and the agent's mark, no
/// longer than GitHub holds; the fake refuses a longer one as GitHub does.
#[test]
fn a_bolt_too_long_for_one_description() {
    let g = OnGitHub::new("pr-long-bolt", "reviews = 1");
    let w = g.worktree("long");
    let ms: Vec<String> = (1..=3)
        .map(|k| long_message(&format!("Add part {k}"), 30_000))
        .collect();
    for (k, m) in ms.iter().enumerate() {
        g.commit_message(&w, &format!("part-{k}.txt"), m);
    }
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/long");
    assert!(pull.open, "{pull:?}");
    assert_eq!(
        pull.body,
        format!(
            "{}\n\n{}\n\n{}\n\n- Add part 3\n\n<!-- sluice: agent=unit/long -->",
            ms[0],
            ms[1],
            more_commits(1, "long")
        )
    );
    assert!(pull.body.chars().count() <= 65_536);
}

/// One message longer than GitHub holds: no part of its body, the line
/// counting it with the compare view, its subject and the agent's mark.
#[test]
fn one_message_longer_than_github_holds() {
    let g = OnGitHub::new("pr-long-message", "reviews = 1");
    let w = g.worktree("huge");
    g.commit_message(&w, "huge.txt", &long_message("Add the huge file", 70_000));
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/huge");
    assert!(pull.open, "{pull:?}");
    assert_eq!(
        pull.body,
        format!(
            "{}\n\n- Add the huge file\n\n<!-- sluice: agent=unit/huge -->",
            more_commits(1, "huge")
        )
    );
}

/// A new commit, submitted again: the description holds both messages,
/// oldest first, then the agent's mark.
#[test]
fn a_new_commit_submitted_again_is_described() {
    let g = OnGitHub::new("pr-described-again", "reviews = 1");
    let w = g.worktree("again");
    g.commit_message(&w, "first.txt", "Add the first file");
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(
        g.pull_from("unit/again").body,
        "Add the first file\n\n<!-- sluice: agent=unit/again -->"
    );
    g.commit_message(&w, "second.txt", "Add the second file");
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(
        g.pull_from("unit/again").body,
        "Add the first file\n\nAdd the second file\n\n<!-- sluice: agent=unit/again -->"
    );
}

/// Submitted again at the same head: the second submit does not edit the
/// pull request's description; the request stands there, so `wt submit`
/// answers with it (record 0065).
#[test]
fn submitted_again_at_the_same_head_edits_nothing() {
    let g = OnGitHub::new("pr-same-head", "reviews = 1");
    let w = g.session("same", &[("same.txt", "same\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let n = g.pull_from("unit/same").number;
    let edits = || {
        let edit = format!("/repos/o/r/pulls/{n}");
        g.gh.fake
            .requests()
            .iter()
            .filter(|r| r.method == "PATCH" && r.path == edit)
            .count()
    };
    let before = edits();
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(
        text(&out).contains("waiting: needs 1 review"),
        "{}",
        text(&out)
    );
    assert_eq!(edits(), before);
}

/// A worktree holding more than its commits is refused as today, and
/// nothing is pushed.
#[test]
fn submit_refuses_a_dirty_worktree() {
    let g = OnGitHub::new("pr-dirty", "");
    let w = g.session("dirty", &[("d.txt", "d\n")]);
    std::fs::write(w.join("d.txt"), "changed\n").unwrap();
    let out = g.wt_submit(&w, &[]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("d.txt"), "{}", text(&out));
    assert_eq!(github::rev(g.bare.path(), "refs/heads/unit/dirty"), None);
    assert!(g.gh.forge.held().unwrap().pulls.is_empty());
}

/// ADR 0050: a branch requiring a task moon keeps out of CI in the
/// worktree refuses the submission, naming the task and its line of the
/// policy, before anything is pushed: no branch on GitHub, and no pull
/// request opened or marked ready.
#[test]
fn submit_refuses_a_gate_requiring_a_task_kept_out_of_ci() {
    let g = OnGitHub::with(
        "pr-kept-out",
        "[tests]\nrunner = \"moon\"\n\n\
         [branches.main]\nremote = \"origin\"\nrequired = [\"app:integration\"]\n\n\
         [branches.main.pull_requests]\n",
        &[
            (
                ".moon/workspace.yml",
                "projects:\n  app: 'app'\nvcs:\n  client: 'git'\n  defaultBranch: 'main'\n\
                 telemetry: false\n",
            ),
            (
                "app/moon.yml",
                "tasks:\n  integration:\n    command: 'true'\n    options:\n      runInCI: false\n",
            ),
            (".gitignore", ".moon/cache/\n"),
        ],
    );
    let w = g.session("kept", &[("k.txt", "k\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains(
            "refused: main requires app:integration (.config/sluice.toml line 6), which the \
             task runner in this worktree keeps out of CI"
        ),
        "{}",
        text(&out)
    );
    assert_eq!(github::rev(g.bare.path(), "refs/heads/unit/kept"), None);
    let held = g.gh.forge.held().unwrap();
    assert!(held.pulls.is_empty(), "{:?}", held.pulls);
}

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
    // rounds take on a loaded host.
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

/// `--no-wait` returns once the pull request is open; `--status` later
/// reports its state, and removes nothing.
#[test]
fn no_wait_then_status() {
    let g = OnGitHub::new("pr-status", "reviews = 1");
    let w = g.session("later", &[("l.txt", "l\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let out = g.wt_submit(&w, &["--status"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(
        text(&out).contains("waiting: needs 1 review"),
        "{}",
        text(&out)
    );
    assert!(w.exists());
}

/// Waiting for a review: `--status --json` names the pull request's head,
/// that it is no draft, and what a submit from the worktree would send,
/// beside its number, branch and state.
#[test]
fn status_names_the_head_waiting_for_a_review() {
    let g = OnGitHub::new("pr-status-head", "reviews = 1");
    let w = g.session("headed", &[("h.txt", "h\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/headed");
    let out = g.wt_submit(&w, &["--status", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    assert_eq!(
        doc,
        serde_json::json!({
            "pull_request": pull.number, "branch": "main",
            "state": "waiting: needs 1 review", "head": pull.head, "draft": false,
            "sends": pull.head,
        })
    );
}

/// A draft: `--status --json` names its head and that it is a draft.
#[test]
fn status_names_a_draft() {
    let g = OnGitHub::new("pr-status-draft", "reviews = 1");
    let w = g.session("drafted", &[("d.txt", "d\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/drafted");
    g.gh.forge.set_draft(pull.number, true).unwrap();
    let out = g.wt_submit(&w, &["--status", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    assert_eq!(doc["pull_request"], pull.number, "{doc}");
    assert_eq!(doc["head"], pull.head.as_str(), "{doc}");
    assert_eq!(doc["draft"], true, "{doc}");
}

/// Record 0065: run again under a time limit while its pull request waits
/// for a review, `wt submit` pushes nothing and edits nothing; it follows
/// the request as `sluice wait main` does, out of time with the document
/// `--status` prints.
#[test]
fn run_again_follows_a_pull_request_waiting_for_a_review() {
    let g = OnGitHub::new("pr-again-review", "reviews = 1");
    let w = g.session("again", &[("a.txt", "a\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/again");
    let n = pull.number;
    let pushed = github::rev(g.bare.path(), "refs/heads/unit/again");
    let writes = || {
        let edit = format!("/repos/o/r/pulls/{n}");
        g.gh.fake
            .requests()
            .iter()
            .filter(|r| {
                (r.method == "PATCH" && r.path == edit)
                    || (r.path == "/graphql" && !r.body.contains("pullRequests("))
            })
            .count()
    };
    let before = writes();
    let out = g.wt_submit(&w, &["--timeout", "2s", "--json"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    assert_eq!(
        doc,
        serde_json::json!({
            "pull_request": n, "branch": "main", "state": "waiting: needs 1 review",
        })
    );
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        said.contains(&format!(
            "wt submit: following pull request #{n}, already open at this revision"
        )),
        "{said}"
    );
    assert!(!said.contains("pushed"), "{said}");
    assert_eq!(writes(), before);
    assert_eq!(github::rev(g.bare.path(), "refs/heads/unit/again"), pushed);
    assert!(!g.pull_from("unit/again").draft);
    assert!(w.exists());
}

/// Record 0065: run again without waiting while its pull request waits
/// for a review, `wt submit` pushes nothing and edits nothing; it answers
/// with the request as `--status` prints it, and exits 0.
#[test]
fn run_again_without_waiting_answers_with_a_pull_request() {
    let g = OnGitHub::new("pr-again-no-wait", "reviews = 1");
    let w = g.session("asked", &[("a.txt", "a\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let n = g.pull_from("unit/asked").number;
    let pushed = github::rev(g.bare.path(), "refs/heads/unit/asked");
    let writes = || {
        let edit = format!("/repos/o/r/pulls/{n}");
        g.gh.fake
            .requests()
            .iter()
            .filter(|r| {
                (r.method == "PATCH" && r.path == edit)
                    || (r.path == "/graphql" && !r.body.contains("pullRequests("))
            })
            .count()
    };
    let before = writes();
    let out = g.wt_submit(&w, &["--no-wait", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    assert_eq!(doc["pull_request"], n, "{doc}");
    assert_eq!(doc["branch"], "main", "{doc}");
    assert_eq!(doc["state"], "waiting: needs 1 review", "{doc}");
    assert!(!text(&out).contains("pushed"), "{}", text(&out));
    assert_eq!(writes(), before);
    assert_eq!(github::rev(g.bare.path(), "refs/heads/unit/asked"), pushed);
    assert!(!g.pull_from("unit/asked").draft);
    assert!(w.exists());
}

/// Record 0065: a pull request sent back as a draft stands no more, so a
/// run again with nothing committed submits: it marks the request ready
/// for review at that head, and intake takes that head again.
#[test]
fn run_again_marks_a_draft_ready() {
    let g = OnGitHub::new("pr-again-draft", "");
    let w = g.session("drafted", &[("broken", "x\n")]);
    let out = g.wt_submit(&w, &[]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    let pull = g.pull_from("unit/drafted");
    assert!(pull.draft, "{pull:?}");

    let out = g.wt_submit(&w, &[]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(!said.contains("following"), "{said}");
    assert!(
        said.contains(&format!("updated #{}", pull.number))
            && said.contains("marked ready for review"),
        "{said}"
    );
    let subs = g.of(pull.number);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], pull.head.as_str());
}

/// No wait, its conditions hold: the submit answers with the submission
/// sluice took the pull request in as, from its checkout `pull/<n>` at the
/// head pushed, with the pull request beside it; `sluice wait <id>`
/// follows that submission to its landing.
#[test]
fn no_wait_answers_with_its_submission() {
    let g = OnGitHub::new("pr-no-wait-id", "");
    let w = g.session("ready", &[("ready.txt", "ready\n")]);
    let out = g.wt_submit(&w, &["--no-wait", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    let n = g.pull_from("unit/ready").number;
    assert_eq!(doc["pull_request"], n, "{doc}");
    assert_eq!(doc["branch"], "main", "{doc}");
    assert_eq!(doc["ahead"], serde_json::json!([]), "{doc}");
    let id = doc["id"]
        .as_u64()
        .unwrap_or_else(|| panic!("{doc}"))
        .to_string();
    let out = g.sluice(&["status", &id, "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let record: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    assert_eq!(record["from"]["worktree"], format!("pull/{n}").as_str());
    let pushed = github::rev(g.bare.path(), "refs/heads/unit/ready").unwrap();
    assert_eq!(record["head"], pushed.as_str());
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["wait", &id, "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    assert_eq!(doc["state"], "landed");
}

/// No wait, waiting for a review: the submit answers with the pull request
/// and what it waits for, and nothing of it is queued.
#[test]
fn no_wait_waiting_for_a_review_answers_with_the_pull_request() {
    let g = OnGitHub::new("pr-no-wait-review", "reviews = 1");
    let w = g.session("unreviewed", &[("u.txt", "u\n")]);
    let out = g.wt_submit(&w, &["--no-wait", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    let n = g.pull_from("unit/unreviewed").number;
    assert_eq!(
        doc,
        serde_json::json!({
            "pull_request": n, "branch": "main", "state": "waiting: needs 1 review",
        })
    );
    assert!(g.of(n).is_empty(), "{:?}", g.subs());
}

/// No wait, in words: after the line naming the pull request opened, the
/// submission it was taken in as and what is queued ahead of it.
#[test]
fn no_wait_says_its_submission() {
    let g = OnGitHub::new("pr-no-wait-words", "");
    let w = g.session("worded", &[("worded.txt", "w\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let n = g.pull_from("unit/worded").number;
    let id = g.of(n)[0]["id"].as_u64().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    let opened = format!("pushed unit/worded · opened #{n}: unit/worded");
    let at = lines.iter().position(|l| *l == opened).expect(&stdout);
    assert_eq!(
        lines.get(at + 1).copied(),
        Some(format!("submission {id} to main from #{n} · nothing queued ahead").as_str()),
        "{stdout}"
    );
}

/// A pull request sent back keeps the agent's worktree: the waiting
/// submit exits 1 with the failure.
/// ADR 0041: `sluice wait <branch>` follows this worktree's pull request,
/// out of time with the document `--status` prints; `sluice wait <n>`
/// takes the pull requests in before each round, and lands it.
#[test]
fn wait_follows_a_pull_request() {
    let g = OnGitHub::new("pr-wait-cmd", "reviews = 1");
    let w = g.session("waits", &[("v.txt", "v\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let wait = |args: &[&str]| {
        g.command(env!("CARGO_BIN_EXE_sluice"), &w)
            .arg("wait")
            .args(args)
            .output()
            .unwrap()
    };
    let out = wait(&["main", "--timeout", "2s", "--json"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let pull = g.gh.forge.by_ref("unit/waits").unwrap().unwrap();
    let doc: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    assert_eq!(doc["pull_request"], pull.number);
    assert_eq!(doc["state"], "waiting: needs 1 review");

    g.gh.forge
        .review(pull.number, "reviewer", "APPROVED", &pull.head)
        .unwrap();
    g.step();
    let status: serde_json::Value =
        serde_json::from_slice(&g.sluice(&["status", "main", "--json"]).stdout).unwrap();
    let id = status["submissions"][0]["id"]
        .as_u64()
        .unwrap_or_else(|| panic!("{status}"));
    let out = wait(&[&id.to_string(), "--json"]);
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    let doc: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    assert_eq!(doc["state"], "landed");
}

/// A pull request taken in, out of time: `sluice wait <branch>` follows its
/// submission, and ends with that submission's record, as `sluice wait
/// <id>` does.
#[test]
fn wait_follows_a_pull_requests_submission() {
    let g = OnGitHub::new("pr-wait-taken", "");
    let w = g.session("taken", &[("slow", "x\n")]);
    let out = g.wt_submit(&w, &["--no-wait", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    let id = doc["id"].as_u64().unwrap_or_else(|| panic!("{doc}"));
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["wait", "main", "--timeout", "2s", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let record: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    assert_eq!(record["id"], id, "{record}");
    assert!(
        record["state"] == "queued" || record["state"] == "testing",
        "{record}"
    );
    assert!(record.get("pull_request").is_none(), "{record}");
}

/// 135: beside a live driver, `sluice wait <branch>` takes no pull request
/// in and runs no round: a request ready to be taken in is still not, out
/// of time.
#[test]
fn a_wait_on_a_pull_request_beside_a_driver() {
    let g = OnGitHub::new("pr-wait-driver", "reviews = 1");
    let w = g.session("driven", &[("d.txt", "d\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/driven");
    g.gh.forge
        .review(pull.number, "reviewer", "APPROVED", &pull.head)
        .unwrap();
    let _driver = g.driver();
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["wait", "main", "--timeout", "2s", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    assert_eq!(doc["pull_request"], pull.number, "{doc}");
    assert!(g.of(pull.number).is_empty(), "{:?}", g.subs());
}

/// 135: a pull request's submit beside a live driver is the submit's own
/// act: it opens the pull request, takes it in, and answers with its
/// submission, as where no driver lives.
#[test]
fn a_pull_requests_submit_beside_a_driver() {
    let g = OnGitHub::new("pr-submit-driver", "");
    let w = g.session("submitted", &[("s.txt", "s\n")]);
    let _driver = g.driver();
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "main", "--no-wait", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    let n = g.pull_from("unit/submitted").number;
    assert_eq!(doc["pull_request"], n, "{doc}");
    let id = doc["id"].as_u64().unwrap_or_else(|| panic!("{doc}"));
    let subs = g.of(n);
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0]["id"], id);
    assert_eq!(subs[0]["state"], "queued");
}

/// 135: a pull request's waiting submit beside a live driver takes its
/// request in at its first pass, and runs no round after: its submission
/// stays queued while the driver lives.
#[test]
fn a_pull_requests_waiting_submit_beside_a_driver() {
    let g = OnGitHub::new("pr-waiting-driver", "");
    let w = g.session("waiting", &[("w.txt", "w\n")]);
    let _driver = g.driver();
    let mut child = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "main", "--json"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    let n = loop {
        if let Some(p) = g.gh.forge.by_ref("unit/waiting").unwrap() {
            if !g.of(p.number).is_empty() {
                break p.number;
            }
        }
        assert!(Instant::now() < until, "never taken in");
        std::thread::sleep(Duration::from_millis(100));
    };
    // Two passes more of the submit's, each of which would land it.
    std::thread::sleep(Duration::from_secs(7));
    let alive = child.try_wait().unwrap().is_none();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(alive, "the submit ended beside the driver");
    let subs = g.of(n);
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0]["state"], "queued", "{subs:?}");
}

#[test]
fn sent_back_keeps_the_worktree() {
    let g = OnGitHub::new("pr-sent-back", "");
    let w = g.session("broken", &[("broken", "x\n")]);
    let out = g.wt_submit(&w, &[]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("failed: sh check.sh"), "{}", text(&out));
    assert!(w.exists());
}

/// 122, 123: a waiting submit whose pull request's submission lands, and
/// whose round then fails to release the combination's checkout, says the
/// failed try, reports it landed and exits 0: the round went on, so
/// nothing failed after the landing.
#[test]
fn a_landing_stands_when_its_release_then_fails() {
    let g = OnGitHub::new("pr-release", "");
    // The release runs the script the checkout holds, which the
    // submission breaks, as a landed script taking other arguments does.
    let policy = std::fs::read_to_string(g.repo.dir().join(".config/sluice.toml")).unwrap();
    g.repo.write(
        ".config/sluice.toml",
        &policy.replace(
            "command = \"sh check.sh\"\n",
            "command = \"sh check.sh\"\nrelease = \"sh release.sh\"\n",
        ),
    );
    g.repo.write("release.sh", "exit 0\n");
    g.repo.git(&["add", "-A"]);
    g.repo.git(&["commit", "-q", "-m", "release a checkout"]);
    g.repo.git(&["push", "-q", "origin", "main"]);
    let w = g.session(
        "release",
        &[("release.sh", "echo release.sh takes only up\nexit 1\n")],
    );
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "main"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    let n = g.gh.forge.by_ref("unit/release").unwrap().unwrap().number;
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(&format!("testing main + #{n} ·")),
        "{stdout}"
    );
    let main = g.main_on_github();
    assert!(
        stdout.ends_with(&format!("passed · landed as {}\n", &main[..7])),
        "{}",
        text(&out)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("sluice: after the landing: "), "{stderr}");
    assert!(
        stdout
            .lines()
            .any(|l| l.starts_with("could not release sluice/try/main/")
                && l.contains("the release command of ")
                && l.contains("takes only up")),
        "{}",
        text(&out)
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

/// Blamed, then submitted again on the same head (120): the submit marks
/// the pull request ready for review, intake takes the same head, and its
/// checks run again; the submit follows that test to its outcome.
#[test]
fn blamed_then_submitted_again_on_the_same_head() {
    let g = OnGitHub::new("pr-same-head", "");
    let w = g.session("again", &[("broken", "x\n")]);
    let out = g.wt_submit(&w, &[]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    let pull = g.pull_from("unit/again");
    let n = pull.number;
    assert!(pull.draft, "{pull:?}");

    let out = g.wt_submit(&w, &[]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(
        said.contains(&format!("updated #{n}")) && said.contains("marked ready for review"),
        "{said}"
    );
    assert!(!said.contains("nothing was queued or run again"), "{said}");
    assert!(
        said.contains(&format!("testing main + #{n} · 1 tasks to run")),
        "{said}"
    );
    assert!(said.contains("failed: sh check.sh"), "{said}");
    assert!(
        said.lines().last().is_some_and(
            |l| l == "the agent working in unit/again fixes it there and submits again"
        ),
        "{said}"
    );
    let subs = g.of(n);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], pull.head.as_str());
    assert_eq!(subs[1]["state"], "failed", "{subs:?}");
    // Sent back again: held for the next submit, as before.
    assert!(g.pull_from("unit/again").draft);
    assert_eq!(g.of_after(n, 2).len(), 2, "{:?}", g.subs());
    assert!(w.exists());
}

/// Timed out, then submitted again on the same head: the advice to submit
/// the same head again is one sluice takes, never "nothing was queued or
/// run again".
#[test]
fn timed_out_then_submitted_again_on_the_same_head() {
    let g = OnGitHub::with_timeout("pr-timed-out", "", "2s");
    let w = g.session("slowly", &[("slow", "x\n")]);
    let out = g.wt_submit(&w, &[]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(
        said.contains(
            "if the host was slow, the agent working in unit/slowly submits the same head again"
        ),
        "{said}"
    );
    assert!(!said.contains("draft"), "{said}");
    let n = g.pull_from("unit/slowly").number;

    let out = g.wt_submit(&w, &[]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(!said.contains("nothing was queued or run again"), "{said}");
    assert!(said.contains(&format!("testing main + #{n} ·")), "{said}");
    assert!(said.contains("timed out"), "{said}");
    let subs = g.of(n);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["result"], "timed out", "{subs:?}");
}

/// A submit on a queued pull request replaces its submission: the submit
/// withdraws the old head's submission itself, and the new head is taken
/// with no draft and no hold.
#[test]
fn a_new_head_submitted_while_queued() {
    let g = OnGitHub::new("pr-replaced", "");
    let w = g.session("replaced", &[("slow", "x\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let n = g.pull_from("unit/replaced").number;
    let first = g.of(n);
    assert_eq!(first.len(), 1, "{first:?}");
    let new = g.commit_in(&w, &[("more.txt", "more\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(
        said.contains(&format!("withdrew its submission {}", first[0]["id"])),
        "{said}"
    );
    let pull = g.pull_from("unit/replaced");
    assert!(!pull.draft && pull.head == new, "{pull:?}");
    let subs = g.of_after(n, 1);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[0]["state"], "withdrawn");
    assert_eq!(subs[0]["withdrawn"]["by"], "unit/replaced");
    assert_eq!(subs[1]["head"], new.as_str());
    assert_ne!(subs[1]["state"], "withdrawn", "{subs:?}");
    assert_ne!(
        g.posted(&new).map(|p| p.1),
        Some("waiting: the next submit".into())
    );
}

/// Submitted by the operator's ops: an agent's pull request, sent back, is
/// submitted from a second worktree on the same branch with ops's fix. It
/// is ready for review at the head pushed, names ops as its agent, and
/// lands.
#[test]
fn submitted_by_the_operators_ops() {
    let g = OnGitHub::new("pr-ops", "");
    let w = g.session("unit-1", &[("broken", "x\n")]);
    let out = g.wt_submit(&w, &[]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    let n = g.pull_from("unit/unit-1").number;
    assert!(g.pull_from("unit/unit-1").draft);

    let ops = g.repo.dir().with_extension("ops");
    let _ = std::fs::remove_dir_all(&ops);
    g.repo.git(&[
        "worktree",
        "add",
        "-q",
        "-f",
        ops.to_str().unwrap(),
        "unit/unit-1",
    ]);
    g.repo
        .git(&["-C", ops.to_str().unwrap(), "rm", "-q", "broken"]);
    g.repo.git(&[
        "-C",
        ops.to_str().unwrap(),
        "commit",
        "-q",
        "-m",
        "ops's fix",
    ]);
    let fixed = g
        .repo
        .git(&["-C", ops.to_str().unwrap(), "rev-parse", "HEAD"]);
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &ops)
        .env("SLUICE_AGENT", "madswan-ops")
        .args(["submit", "main", "--no-wait"])
        .output()
        .unwrap();
    let said = text(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(said.contains("marked ready for review"), "{said}");
    let pull = g.pull_from("unit/unit-1");
    assert!(!pull.draft && pull.head == fixed, "{pull:?}");
    assert!(
        pull.body.contains("<!-- sluice: agent=madswan-ops -->")
            && !pull.body.contains("agent=unit/unit-1"),
        "{}",
        pull.body
    );
    g.step_until("landed", |g| g.of(n).iter().any(|s| s["state"] == "landed"));
    let subs = g.of(n);
    let landed = subs.iter().find(|s| s["state"] == "landed").unwrap();
    assert_eq!(landed["from"]["agent"], "madswan-ops", "{subs:?}");
    assert_eq!(landed["head"], fixed.as_str());
}

/// S56 (120): an agent's pull request, refused for a conflict, waits for
/// its submitter. A fix pushed to its branch, standing for one another
/// submission landed there, moves its head; over several rounds it is a
/// draft, and nothing is submitted or landed in the agent's name, though
/// its review holds. The agent's next `wt submit` marks it ready at the
/// new head, and it lands. A pull request opened by hand whose head moves
/// the same way is submitted again once its review holds. One opened by
/// hand that the operator withdrew is not submitted again, though its
/// review holds, until its author pushes a new head.
#[test]
fn s56_a_refused_request_waits_for_its_submitter() {
    let g = OnGitHub::new("pr-s56", "reviews = 1");
    let approve = |n: u64, head: &str| {
        g.gh.forge.review(n, "reviewer", "APPROVED", head).unwrap();
    };
    // Three changes to one file, each off the same `main`: the first
    // lands, so the agent's and the hand-opened one conflict.
    let w = g.session("s56", &[("plan.py", "agent\n")]);
    let (hand, hand_head) = g.by_hand("unit/hand", &[("plan.py", Some("hand\n"))], false);
    let (first, first_head) = g.by_hand("unit/first", &[("plan.py", Some("first\n"))], false);
    approve(first, &first_head);
    g.step_until("the first landed", |g| {
        g.of(first).iter().any(|s| s["state"] == "landed")
    });
    // A fix of the agent's branch onto GitHub's `main`: a merge keeping
    // the branch's tree, which merges onto `main` without conflict.
    let fix_onto_main = |dir: &Path| {
        g.repo.git(&["fetch", "-q", "origin", "main"]);
        let main = g.main_on_github();
        let at = dir.to_str().unwrap();
        g.repo.git(&[
            "-C",
            at,
            "merge",
            "-q",
            "-s",
            "ours",
            &main,
            "-m",
            "Fix onto main",
        ]);
        let branch = g.repo.git(&["-C", at, "rev-parse", "--abbrev-ref", "HEAD"]);
        g.repo.git(&[
            "push",
            "-q",
            "origin",
            &format!("{branch}:refs/heads/{branch}"),
        ]);
        g.repo.git(&["-C", at, "rev-parse", "HEAD"])
    };

    // The agent's pull request, refused for a conflict.
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/s56");
    let n = pull.number;
    approve(n, &pull.head);
    g.step_until("conflict", |g| {
        g.of(n).iter().any(|s| s["state"] == "conflict")
    });

    // Its head moves, and its review holds on the new head: still held.
    let main = g.main_on_github();
    let fixed = fix_onto_main(&w);
    approve(n, &fixed);
    let subs = g.of_after(n, 3);
    assert_eq!(subs.len(), 1, "nothing submitted in its name: {subs:?}");
    assert_eq!(subs[0]["state"], "conflict");
    let pull = g.pull_from("unit/s56");
    assert!(pull.draft && pull.head == fixed, "{pull:?}");
    assert_eq!(g.main_on_github(), main, "nothing landed in its name");
    assert_eq!(g.posted(&fixed).unwrap().1, "waiting: the next submit");

    // The agent's next submit marks it ready at the new head, and it
    // lands.
    let out = g.wt_submit(&w, &[]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(said.contains("marked ready for review"), "{said}");
    assert!(said.contains("landed as"), "{said}");
    let subs = g.of(n);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], fixed.as_str());
    assert_eq!(subs[1]["state"], "landed");
    assert_eq!(subs[1]["from"]["agent"], "unit/s56");

    // Opened by hand, refused the same way and its head moved the same
    // way: submitted again once its review holds on the new head.
    approve(hand, &hand_head);
    g.step_until("the hand-opened conflict", |g| {
        g.of(hand).iter().any(|s| s["state"] == "conflict")
    });
    g.repo.git(&["switch", "-q", "unit/hand"]);
    let moved = fix_onto_main(g.repo.dir());
    g.repo.git(&["switch", "-q", "--detach", "main"]);
    let subs = g.of_after(hand, 2);
    assert_eq!(subs.len(), 1, "waits for its review: {subs:?}");
    assert!(!g.pull_from("unit/hand").draft);
    assert_eq!(g.posted(&moved).unwrap().1, "waiting: needs 1 review");
    approve(hand, &moved);
    g.step_until("the hand-opened landed", |g| {
        g.of(hand)
            .iter()
            .any(|s| s["state"] == "landed" && s["head"] == moved.as_str())
    });

    // Opened by hand and withdrawn by the operator: its offer rests, its
    // review holding, until its author pushes a new head.
    let (rest, rest_head) = g.by_hand("unit/rest", &[("slow", Some("x\n"))], false);
    approve(rest, &rest_head);
    g.step_until("the withdrawn one submitted", |g| !g.of(rest).is_empty());
    let id = g.of(rest)[0]["id"].as_u64().unwrap().to_string();
    let out = g.sluice(&["withdraw", &id, "--as", "operator"]);
    assert!(out.status.success(), "{}", text(&out));
    let subs = g.of_after(rest, 3);
    assert_eq!(subs.len(), 1, "never submitted again: {subs:?}");
    assert_eq!(subs[0]["state"], "withdrawn");
    assert_eq!(
        g.posted(&rest_head).unwrap().1,
        "waiting: withdrawn by the operator"
    );
    let pushed = g.push_more("unit/rest", "rest.txt");
    approve(rest, &pushed);
    g.step_until("the withdrawn one submitted again", |g| {
        g.of(rest).iter().any(|s| s["head"] == pushed.as_str())
    });
}

// ---------------------------------------------------------------------------
// group-submit, under pull requests (125)
// ---------------------------------------------------------------------------

impl OnGitHub {
    /// Sessions `unit/<name>` for each of `names`, each with one commit of
    /// its own file.
    fn sessions(&self, names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|n| self.session(n, &[(&format!("{n}.txt"), "x\n")]))
            .collect()
    }

    /// `sluice submit main` of `worktrees` as one act, then `rest`.
    fn submit_group(&self, worktrees: &[PathBuf], rest: &[&str]) -> Output {
        let mut args = vec!["submit", "main"];
        for w in worktrees {
            args.extend(["--worktree", w.to_str().unwrap()]);
        }
        args.extend(rest);
        self.sluice(&args)
    }

    /// The open pull request from `unit/<name>`.
    fn pull_of(&self, name: &str) -> sluice::profiles::github::forge::Pull {
        self.gh
            .forge
            .by_ref(&format!("unit/{name}"))
            .unwrap()
            .unwrap_or_else(|| panic!("no pull request from unit/{name}"))
    }
}

/// The group mark of a pull request's body: its token and the rest.
fn group_mark(body: &str) -> Option<(String, String)> {
    body.lines().find_map(|l| {
        let (token, rest) = l.strip_prefix("<!-- sluice: group=")?.split_once(' ')?;
        Some((token.to_string(), rest.to_string()))
    })
}

/// Three worktrees submitted as one act, none of their pull requests
/// reviewed: each branch is on GitHub, each has an open pull request into
/// `main`, a draft marked 1, 2 and 3 of 3 of one group at its head, and
/// the refusal names each with the review it needs; nothing is queued.
/// One of them submitted alone afterwards is marked one of no group.
#[test]
fn a_group_submit_opens_every_pull_request_marked() {
    let g = OnGitHub::new("pr-group", "reviews = 1");
    let names = ["g1", "g2", "g3"];
    let ws = g.sessions(&names);
    let out = g.submit_group(&ws, &["--no-wait"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    let mut token = None;
    for (k, (name, w)) in names.iter().zip(&ws).enumerate() {
        let pull = g.pull_of(name);
        assert!(pull.open && pull.draft && pull.base == "main", "{pull:?}");
        let head = github::rev(g.bare.path(), &format!("refs/heads/unit/{name}")).unwrap();
        assert_eq!(pull.head, head);
        assert!(pull
            .body
            .contains(&format!("<!-- sluice: agent=unit/{name} -->")));
        let (t, rest) = group_mark(&pull.body).unwrap_or_else(|| panic!("{}", pull.body));
        assert_eq!(rest, format!("{}/3 head={head} -->", k + 1));
        assert_eq!(*token.get_or_insert(t.clone()), t, "one group");
        let n = pull.number;
        assert!(
            said.contains(&format!("pushed unit/{name} · opened #{n}: unit/{name}")),
            "{said}"
        );
        assert!(
            said.contains(&format!(
                "refused: #{n} from {} needs 1 review",
                w.display()
            )),
            "{said}"
        );
        assert!(g.of(n).is_empty());
    }

    let out = g.wt_submit(&ws[0], &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_of("g1");
    assert_eq!(group_mark(&pull.body), None, "{}", pull.body);
    assert!(pull.body.contains("<!-- sluice: agent=unit/g1 -->"));
}

/// 125: two worktrees submitted with a subject each: each pull request is
/// opened titled by its own, in the worktrees' order; submitted again
/// with the subjects the other way round, each sluice opened takes its
/// new one.
#[test]
fn two_requests_two_subjects() {
    let g = OnGitHub::new("pr-group-subjects", "");
    let ws = g.sessions(&["a", "b"]);
    let subjects = [
        "--subject",
        "Batch review cards",
        "--subject",
        "Card filters",
    ];
    let out = g.submit_group(&ws, &[&["--no-wait"][..], &subjects].concat());
    let said = text(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    for (name, title) in [("a", "Batch review cards"), ("b", "Card filters")] {
        let pull = g.pull_of(name);
        assert_eq!(pull.title, title);
        assert!(
            said.contains(&format!(
                "pushed unit/{name} · opened #{}: {title}",
                pull.number
            )),
            "{said}"
        );
    }
    let swapped = [
        "--subject",
        "Card filters",
        "--subject",
        "Batch review cards",
    ];
    let out = g.submit_group(&ws, &[&["--no-wait"][..], &swapped].concat());
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(g.pull_of("a").title, "Card filters");
    assert_eq!(g.pull_of("b").title, "Batch review cards");
}

/// 125: two worktrees submitted with no subject: each pull request is
/// titled by its own branch.
#[test]
fn two_requests_no_subjects() {
    let g = OnGitHub::new("pr-group-no-subjects", "");
    let ws = g.sessions(&["a", "b"]);
    let out = g.submit_group(&ws, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(g.pull_of("a").title, "unit/a");
    assert_eq!(g.pull_of("b").title, "unit/b");
}

/// Whether pull request `n` is a draft, as GitHub holds it now.
fn is_draft(g: &OnGitHub, n: u64) -> bool {
    g.gh.forge.pull(&github::repo(), n).unwrap().unwrap().draft
}

/// Three pull requests, every condition holding: the act leaves each ready
/// for review at its head, marked 1, 2 and 3 of 3 of one group, and queues
/// a submission from each, from its checkout `pull/<n>` at that head, in
/// the order given and in one write, each said after its pull request;
/// they are tested as one combination and land.
#[test]
fn a_group_submit_queues_three_pull_requests_in_order() {
    let g = OnGitHub::batched("pr-group-three", "");
    let names = ["q1", "q2", "q3"];
    let ws = g.sessions(&names);
    let out = g.submit_group(&ws, &["--no-wait"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    let mut token = None;
    let mut ns = Vec::new();
    let mut ids = Vec::new();
    for (k, name) in names.iter().enumerate() {
        let pull = g.pull_of(name);
        assert!(pull.open && !pull.draft && pull.base == "main", "{pull:?}");
        let head = github::rev(g.bare.path(), &format!("refs/heads/unit/{name}")).unwrap();
        assert_eq!(pull.head, head);
        let (t, rest) = group_mark(&pull.body).unwrap_or_else(|| panic!("{}", pull.body));
        assert_eq!(rest, format!("{}/3 head={head} -->", k + 1));
        assert_eq!(*token.get_or_insert(t.clone()), t, "one group");
        let subs = g.of(pull.number);
        assert_eq!(subs.len(), 1, "{:?}", g.subs());
        assert_eq!(subs[0]["head"], head.as_str());
        assert_eq!(
            subs[0]["submitted"],
            g.of(g.pull_of(names[0]).number)[0]["submitted"]
        );
        ns.push(pull.number);
        ids.push(subs[0]["id"].as_u64().unwrap());
    }
    assert!(
        ids.windows(2).all(|w| w[0] < w[1]),
        "in the order given: {ids:?}"
    );
    let lines: Vec<&str> = said.lines().collect();
    for (k, name) in names.iter().enumerate() {
        let opened = format!(
            "pushed unit/{name} · opened #{}: unit/{name} · marked ready for review",
            ns[k]
        );
        let at = lines
            .iter()
            .position(|l| *l == opened)
            .unwrap_or_else(|| panic!("{opened:?} in {said}"));
        let ahead = match k {
            0 => "nothing queued ahead".to_string(),
            _ => format!(
                "{k} queued ahead ({})",
                ns[..k]
                    .iter()
                    .map(|n| format!("#{n}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        assert_eq!(
            lines.get(at + 1).copied(),
            Some(format!("submission {} to main from #{} · {ahead}", ids[k], ns[k]).as_str()),
            "{said}"
        );
    }
    g.step_until("landed", |g| {
        ns.iter()
            .all(|n| g.of(*n).first().is_some_and(|s| s["state"] == "landed"))
    });
    for n in &ns {
        assert_eq!(
            g.of(*n)[0]["tested"]["submissions"],
            serde_json::json!(ids),
            "one combination of all three"
        );
    }
}

/// Under `--json` with `--no-wait`, the act answers with each submission
/// beside its worktree and pull request, and what is queued ahead of it.
#[test]
fn a_group_submit_answers_with_each_submission_in_json() {
    let g = OnGitHub::new("pr-group-json", "");
    let names = ["j1", "j2"];
    let ws = g.sessions(&names);
    let out = g.submit_group(&ws, &["--no-wait", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", text(&out)));
    assert_eq!(doc["branch"], "main", "{doc}");
    let items = doc["submissions"].as_array().expect("submissions");
    assert_eq!(items.len(), 2, "{doc}");
    let mut ahead = Vec::new();
    for ((item, w), name) in items.iter().zip(&ws).zip(names) {
        let n = g.pull_of(name).number;
        let id = g.of(n)[0]["id"].as_u64().unwrap();
        assert_eq!(item["id"], id, "{doc}");
        assert_eq!(item["worktree"], w.to_str().unwrap(), "{doc}");
        assert_eq!(item["pull_request"], n, "{doc}");
        assert_eq!(item["ahead"], serde_json::json!(ahead), "{doc}");
        ahead.push(id);
    }
}

/// One short of a review: the pull requests of `s1` and `s3` are open at
/// the heads their worktrees hold, ready and approved there, and `s2`'s is
/// not open. The act queues nothing and exits 2, naming `s2`'s pull
/// request and that it needs 1 review, and saying all three are drafts;
/// each is a draft at its head, marked one of the group, and no round
/// takes any of them.
#[test]
fn a_group_submit_one_short_of_a_review_is_refused() {
    let g = OnGitHub::new("pr-group-short", "reviews = 1");
    let names = ["s1", "s2", "s3"];
    let ws = g.sessions(&names);
    let mut by_hand = Vec::new();
    for name in [names[0], names[2]] {
        let branch = format!("unit/{name}");
        g.repo.git(&[
            "push",
            "-q",
            "origin",
            &format!("{branch}:refs/heads/{branch}"),
        ]);
        let head = g.repo.rev(&branch);
        let n =
            g.gh.forge
                .open_by("person", "main", &branch, &head, false, None)
                .unwrap();
        g.gh.forge.review(n, "reviewer", "APPROVED", &head).unwrap();
        by_hand.push(n);
    }
    let out = g.submit_group(&ws, &["--no-wait"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    let ns: Vec<u64> = names.iter().map(|n| g.pull_of(n).number).collect();
    assert_eq!([ns[0], ns[2]], [by_hand[0], by_hand[1]]);
    assert!(
        said.contains(&format!(
            "refused: #{} from {} needs 1 review",
            ns[1],
            ws[1].display()
        )),
        "{said}"
    );
    for n in [ns[0], ns[2]] {
        assert!(!said.contains(&format!("refused: #{n}")), "{said}");
    }
    assert!(
        said.contains(&format!(
            "nothing submitted: #{}, #{}, #{} are drafts at their heads",
            ns[0], ns[1], ns[2]
        )),
        "{said}"
    );
    let again = format!(
        "sluice submit main {}",
        ws.iter()
            .map(|w| format!("--worktree {}", w.display()))
            .collect::<Vec<_>>()
            .join(" ")
    );
    assert!(
        said.contains(&format!(
            "once each meets its conditions, the same submit takes them all: {again}"
        )),
        "{said}"
    );
    let mut token = None;
    for (k, name) in names.iter().enumerate() {
        let pull = g.pull_of(name);
        assert!(pull.open && pull.draft, "{pull:?}");
        let head = github::rev(g.bare.path(), &format!("refs/heads/unit/{name}")).unwrap();
        assert_eq!(pull.head, head);
        let (t, rest) = group_mark(&pull.body).unwrap_or_else(|| panic!("{}", pull.body));
        assert_eq!(rest, format!("{}/3 head={head} -->", k + 1));
        assert_eq!(*token.get_or_insert(t.clone()), t, "one group");
        assert!(g.of(pull.number).is_empty(), "{:?}", g.subs());
    }
    g.step();
    g.step();
    for n in &ns {
        assert!(g.of(*n).is_empty(), "{:?}", g.subs());
        assert!(is_draft(&g, *n));
    }

    let out = g.submit_group(&ws, &["--no-wait", "--json"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", text(&out)));
    assert_eq!(
        doc,
        serde_json::json!({ "error": {
            "kind": "refused",
            "message": format!("#{} from {} needs 1 review", ns[1], ws[1].display()),
            "next": [again],
            "worktrees": [ws[1].to_str().unwrap()],
        } })
    );
    for n in &ns {
        assert!(is_draft(&g, *n) && g.of(*n).is_empty(), "{:?}", g.subs());
    }
}

/// Refused, reviewed, submitted again: an act over two worktrees refused
/// for want of reviews leaves two drafts; once a reviewer approves each at
/// its head, the same act leaves both ready and queues both, in the order
/// given and in one write.
#[test]
fn a_group_submit_refused_then_reviewed_is_taken() {
    let g = OnGitHub::new("pr-group-again", "reviews = 1");
    let names = ["r1", "r2"];
    let ws = g.sessions(&names);
    let out = g.submit_group(&ws, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let ns: Vec<u64> = names.iter().map(|n| g.pull_of(n).number).collect();
    for n in &ns {
        assert!(is_draft(&g, *n));
        assert!(g.of(*n).is_empty(), "{:?}", g.subs());
        approve(&g, *n);
    }
    let out = g.submit_group(&ws, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let subs: Vec<Value> = ns
        .iter()
        .map(|n| {
            assert!(!is_draft(&g, *n));
            let mut of = g.of(*n);
            assert_eq!(of.len(), 1, "{:?}", g.subs());
            of.remove(0)
        })
        .collect();
    assert!(subs[0]["id"].as_u64() < subs[1]["id"].as_u64(), "{subs:?}");
    assert_eq!(subs[0]["submitted"], subs[1]["submitted"], "one write");
}

/// A pull request sluice held as a draft: one of an earlier act is blamed
/// and sluice marks it a draft. Fixed, its worktree is named again in a
/// new act whose conditions hold: it is ready for review at the head
/// pushed, and queued with the rest of the new act.
#[test]
fn a_group_submit_takes_a_pull_request_held_as_a_draft() {
    let g = OnGitHub::new("pr-group-held", "");
    let w1 = g.session("k1", &[("k1.txt", "x\n"), ("broken", "x\n")]);
    let w2 = g.session("k2", &[("k2.txt", "x\n")]);
    let out = g.submit_group(&[w1.clone(), w2], &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let n1 = g.pull_of("k1").number;
    g.step_until("blamed and held", |g| {
        g.of(n1).first().is_some_and(|s| s["state"] == "failed") && is_draft(g, n1)
    });
    std::fs::remove_file(w1.join("broken")).unwrap();
    let fixed = g.commit_in(&w1, &[("k1.txt", "fixed\n")]);
    let w3 = g.session("k3", &[("k3.txt", "x\n")]);
    let out = g.submit_group(&[w1, w3], &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let p1 = g.pull_of("k1");
    assert!(!p1.draft && p1.head == fixed, "{p1:?}");
    let again: Vec<Value> = g
        .of(n1)
        .into_iter()
        .filter(|s| s["head"] == fixed.as_str())
        .collect();
    assert_eq!(again.len(), 1, "{:?}", g.subs());
    assert!(!again[0]["state"].as_str().is_some_and(|s| s == "withdrawn"));
    let n3 = g.pull_of("k3").number;
    let with = g.of(n3);
    assert_eq!(with.len(), 1, "{:?}", g.subs());
    assert_eq!(with[0]["submitted"], again[0]["submitted"], "one write");
    assert!(again[0]["id"].as_u64() < with[0]["id"].as_u64());
}

/// A fork's pull request on a branch of the same name: a fork has a pull
/// request into `main` from its own `unit/f1`, and the act names the
/// worktree on the repository's `unit/f1`. The fork's is not edited,
/// marked or queued; the act's pull request is the repository's own.
#[test]
fn a_group_submit_leaves_a_forks_pull_request_alone() {
    let g = OnGitHub::new("pr-group-fork", "");
    let ws = g.sessions(&["f1", "f2"]);
    let fork_head = g
        .repo
        .branch("fork-f1", "main", &[("fork.txt", Some("f\n"))]);
    g.repo.git(&["switch", "-q", "--detach", "main"]);
    let fork =
        g.gh.forge
            .open_by(
                "forker",
                "main",
                "unit/f1",
                &fork_head,
                false,
                Some("forker/r"),
            )
            .unwrap();
    let before = g.gh.forge.pull(&github::repo(), fork).unwrap().unwrap();
    let out = g.submit_group(&ws, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(
        g.gh.forge.pull(&github::repo(), fork).unwrap().unwrap(),
        before
    );
    assert!(g
        .gh
        .forge
        .readied(&github::repo(), fork)
        .unwrap()
        .is_empty());
    assert!(g.of(fork).is_empty(), "{:?}", g.subs());
    let ours = g.pull_of("f1");
    assert_ne!(ours.number, fork);
    assert_eq!(ours.head_repo, "o/r");
    assert!(!ours.draft, "{ours:?}");
    assert_eq!(g.of(ours.number).len(), 1, "{:?}", g.subs());
}

/// A group whose second worktree holds more than its commits is refused
/// whole: it names the file, exits 2, and pushes and opens nothing for
/// either.
#[test]
fn a_group_submit_with_a_dirty_worktree_pushes_nothing() {
    let g = OnGitHub::new("pr-group-dirty", "");
    let ws = g.sessions(&["d1", "d2"]);
    std::fs::write(ws[1].join("d2.txt"), "changed\n").unwrap();
    let out = g.submit_group(&ws, &[]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("d2.txt"), "{}", text(&out));
    for name in ["d1", "d2"] {
        assert_eq!(
            github::rev(g.bare.path(), &format!("refs/heads/unit/{name}")),
            None
        );
    }
    assert!(g.gh.forge.held().unwrap().pulls.is_empty());
}

/// A waiting act follows each pull request: it says each one's state as
/// it reaches it, then each one's outcome in the order given, and exits 0
/// once all three land.
#[test]
fn a_waiting_group_submit_follows_each_pull_request() {
    let g = OnGitHub::new("pr-group-wait", "");
    let names = ["w1", "w2", "w3"];
    let ws = g.sessions(&names);
    let out = g.submit_group(&ws, &[]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    let lines: Vec<&str> = said.lines().collect();
    let mut at = Vec::new();
    for (name, w) in names.iter().zip(&ws) {
        let n = g.pull_of(name).number;
        assert!(
            g.of(n).iter().any(|s| s["state"] == "landed"),
            "{:?}",
            g.subs()
        );
        let header = format!("#{n} from {}:", w.display());
        let i = lines
            .iter()
            .position(|l| *l == header)
            .unwrap_or_else(|| panic!("{header:?} in {said}"));
        assert!(
            lines[..i]
                .iter()
                .any(|l| l.starts_with(&format!("#{n} queued"))
                    || l.starts_with(&format!("#{n} testing"))),
            "#{n}'s states before its outcome: {said}"
        );
        at.push(i);
    }
    assert!(
        at.windows(2).all(|w| w[0] < w[1]),
        "in the order given: {said}"
    );
    for (k, i) in at.iter().enumerate() {
        let end = at.get(k + 1).copied().unwrap_or(lines.len());
        assert!(
            lines[*i..end]
                .iter()
                .any(|l| l.starts_with("passed · landed as ")),
            "{said}"
        );
    }
}

// ---------------------------------------------------------------------------
// pull-request-intake, a group's pull requests (125)
// ---------------------------------------------------------------------------

/// A person approves pull request `n` at its head.
fn approve(g: &OnGitHub, n: u64) {
    let head = g.gh.forge.pull(&github::repo(), n).unwrap().unwrap().head;
    g.gh.forge.review(n, "reviewer", "APPROVED", &head).unwrap();
}

/// What sluice's check on pull request `n`'s head reads, its title.
fn shows(g: &OnGitHub, n: u64) -> String {
    let head = g.gh.forge.pull(&github::repo(), n).unwrap().unwrap().head;
    g.posted(&head).map(|p| p.1).unwrap_or_default()
}

/// Two of three are ready: a group submit of three refused for want of
/// reviews leaves three drafts marked as one group; a reviewer approves
/// the first two, and a person marks all three ready for review. None is
/// submitted, and each of the two says it waits for the third; once the
/// third is approved, one round submits all three in the group's order,
/// and the first combination formed holds all three.
#[test]
fn two_of_a_group_ready_submits_none() {
    let g = OnGitHub::batched("pr-group-two", "reviews = 1");
    let names = ["t1", "t2", "t3"];
    let out = g.submit_group(&g.sessions(&names), &["--no-wait"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let ns: Vec<u64> = names.iter().map(|n| g.pull_of(n).number).collect();
    approve(&g, ns[0]);
    approve(&g, ns[1]);
    for n in &ns {
        assert!(is_draft(&g, *n));
        g.gh.forge.set_draft(*n, false).unwrap();
    }
    g.step();
    g.step();
    for n in &ns {
        assert!(g.of(*n).is_empty(), "{:?}", g.subs());
    }
    for n in &ns[..2] {
        assert_eq!(shows(&g, *n), format!("waiting for its group: #{}", ns[2]));
    }
    assert_eq!(shows(&g, ns[2]), "waiting: needs 1 review");

    approve(&g, ns[2]);
    g.step();
    let ids: Vec<u64> = ns
        .iter()
        .map(|n| g.of(*n)[0]["id"].as_u64().unwrap())
        .collect();
    assert!(
        ids.windows(2).all(|w| w[0] < w[1]),
        "in the group's order: {ids:?}"
    );
    g.step_until("landed", |g| {
        ns.iter()
            .all(|n| g.of(*n).first().is_some_and(|s| s["state"] == "landed"))
    });
    for n in &ns {
        let tested = &g.of(*n)[0]["tested"]["submissions"];
        assert_eq!(
            *tested,
            serde_json::json!(ids),
            "one combination of all three"
        );
    }
}

/// A round between the opens: with two of a group of three open, no round
/// submits either; once the third is open, the next round submits all
/// three, and the first combination formed holds all three.
#[test]
fn a_round_between_a_groups_opens_submits_none() {
    let g = OnGitHub::batched("pr-group-between", "");
    let mut ns = Vec::new();
    let mark = |k: usize, n: u64, head: &str| {
        g.gh.forge
            .edit(
                &github::repo(),
                n,
                None,
                &format!("<!-- sluice: agent=ops -->\n<!-- sluice: group=t {k}/3 head={head} -->"),
            )
            .unwrap();
    };
    for k in 1..=2 {
        let (n, head) = g.by_hand(
            &format!("unit/o{k}"),
            &[(&format!("o{k}.txt"), Some("o\n"))],
            false,
        );
        mark(k, n, &head);
        ns.push(n);
    }
    g.step();
    g.step();
    for n in &ns {
        assert!(g.of(*n).is_empty(), "{:?}", g.subs());
        assert_eq!(
            shows(&g, *n),
            "waiting for its group: the request marked 3/3, not open"
        );
    }
    let (n, head) = g.by_hand("unit/o3", &[("o3.txt", Some("o\n"))], false);
    mark(3, n, &head);
    ns.push(n);
    g.step();
    let ids: Vec<u64> = ns
        .iter()
        .map(|n| g.of(*n)[0]["id"].as_u64().unwrap())
        .collect();
    g.step_until("landed", |g| {
        ns.iter()
            .all(|n| g.of(*n).first().is_some_and(|s| s["state"] == "landed"))
    });
    for n in &ns {
        assert_eq!(g.of(*n)[0]["tested"]["submissions"], serde_json::json!(ids));
    }
}

/// One of a group closed before it is taken: a group submit refused for
/// want of reviews leaves three drafts; the second is closed, and the
/// others approved and marked ready for review by a person. They are not
/// submitted and say they wait for it, until a submit names one of them
/// again, alone: `sluice submit`'s, since `wt submit` answers with a
/// request open and ready at its head (record 0065).
#[test]
fn one_of_a_group_closed_holds_the_rest() {
    let g = OnGitHub::new("pr-group-closed", "reviews = 1");
    let names = ["c1", "c2", "c3"];
    let ws = g.sessions(&names);
    let out = g.submit_group(&ws, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let ns: Vec<u64> = names.iter().map(|n| g.pull_of(n).number).collect();
    g.gh.forge.close(ns[1]).unwrap();
    for n in [ns[0], ns[2]] {
        approve(&g, n);
        g.gh.forge.set_draft(n, false).unwrap();
    }
    g.step();
    g.step();
    for n in [ns[0], ns[2]] {
        assert!(g.of(n).is_empty(), "{:?}", g.subs());
        assert_eq!(
            shows(&g, n),
            "waiting for its group: the request marked 2/3, not open"
        );
    }
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &ws[0])
        .args(["submit", "main", "--no-wait"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    approve(&g, ns[0]);
    g.step();
    assert_eq!(g.of(ns[0]).len(), 1, "{:?}", g.subs());
    assert!(g.of(ns[2]).is_empty(), "{:?}", g.subs());
}

/// Each its own once taken: a commit no submit made, pushed to the second
/// of a group already submitted, withdraws only the second's submission,
/// and sluice marks the second a draft; the first and third are tested
/// without it and land, and no round submits the second again. Its
/// worktree's `sluice submit main` marks it ready at its new head, one of
/// no group, and it is submitted again alone and lands (120, 125).
#[test]
fn one_of_a_group_on_a_new_head_is_withdrawn_alone() {
    let g = OnGitHub::new("pr-group-new-head", "");
    let names = ["h1", "h2", "h3"];
    let ws: Vec<PathBuf> = names
        .iter()
        .map(|n| g.session(n, &[(&format!("{n}.txt"), "x\n"), ("slow", "x\n")]))
        .collect();
    let out = g.submit_group(&ws, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let ns: Vec<u64> = names.iter().map(|n| g.pull_of(n).number).collect();
    g.step();
    let first: Vec<u64> = ns
        .iter()
        .map(|n| g.of(*n)[0]["id"].as_u64().expect("submitted together"))
        .collect();
    let moved = g.commit_in(&ws[1], &[("h2-more.txt", "more\n")]);
    g.repo
        .git(&["push", "-q", "origin", "unit/h2:refs/heads/unit/h2"]);
    g.step();
    assert_eq!(g.of(ns[1])[0]["state"], "withdrawn", "{:?}", g.subs());
    assert!(is_draft(&g, ns[1]));
    for n in [ns[0], ns[2]] {
        assert_ne!(g.of(n)[0]["state"], "withdrawn", "{:?}", g.subs());
    }
    g.step_until("the first and third landed", |g| {
        [ns[0], ns[2]]
            .iter()
            .all(|n| g.of(*n).iter().any(|s| s["state"] == "landed"))
    });
    for n in [ns[0], ns[2]] {
        let landed = g
            .of(n)
            .into_iter()
            .find(|s| s["state"] == "landed")
            .unwrap();
        let tested = landed["tested"]["submissions"].as_array().unwrap().clone();
        assert!(!tested.contains(&serde_json::json!(first[1])), "{landed}");
    }
    let held = g.of_after(ns[1], 2);
    assert_eq!(held.len(), 1, "never submitted again by a round: {held:?}");
    assert!(is_draft(&g, ns[1]));

    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &ws[1])
        .args(["submit", "main", "--no-wait"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_of("h2");
    assert!(!pull.draft && pull.head == moved, "{pull:?}");
    assert_eq!(group_mark(&pull.body), None, "{}", pull.body);
    g.step_until("the second submitted again and landed", |g| {
        g.of(ns[1])
            .iter()
            .any(|s| s["head"] == moved.as_str() && s["state"] == "landed")
    });
    let again: Vec<Value> = g
        .of(ns[1])
        .into_iter()
        .filter(|s| s["head"] == moved.as_str())
        .collect();
    assert_eq!(again.len(), 1, "{:?}", g.subs());
    assert_eq!(
        again[0]["tested"]["submissions"],
        serde_json::json!([again[0]["id"]]),
        "alone"
    );
}

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
    /// `sluice submit main --head <branch>` with `args`, run in the
    /// repository's git directory, where no files are checked out.
    fn submit_by_name(&self, branch: &str, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_sluice"), &self.repo.dir().join(".git"))
            .args(["submit", "main", "--head", branch])
            .args(args)
            .output()
            .unwrap()
    }

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
        std::thread::sleep(Duration::from_millis(200));
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
/// started.
fn started(pid: &Path) {
    let until = Instant::now() + Duration::from_secs(60);
    while !pid.exists() {
        assert!(Instant::now() < until, "the check never started");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// 135, 67; record 0100 D2, D4: a waiting submit by name whose request
/// `bolt/x` moves under, taken in again on its new head and landed there,
/// ends with the submission it made, landed by that one's landing and
/// naming it in `carried_by`, never with that one's record.
#[test]
fn a_waiting_submit_ends_with_its_own_submission_when_the_branch_moves_under_it() {
    let (g, pid) = with_a_hanging_bolt("pr-by-name-moves");
    let child = g.spawn_by_name("bolt/x", &["--json"]);
    started(&pid);
    let n = g.pull_from("bolt/x").number;
    let first = g.of(n)[0]["id"].as_u64().unwrap();
    // A commit lands on GitHub's bolt/x, and the next submit by name takes
    // the request in again on it.
    let head = g.bolt_on_github();
    let tree = g.on_github(&["rev-parse", &format!("{head}^{{tree}}")]);
    let moved = g.on_github(&["commit-tree", &tree, "-p", &head, "-m", "more on bolt/x"]);
    g.on_github(&["update-ref", "refs/heads/bolt/x", &moved, &head]);
    let out = g.submit_by_name("bolt/x", &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));

    let out = ended(child);
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    let subs = g.of(n);
    let second = subs
        .iter()
        .find(|x| x["head"] == moved.as_str())
        .unwrap_or_else(|| panic!("{subs:?}"));
    assert_eq!(second["state"], "landed", "{second}");
    assert_eq!(doc["id"], first, "{doc}");
    assert_eq!(doc["state"], "landed", "{doc}");
    assert_eq!(doc["carried_by"]["id"], second["id"], "{doc}");
    assert_eq!(doc["carried_by"]["state"], "landed", "{doc}");
}

/// 122, 135; the signal `agentplot/crew-state:agentplot/main
/// signals/2026-11-07-agentplot-ops-d4947942/01-sluice-exits-4-after-its-own-landing`:
/// a waiting submit by name in the repository's git directory, whose
/// request another queue of `main` lands once this host took it in, exits
/// 0 and says `landed as <rev>`, never `withdrawn`, and its submission's
/// record reads landed, `landed_elsewhere`.
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
    let sub = g
        .of(pull.number)
        .into_iter()
        .find(|x| x["id"] == first)
        .unwrap();
    assert_eq!(sub["state"], "landed", "{sub}");
    assert_eq!(sub["landed_elsewhere"], true, "{sub}");
}

/// 124, C.5, D7: `main` takes pull requests, and this host's `bolt/x`
/// stands two landings behind GitHub's. `sluice submit main --head bolt/x`
/// in the repository's git directory opens the request from `bolt/x` into
/// `main`, ready, titled `bolt/x`, at GitHub's revision; nothing is pushed
/// and nothing is said of the host's copy; intake takes it in as the
/// request's, its `from.head` `bolt/x`, with no path and no session.
/// Submitted again with a subject, the same request is titled by it.
#[test]
fn a_submit_by_name_opens_the_request_from_the_branch() {
    let g = OnGitHub::with_bolt("pr-by-name");
    let made = g.bolt_on_github();
    g.land_on_bolt("one", &[("one.txt", "1\n")]);
    let remote = g.land_on_bolt("two", &[("two.txt", "2\n")]);
    g.repo.git(&[
        "-c",
        "core.hooksPath=/dev/null",
        "update-ref",
        "refs/heads/bolt/x",
        &made,
    ]);

    let out = g.submit_by_name("bolt/x", &["--no-wait", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    let pull = g.pull_from("bolt/x");
    assert!(pull.open && !pull.draft && pull.base == "main", "{pull:?}");
    assert_eq!(pull.title, "bolt/x");
    assert_eq!(pull.head, remote);
    assert_eq!(
        g.bolt_on_github(),
        remote,
        "nothing pushed to GitHub's bolt/x"
    );
    assert_eq!(g.repo.rev("bolt/x"), made, "this host's bolt/x unmoved");
    assert_eq!(doc["pull_request"], pull.number, "{doc}");
    assert_eq!(doc["branch"], "main", "{doc}");
    assert!(doc.get("copy").is_none(), "{doc}");
    let id = doc["id"].as_u64().unwrap_or_else(|| panic!("{doc}"));
    let subs = g.of(pull.number);
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0]["id"], id);
    assert_eq!(subs[0]["head"], remote.as_str());
    assert_eq!(subs[0]["from"]["head"], "bolt/x");
    assert_eq!(subs[0]["from"]["path"], Value::Null);
    assert_eq!(subs[0]["from"]["session"], Value::Null);

    let out = g.submit_by_name(
        "bolt/x",
        &["--subject", "Batch the review cards", "--no-wait"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let said = text(&out);
    assert!(
        said.contains(&format!("updated #{}: Batch the review cards", pull.number)),
        "{said}"
    );
    assert!(
        !said.contains("pushed") && !said.contains("this host's"),
        "{said}"
    );
    let held = g.gh.forge.held().unwrap();
    assert_eq!(held.pulls.len(), 1);
    assert_eq!(held.pulls[0].title, "Batch the review cards");
    assert_eq!(g.bolt_on_github(), remote);
}

/// 124, 51, D6: the request a submit by name opened is blamed while GitHub
/// refuses every post of its outcome, so the outcome stays owed to the
/// request with no hand-over;
/// `sluice inbox --json` in a worktree that has checked out `bolt/x`
/// collects nothing of it, since no worktree pushed a branch sluice lands
/// on, and `main`'s status shows it owed to the request.
#[test]
fn a_request_by_name_is_collected_by_no_worktree() {
    let g = OnGitHub::checked_policy("pr-by-name-inbox", "", "1m", "", BOLTS);
    // GitHub's bolt/x breaks main's check.
    g.repo.branch("bolt/x", "main", &[("broken", Some("x\n"))]);
    g.repo
        .git(&["push", "-q", "origin", "bolt/x:refs/heads/bolt/x"]);
    let out = g.submit_by_name("bolt/x", &["--no-wait", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    let id = doc["id"].as_u64().unwrap_or_else(|| panic!("{doc}"));
    let n = g.pull_from("bolt/x").number;

    for (method, path) in [
        ("POST", "/repos/o/r/check-runs"),
        ("PATCH", "/repos/o/r/check-runs"),
        ("POST", "/repos/o/r/issues"),
        ("PATCH", "/repos/o/r/issues"),
    ] {
        g.gh.trouble(method, path, usize::MAX, github::Trouble::Status(422));
    }
    let until = Instant::now() + Duration::from_secs(60);
    while g.of(n).iter().all(|s| s["state"] != "failed") {
        assert!(Instant::now() < until, "never failed: {:?}", g.subs());
        g.discovery_passes();
        g.sluice(&["step", "main"]);
        std::thread::sleep(Duration::from_millis(200));
    }
    let sub = g.of(n).into_iter().find(|s| s["id"] == id).unwrap();
    assert_eq!(sub["state"], "failed", "{sub}");
    assert_eq!(sub["from"]["head"], "bolt/x", "{sub}");
    assert_eq!(g.delivery(&sub["id"])["state"], "owed", "{sub}");

    let w = g.repo.dir().with_extension("on-bolt");
    let _ = std::fs::remove_dir_all(&w);
    g.repo
        .git(&["worktree", "add", "-q", w.to_str().unwrap(), "bolt/x"]);
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["inbox", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    assert_eq!(doc, serde_json::json!({ "outcomes": [] }));
    let id = Value::from(id);
    assert_eq!(g.delivery(&id)["reader"], format!("pull/{n}").as_str());
    assert_eq!(g.delivery(&id)["handed"], serde_json::json!([]));
    // Submit-by-name spec, "No worktree on the branch collects its
    // outcome": still owed to the request, it is shown in `main`'s status
    // under the request, handed over by nothing (record 0102, D13).
    let out = g.sluice(&["status", "main"]);
    let said = text(&out);
    let owed = said
        .split_once("OWED\n")
        .unwrap_or_else(|| panic!("no OWED section: {said}"))
        .1;
    let mut lines = owed.lines();
    assert!(
        lines
            .next()
            .unwrap_or_default()
            .starts_with(&format!("  pull/{n}")),
        "{said}"
    );
    let row = lines.next().unwrap_or_default();
    assert!(row.contains(&format!("{id}:failed #{id}")), "{said}");
    assert!(row.contains("handed over by nothing"), "{said}");
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
    assert!(!g.pull_from("unit/paused").open, "landed");
}

/// The forge-allowance spec's "A wait whose timeout comes first": `sluice
/// wait --timeout 5m` beside a pause twenty minutes long exits `5` with
/// the submission as it stands, having made no call to GitHub.
#[test]
fn a_wait_whose_timeout_comes_first_calls_nothing() {
    let g = OnGitHub::new("pr-spent-wait", "");
    let (n, _) = g.by_hand("unit/slow", &[("slow", Some("x\n"))], false);
    g.step();
    let id = g.of(n)[0]["id"].as_u64().expect("taken in").to_string();
    g.spent_for(20);
    let before = g.repo_calls();
    let out = g.sluice(&["wait", &id, "--timeout", "5m"]);
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
    /// The host's config sets `[forge] discovery = "<every>"`.
    fn discovery(&self, every: &str) {
        let path = self.config.path().join("sluice/config.toml");
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str(&format!("discovery = {every:?}\n"));
        std::fs::write(&path, text).unwrap();
    }

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

    /// How many times what pull request `n` at `head` is beside its list
    /// entry was read: its reviews, other systems' checks and statuses on
    /// its head, its timeline and its comments.
    fn beside_reads(&self, n: u64, head: &str) -> usize {
        [
            format!("/repos/o/r/pulls/{n}/reviews"),
            format!("/repos/o/r/commits/{head}/check-runs?per_page"),
            format!("/repos/o/r/commits/{head}/status"),
            format!("/repos/o/r/issues/{n}/timeline"),
            format!("/repos/o/r/issues/{n}/comments"),
        ]
        .iter()
        .map(|p| self.gh.calls("GET", p))
        .sum()
    }

    /// How many times sluice's own check runs on `head` were read back.
    fn posted_reads(&self, head: &str) -> usize {
        self.gh.calls(
            "GET",
            &format!("/repos/o/r/commits/{head}/check-runs?app_id"),
        )
    }
}

/// forge-reads, "Nothing moved" and "A restart asks nothing new": twenty
/// passes of `sluice run` over a minute ask for main's open pull requests
/// once, and a step started again then asks nothing; the one waiting for a
/// review is read again only at that asking, by the one query.
#[test]
fn nothing_moved_asks_once_a_minute() {
    let g = OnGitHub::new("pr-discovery", "reviews = 1");
    let (n, head) = g.by_hand("unit/review", &[("r.txt", Some("r\n"))], false);
    g.settle();
    g.on_main();
    g.discovery_passes();
    let (lists, beside, queries) = (g.lists(), g.beside_reads(n, &head), g.gh.queries());
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "20"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(g.lists(), lists + 1, "{}", text(&out));
    assert_eq!(g.gh.queries(), queries + 1, "its reviews, at the asking");
    assert_eq!(g.beside_reads(n, &head), beside, "nothing read alone");
    assert_eq!(
        g.posted(&head).unwrap().1,
        "waiting: needs 1 review",
        "{}",
        text(&out)
    );
    let posts = g.posts();
    for _ in 0..3 {
        let out = g.sluice(&["step", "main"]);
        assert!(out.status.success(), "{}", text(&out));
    }
    assert_eq!(
        g.lists(),
        lists + 1,
        "a restart asks nothing within the minute"
    );
    assert_eq!(g.gh.queries(), queries + 1);
    assert_eq!(g.beside_reads(n, &head), beside);
    assert_eq!(g.posts(), posts);
}

/// reader-tick, "A branch that takes pull requests between discoveries":
/// none open and the host's interval a minute, six passes of `sluice run`
/// within it start git exactly as often as one, and ask GitHub nothing
/// after the first (129, 136).
#[test]
fn passes_between_discoveries_start_no_git_and_ask_github_nothing() {
    let g = OnGitHub::new("pr-idle-ticks", "");
    g.settle();
    g.on_main();
    let git = support::CountingGit::new();
    let run = |rounds: &str| -> (Vec<String>, Vec<(String, String)>) {
        g.discovery_passes();
        let (starts, calls) = (git.count(), g.gh.fake.requests().len());
        let out = git
            .on(&mut g.command(env!("CARGO_BIN_EXE_sluice"), g.repo.dir()))
            .env("SLUICE_ROUND_SLEEP", "record")
            .args(["run", "--rounds", rounds])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        let asked = g.gh.fake.requests()[calls..]
            .iter()
            .map(|r| (r.method.clone(), r.path.clone()))
            .collect();
        (git.starts()[starts..].to_vec(), asked)
    };
    run("1");
    let lists = g.lists();
    let (one, asked_one) = run("1");
    let (six, asked_six) = run("6");
    // Main's records are on its remote: each pass asks it once, and starts
    // nothing else (129).
    let fetches = |s: &[String]| s.iter().filter(|a| a.contains(" fetch ")).count();
    assert_eq!(six.len(), one.len() + 5, "one: {one:?}; six: {six:?}");
    assert_eq!(fetches(&six), fetches(&one) + 5, "{six:?}");
    assert_eq!(g.lists(), lists + 2, "asked at each run's first pass alone");
    for call in &asked_six {
        assert!(
            asked_one.contains(call),
            "{call:?} beyond one pass's: {asked_six:?}"
        );
    }
    assert!(asked_six.len() <= asked_one.len(), "{asked_six:?}");
}

/// forge-reads, "The host sets the interval": under `discovery = "2m"`,
/// forty passes over two minutes less three seconds ask once; the next
/// pass, two minutes after, asks again. Too short a `discovery` is
/// refused, naming it and ten seconds.
#[test]
fn the_host_sets_the_interval() {
    let g = OnGitHub::new("pr-discovery-2m", "reviews = 1");
    g.discovery("2m");
    g.by_hand("unit/slowly", &[("s.txt", Some("s\n"))], false);
    g.on_main();
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "40"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(g.lists(), 1, "{}", text(&out));
    g.discovery_passes();
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "41"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(g.lists(), 3, "asked at once, and two minutes on");

    let short = OnGitHub::new("pr-discovery-5s", "");
    short.discovery("5s");
    let out = short.sluice(&["step", "main"]);
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    assert!(
        text(&out).contains("[forge] discovery = \"5s\" is shorter than ten seconds"),
        "{}",
        text(&out)
    );
}

/// forge-reads, "A submit on the host asks at once": seconds after main's
/// last asking, `sluice submit main --no-wait` opens a pull request, and
/// its own pass asks for main's pull requests and takes it in.
#[test]
fn a_submit_on_the_host_asks_at_once() {
    let g = OnGitHub::new("pr-discovery-submit", "");
    g.step();
    let asked = g.lists();
    let w = g.session("prompt", &[("p.txt", "p\n")]);
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "main", "--no-wait"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let n = g.pull_from("unit/prompt").number;
    assert!(
        text(&out).contains(&format!("from #{n}")),
        "taken in: {}",
        text(&out)
    );
    assert_eq!(g.of(n).len(), 1, "{:?}", g.subs());
    // The open's own list and the pass's.
    assert_eq!(g.lists(), asked + 2);
}

/// forge-reads, "A landing asks at once": the pass after a round lands a
/// submission asks for main's pull requests, and once nothing moves again
/// the next asks nothing.
#[test]
fn a_landing_asks_at_once() {
    let g = OnGitHub::new("pr-discovery-landing", "");
    let (n, _) = g.by_hand("unit/lands", &[("l.txt", Some("l\n"))], false);
    let until = Instant::now() + Duration::from_secs(60);
    while g.of(n).first().is_none_or(|s| s["state"] != "landed") {
        assert!(Instant::now() < until, "never landed: {:?}", g.subs());
        let out = g.sluice(&["step", "main"]);
        assert!(out.status.success(), "{}", text(&out));
    }
    let asked = g.lists();
    let out = g.sluice(&["step", "main"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(g.lists(), asked + 1, "the landing asks");
    let out = g.sluice(&["step", "main"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(g.lists(), asked + 1, "then nothing moved");
}

/// forge-reads, "A waiting pull request that stands" and "Nothing new to
/// say": a pull request taken into the queue, asked for three times more
/// with nothing moved, has nothing beside its list entry read again, and
/// its check is neither read back nor posted again.
#[test]
fn a_queued_pull_request_that_stands_is_read_by_the_list_alone() {
    let g = OnGitHub::new("pr-discovery-stands", "reviews = 1");
    let w = g.session("stands", &[("slow", "x\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/stands");
    let n = pull.number;
    g.gh.forge
        .review(n, "reviewer", "APPROVED", &pull.head)
        .unwrap();
    let status = |g: &OnGitHub| {
        g.discovery_passes();
        let out = g
            .command(env!("CARGO_BIN_EXE_sluice"), &w)
            .args(["submit", "main", "--status"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{}", text(&out));
        text(&out)
    };
    let said = status(&g);
    assert_eq!(g.of(n).len(), 1, "taken in: {said}");
    assert!(said.starts_with("queued"), "{said}");
    let (lists, beside) = (g.lists(), g.beside_reads(n, &pull.head));
    let (read_back, posts) = (g.posted_reads(&pull.head), g.posts());
    for _ in 0..3 {
        let said = status(&g);
        assert!(said.starts_with("queued"), "{said}");
    }
    // Each finds its pull request in the list, and asks.
    assert_eq!(g.lists(), lists + 6);
    assert_eq!(
        g.beside_reads(n, &pull.head),
        beside,
        "nothing beside the list"
    );
    assert_eq!(
        g.posted_reads(&pull.head),
        read_back,
        "its check not read back"
    );
    assert_eq!(g.posts(), posts, "nor posted again");
}

/// forge-reads, "The state moves": the pull request's submission goes
/// from queued to testing, and its check is posted once, saying testing,
/// with no read of GitHub's copy first.
#[test]
fn the_state_moves_and_is_posted_unread() {
    let g = OnGitHub::new("pr-discovery-moves", "");
    let (n, head) = g.by_hand("unit/moves", &[("slow", Some("x\n"))], false);
    g.discovery_passes();
    let out = g.sluice(&["step", "main"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(g.of(n).len(), 1, "{:?}", g.subs());
    let (read_back, posts) = (g.posted_reads(&head), g.posts());
    // The round started its test: the records moved, and the next pass
    // asks and shows it.
    let out = g.sluice(&["step", "main"]);
    assert!(out.status.success(), "{}", text(&out));
    let (status, title, _) = g.posted(&head).unwrap();
    assert_eq!(status, Status::InProgress, "{title}");
    assert!(title.starts_with("testing main + #"), "{title}");
    assert_eq!(g.posts(), posts + 1);
    assert_eq!(g.posted_reads(&head), read_back);
}

/// forge-reads, "A draft": each asking reads a draft from the list alone.
#[test]
fn a_draft_is_read_from_the_list_alone() {
    let g = OnGitHub::new(
        "pr-discovery-draft",
        "reviews = 1\nchecks = [\"qa/approved\"]",
    );
    let (n, head) = g.by_hand("unit/drafty", &[("d.txt", Some("d\n"))], true);
    for _ in 0..3 {
        g.step();
    }
    assert_eq!(g.lists(), 3);
    assert_eq!(g.beside_reads(n, &head), 0);
    assert_eq!(g.posted(&head).unwrap().1, "waiting: a draft");
}

/// forge-reads, "A pull request waiting on a check": out of draft, it
/// waits for `qa/approved`, which another system passes on its head,
/// moving nothing GitHub lists; the next asking reads its checks again,
/// by the one query, and submits it. pull-request-intake: "the next
/// asking submits it".
#[test]
fn a_pull_request_waiting_on_a_check_is_read_at_each_asking() {
    let g = OnGitHub::new("pr-discovery-qa", "checks = [\"qa/approved\"]");
    let (n, head) = g.by_hand("unit/qa", &[("q.txt", Some("q\n"))], false);
    g.step();
    assert!(g.of(n).is_empty(), "{:?}", g.subs());
    let checks = |g: &OnGitHub| {
        g.gh.calls(
            "GET",
            &format!("/repos/o/r/commits/{head}/check-runs?per_page"),
        )
    };
    let (before, queries) = (checks(&g), g.gh.queries());
    let updated = g.pull_from("unit/qa").updated_at;
    g.gh.forge
        .check(&head, "qa/approved", Standing::Passed)
        .unwrap();
    assert_eq!(g.pull_from("unit/qa").updated_at, updated);
    g.step();
    assert_eq!(g.gh.queries(), queries + 1);
    assert_eq!(checks(&g), before, "nothing read alone");
    assert_eq!(g.of(n).len(), 1, "{:?}", g.subs());
}

/// forge-reads, "A new head": a pull request's head moves, and its reviews
/// and checks are read again at the next asking, against its new head, by
/// the one query.
#[test]
fn a_new_head_is_read_again() {
    let g = OnGitHub::new(
        "pr-discovery-head",
        "reviews = 1\nchecks = [\"qa/approved\"]",
    );
    let (n, old) = g.by_hand("unit/heads", &[("h.txt", Some("h\n"))], false);
    g.step();
    let reviews = g.gh.calls("GET", &format!("/repos/o/r/pulls/{n}/reviews"));
    let queries = g.gh.queries();
    let new = g.push_more("unit/heads", "h2.txt");
    g.step();
    assert_eq!(g.gh.queries(), queries + 1);
    assert_eq!(
        g.gh.calls("GET", &format!("/repos/o/r/pulls/{n}/reviews")),
        reviews
    );
    assert_eq!(g.beside_reads(n, &new), 0);
    assert_eq!(
        g.posted(&new).unwrap().1,
        "waiting: needs 1 review, qa/approved"
    );
    assert!(g.posted(&old).is_some());
}

/// 136: a waiting `sluice wait main` that follows its pull request for a
/// minute of a clock its pauses move, its twenty rounds three seconds
/// apart, nothing on the host moving, reads the request from GitHub no
/// more often than once in a quarter of the host's interval: four times,
/// at most.
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
    let out = g.skipping(&w, &["wait", "main", "--timeout", "57s"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let reads = g.pull_reads(n) - before;
    assert!((1..=4).contains(&reads), "{reads} reads: {}", text(&out));
    // Its own asking found nothing due: only the wait's finding of it.
    assert_eq!(g.lists(), lists + 1, "{}", text(&out));
}

/// Another host over `main`'s remote and GitHub: its own clone, config and
/// state, its name among its peers given (137).
struct Other<'a> {
    g: &'a OnGitHub,
    repo: Repo,
    config: Scratch,
    state: Scratch,
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

    /// Record each push of an intake mark to the remote, its new value or
    /// zeros for a removal, in `intake.log` beside it.
    fn record_marks(&self) {
        let log = self.bare.path().join("intake.log");
        let hook = self.bare.path().join("hooks/post-receive");
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\nwhile read old new ref; do\n  case \"$ref\" in\n    \
                 refs/sluice/intake/*) echo \"$new\" >> {:?};;\n  esac\ndone\n",
                log.display().to_string()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Each intake mark pushed to the remote since [`OnGitHub::record_marks`],
    /// what it says, or `null` for a removal.
    fn marks_pushed(&self) -> Vec<Value> {
        let log = std::fs::read_to_string(self.bare.path().join("intake.log")).unwrap_or_default();
        log.lines()
            .map(|oid| match oid.trim_matches('0') {
                "" => Value::Null,
                _ => self.said(oid),
            })
            .collect()
    }

    /// What the mark commit `oid` on the remote says.
    fn said(&self, oid: &str) -> Value {
        let out = support::git_command()
            .arg("--git-dir")
            .arg(self.bare.path())
            .args(["log", "-1", "--format=%B", oid])
            .output()
            .unwrap();
        serde_json::from_slice(String::from_utf8_lossy(&out.stdout).trim().as_bytes()).unwrap()
    }

    /// main's intake mark on the remote: its commit and what it says.
    fn mark(&self) -> Option<(String, Value)> {
        let oid = github::rev(self.bare.path(), "refs/sluice/intake/main")?;
        let said = self.said(&oid);
        Some((oid, said))
    }

    /// Push a mark naming `host` and `holder` to the remote, as another
    /// host would, where none stands; its commit.
    fn push_mark(&self, host: &str, holder: &str) -> String {
        let tree = self
            .repo
            .git(&["hash-object", "-t", "tree", "-w", "/dev/null"]);
        let said = format!("{{\"host\":\"{host}\",\"holder\":\"{holder}\",\"sluice\":\"0.0.0\"}}");
        let commit = self.repo.git(&["commit-tree", &tree, "-m", &said]);
        self.repo.git(&[
            "push",
            "-q",
            "origin",
            &format!("{commit}:refs/sluice/intake/main"),
        ]);
        commit
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

    /// A worktree on the remote's branch `name`, as its session would have.
    fn worktree_of(&self, name: &str) -> PathBuf {
        let dir = self
            .repo
            .dir()
            .with_extension(format!("s-{}", name.replace('/', "-")));
        let _ = std::fs::remove_dir_all(&dir);
        self.repo.git(&[
            "fetch",
            "-q",
            "origin",
            &format!("{name}:refs/remotes/origin/{name}"),
        ]);
        self.repo.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            name,
            dir.to_str().unwrap(),
            &format!("origin/{name}"),
        ]);
        dir
    }
}

/// request-intake-host, "Records on each host": host A's `sluice run`
/// holds main's intake mark, and host B's `sluice run` asks GitHub nothing
/// about main's open pull requests and takes nothing in.
#[test]
fn records_on_each_host_one_host_asks() {
    let g = OnGitHub::new("intake-each", "");
    g.named("a", "");
    let (n, _) = g.by_hand("unit/each", &[("e.txt", Some("e\n"))], false);
    g.on_main();
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "1"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(g.mark().unwrap().1["host"], "a");
    let b = g.other("b", "", "");
    let lists = g.lists();
    let said = b.run("5");
    assert_eq!(g.lists(), lists, "{said}");
    assert!(b.of(n).is_empty(), "{said}");
}

/// request-intake-host, "Records on the remote": another host keeps main,
/// its lease not passed by this host's observation, and a `sluice wait` on
/// this host asks GitHub nothing about main's open pull requests and runs
/// no round of main.
#[test]
fn records_on_the_remote_only_the_lead_asks() {
    let g = OnGitHub::new("intake-signed", "");
    g.named("a", "");
    // Slow, so the submission is still under test when B waits on it.
    let (n, _) = g.by_hand("unit/signed", &[("slow", Some("s\n"))], false);
    g.step();
    let id = g.of(n)[0]["id"].as_u64().unwrap().to_string();
    let b = g.other("b", "", "");
    let lists = g.lists();
    let out = b.skipping(b.repo.dir(), &["wait", &id, "--timeout", "30s"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    assert_eq!(g.lists(), lists, "{}", text(&out));
}

/// request-intake-host, "A live driver keeps it": host A's `sluice run`
/// holds the mark, and host B's `sluice run` passing main within the lease
/// leaves it A's.
#[test]
fn a_live_driver_keeps_the_mark() {
    let g = OnGitHub::new("intake-kept", "");
    g.named("a", "");
    g.on_main();
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "1"]);
    assert!(out.status.success(), "{}", text(&out));
    let (held, said) = g.mark().unwrap();
    assert_eq!(
        said,
        serde_json::json!({"host": "a", "holder": "driver", "sluice": said["sluice"]})
    );
    let b = g.other("b", "", "");
    let out = b.run("20");
    assert_eq!(g.mark().unwrap().0, held, "{out}");
}

/// request-intake-host, "Two takers at once": waits on hosts A and B,
/// where no `sluice run` lives, each push the mark leased on its absence;
/// one host holds it at a time, and the other takes nothing in.
#[test]
fn two_takers_at_once_one_holds() {
    let g = OnGitHub::new("intake-two", "reviews = 1");
    g.named("a", "");
    g.record_marks();
    let (n, _) = g.by_hand("unit/two", &[("t.txt", Some("t\n"))], false);
    let b = g.other("b", "", "");
    let wa = g.repo.dir().with_extension("s-two");
    g.repo
        .git(&["worktree", "add", "-q", wa.to_str().unwrap(), "unit/two"]);
    let wb = b.worktree_of("unit/two");
    let wait = |c: &mut Command| {
        c.args(["wait", "main", "--timeout", "4s"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let pa = wait(&mut g.command(env!("CARGO_BIN_EXE_sluice"), &wa));
    let pb = wait(&mut b.command(env!("CARGO_BIN_EXE_sluice"), &wb));
    let (oa, ob) = (
        pa.wait_with_output().unwrap(),
        pb.wait_with_output().unwrap(),
    );
    let shown = format!("{}\n{}", text(&oa), text(&ob));
    // One host holds it at a time: a push by another host only once the
    // holder let it go. The wait that ends first drops it, and the other,
    // still waiting, may then push its own where none stands.
    let pushed = g.marks_pushed();
    assert!(pushed.iter().any(|m| !m.is_null()), "{shown}");
    let mut holder: Option<&str> = None;
    for m in &pushed {
        if m.is_null() {
            holder = None;
            continue;
        }
        let host = m["host"].as_str().unwrap();
        assert!(
            holder.is_none_or(|h| h == host),
            "one host held it at a time: {pushed:?}\n{shown}"
        );
        assert_eq!(m["holder"], "wait", "{pushed:?}");
        holder = Some(host);
    }
    // Waiting on a review, nothing is submitted; the one that held it read
    // the request, and the other asked nothing of the list but its own
    // finding of its request.
    assert!(g.of(n).is_empty() && b.of(n).is_empty());
    assert_eq!(g.mark(), None, "let go when each wait ended: {shown}");
}

/// request-intake-host, "A driver shows itself": main has no mark, and
/// host A's `sluice run` pushes one naming A and a driver, and takes main's
/// pull requests in.
#[test]
fn a_driver_shows_itself() {
    let g = OnGitHub::new("intake-driver", "");
    g.named("a", "");
    let (n, _) = g.by_hand("unit/shown", &[("s.txt", Some("s\n"))], false);
    g.on_main();
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "1"]);
    assert!(out.status.success(), "{}", text(&out));
    let (_, said) = g.mark().unwrap();
    assert_eq!(
        (&said["host"], &said["holder"]),
        (&"a".into(), &"driver".into())
    );
    assert_eq!(g.of(n).len(), 1, "{}", text(&out));
}

/// request-intake-host, "Two drivers see each other": host A's `sluice
/// run` holds the mark, and host B's, starting, pushes none and takes
/// nothing in.
#[test]
fn two_drivers_see_each_other() {
    let g = OnGitHub::new("intake-drivers", "");
    g.named("a", "");
    g.on_main();
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "1"]);
    assert!(out.status.success(), "{}", text(&out));
    g.record_marks();
    let (n, _) = g.by_hand("unit/drivers", &[("d.txt", Some("d\n"))], false);
    let b = g.other("b", "", "");
    let said = b.run("3");
    assert!(g.marks_pushed().is_empty(), "{said}");
    assert!(b.of(n).is_empty(), "{said}");
}

/// request-intake-host, "A wait shows itself": main has no mark, and a
/// wait on host B, where no `sluice run` lives, pushes the mark naming B
/// and a wait and takes main's pull requests in; "A wait lets go": once it
/// ends, the mark is gone.
#[test]
fn a_wait_shows_itself_and_lets_go() {
    let g = OnGitHub::new("intake-wait", "");
    g.named("a", "");
    g.record_marks();
    let (n, _) = g.by_hand("unit/waits", &[("w.txt", Some("w\n"))], false);
    let b = g.other("b", "", "");
    let w = b.worktree_of("unit/waits");
    let out = b.skipping(&w, &["wait", "main", "--timeout", "1m"]);
    let pushed = g.marks_pushed();
    assert!(
        matches!(pushed.first(), Some(m) if m["host"] == "b" && m["holder"] == "wait"),
        "{pushed:?}\n{}",
        text(&out)
    );
    assert!(!b.of(n).is_empty(), "{}", text(&out));
    assert_eq!(pushed.last(), Some(&Value::Null), "let go: {pushed:?}");
    assert_eq!(g.mark(), None);
}

/// request-intake-host, "Waits and steps alone keep no mark": host A
/// reaches main by a wait and steps alone; once the wait ends no mark
/// stands, and its steps take main's pull requests in and push none.
#[test]
fn waits_and_steps_alone_keep_no_mark() {
    let g = OnGitHub::new("intake-alone", "");
    g.named("a", "");
    let w = g.session("alone", &[("a.txt", "a\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let out = g.skipping(&w, &["wait", "main", "--timeout", "30s"]);
    assert!(out.status.code().is_some(), "{}", text(&out));
    assert_eq!(g.mark(), None, "{}", text(&out));
    g.record_marks();
    let (n, _) = g.by_hand("unit/stepped", &[("s.txt", Some("s\n"))], false);
    g.step();
    assert_eq!(g.of(n).len(), 1);
    assert!(g.marks_pushed().is_empty());
    assert_eq!(g.mark(), None);
}

/// request-intake-host, "A driver takes from a wait": a wait on host A
/// holds the mark, and host B's `sluice run` replaces it at once; A's next
/// pass takes nothing in.
#[test]
fn a_driver_takes_from_a_wait() {
    let g = OnGitHub::new("intake-take", "");
    g.named("a", "");
    g.push_mark("a", "wait");
    let b = g.other("b", "", "");
    b.run("1");
    let (_, said) = g.mark().unwrap();
    assert_eq!(
        (&said["host"], &said["holder"]),
        (&"b".into(), &"driver".into())
    );
    let (n, _) = g.by_hand("unit/taken", &[("t.txt", Some("t\n"))], false);
    let lists = g.lists();
    g.step();
    assert!(g.of(n).is_empty());
    assert_eq!(g.lists(), lists, "A asked GitHub nothing of main's list");
}

/// request-intake-host, "A host that stopped": the host that held the mark
/// stopped, and host B, having seen it unchanged for its lease, takes it
/// and takes main's pull requests in.
#[test]
fn a_host_that_stopped_loses_the_mark() {
    let g = OnGitHub::new("intake-lapsed", "");
    g.named("a", "");
    let stopped = g.push_mark("a", "driver");
    let (n, _) = g.by_hand("unit/lapsed", &[("l.txt", Some("l\n"))], false);
    let b = g.other("b", "lease = \"30s\"\nalive = \"10s\"", "");
    let said = b.run("3");
    assert_eq!(g.mark().unwrap().0, stopped, "within the lease: {said}");
    assert!(b.of(n).is_empty());
    let said = b.run("12");
    let (_, mark) = g.mark().unwrap();
    assert_eq!(mark["host"], "b", "{said}");
    assert!(!b.of(n).is_empty(), "{said}");
}

/// Host A, called `a`, testing `pull/<n>` with a check that goes on until
/// it is ended, when host B's `sluice run` takes main's mark and the
/// request in: the request's number, its submission's id on A, the run's
/// process, and the place and combination A's log says it is tested in.
fn handed_mid_test(g: &OnGitHub) -> (u64, u64, String, String, String) {
    let (n, id, process, place, combination, _) = handing_mid_test(g);
    (n, id, process, place, combination)
}

/// [`handed_mid_test`], with host B.
fn handing_mid_test(g: &OnGitHub) -> (u64, u64, String, String, String, Other<'_>) {
    g.named("a", "");
    let pid = g.repo.dir().join(".git/hang.pid");
    let hang = format!("{}\n", pid.display());
    let (n, _) = g.by_hand("unit/handed", &[("hang", Some(&hang))], false);
    g.step();
    let id = g.of(n)[0]["id"].as_u64().expect("taken in");
    let until = Instant::now() + Duration::from_secs(60);
    while !pid.exists() {
        assert!(Instant::now() < until, "the check never started");
        std::thread::sleep(Duration::from_millis(100));
    }
    let process = std::fs::read_to_string(&pid).unwrap().trim().to_string();
    assert!(alive(&process));
    let testing = g
        .log()
        .into_iter()
        .find(|l| l["event"] == "testing" && l["submissions"][0] == id)
        .expect("A tests it");
    let place = testing["place"].as_str().unwrap().to_string();
    let combination = testing["combination"].as_str().unwrap().to_string();
    let b = g.other("b", "", "");
    b.run("1");
    assert_eq!(g.mark().unwrap().1["host"], "b");
    assert!(!b.of(n).is_empty(), "B takes it in");
    (n, id, process, place, combination, b)
}

/// Whether the process `pid` is there.
fn alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

/// Host A's pass after B took main's mark ended A's run of `pull/<n>` and
/// released its checkout, and read nothing of the run (the
/// obsolete-combinations spec).
fn ended_by_the_hand_over(g: &OnGitHub, process: &str, place: &str, combination: &str) {
    let until = Instant::now() + Duration::from_secs(10);
    while alive(process) {
        assert!(Instant::now() < until, "A's run goes on");
        std::thread::sleep(Duration::from_millis(100));
    }
    let log = g.log();
    assert!(
        log.iter()
            .any(|l| l["event"] == "released" && l["place"] == place),
        "{log:?}"
    );
    assert!(
        !log.iter()
            .any(|l| l["event"] == "result" && l["combination"] == combination),
        "{log:?}"
    );
}

/// request-intake-host, "Handed over" and "Handed over mid-test by a
/// driver": host A has `pull/<n>` under test in its records and host B's
/// `sluice run` takes main's mark; A's next pass withdraws it naming B,
/// ends its run and releases its checkout, reading no result of it, and B
/// takes it in.
#[test]
fn handed_over() {
    let g = OnGitHub::new("intake-handed", "");
    let (n, _, process, place, combination) = handed_mid_test(&g);
    let posts = g.posts();
    g.step();
    let a = g.of(n);
    assert_eq!(a[0]["state"], "withdrawn", "{a:?}");
    assert_eq!(a[0]["withdrawn"]["by"], "b takes main's pull requests in");
    assert_eq!(a[0]["withdrawn"]["handed_over_to"], "b");
    assert_eq!(g.posts(), posts, "A posts nothing");
    ended_by_the_hand_over(&g, &process, &place, &combination);
}

/// request-intake-host, "Handed over mid-test by a wait": host A, with no
/// driver, is testing `pull/<n>` when B takes main's mark; a wait on A
/// withdraws it, ends its run and releases its checkout, and runs no round
/// for `main`, which would read main moved since A last read it. The
/// request carries the submission's work on B, so the wait follows the
/// request and is never told it was withdrawn (122, 137). Out of time, it
/// prints its own submission's record, naming the request it follows as it
/// stands (record 0100 D2, D3). Its time is up as it starts, as on a host
/// so busy that its start takes its whole limit: its first pass is the one
/// that withdraws (1).
#[test]
fn handed_over_by_a_wait() {
    let g = OnGitHub::new("intake-handed-wait", "");
    let (n, id, process, place, combination) = handed_mid_test(&g);
    let moved = g
        .repo
        .branch("elsewhere", "main", &[("e.txt", Some("e\n"))]);
    g.repo
        .git(&["push", "-q", "origin", &format!("{moved}:refs/heads/main")]);
    let posts = g.posts();
    let out = g.sluice(&["wait", &id.to_string(), "--timeout", "0s", "--json"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["id"], id, "{doc}");
    assert_eq!(
        doc["carried_by"],
        serde_json::json!({ "pull_request": n, "state": "open" }),
        "{doc}"
    );
    let a = g.of(n);
    assert_eq!(a[0]["withdrawn"]["by"], "b takes main's pull requests in");
    assert_eq!(a[0]["withdrawn"]["handed_over_to"], "b");
    assert_eq!(g.posts(), posts, "A posts nothing");
    ended_by_the_hand_over(&g, &process, &place, &combination);
    let log = g.log();
    assert!(!log.iter().any(|l| l["event"] == "moved"), "{log:?}");
}

/// request-intake-host, "A landing is not handed over": host A's
/// `pull/<n>` submission is landing when B takes the mark; A's landing
/// finishes, and nothing of it is withdrawn.
#[test]
fn a_landing_is_not_handed_over() {
    let g = OnGitHub::new("intake-landing", "");
    g.named("a", "");
    // The remote refuses main's pushes while `refuse` exists beside it, so
    // A's landing is left for its next step to settle (122).
    let refuse = g.bare.path().join("refuse");
    let hook = g.bare.path().join("hooks/pre-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nwhile read old new ref; do\n  \
             if [ \"$ref\" = refs/heads/main ] && [ -e {:?} ]; then exit 1; fi\ndone\n",
            refuse.display().to_string()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (n, _) = g.by_hand("unit/landing", &[("l.txt", Some("l\n"))], false);
    std::fs::write(&refuse, "").unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    let step = || {
        g.discovery_passes();
        g.sluice(&["step", "main"])
    };
    while !g.of(n).first().is_some_and(|s| s["state"] == "landing") {
        assert!(Instant::now() < until, "never landing: {:?}", g.of(n));
        step();
        std::thread::sleep(Duration::from_millis(200));
    }
    let b = g.other("b", "", "");
    b.run("1");
    assert_eq!(g.mark().unwrap().1["host"], "b");
    std::fs::remove_file(&refuse).unwrap();
    while !g.of(n).first().is_some_and(|s| s["state"] == "landed") {
        assert!(Instant::now() < until, "never landed: {:?}", g.of(n));
        step();
        std::thread::sleep(Duration::from_millis(200));
    }
    let a = g.of(n);
    assert_eq!(a[0]["state"], "landed", "{a:?}");
}

/// request-intake-host, "A worktree's submission stays": host A, without
/// the mark, has a submission to `bolt/x` from one of its worktrees, and
/// its rounds advance it as before.
#[test]
fn a_worktrees_submission_stays() {
    let g = OnGitHub::with_bolt("intake-bolt");
    g.named("a", "");
    let b = g.other("b", "", "");
    b.run("1");
    assert_eq!(g.mark().unwrap().1["host"], "b");
    let w = g.session("bolted", &[("b.txt", "b\n")]);
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "bolt/x", "--no-wait", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    g.step();
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
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// request-intake-host, "Read at its pace": a wait on host A follows its
/// pull request, which host B takes in, for a minute with `discovery` at
/// sixty seconds; it reads the request and its check from GitHub no more
/// than four times each, and runs no intake and no round.
#[test]
fn a_wait_across_hosts_reads_at_its_pace() {
    let g = OnGitHub::new("intake-pace", "reviews = 1");
    g.named("a", "");
    let b = g.other("b", "", "");
    b.run("1");
    let w = g.session("paced", &[("p.txt", "p\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/paced");
    let (before, checks, lists) = (
        g.pull_reads(pull.number),
        g.posted_reads(&pull.head),
        g.lists(),
    );
    let out = g.skipping(&w, &["wait", "main", "--timeout", "57s"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let reads = g.pull_reads(pull.number) - before;
    assert!((1..=4).contains(&reads), "{reads} reads: {}", text(&out));
    assert!(g.posted_reads(&pull.head) - checks <= 4, "{}", text(&out));
    assert_eq!(g.lists(), lists + 1, "its finding of its request alone");
    assert!(g.of(pull.number).is_empty(), "no intake, no round");
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

    /// The pull requests into `main` this host follows, by number, as its
    /// forge state keeps them (137); `null` where it keeps none.
    fn follows(&self) -> Value {
        let path = self.state.path().join("sluice/forge/follows/o/r/main.json");
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .map_or(Value::Null, |v| v["requests"].clone())
    }
}

/// request-intake-host, "Submitted here, taken in there": host B keeps
/// main, and `wt submit main --no-wait` on host A
/// opens a pull request; A's follows name it at its head, and A's next
/// round asks GitHub nothing of main's open pull requests.
#[test]
fn submitted_here_taken_in_there() {
    let g = OnGitHub::new("follows-submitted", "");
    g.named("a", "");
    let b = g.other("b", "", "");
    b.run("1");
    assert_eq!(g.lead().as_deref(), Some("b"));
    let w = g.session("there", &[("t.txt", "t\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let lists = g.lists();
    let pull = g.pull_from("unit/there");
    let follows = g.follows();
    let row = &follows[pull.number.to_string()];
    assert_eq!(row["head"], pull.head.as_str(), "{follows}");
    assert_eq!(
        (&row["from"], &row["open"], &row["merged"]),
        (&"unit/there".into(), &true.into(), &false.into()),
        "{follows}"
    );
    g.step();
    assert_eq!(g.lists(), lists, "A's round asks nothing of the list");
    assert!(g.of(pull.number).is_empty(), "A took nothing in");
    assert!(g.follows()[pull.number.to_string()].is_object());
}

/// request-intake-host, "Handed over": host A has `pull/<n>` in its queue
/// and host B's `sluice run` takes main's mark; A's next pass withdraws its
/// submission, and A's follows name the request at the head it withdrew.
#[test]
fn handed_over_is_followed() {
    let g = OnGitHub::new("follows-handed", "");
    let (n, _, process, place, combination) = handed_mid_test(&g);
    assert!(g.follows()[n.to_string()].is_null(), "taken in here");
    g.step();
    let a = g.of(n);
    assert_eq!(a[0]["state"], "withdrawn", "{a:?}");
    let follows = g.follows();
    assert_eq!(follows[n.to_string()]["head"], a[0]["head"], "{follows}");
    ended_by_the_hand_over(&g, &process, &place, &combination);
}

/// request-intake-host, "Merged": host A follows the pull request its
/// waiting submit opened, host B lands it, and A's submit reads it merged:
/// it says it landed, and A's follows name it no longer.
#[test]
fn a_merged_request_leaves_the_follows() {
    let g = OnGitHub::new("follows-merged", "");
    g.discovery("10s");
    g.named("a", "");
    let b = g.other("b", "", "");
    b.run("1");
    let w = g.session("merged", &[("m.txt", "m\n")]);
    let mut submit = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "main"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    let followed = |g: &OnGitHub| {
        let n = g.gh.forge.by_ref("unit/merged").unwrap()?.number;
        g.follows()[n.to_string()].is_object().then_some(n)
    };
    let n = loop {
        if let Some(n) = followed(&g) {
            break n;
        }
        if Instant::now() >= until || submit.try_wait().unwrap().is_some() {
            let _ = submit.kill();
            panic!(
                "never followed: {}\n{}",
                g.follows(),
                text(&submit.wait_with_output().unwrap())
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    while !b.of(n).first().is_some_and(|s| s["state"] == "landed") {
        assert!(Instant::now() < until, "B never landed it: {:?}", b.of(n));
        b.discovery_passes();
        b.run("2");
    }
    while submit.try_wait().unwrap().is_none() {
        if Instant::now() >= until {
            let _ = submit.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let out = submit.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(text(&out).contains("landed"), "{}", text(&out));
    assert!(g.follows().get(n.to_string()).is_none(), "{}", g.follows());
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

/// request-intake-host, "Landed on another host": a waiting `sluice submit
/// main` on host A opened its pull request, host B takes main's pull
/// requests in and lands it, and A's submit exits 0, reporting it landed.
#[test]
fn landed_on_another_host() {
    let g = OnGitHub::new("intake-landed", "");
    g.discovery("10s");
    g.named("a", "");
    let b = g.other("b", "", "");
    b.run("1");
    let w = g.session("landed", &[("l.txt", "l\n")]);
    let mut submit = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "main"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    while g.gh.forge.by_ref("unit/landed").unwrap().is_none() {
        if Instant::now() >= until || submit.try_wait().unwrap().is_some() {
            let _ = submit.kill();
            panic!(
                "never opened: {}",
                text(&submit.wait_with_output().unwrap())
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let n = g.pull_from("unit/landed").number;
    while !b.of(n).first().is_some_and(|s| s["state"] == "landed") {
        assert!(Instant::now() < until, "B never landed it: {:?}", b.of(n));
        b.discovery_passes();
        b.run("2");
    }
    while submit.try_wait().unwrap().is_none() {
        if Instant::now() >= until {
            let _ = submit.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let out = submit.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    // B's landing marks the request merged before B's round posts its
    // check, so A reads `landed as …`, the check's title, or `landed`
    // where it read the request between the two (this change's sluice
    // exec main, cli:tools, 2026-11-05): either reports it landed.
    assert!(text(&out).contains("passed · landed"), "{}", text(&out));
    assert!(g.of(n).is_empty(), "A took nothing in");
}

/// request-intake-host, "Blamed on another host": B's round blames the
/// pull request, sluice's check on its head concludes as a failure naming
/// the blamed check, and A's wait exits as a failed submission does,
/// naming what the check names.
#[test]
fn blamed_on_another_host() {
    let g = OnGitHub::new("intake-blamed", "");
    g.named("a", "");
    let b = g.other("b", "", "");
    b.run("1");
    let w = g.session("blamed", &[("broken", "b\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/blamed");
    let until = Instant::now() + Duration::from_secs(60);
    while !b
        .of(pull.number)
        .first()
        .is_some_and(|s| s["state"] == "failed")
    {
        assert!(
            Instant::now() < until,
            "B never blamed it: {:?}",
            b.of(pull.number)
        );
        b.discovery_passes();
        b.run("2");
    }
    let out = g.skipping(&w, &["wait", "main", "--timeout", "1m"]);
    assert_eq!(out.status.code(), Some(sluice::FAILED), "{}", text(&out));
    let said = g.posted(&pull.head).unwrap().1;
    assert!(said.starts_with("failed: "), "{said}");
    assert!(text(&out).contains(&said), "{}", text(&out));
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

/// request-intake-host, "GitHub keeps a run completed": host B's `sluice
/// run` holds main's mark, and GitHub shows sluice's run on the head of
/// host A's pull request completed as a failure from an earlier blame,
/// while its block now reads it taken in and under test. A's wait reads
/// it under test and does not end (D6).
#[test]
fn a_run_github_keeps_completed_reads_by_its_block() {
    let g = OnGitHub::new("fields-completed", "");
    g.named("a", "");
    let b = g.other("b", "", "");
    b.run("1");
    let w = g.session("kept", &[("k.txt", "k\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/kept");
    let n = pull.number;
    let block = serde_json::json!({
        "host": "b", "taken_in": true, "submission": 1406, "state": "testing",
        "ahead": [], "waits_for": [],
    });
    let testing = format!("testing main + #{n}");
    g.posted_elsewhere(
        n,
        &pull.head,
        Status::Failure,
        &testing,
        &format!(
            "Submission 1406 on b.\n\nUnder test in main + #{n}.\n\n\
             <!-- sluice:queue {block} -->"
        ),
    );
    let out = g.skipping(&w, &["wait", "main", "--timeout", "3s"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    assert!(text(&out).contains(&testing), "{}", text(&out));
    assert!(g.follows()[n.to_string()].is_object(), "{}", g.follows());
    assert!(g.of(n).is_empty(), "A took nothing in");
}

/// request-intake-host, "An older sluice's check": host B's older sluice
/// posted the check on the head of host A's pull request with no block,
/// titled `failed: app:test`, and A's wait reads it sent back by its
/// words, as it did before checks carried their fields (D6).
#[test]
fn an_older_sluices_check_reads_by_its_words() {
    let g = OnGitHub::new("fields-older", "");
    g.named("a", "");
    let b = g.other("b", "", "");
    b.run("1");
    let w = g.session("older", &[("o.txt", "o\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/older");
    g.posted_elsewhere(
        pull.number,
        &pull.head,
        Status::Failure,
        "failed: app:test",
        "app:test failed. Push a fix and submit again.",
    );
    let out = g.skipping(&w, &["wait", "main", "--timeout", "1m"]);
    assert_eq!(out.status.code(), Some(sluice::FAILED), "{}", text(&out));
    assert!(text(&out).contains("failed: app:test"), "{}", text(&out));
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

/// The last document a `--json` command printed.
fn last_document(out: &Output) -> Value {
    let said = String::from_utf8_lossy(&out.stdout);
    let last = said.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("");
    serde_json::from_str(last).unwrap_or_else(|e| panic!("{e}: {}", text(out)))
}

/// submission-wait, "Out of time, queued on the other host": host B's
/// `sluice run` holds main's mark and holds host A's pull request as
/// submission 1406, and A's `sluice wait main --timeout 2s --json` in its
/// worktree exits 5, naming the request, the mark's host and holder, and
/// the queue B's check carries.
#[test]
fn out_of_time_queued_on_the_other_host() {
    let g = OnGitHub::new("wait-there-timeout", "");
    let (w, pull) = g.opened_here_held_by_b("timeout");
    g.held_there(pull.number, &pull.head, 1406);
    let out = g.skipping(&w, &["wait", "main", "--timeout", "2s", "--json"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let doc = last_document(&out);
    assert_eq!(doc["pull_request"], pull.number, "{doc}");
    assert_eq!(
        doc["taken_in_by"],
        serde_json::json!({ "host": "b" }),
        "{doc}"
    );
    assert_eq!(doc["queue"]["submission"], 1406, "{doc}");
    assert_eq!(doc["queue"]["host"], "b", "{doc}");
    assert!(g.of(pull.number).is_empty(), "A took nothing in");
}

/// submission-wait, "The status a submit reads": the same, and `sluice
/// submit main --status --json` in that worktree gives the request's head,
/// its draft mark, the mark's host and the check's queue beside the
/// request, its branch and its state.
#[test]
fn the_status_a_submit_reads_across_hosts() {
    let g = OnGitHub::new("wait-there-status", "");
    let (w, pull) = g.opened_here_held_by_b("status");
    g.held_there(pull.number, &pull.head, 1406);
    let out = g.skipping(&w, &["submit", "main", "--status", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc = last_document(&out);
    assert_eq!(
        (&doc["pull_request"], &doc["branch"], &doc["state"]),
        (
            &pull.number.into(),
            &"main".into(),
            &"queued: next on main".into()
        ),
        "{doc}"
    );
    assert_eq!(
        (&doc["head"], &doc["draft"]),
        (&pull.head.as_str().into(), &false.into()),
        "{doc}"
    );
    assert_eq!(
        doc["taken_in_by"],
        serde_json::json!({ "host": "b" }),
        "{doc}"
    );
    assert_eq!(
        doc["queue"],
        serde_json::json!({
            "host": "b", "taken_in": true, "submission": 1406, "state": "queued",
            "ahead": [], "waits_for": [],
        }),
        "{doc}"
    );
}

/// submission-wait, "Its words": `sluice wait main` on host A follows its
/// pull request while host B holds it as submission 1406, with `discovery`
/// at ten seconds, so each pass reads it again. It says once that the
/// request is submission 1406 on `b`, taken in by `b`, and
/// not again over the reads that find the same; once B holds it as 1407,
/// it says so, once. The check's own first line, which says the same, is
/// not said beside it.
#[test]
fn a_waits_words_across_hosts() {
    let g = OnGitHub::new("wait-there-words", "");
    g.discovery("10s");
    let (w, pull) = g.opened_here_held_by_b("words");
    let n = pull.number;
    g.held_there(n, &pull.head, 1406);
    let said = Scratch::new("wait-there-words-said");
    let path = said.path().join("out");
    let mut wait = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["wait", "main", "--timeout", "1m"])
        .stdout(std::fs::File::create(&path).unwrap())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let shown = || std::fs::read_to_string(&path).unwrap_or_default();
    let until = Instant::now() + Duration::from_secs(60);
    let mut said_once = |line: &str| {
        while !shown().contains(line) {
            if Instant::now() >= until || wait.try_wait().unwrap().is_some() {
                let _ = wait.kill();
                panic!("never said {line:?}: {}", shown());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    };
    let first = format!("#{n} is submission 1406 on b, taken in by b");
    said_once(&first);
    // Two passes more, each reading the request and its check again.
    std::thread::sleep(Duration::from_secs(7));
    let run = g.gh.forge.posted(&github::repo(), &pull.head).unwrap();
    let run = run.map(|r| r.id);
    g.gh.forge
        .post_named(
            &github::repo(),
            &pull.head,
            "sluice",
            run,
            &queued_on_b(1407),
        )
        .unwrap();
    let next = format!("#{n} is submission 1407 on b, taken in by b");
    said_once(&next);
    let _ = wait.kill();
    let _ = wait.wait();
    let shown = shown();
    assert_eq!(shown.matches(&first).count(), 1, "{shown}");
    assert_eq!(shown.matches(&next).count(), 1, "{shown}");
    assert!(!shown.contains("Submission 1406 on b."), "{shown}");
    assert!(shown.contains("queued: next on main"), "{shown}");
}

/// submission-wait, "Not yet taken in": host B keeps main, and B's check on
/// the head of host A's pull request says it needs one review; A's `sluice
/// wait main --timeout 2s --json` exits 5, its queue not taken in and
/// waiting for the review, taken in by B. Without `--json`, it says once
/// that the request waits to be taken in by B.
#[test]
fn not_yet_taken_in_across_hosts() {
    let g = OnGitHub::new("wait-there-untaken", "reviews = 1");
    g.named("a", "");
    g.other("b", "", "").run("1");
    let w = g.session("untaken", &[("u.txt", "u\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/untaken");
    let block = serde_json::json!({
        "host": "b", "taken_in": false, "submission": null, "state": null,
        "ahead": [], "waits_for": ["1 review"],
    });
    g.posted_elsewhere(
        pull.number,
        &pull.head,
        Status::Queued,
        "waiting: needs 1 review",
        &format!(
            "Waiting to be taken in on b.\n\n\
             Needs 1 review on this commit before sluice takes it.\n\n\
             <!-- sluice:queue {block} -->"
        ),
    );
    let out = g.skipping(&w, &["wait", "main", "--timeout", "2s", "--json"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let doc = last_document(&out);
    assert_eq!(doc["pull_request"], pull.number, "{doc}");
    assert_eq!(doc["queue"]["taken_in"], false, "{doc}");
    assert_eq!(
        doc["queue"]["waits_for"],
        serde_json::json!(["1 review"]),
        "{doc}"
    );
    assert_eq!(
        doc["taken_in_by"],
        serde_json::json!({ "host": "b" }),
        "{doc}"
    );
    let out = g.skipping(&w, &["wait", "main", "--timeout", "2s"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let line = format!("#{} waits to be taken in by b", pull.number);
    assert_eq!(text(&out).matches(&line).count(), 1, "{}", text(&out));
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

/// request-intake-host, "Queued on the other host": host B's `sluice run`
/// holds main's mark and holds host A's pull request as submission 1406
/// behind `#10`'s 1405. `sluice status main --json` on A gives the mark's
/// host and holder, and a row for the request whose queue names 1406,
/// queued, behind 1405 and `#10`; its words name B's run and the
/// submission there.
#[test]
fn queued_on_the_other_host() {
    let g = OnGitHub::new("status-there-queued", "");
    let (_w, pull) = g.opened_here_held_by_b("queued");
    let n = pull.number;
    let block = serde_json::json!({
        "host": "b", "taken_in": true, "submission": 1406, "state": "queued",
        "ahead": [{ "submission": 1405, "pull_request": 10 }], "waits_for": [],
    });
    g.posted_elsewhere(
        n,
        &pull.head,
        Status::Queued,
        "queued behind #10",
        &format!(
            "Submission 1406 on b.\n\n\
             In main's queue; it lands once its combination passes.\n\n\
             <!-- sluice:queue {block} -->"
        ),
    );
    let doc = g.status_document();
    assert_eq!(
        doc["taken_in_by"],
        serde_json::json!({ "host": "b" }),
        "{doc}"
    );
    let row = row_of(&doc, n);
    assert_eq!(row["queue"], block, "{doc}");
    assert_eq!(
        (&row["title"], &row["from"], &row["head"]),
        (
            &pull.title.as_str().into(),
            &"unit/queued".into(),
            &pull.head.as_str().into()
        ),
        "{doc}"
    );
    assert_eq!(
        (&row["draft"], &row["state"]),
        (&false.into(), &"queued behind #10".into()),
        "{doc}"
    );
    assert!(row["read_at"].is_string(), "{doc}");
    let said = g.status_words();
    assert!(said.contains("pull requests b takes in:"), "{said}");
    let line = format!(
        "#{n} {} · submission 1406 there · queued behind #10",
        pull.title
    );
    assert!(said.contains(&line), "{said}");
    assert!(g.of(n).is_empty(), "A took nothing in");
}

/// request-intake-host, "Waiting for a review there": host B keeps main,
/// and B's check on host A's pull request says it needs one review.
/// `sluice status main` on A names B, and says the request waits to be
/// taken in, needing 1 review.
#[test]
fn waiting_for_a_review_there() {
    let g = OnGitHub::new("status-there-review", "reviews = 1");
    g.named("a", "");
    g.other("b", "", "").run("1");
    let w = g.session("review", &[("r.txt", "r\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/review");
    let n = pull.number;
    let block = serde_json::json!({
        "host": "b", "taken_in": false, "submission": null, "state": null,
        "ahead": [], "waits_for": ["1 review"],
    });
    g.posted_elsewhere(
        n,
        &pull.head,
        Status::Queued,
        "waiting: needs 1 review",
        &format!(
            "Waiting to be taken in on b.\n\n\
             Needs 1 review on this commit before sluice takes it.\n\n\
             <!-- sluice:queue {block} -->"
        ),
    );
    let said = g.status_words();
    assert!(said.contains("pull requests b takes in:"), "{said}");
    let line = format!(
        "#{n} {} · waiting to be taken in by b: needs 1 review",
        pull.title
    );
    assert!(said.contains(&line), "{said}");
    let doc = g.status_document();
    assert_eq!(
        doc["taken_in_by"],
        serde_json::json!({ "host": "b" }),
        "{doc}"
    );
    let row = row_of(&doc, n);
    assert_eq!(row["queue"], block, "{doc}");
    assert_eq!(row["state"], "waiting: needs 1 review", "{doc}");
}

/// request-intake-host, "No check on its head yet": host B's `sluice run`
/// holds main's mark, and host A's submit has just pushed a new head to
/// its pull request, on which no `sluice` check stands. `sluice status
/// main --json` on A gives the request at its new head with a `queue` of
/// `null`, and its words say it waits to be taken in by B.
#[test]
fn no_check_on_its_head_yet() {
    let g = OnGitHub::new("status-there-new-head", "");
    let (w, pull) = g.opened_here_held_by_b("fresh");
    let n = pull.number;
    g.held_there(n, &pull.head, 1406);
    g.commit_in(&w, &[("more.txt", "more\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let head = g.pull_from("unit/fresh").head;
    assert_ne!(head, pull.head, "the submit pushed a new head");
    let doc = g.status_document();
    let row = row_of(&doc, n);
    assert_eq!(row["head"], head.as_str(), "{doc}");
    assert_eq!(row["queue"], Value::Null, "{doc}");
    assert_eq!(row["state"], "waiting", "{doc}");
    let said = g.status_words();
    let line = format!("#{n} {} · waiting to be taken in by b", pull.title);
    assert!(said.contains(&line), "{said}");
}

/// request-intake-host, "The keeper and the check name different hosts":
/// host C keeps main, while sluice's check on host A's pull request,
/// posted by host B, still names submission 1406 there. `sluice status
/// main --json` on A gives `taken_in_by` naming C and the request's queue
/// naming B and 1406, and its words say both (design D25).
#[test]
fn the_keeper_and_the_check_name_different_hosts() {
    let g = OnGitHub::new("status-there-two-hosts", "");
    g.named("a", "");
    g.other("c", "", "").run("1");
    let w = g.session("moved", &[("m.txt", "m\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/moved");
    let n = pull.number;
    g.held_there(n, &pull.head, 1406);
    let doc = g.status_document();
    assert_eq!(
        doc["taken_in_by"],
        serde_json::json!({ "host": "c" }),
        "{doc}"
    );
    let row = row_of(&doc, n);
    assert_eq!(
        (&row["queue"]["host"], &row["queue"]["submission"]),
        (&"b".into(), &1406.into()),
        "{doc}"
    );
    let said = g.status_words();
    assert!(said.contains("pull requests c takes in:"), "{said}");
    let line = format!(
        "#{n} {} · submission 1406 on b · queued: next on main",
        pull.title
    );
    assert!(said.contains(&line), "{said}");
}

/// request-intake-host, "Merged", read by status: host A follows its pull
/// request, which host B holds, and GitHub marks it merged. A's `sluice
/// status main` says it landed, once: A's follows name it no longer, and
/// the next status shows no row for it.
#[test]
fn a_status_says_a_merged_request_landed_once() {
    let g = OnGitHub::new("status-there-merged", "");
    let (_w, pull) = g.opened_here_held_by_b("merged");
    let n = pull.number;
    g.held_there(n, &pull.head, 1406);
    g.gh.forge.merge(n).unwrap();
    g.follow_due(n);
    let said = g.status_words();
    assert!(
        said.contains(&format!("#{n} {} · landed", pull.title)),
        "{said}"
    );
    assert!(g.follows()[n.to_string()].is_null(), "{}", g.follows());
    let said = g.status_words();
    assert!(!said.contains(&format!("#{n} ")), "{said}");
    assert!(g.status_document()["pull_requests"].is_null());
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
/// of it moves; and A reads neither the request nor its check from GitHub
/// meanwhile, though the kept row is due to be read again (core.31).
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
    assert_eq!(g.pull_reads(n), pulls, "{}", text(&out));
    assert_eq!(g.posted_reads(&pull.head), checks, "{}", text(&out));
    assert!(g.of(n).is_empty(), "A took nothing in");
}

/// Host B takes main's pull requests in and has blamed the pull request a
/// `wt submit` on host A opened from a session's worktree with `changes`,
/// a `broken` among them, and marked it a draft, its check saying
/// `waiting: the next submit` while the double keeps that check completed
/// and failed, as GitHub kept switchboard-kit #268's (record 0096). Host
/// B, the worktree, and the pull request as it was opened; a second later
/// than B's leaving, so a ready mark made next reads as since it (120).
fn held_on_b<'g>(
    g: &'g OnGitHub,
    name: &str,
    changes: &[(&str, &str)],
) -> (Other<'g>, PathBuf, sluice::profiles::github::forge::Pull) {
    g.named("a", "");
    let b = g.other("b", "", "");
    b.run("1");
    let w = g.session(name, changes);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from(&format!("unit/{name}"));
    let until = Instant::now() + Duration::from_secs(60);
    let held = || {
        g.pull_from(&format!("unit/{name}")).draft
            && g.posted(&pull.head)
                .is_some_and(|p| p.1 == "waiting: the next submit")
    };
    // A pass at a time: one run of a round moves no clock on, so B records
    // the submission's leaving at the time the double stamps marks by.
    while !held() {
        assert!(
            Instant::now() < until,
            "B never held it: {:?}, {:?}",
            b.of(pull.number),
            g.posted(&pull.head)
        );
        b.discovery_passes();
        b.run("1");
    }
    assert_eq!(b.of(pull.number)[0]["state"], "failed");
    assert_eq!(
        g.posted(&pull.head).unwrap().0,
        Status::Failure,
        "the double keeps the check failed"
    );
    // The ready mark later than the leaving, to the second GitHub and the
    // records keep, as B reads a ready mark since (120).
    std::thread::sleep(Duration::from_millis(1100));
    // A quarter of `discovery` has passed since A's submit last read the
    // request, as B's blame takes on a host, so A's next follow reads what
    // B's check says now (136; record 0095 D4).
    g.follow_due(pull.number);
    (b, w, pull)
}

/// [`held_on_b`], then a `sluice submit main --no-wait --json` on A has
/// marked the pull request ready for review at the same head. Host B, the
/// worktree, the pull request as it was opened, and the submit's document.
fn held_on_b_then_marked_ready<'g>(
    g: &'g OnGitHub,
    name: &str,
    changes: &[(&str, &str)],
) -> (
    Other<'g>,
    PathBuf,
    sluice::profiles::github::forge::Pull,
    Value,
) {
    let (b, w, pull) = held_on_b(g, name, changes);
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "main", "--no-wait", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    let ready = g.pull_from(&format!("unit/{name}"));
    assert!(!ready.draft && ready.head == pull.head, "{ready:?}");
    (b, w, pull, doc)
}

/// request-intake-host, "Held, then marked ready": B blamed `#n` and
/// marked it a draft, its check saying `waiting: the next submit`, and a
/// submit on A marked it ready at the same head; before B's next pass, a
/// wait on A reads it waiting to be taken in by B, not sent back, though
/// GitHub keeps the check failed (120, 137, C.5; record 0096).
#[test]
fn held_then_marked_ready_waits_to_be_taken_in() {
    let g = OnGitHub::new("intake-held-ready", "");
    let (b, w, pull, doc) = held_on_b_then_marked_ready(&g, "held", &[("broken", "b\n")]);
    assert_eq!(doc["pull_request"], pull.number, "{doc}");
    assert_eq!(doc["state"], "waiting to be taken in by b", "{doc}");
    let out = g.skipping(&w, &["wait", "main", "--timeout", "5s", "--json"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    assert_eq!(doc["pull_request"], pull.number, "{doc}");
    assert_eq!(doc["state"], "waiting to be taken in by b", "{doc}");
    assert_eq!(b.of(pull.number).len(), 1, "B has not taken it in again");
    assert!(g.of(pull.number).is_empty(), "A took nothing in");
}

/// request-intake-host, "Taken in again while GitHub keeps the check
/// failed": after a submit on A marked `#n` ready, main's check is mended,
/// and B takes `#n` in again at the same head and lands it, its check
/// saying `queued: …`, then `testing …`, while the double keeps that check
/// completed and failed; a wait on A says each as it changes and exits 0
/// at the landing (137; record 0096).
#[test]
fn taken_in_again_while_github_keeps_the_check_failed() {
    let g = OnGitHub::new("intake-taken-again", "");
    g.discovery("10s");
    let (b, w, pull, _) = held_on_b_then_marked_ready(&g, "again", &[("broken", "b\n")]);
    let n = pull.number;
    // main's check mended: `broken` no longer fails it, and it takes five
    // seconds, so A's wait reads it under test.
    let mended = g
        .repo
        .branch("mended", "main", &[("check.sh", Some("sleep 5\n"))]);
    g.repo
        .git(&["push", "-q", "origin", &format!("{mended}:refs/heads/main")]);
    let said = g.repo.dir().with_extension("wait.out");
    let mut wait = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["wait", "main", "--timeout", "2m"])
        .stdout(std::fs::File::create(&said).unwrap())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let words = || std::fs::read_to_string(&said).unwrap_or_default();
    let until = Instant::now() + Duration::from_secs(90);
    // B passes again once A's wait has said what B's check says now, or
    // where B's last pass changed nothing, so the wait reads each state B
    // posts; the check held for the next submit, A says waiting for B.
    let shown = |title: &str| match title {
        "waiting: the next submit" => "waiting to be taken in by b".to_string(),
        t => t.to_string(),
    };
    let mut last = String::new();
    while wait.try_wait().unwrap().is_none() {
        if Instant::now() >= until {
            let _ = wait.kill();
            panic!("A's wait never ended: {}", words());
        }
        let (status, now, _) = g.posted(&pull.head).unwrap();
        if !now.starts_with("landed") {
            assert_eq!(status, Status::Failure, "the double keeps the check failed");
        }
        if now == last || words().contains(&shown(&now)) {
            b.discovery_passes();
            b.run("1");
            last = now;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let out = wait.wait_with_output().unwrap();
    let said = words();
    assert_eq!(
        out.status.code(),
        Some(sluice::LANDED),
        "{said}{}",
        text(&out)
    );
    let at = |what: &str| {
        said.find(what)
            .unwrap_or_else(|| panic!("never said {what:?}: {said}"))
    };
    assert!(
        at("waiting to be taken in by b") < at("queued: next on main"),
        "{said}"
    );
    assert!(
        at("queued: next on main") < at("testing main + #"),
        "{said}"
    );
    assert!(at("testing main + #") < at("passed · landed"), "{said}");
    assert!(!said.contains("failed: "), "{said}");
    let subs = b.of(n);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], pull.head.as_str());
    assert_eq!(subs[1]["state"], "landed", "{subs:?}");
    assert!(g.of(n).is_empty(), "A took nothing in");
}

/// pull-request-submit, "Sent back on another host": B blamed `#n` and
/// marked it a draft; `wt submit main --status --json` on A exits 0 and
/// prints it sent back, with sluice's check's title and summary on its
/// head, its head, its draft mark and `sends`, as `--status` answers where
/// the host takes the request in; in words, the check's title (124, 137;
/// record 0096).
#[test]
fn status_of_a_request_another_host_sent_back() {
    let g = OnGitHub::new("intake-status-sent-back", "");
    let (_b, w, pull) = held_on_b(&g, "statused", &[("broken", "b\n")]);
    let out = g.wt_submit(&w, &["--status", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    let (_, title, summary) = g.posted(&pull.head).unwrap();
    assert_eq!(title, "waiting: the next submit");
    // The check's summary as a person reads it, its queue block left out
    // (record 0095 D1).
    let summary = summary
        .lines()
        .filter(|l| !l.trim_start().starts_with("<!-- sluice:queue "))
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string();
    assert_eq!(
        doc,
        serde_json::json!({
            "pull_request": pull.number, "branch": "main", "state": "failed",
            "title": title, "summary": summary,
            "head": pull.head, "draft": true, "sends": pull.head,
        })
    );
    let out = g.wt_submit(&w, &["--status"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(
        String::from_utf8_lossy(&out.stdout).starts_with(&title),
        "{}",
        text(&out)
    );
    assert!(g.of(pull.number).is_empty(), "A took nothing in");
}

/// worktrunk-submit, "Sent back as a draft on another host", then "Run
/// again before the other host takes it in again": the across twin of
/// [`run_again_marks_a_draft_ready`]. B blamed `#n` and marked it a draft,
/// its check saying `waiting: the next submit` while the double keeps that
/// check failed; `wt submit main --no-wait --json` on A, nothing committed
/// since, marks it ready at that head and answers that it waits for B; run
/// again under a time limit, it pushes and marks nothing, follows `#n`, and
/// exits 5 saying the same; and B's next pass takes `#n` in again at that
/// head (67, 120, 137, C.5; record 0096).
#[test]
fn run_again_marks_ready_a_draft_another_host_sent_back() {
    let g = OnGitHub::new("intake-again-draft", "");
    let (b, w, pull) = held_on_b(&g, "redrafted", &[("broken", "b\n")]);
    let n = pull.number;
    // Beside its state, the mark's host and holder and the queue B's check
    // carries, B's submission of it failed and held for the next submit
    // (record 0095 D5).
    let waiting = serde_json::json!({
        "pull_request": n, "branch": "main", "state": "waiting to be taken in by b",
        "taken_in_by": { "host": "b" },
        "queue": {
            "host": "b", "taken_in": true, "submission": b.of(n)[0]["id"],
            "state": "failed", "ahead": [], "waits_for": ["the next submit"],
        },
    });
    let out = g.wt_submit(&w, &["--no-wait", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    assert_eq!(doc, waiting);
    let ready = g.pull_from("unit/redrafted");
    assert!(!ready.draft && ready.head == pull.head, "{ready:?}");

    let pushed = github::rev(g.bare.path(), "refs/heads/unit/redrafted");
    let writes = || {
        let edit = format!("/repos/o/r/pulls/{n}");
        g.gh.fake
            .requests()
            .iter()
            .filter(|r| {
                (r.method == "PATCH" && r.path == edit)
                    || (r.path == "/graphql" && r.body.contains("mutation("))
            })
            .count()
    };
    let before = writes();
    let out = g.wt_submit(&w, &["--timeout", "2s", "--json"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let doc: Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    assert_eq!(doc, waiting);
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        said.contains(&format!(
            "wt submit: following pull request #{n}, already open at this revision"
        )),
        "{said}"
    );
    assert!(!said.contains("pushed"), "{said}");
    assert_eq!(writes(), before, "nothing marked");
    assert_eq!(
        github::rev(g.bare.path(), "refs/heads/unit/redrafted"),
        pushed
    );
    assert!(!g.pull_from("unit/redrafted").draft);
    assert!(g.of(n).is_empty(), "A took nothing in");

    let until = Instant::now() + Duration::from_secs(60);
    while b.of(n).len() < 2 {
        assert!(
            Instant::now() < until,
            "B never took it again: {:?}",
            b.of(n)
        );
        b.discovery_passes();
        b.run("1");
    }
    let subs = b.of(n);
    assert_eq!(subs[1]["head"], pull.head.as_str(), "{subs:?}");
}

/// How many GraphQL requests naming `mutation` GitHub's double was sent:
/// `convertPullRequestToDraft` marks a pull request a draft, and
/// `markPullRequestReadyForReview` ready for review.
fn mutations(g: &OnGitHub, mutation: &str) -> usize {
    g.gh.fake
        .requests()
        .iter()
        .filter(|r| r.path == "/graphql" && r.body.contains(mutation))
        .count()
}

/// pull-request-submit, "Sent back, not a draft": host B blamed `#n`,
/// which a person opened from `unit/review` and which B therefore left out
/// of draft, its check saying `failed: …`; `sluice submit main --no-wait`
/// on host A from a worktree on `unit/review`, nothing committed since,
/// marks it a draft and then ready for review at that head, though A's
/// records hold no submission of it, and B's next pass takes it in again
/// (120, 137, C.5; record 0096).
#[test]
fn a_submit_marks_ready_a_request_another_host_sent_back_not_a_draft() {
    let g = OnGitHub::new("intake-sent-back-ready", "");
    g.named("a", "");
    let b = g.other("b", "", "");
    b.run("1");
    let w = g.session("review", &[("broken", "b\n")]);
    g.repo
        .git(&["push", "-q", "origin", "unit/review:refs/heads/unit/review"]);
    let head = github::rev(g.bare.path(), "refs/heads/unit/review").unwrap();
    let n =
        g.gh.forge
            .open_by("person", "main", "unit/review", &head, false, None)
            .unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    let blamed = || {
        b.of(n).first().is_some_and(|s| s["state"] == "failed")
            && g.posted(&head).is_some_and(|p| p.1.starts_with("failed: "))
    };
    while !blamed() {
        assert!(
            Instant::now() < until,
            "B never blamed it: {:?}, {:?}",
            b.of(n),
            g.posted(&head)
        );
        b.discovery_passes();
        b.run("1");
    }
    assert!(
        !g.pull_from("unit/review").draft,
        "B leaves a request opened by hand out of draft"
    );
    // The marks later than B's leaving, to the second GitHub and the
    // records keep, as B reads a ready mark since (120).
    std::thread::sleep(Duration::from_millis(1100));
    let drafted = mutations(&g, "convertPullRequestToDraft");
    let readied = mutations(&g, "markPullRequestReadyForReview");
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "main", "--no-wait"])
        .output()
        .unwrap();
    assert!(
        text(&out).contains(&format!("updated #{n}")) && text(&out).contains("marked ready"),
        "{}",
        text(&out)
    );
    assert_eq!(
        mutations(&g, "convertPullRequestToDraft"),
        drafted + 1,
        "a draft first: {}",
        text(&out)
    );
    assert_eq!(
        mutations(&g, "markPullRequestReadyForReview"),
        readied + 1,
        "then ready: {}",
        text(&out)
    );
    let ready = g.pull_from("unit/review");
    assert!(!ready.draft && ready.head == head, "{ready:?}");
    assert!(g.of(n).is_empty(), "A took nothing in");
    while b.of(n).len() < 2 {
        assert!(
            Instant::now() < until,
            "B never took it again: {:?}",
            b.of(n)
        );
        b.discovery_passes();
        b.run("1");
    }
    let subs = b.of(n);
    assert_eq!(subs[1]["head"], head.as_str(), "{subs:?}");
}

/// pull-request-submit, "Still queued on the other host": host B has `#n`
/// queued, its check saying `queued: next on main`; `sluice submit main
/// --no-wait` on host A, nothing committed since, marks it no draft, even
/// for a moment, and B's submission of it stays in B's queue, which a
/// draft mark would withdraw (C.5; record 0096).
#[test]
fn a_submit_leaves_a_request_queued_on_another_host() {
    let g = OnGitHub::new("intake-still-queued", "");
    g.named("a", "");
    let b = g.other("b", "", "");
    b.run("1");
    let w = g.session("queued", &[("q.txt", "q\n")]);
    let out = g.wt_submit(&w, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from("unit/queued");
    let n = pull.number;
    let until = Instant::now() + Duration::from_secs(60);
    while !g
        .posted(&pull.head)
        .is_some_and(|p| p.1 == "queued: next on main")
    {
        assert!(
            Instant::now() < until,
            "B never queued it: {:?}, {:?}",
            b.of(n),
            g.posted(&pull.head)
        );
        b.discovery_passes();
        b.run("1");
    }
    // In B's queue: the pass that took it in may have formed its
    // combination, which B's check says at its next pass.
    let id = b.of(n)[0]["id"].clone();
    assert!(
        matches!(b.of(n)[0]["state"].as_str(), Some("queued" | "testing")),
        "{:?}",
        b.of(n)
    );
    let drafted = mutations(&g, "convertPullRequestToDraft");
    let out = g
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["submit", "main", "--no-wait"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(!text(&out).contains("marked ready"), "{}", text(&out));
    assert_eq!(
        mutations(&g, "convertPullRequestToDraft"),
        drafted,
        "never a draft: {}",
        text(&out)
    );
    assert!(!g.pull_from("unit/queued").draft);
    assert!(g.of(n).is_empty(), "A took nothing in");
    // B's next pass reads no draft mark, so withdraws nothing (C.5): its
    // one submission of `#n` goes on.
    b.discovery_passes();
    b.run("1");
    let subs = b.of(n);
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0]["id"], id, "{subs:?}");
    assert_ne!(subs[0]["state"], "withdrawn", "{subs:?}");
}

/// request-intake-host, "A wait lets go", stopped by Ctrl-C: a wait on
/// host B, where no `sluice run` lives, holds main's mark, and once Ctrl-C
/// stops it the mark is gone from the remote.
#[test]
fn a_wait_stopped_by_ctrl_c_lets_go() {
    let g = OnGitHub::new("intake-ctrl-c", "reviews = 1");
    g.named("a", "");
    g.by_hand("unit/stopped", &[("s.txt", Some("s\n"))], false);
    let b = g.other("b", "", "");
    let w = b.worktree_of("unit/stopped");
    let wait = b
        .command(env!("CARGO_BIN_EXE_sluice"), &w)
        .args(["wait", "main", "--timeout", "2m"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    while g.mark().is_none() {
        assert!(Instant::now() < until, "never held");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(g.mark().unwrap().1["host"], "b");
    let sent = Command::new("kill")
        .args(["-INT", &wait.id().to_string()])
        .status()
        .unwrap();
    assert!(sent.success());
    let out = wait.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(sluice::STOPPED), "{}", text(&out));
    assert_eq!(g.mark(), None, "{}", text(&out));
}

impl OnGitHub {
    /// Host A, called `a`: its `sluice run` pushes main's intake mark and
    /// takes `pull/<n>` in, testing it with a check that goes on until it
    /// is ended. The request's number, its submission's id, and the run's
    /// process.
    fn testing_under_its_mark(&self, name: &str) -> (u64, u64, String) {
        self.named("a", "");
        let pid = self.repo.dir().join(".git/hang.pid");
        let hang = format!("{}\n", pid.display());
        let (n, _) = self.by_hand(name, &[("hang", Some(&hang))], false);
        self.on_main();
        let out = self.skipping(self.repo.dir(), &["run", "--rounds", "1"]);
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(self.mark().unwrap().1["host"], "a", "{}", text(&out));
        let id = self.of(n)[0]["id"].as_u64().expect("taken in");
        let until = Instant::now() + Duration::from_secs(60);
        while !pid.exists() {
            assert!(Instant::now() < until, "the check never started");
            std::thread::sleep(Duration::from_millis(100));
        }
        let process = std::fs::read_to_string(&pid).unwrap().trim().to_string();
        (n, id, process)
    }

    /// This host last pushed main's intake mark longer ago than a third of
    /// its lease, and its next pass is due: that pass renews it (137).
    fn renewal_due(&self) {
        let path = self.state.path().join("sluice/forge/intake/o/r/main.json");
        let mut v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        v["renewed"] = "2000-01-01T00:00:00Z".into();
        std::fs::write(&path, v.to_string()).unwrap();
        self.discovery_passes();
    }

    /// The next fetch here that brings main's intake mark has another
    /// process of this host move the mark on the remote before this pass
    /// pushes it: `"drop"` lets it go, as a wait of the host does when it
    /// ends, and `"renew"` pushes a mark of this host's driver's over it.
    /// The mark this repository last read is forgotten, so that the fetch
    /// brings it again.
    fn moved_under_the_next_push(&self, how: &str) {
        let armed = self.repo.dir().join(".git/armed");
        std::fs::write(&armed, "").unwrap();
        let bare = format!("{:?}", self.bare.path().display().to_string());
        let act = match how {
            "drop" => format!("git --git-dir={bare} update-ref -d refs/sluice/intake/main"),
            _ => format!(
                "t=$(git --git-dir={bare} mktree </dev/null)\n      \
                 c=$(git -c user.name=s -c user.email=s@s --git-dir={bare} commit-tree \"$t\" \
                 -m '{{\"host\":\"a\",\"holder\":\"driver\",\"sluice\":\"0.0.0\"}}')\n      \
                 git --git-dir={bare} update-ref refs/sluice/intake/main \"$c\""
            ),
        };
        let hook = self.repo.dir().join(".git/hooks/reference-transaction");
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\n[ \"$1\" = committed ] || exit 0\n\
                 while read old new ref; do\n  \
                 if [ \"$ref\" = refs/sluice-fetched/origin/intake/main ] && \
                 [ \"$new\" != 0000000000000000000000000000000000000000 ] && [ -e {armed:?} ]; then\n    \
                 rm -f {armed:?}\n      {act}\n  fi\ndone\nexit 0\n",
                armed = armed.display().to_string()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        self.repo
            .git(&["update-ref", "-d", "refs/sluice-fetched/origin/intake/main"]);
    }
}

/// Whether `log` says submission `id` was withdrawn.
fn withdrawn_in(log: &[Value], id: u64) -> bool {
    log.iter()
        .any(|l| l["event"] == "withdrawn" && l["id"] == id)
}

/// End the check process `pid` where it still runs.
fn end(pid: &str) {
    let _ = Command::new("kill")
        .args(["-9", pid])
        .stderr(Stdio::null())
        .status();
}

/// request-intake-host, "A mark let go under a renewal": host A's `sluice
/// run` holds main's mark and is testing `pull/<n>`; another process of A
/// lets the mark go between the pass's fetch and its renewal, so the
/// renewal is refused at its lease and the remote holds no mark. A pushes
/// it again leased on its absence, holds it, and withdraws nothing: no
/// host but A takes main's pull requests in (137; rule core.33; the
/// chuck-herdr-alpha hand-over to "another host" of 2026-11-06).
#[test]
fn a_mark_let_go_under_a_renewal_is_pushed_again() {
    let g = OnGitHub::new("intake-again", "");
    let (n, id, process) = g.testing_under_its_mark("unit/again");
    g.renewal_due();
    g.moved_under_the_next_push("drop");
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "1"]);
    end(&process);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        !g.repo.dir().join(".git/armed").exists(),
        "the mark was let go under the renewal"
    );
    let log = g.log();
    assert!(!withdrawn_in(&log, id), "{log:?}\n{}", text(&out));
    assert_ne!(g.of(n)[0]["state"], "withdrawn");
    let (_, mark) = g.mark().expect("pushed again");
    assert_eq!(
        (&mark["host"], &mark["holder"]),
        (&"a".into(), &"driver".into())
    );
}

/// request-intake-host, "A mark this host moved under a renewal": host A's
/// `sluice run` holds main's mark and is testing `pull/<n>`; another
/// process of A pushes A's own mark over it between the pass's fetch and
/// its renewal, so the renewal is refused at its lease. The mark names A:
/// A holds it, and hands nothing over to itself (137; rule core.33).
#[test]
fn a_mark_this_host_moved_under_a_renewal_stays_its_own() {
    let g = OnGitHub::new("intake-own", "");
    let (n, id, process) = g.testing_under_its_mark("unit/own");
    g.renewal_due();
    g.moved_under_the_next_push("renew");
    let out = g.skipping(g.repo.dir(), &["run", "--rounds", "1"]);
    end(&process);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!g.repo.dir().join(".git/armed").exists());
    let log = g.log();
    assert!(!withdrawn_in(&log, id), "{log:?}\n{}", text(&out));
    assert_ne!(g.of(n)[0]["state"], "withdrawn");
    assert_eq!(g.mark().unwrap().1["host"], "a");
}

/// request-intake-host, "A host's waits hold its mark together": two waits
/// on host B, where no `sluice run` lives, read main's pull requests; the
/// first pushed the mark. When the first ends the mark stays, since the
/// second still holds it; when the second ends, though it never pushed
/// the mark, it lets it go (137).
#[test]
fn a_hosts_waits_hold_its_mark_together() {
    let g = OnGitHub::new("intake-together", "reviews = 1");
    g.named("a", "");
    g.record_marks();
    g.by_hand("unit/together", &[("t.txt", Some("t\n"))], false);
    let b = g.other("b", "", "");
    let w = b.worktree_of("unit/together");
    let wait = |timeout: &str| {
        b.command(env!("CARGO_BIN_EXE_sluice"), &w)
            .args(["wait", "main", "--timeout", timeout])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let first = wait("10s");
    let until = Instant::now() + Duration::from_secs(60);
    while g.mark().is_none() {
        assert!(Instant::now() < until, "never held");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(g.mark().unwrap().1["host"], "b");
    let second = wait("2m");
    let waits = b.state.path().join("sluice/forge/intake/o/r/main.waits");
    let holds = || {
        std::fs::read_dir(&waits)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "hold"))
            .count()
    };
    while holds() < 2 {
        assert!(Instant::now() < until, "the second wait never held it");
        std::thread::sleep(Duration::from_millis(100));
    }
    let out = first.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    assert_eq!(
        g.mark().map(|m| m.1["host"].clone()),
        Some("b".into()),
        "the second wait holds it still: {}",
        text(&out)
    );
    assert!(
        !g.marks_pushed().contains(&Value::Null),
        "{:?}",
        g.marks_pushed()
    );
    let sent = Command::new("kill")
        .args(["-INT", &second.id().to_string()])
        .status()
        .unwrap();
    assert!(sent.success());
    let out = second.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(sluice::STOPPED), "{}", text(&out));
    assert_eq!(
        g.mark(),
        None,
        "the host's last wait let it go: {}",
        text(&out)
    );
    assert_eq!(holds(), 0);
}

/// request-intake-host, "A request that landed elsewhere is not handed
/// over": host A is testing `pull/<n>` when host B's `sluice run` takes
/// main's mark and the request in, and lands it. A's next pass finds B
/// holding the mark and main holding the request's head: A withdraws
/// nothing, and its round lands its submission as the commit that landed
/// the work (122, 137; rule core.48).
#[test]
fn a_request_that_landed_elsewhere_is_not_handed_over() {
    let g = OnGitHub::new("intake-landed-over", "");
    let (n, id, process, _, _, b) = handing_mid_test(&g);
    let until = Instant::now() + Duration::from_secs(60);
    while !b.of(n).first().is_some_and(|s| s["state"] == "landed") {
        assert!(Instant::now() < until, "B never landed it: {:?}", b.of(n));
        b.discovery_passes();
        b.run("2");
    }
    g.step();
    end(&process);
    let log = g.log();
    assert!(!withdrawn_in(&log, id), "{log:?}");
    let a = g.of(n);
    assert_eq!(a[0]["state"], "landed", "{a:?}");
    assert_eq!(a[0]["landed_elsewhere"], true, "{a:?}");
}

// ---------------------------------------------------------------------------
// request-intake-host: a withdrawal for a hand-over holds no request, which
// the holder of the mark takes in again at the head it holds, the host that
// withdrew it included (120, 137; record 0088 as amended; rule core.33)
// ---------------------------------------------------------------------------

/// A pull request a submit opened on host A, handed over to host B.
struct HandedOver {
    /// The request's number.
    n: u64,
    /// A's submission of it, withdrawn for the hand-over.
    id: u64,
    /// Its head.
    head: String,
    /// The session's worktree it was submitted from.
    worktree: PathBuf,
    /// How many ready marks GitHub's timeline held of it at the hand-over.
    readied: usize,
}

/// Host A, called `a`, has the pull request the session's submit opened
/// from `unit/<name>` under test with a check that goes on until it is
/// ended, when another host's `sluice run` takes main's mark: `b`'s, as
/// that run would push it. A's next pass hands the request over to B,
/// naming B in its withdrawal's record and log line, and ends the check.
fn handed_over_mid_test(g: &OnGitHub, name: &str) -> HandedOver {
    g.named("a", "");
    let pid = g.repo.dir().join(format!(".git/{name}.pid"));
    let worktree = g.session(name, &[("hang", &format!("{}\n", pid.display()))]);
    let out = g.wt_submit(&worktree, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pull = g.pull_from(&format!("unit/{name}"));
    let n = pull.number;
    g.step();
    let id = g.of(n)[0]["id"].as_u64().expect("taken in");
    let until = Instant::now() + Duration::from_secs(60);
    while !pid.exists() {
        assert!(Instant::now() < until, "the check never started");
        std::thread::sleep(Duration::from_millis(100));
    }
    let process = std::fs::read_to_string(&pid).unwrap().trim().to_string();
    g.push_mark("b", "driver");
    g.step();
    let a = g.of(n);
    assert_eq!(a[0]["state"], "withdrawn", "{a:?}");
    assert_eq!(a[0]["withdrawn"]["by"], "b takes main's pull requests in");
    assert_eq!(a[0]["withdrawn"]["handed_over_to"], "b", "{a:?}");
    let line = g
        .log()
        .into_iter()
        .find(|l| l["event"] == "withdrawn" && l["id"] == id)
        .expect("the log says it");
    assert_eq!(line["handed_over_to"], "b", "{line}");
    let until = Instant::now() + Duration::from_secs(10);
    while alive(&process) {
        assert!(Instant::now() < until, "A's run goes on");
        std::thread::sleep(Duration::from_millis(100));
    }
    HandedOver {
        n,
        id,
        head: pull.head,
        worktree,
        readied: readied(g, n),
    }
}

/// How many ready marks GitHub's timeline holds of pull request `n`: a
/// draft sluice makes and takes back leaves one more.
fn readied(g: &OnGitHub, n: u64) -> usize {
    g.gh.forge.readied(&github::repo(), n).unwrap().len()
}

/// main's intake mark comes back to host A, its `sluice run` returning
/// once the host that held it stopped: the mark names A's driver.
fn mark_comes_back(g: &OnGitHub) {
    github::delete_ref(g.bare.path(), "refs/sluice/intake/main");
    g.push_mark("a", "driver");
}

/// request-intake-host, "The mark comes back": host A handed the pull
/// request a session's submit opened over to B, and main's mark comes back
/// to A. A takes it in again at the head it holds, as its conditions hold,
/// and lands it; nothing marks it a draft, and it waits for no act of its
/// submitter's (120, 137; agentplot/sluice pull 171, whose #345 deneb
/// withdrew at the hand-over of 2026-10-30 and took in again as #347).
#[test]
fn the_mark_comes_back_and_its_request_is_taken_in_again() {
    let g = OnGitHub::new("intake-comes-back", "");
    let h = handed_over_mid_test(&g, "back");
    mark_comes_back(&g);
    g.step();
    let a = g.of(h.n);
    assert_eq!(a.len(), 2, "{a:?}");
    assert_eq!(a[1]["head"], h.head.as_str(), "{a:?}");
    assert_ne!(a[1]["state"], "withdrawn", "{a:?}");
    assert!(a[1]["id"].as_u64().unwrap() > h.id);
    assert!(!g.pull_from("unit/back").draft);
    assert_eq!(readied(&g, h.n), h.readied, "never a draft and back");
    g.step_until("landed", |g| g.of(h.n)[1]["state"] == "landed");
    assert!(!g.pull_from("unit/back").draft);
    assert_eq!(readied(&g, h.n), h.readied);
}

/// request-intake-host, "An older hand-over comes back": as "The mark
/// comes back", with A's withdrawal written as sluice 0.60.0 to 0.70.2
/// wrote one, naming no host in its record, in its words alone. It reads
/// as that sluice read it: the request is taken in again, never a draft
/// (120; record 0088 as amended, design D2).
#[test]
fn an_older_hand_overs_request_is_taken_in_again() {
    let g = OnGitHub::new("intake-older-back", "");
    let h = handed_over_mid_test(&g, "older");
    assert!(written_as_before(&g, h.id) > 0, "its record was found");
    let a = g.of(h.n);
    assert_eq!(a[0]["withdrawn"]["handed_over_to"], Value::Null, "{a:?}");
    mark_comes_back(&g);
    g.step();
    let a = g.of(h.n);
    assert_eq!(a.len(), 2, "{a:?}");
    assert_eq!(a[1]["head"], h.head.as_str(), "{a:?}");
    assert_ne!(a[1]["state"], "withdrawn", "{a:?}");
    assert!(!g.pull_from("unit/older").draft);
    assert_eq!(readied(&g, h.n), h.readied);
}

/// Rewrite host A's records of submission `id`'s withdrawal as a sluice
/// before record 0088's amendment wrote them, the host it handed over to
/// named nowhere: its record, wherever it is kept, and its log line. How
/// many records named it.
fn written_as_before(g: &OnGitHub, id: u64) -> usize {
    let dir = g.repo.dir().join(".git/sluice/main");
    let mut named = 0;
    for path in [
        dir.join(format!("submissions/{id}.json")),
        dir.join(format!(".finished/{id}.json")),
    ] {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let mut v: Value = serde_json::from_slice(&bytes).unwrap();
        if v["withdrawn"]
            .as_object_mut()
            .and_then(|w| w.remove("handed_over_to"))
            .is_some()
        {
            named += 1;
        }
        std::fs::write(&path, serde_json::to_vec_pretty(&v).unwrap()).unwrap();
    }
    let log = dir.join("log.jsonl");
    let lines: Vec<String> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|l| {
            let mut v: Value = serde_json::from_str(l).unwrap();
            if v["event"] == "withdrawn" && v["id"] == id {
                v.as_object_mut().unwrap().remove("handed_over_to");
            }
            v.to_string()
        })
        .collect();
    std::fs::write(&log, lines.join("\n") + "\n").unwrap();
    named
}

/// request-intake-host, "A submit after a hand-over": host A handed the
/// pull request a session's submit opened over to B, and its agent submits
/// again from its worktree, with no new commits. The withdrawal is no
/// leaving, so the submit marks it no draft and ready again, as it does
/// after a blame (120: nothing marks it a draft for a hand-over).
#[test]
fn a_submit_after_a_hand_over_marks_no_draft() {
    let g = OnGitHub::new("intake-resubmit", "");
    let h = handed_over_mid_test(&g, "again");
    let out = g.wt_submit(&h.worktree, &["--no-wait"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(
        !text(&out).contains("marked ready for review"),
        "{}",
        text(&out)
    );
    assert!(!g.pull_from("unit/again").draft);
    assert_eq!(readied(&g, h.n), h.readied, "never a draft and back");
}

/// request-intake-host, "A wait on a hand-over says where it went": host A
/// handed the pull request a session's submit opened over to B; a wait on
/// A's submission follows its pull request, saying it was handed over to
/// B, and is never told it was withdrawn, and A's status of it says so
/// (122, 137; rule core.48).
#[test]
fn a_wait_on_a_hand_over_says_where_it_went() {
    let g = OnGitHub::new("intake-wait-says", "");
    let h = handed_over_mid_test(&g, "said");
    let id = h.id.to_string();
    let out = g.sluice(&["wait", &id, "--timeout", "2s"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(
        said.contains(&format!(
            "#{id} handed over to b: its pull request #{} carries its work, so this wait \
             follows it",
            h.n
        )),
        "{said}"
    );
    let out = g.sluice(&["status", &id]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("handed over to b"), "{}", text(&out));
}

/// request-intake-host, "A request closed is still withdrawn": host A
/// handed the pull request a session's submit opened over to B, main's
/// mark came back and A took it in again; a person closes it. It is
/// withdrawn, and opened again it waits for its submitter's act as a
/// draft, as any request closed does where its submission was (120, C.5).
#[test]
fn a_request_closed_after_it_came_back_is_still_held() {
    let g = OnGitHub::new("intake-closed-back", "");
    let h = handed_over_mid_test(&g, "closed");
    mark_comes_back(&g);
    g.step();
    assert_eq!(g.of(h.n).len(), 2);
    g.gh.forge.close(h.n).unwrap();
    g.step();
    let a = g.of(h.n);
    assert_eq!(a[1]["state"], "withdrawn", "{a:?}");
    assert_eq!(a[1]["withdrawn"]["handed_over_to"], Value::Null, "{a:?}");
    g.gh.forge
        .change(|held| {
            let p = held.pulls.iter_mut().find(|p| p.number == h.n).unwrap();
            p.open = true;
            p.updated_at = Some(chrono::Utc::now());
            Ok(())
        })
        .unwrap();
    let a = g.of_after(h.n, 2);
    assert_eq!(a.len(), 2, "never taken in again: {a:?}");
    assert!(g.pull_from("unit/closed").draft);
}

// ---------------------------------------------------------------------------
// forge-reads: one query reads a branch's open pull requests whole, and a
// refused one falls back to the reads per pull request (136; rule core.32)
// ---------------------------------------------------------------------------

/// forge-reads, "Three pull requests, one query": main has three open pull
/// requests out of draft, waiting on a review and `qa/approved`, and its
/// list comes back changed: one GraphQL query reads them, and nothing of
/// any one is read alone, its reviews, checks, timeline or sluice's check
/// runs; each is shown what it lacks.
#[test]
fn three_pull_requests_one_query() {
    let g = OnGitHub::new("pr-one-query", "reviews = 1\nchecks = [\"qa/approved\"]");
    let opened: Vec<(u64, String)> = ["a", "b", "c"]
        .iter()
        .map(|x| {
            g.by_hand(
                &format!("unit/three-{x}"),
                &[(&format!("{x}.txt"), Some("x\n"))],
                false,
            )
        })
        .collect();
    g.step();
    assert_eq!(g.gh.queries(), 1);
    for (n, head) in &opened {
        assert_eq!(g.beside_reads(*n, head), 0, "#{n} read alone");
        assert_eq!(g.posted_reads(head), 0, "#{n}'s check runs read back");
        assert_eq!(
            g.posted(head).unwrap().1,
            "waiting: needs 1 review, qa/approved"
        );
    }
}

/// forge-reads, "Reviews counted as before": one reviewer approved the
/// pull request's older head and another its head; under `reviews = 2`
/// it counts one approval, and lacks one review.
#[test]
fn the_querys_reviews_count_as_before() {
    let g = OnGitHub::new("pr-one-query-reviews", "reviews = 2");
    let (n, old) = g.by_hand("unit/counted", &[("c.txt", Some("c\n"))], false);
    g.gh.forge.review(n, "ann", "APPROVED", &old).unwrap();
    let head = g.push_more("unit/counted", "c2.txt");
    g.gh.forge.review(n, "bob", "APPROVED", &head).unwrap();
    g.step();
    assert_eq!(g.gh.queries(), 1);
    assert_eq!(g.beside_reads(n, &head), 0);
    assert_eq!(g.posted(&head).unwrap().1, "waiting: needs 1 review");
    // A second approval on the head: the list changes, and the next
    // asking submits it.
    g.gh.forge.review(n, "cat", "APPROVED", &head).unwrap();
    g.step();
    assert_eq!(g.gh.queries(), 2);
    assert_eq!(g.of(n).len(), 1, "{:?}", g.subs());
}

/// forge-reads, "The query refused": GitHub answers the query with an
/// error that is not `RATE_LIMITED`; that pass reads each pull request by
/// REST, says so once, and shows each what it lacks.
#[test]
fn a_refused_query_falls_back_to_the_reads_per_pull_request() {
    let g = OnGitHub::new("pr-one-query-refused", "reviews = 1");
    let (one, one_head) = g.by_hand("unit/refused-one", &[("1.txt", Some("1\n"))], false);
    let (two, two_head) = g.by_hand("unit/refused-two", &[("2.txt", Some("2\n"))], false);
    g.gh.refuse_query(true);
    let said = g.round();
    let warning = "GitHub refused the query that reads them whole, so each is read alone this pass";
    assert_eq!(said.matches(warning).count(), 1, "{said}");
    assert!(
        said.contains("Resource not accessible by integration"),
        "{said}"
    );
    assert_eq!(g.gh.queries(), 1);
    for (n, head) in [(one, &one_head), (two, &two_head)] {
        assert_eq!(
            g.gh.calls("GET", &format!("/repos/o/r/pulls/{n}/reviews")),
            1,
            "#{n}"
        );
        assert_eq!(g.posted(head).unwrap().1, "waiting: needs 1 review");
    }
    // Answered again: the next asking reads them whole.
    g.gh.refuse_query(false);
    g.gh.forge
        .review(one, "reviewer", "APPROVED", &one_head)
        .unwrap();
    let said = g.round();
    assert!(!said.contains(warning), "{said}");
    assert_eq!(g.gh.queries(), 2);
    assert_eq!(
        g.gh.calls("GET", &format!("/repos/o/r/pulls/{one}/reviews")),
        1
    );
    assert_eq!(g.of(one).len(), 1, "{:?}", g.subs());
}
