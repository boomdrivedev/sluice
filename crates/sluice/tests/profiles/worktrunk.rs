//! The worktrunk driver end to end through the real binaries: `wt
//! submit` runs `wt-submit`, which runs `sluice submit` from the session's
//! worktree; section 7's acceptance tests in
//! `docs/plans/phase-1-integrations.md`. A scratch repository gates
//! `bolt/*` on one command, in git-worktree places. Needs git and wt on
//! the path, so it is in the check-tools tier.

#![cfg(feature = "tools")]

#[path = "../support/mod.rs"]
mod support;

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use sluice::profiles::worktrunk::Worktrunk;
use sluice_seams::{Driver as _, Submitter};
use support::Repo;

const POLICY: &str = "[tests]\nrunner = \"command\"\ncommand = \"sh check.sh\"\n\n\
[branches.\"bolt/*\"]\ntimeout = \"1m\"\ntrust = [\"open\", \"sandbox\", \"container\"]\n";

/// The check: fails while a file named `broken` exists, after `SLOW`
/// seconds, and takes five seconds more while one named `slow` does.
const CHECK: &str = "sleep ${SLOW:-0}\nif test -e slow; then sleep 5; fi\ntest ! -e broken\n";

struct Scratch {
    repo: Repo,
    config: PathBuf,
    state: PathBuf,
    /// First on the path of every command the scratch runs, so no round
    /// reaches the host's own herdr.
    herdr: FakeHerdr,
}

/// A herdr of the test's own: it keeps each call's arguments, the socket
/// it was handed and the pane it ran in, and answers as the test sets.
struct FakeHerdr {
    dir: PathBuf,
}

impl FakeHerdr {
    fn new(dir: PathBuf) -> FakeHerdr {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.display();
        let script = format!(
            "#!/bin/sh\n\
             {{ printf 'call'; for a in \"$@\"; do printf ' <%s>' \"$a\"; done\n\
             printf '\\nsocket %s\\npane %s\\n' \"${{HERDR_SOCKET_PATH-unset}}\" \"${{HERDR_PANE_ID-unset}}\"; }} >> {d}/calls\n\
             if test -f {d}/stderr; then cat {d}/stderr >&2; fi\n\
             exit \"$(cat {d}/exit 2>/dev/null || echo 0)\"\n"
        );
        let herdr = dir.join("herdr");
        std::fs::write(&herdr, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&herdr, std::fs::Permissions::from_mode(0o755)).unwrap();
        FakeHerdr { dir }
    }

    /// From now it refuses, writing `stderr` and exiting `code`.
    fn refuses(&self, code: i32, stderr: &str) {
        std::fs::write(self.dir.join("exit"), code.to_string()).unwrap();
        std::fs::write(self.dir.join("stderr"), stderr).unwrap();
    }

    /// Each call: its arguments, the socket and the pane it ran under.
    fn calls(&self) -> Vec<[String; 3]> {
        let text = std::fs::read_to_string(self.dir.join("calls")).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        lines
            .chunks(3)
            .map(|c| [c[0].to_string(), c[1].to_string(), c[2].to_string()])
            .collect()
    }
}

impl Scratch {
    fn new(name: &str) -> Scratch {
        Scratch::with_policy(name, POLICY)
    }

    /// A scratch whose release command writes the checkout it ran for to
    /// the log it returns, while that checkout still exists.
    fn releasing(name: &str) -> (Scratch, PathBuf) {
        let log = std::env::temp_dir().join(format!("sluice-{name}-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&log);
        let policy = POLICY.replace(
            "command = \"sh check.sh\"\n",
            &format!(
                "command = \"sh check.sh\"\nrelease = \"test -d \\\"$SLUICE_CHECKOUT\\\" && echo \\\"$SLUICE_CHECKOUT\\\" >> {}\"\n",
                log.display()
            ),
        );
        (Scratch::with_policy(name, &policy), log)
    }

    fn with_policy(name: &str, policy: &str) -> Scratch {
        let repo = Repo::new(name);
        repo.write(".config/sluice.toml", policy);
        repo.write("check.sh", CHECK);
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "sluice's policy"]);
        repo.git(&["branch", "bolt/x", "main"]);
        let config = repo.dir().with_extension("config");
        std::fs::create_dir_all(config.join("sluice")).unwrap();
        std::fs::write(config.join("sluice/config.toml"), "[runs]\nmax = 2\n").unwrap();
        let state = repo.dir().with_extension("state");
        let _ = std::fs::remove_dir_all(&state);
        let herdr = FakeHerdr::new(repo.dir().with_extension("herdr"));
        Scratch {
            repo,
            config,
            state,
            herdr,
        }
    }

    /// A session's worktree on `unit/<name>` off `bolt/x`, with one commit
    /// of `changes`.
    fn session(&self, name: &str, changes: &[(&str, &str)]) -> PathBuf {
        let dir = self.repo.dir().with_extension(format!("s-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        self.repo.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            &format!("unit/{name}"),
            dir.to_str().unwrap(),
            "bolt/x",
        ]);
        for (p, c) in changes {
            std::fs::write(dir.join(p), c).unwrap();
        }
        let git = |args: &[&str]| {
            let mut a = vec!["-C", dir.to_str().unwrap()];
            a.extend(args);
            self.repo.git(&a)
        };
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", name]);
        dir
    }

    fn command(&self, program: &str, dir: &Path) -> Command {
        self.command_under(Command::new(program), dir)
    }

    /// `c` as a test runs it, whatever its parent gave it: isolated, with
    /// the scratch's own config and state, and with
    /// `SLUICE_ROUND_SLEEP=record`, so the host process its commands start
    /// passes only when asked, a test's rounds those its steps and waits
    /// ask for (135; design D14).
    fn command_under(&self, mut c: Command, dir: &Path) -> Command {
        let bin = Path::new(env!("CARGO_BIN_EXE_sluice")).parent().unwrap();
        let path = format!(
            "{}:{}:{}",
            self.herdr.dir.display(),
            bin.display(),
            std::env::var("PATH").unwrap()
        );
        support::isolated(&mut c)
            .current_dir(dir)
            .env("PATH", path)
            .env("XDG_CONFIG_HOME", &self.config)
            .env("XDG_STATE_HOME", &self.state)
            .env("SLUICE_ROUND_SLEEP", "record")
            .env_remove("SLUICE_AGENT")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null());
        c
    }

    fn wt_submit(&self, dir: &Path, args: &[&str]) -> Output {
        self.command("wt", dir)
            .arg("submit")
            .arg("bolt/x")
            .args(args)
            .output()
            .unwrap()
    }

    fn sluice(&self, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_sluice"), self.repo.dir())
            .args(args)
            .output()
            .unwrap()
    }

    fn status(&self, id: u64) -> serde_json::Value {
        let out = self.sluice(&["status", &id.to_string(), "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    fn tip(&self) -> String {
        self.repo.rev("bolt/x")
    }

    /// Every submission `bolt/x` has had from the worktree on
    /// `unit/<name>`.
    fn subs_from(&self, name: &str) -> Vec<serde_json::Value> {
        let out = self.sluice(&["status", "bolt/x", "--json", "--all"]);
        assert!(out.status.success(), "{}", text(&out));
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let from = format!("unit/{name}");
        v["submissions"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|x| x["from"]["worktree"] == from.as_str())
            .collect()
    }

    /// Rounds of `bolt/x` until submission `id` lands, at most a minute.
    fn land(&self, id: u64) {
        let until = Instant::now() + Duration::from_secs(60);
        while self.status(id)["state"] != "landed" {
            assert!(Instant::now() < until, "{}", self.status(id));
            let out = self.sluice(&["step", "bolt/x"]);
            assert!(out.status.success(), "{}", text(&out));
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

/// How many times the release `log` names `dir`.
fn released(log: &Path, dir: &Path) -> usize {
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter(|l| Path::new(l) == dir)
        .count()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// The submission number a `--no-wait --json` submit printed.
fn id(out: &Output) -> u64 {
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("no JSON: {}", text(out)));
    v["id"].as_u64().unwrap()
}

/// 1, 47, 50, S1, S3: a clean session's head lands and its submit exits
/// 0; a failing one exits 1, and the branch holds only what passed.
#[test]
fn wt_submit_lands_or_fails_by_its_exit() {
    let s = Scratch::new("wt-outcomes");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let out = s.wt_submit(&a, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(text(&out).contains("landed"), "{}", text(&out));
    assert_eq!(s.repo.git(&["show", "bolt/x:a.txt"]), "a");
    // C.4: the driver removes the landed session's worktree, as `wt
    // merge` does; the core never does (25, S11).
    assert!(!a.exists(), "{}", text(&out));
    // Told not to, it is kept.
    let k = s.session("k", &[("k.txt", "k\n")]);
    let out = s.wt_submit(&k, &["--no-remove"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(k.join("k.txt").exists());
    let landed = s.tip();

    let b = s.session("b", &[("broken", "x\n")]);
    let out = s.wt_submit(&b, &[]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("sh check.sh"), "{}", text(&out));
    assert_eq!(s.tip(), landed);
    // Every place sluice made is gone (34).
    assert!(!s.repo.git(&["worktree", "list"]).contains("sluice/.places"));
}

/// C.4: a worktree goes only once sluice records its head landed. Asking
/// for help, or arguments sluice refuses, exit as sluice does and keep
/// it, though `--help` exits 0.
#[test]
fn only_a_landing_removes_the_worktree() {
    let s = Scratch::new("wt-help");
    let a = s.session("a", &[("a.txt", "a\n")]);
    for asks in [&["--help"][..], &["-h"]] {
        let out = s
            .command("wt", &a)
            .arg("submit")
            .args(asks)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{asks:?}: {}", text(&out));
        assert!(a.join("a.txt").exists(), "{asks:?}: {}", text(&out));
    }
    let out = s.wt_submit(&a, &["--help"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(text(&out).contains("Usage"), "{}", text(&out));
    assert!(a.join("a.txt").exists(), "{}", text(&out));
    let out = s.wt_submit(&a, &["--version"]);
    assert_eq!(out.status.code(), Some(sluice::USAGE), "{}", text(&out));
    assert!(a.join("a.txt").exists(), "{}", text(&out));
    let out = s.wt_submit(&a, &["--no-such-flag"]);
    assert_eq!(out.status.code(), Some(sluice::USAGE), "{}", text(&out));
    assert!(a.join("a.txt").exists(), "{}", text(&out));
    assert_eq!(
        s.repo.git(&["log", "-1", "--format=%s", "bolt/x"]),
        "sluice's policy"
    );
}

/// 12, S2: a head whose content conflicts with what landed exits 2.
#[test]
fn wt_submit_conflicts_with_exit_2() {
    let s = Scratch::new("wt-conflict");
    let a = s.session("a", &[("same.txt", "a\n")]);
    let c = s.session("c", &[("same.txt", "c\n")]);
    assert_eq!(s.wt_submit(&a, &[]).status.code(), Some(0));
    let out = s.wt_submit(&c, &[]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("same.txt"), "{}", text(&out));
}

/// 2: a worktree holding anything beyond its commits is refused, every
/// path named, nothing submitted.
#[test]
fn a_dirty_worktree_is_refused_naming_each_path() {
    let s = Scratch::new("wt-dirty");
    let a = s.session("a", &[("a.txt", "a\n"), ("b.txt", "b\n")]);
    std::fs::write(a.join("a.txt"), "changed\n").unwrap();
    std::fs::create_dir_all(a.join("sub")).unwrap();
    std::fs::write(a.join("sub/new.txt"), "n\n").unwrap();
    std::fs::write(a.join("sp ace.txt"), "s\n").unwrap();
    s.repo
        .git(&["-C", a.to_str().unwrap(), "mv", "b.txt", "b-renamed.txt"]);
    let out = s.wt_submit(&a, &[]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let said = text(&out);
    for p in [
        "a.txt",
        "sub/new.txt",
        "sp ace.txt",
        "b-renamed.txt <- b.txt",
    ] {
        assert!(said.contains(p), "{p} not named: {said}");
    }
    assert!(!s.repo.dir().join(".git/sluice/bolt/x").exists());
}

/// 3: a detached worktree names no session's work, and is refused.
#[test]
fn a_detached_worktree_is_refused() {
    let s = Scratch::new("wt-detached");
    let a = s.session("a", &[("a.txt", "a\n")]);
    s.repo
        .git(&["-C", a.to_str().unwrap(), "switch", "-q", "--detach"]);
    let out = s.wt_submit(&a, &[]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("detached"), "{}", text(&out));
}

/// 47, 51: `--no-wait` returns at once; rounds run later land it, and
/// `sluice status` reads the outcome.
#[test]
fn no_wait_returns_and_status_reads_the_outcome() {
    let s = Scratch::new("wt-nowait");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let out = s.wt_submit(&a, &["--no-wait", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let n = id(&out);
    assert_eq!(s.status(n)["state"], "queued");
    let until = Instant::now() + Duration::from_secs(60);
    while s.status(n)["state"] != "landed" {
        assert!(Instant::now() < until, "{}", s.status(n));
        let out = s.sluice(&["step", "bolt/x"]);
        assert!(out.status.success(), "{}", text(&out));
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// 64, 47a: `--check` submits nothing and moves nothing.
#[test]
fn check_writes_nothing() {
    let s = Scratch::new("wt-check");
    let a = s.session("a", &[("same.txt", "a\n")]);
    let c = s.session("c", &[("same.txt", "c\n")]);
    let tip = s.tip();
    let out = s.wt_submit(&a, &["--check"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(s.wt_submit(&a, &["--no-wait"]).status.code(), Some(0));
    let out = s.wt_submit(&c, &["--check"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("same.txt"), "{}", text(&out));
    assert_eq!(s.tip(), tip);
    let status = s.sluice(&["status", "bolt/x", "--json", "--all"]);
    let v: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(v["submissions"].as_array().unwrap().len(), 1, "{v}");
}

/// 3: who submitted is `SLUICE_AGENT`, else the branch.
#[test]
fn the_submitter_is_the_agent_or_the_branch() {
    let s = Scratch::new("wt-agent");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let b = s.session("b", &[("b.txt", "b\n")]);
    let one = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    let out = s
        .command("wt", &b)
        .args(["submit", "bolt/x", "--no-wait", "--json"])
        .env("SLUICE_AGENT", "crw-1-unit-1")
        .output()
        .unwrap();
    let two = id(&out);
    assert_eq!(s.status(one)["from"]["agent"], "unit/a");
    assert_eq!(s.status(one)["from"]["worktree"], "unit/a");
    assert_eq!(s.status(two)["from"]["agent"], "crw-1-unit-1");
    assert_eq!(s.status(two)["from"]["worktree"], "unit/b");
}

/// What [`name_a_branchs_submitter`] prints its answer after.
const NAMED: &str = "named: ";

/// The child of [`named_records_no_worktree`]: the worktrunk driver's
/// submitter of the branch `SLUICE_TEST_NAMED` names, on the host
/// `box-1`, as JSON, in the folder and the session its parent gave it.
/// Run any other way, it does nothing.
#[test]
fn name_a_branchs_submitter() {
    let Ok(branch) = std::env::var("SLUICE_TEST_NAMED") else {
        return;
    };
    let from = Worktrunk::new("box-1").named(&branch).unwrap();
    println!("{NAMED}{}", serde_json::to_string(&from).unwrap());
}

/// 124: a branch submitted by name records no worktree and no session.
/// Run in a herdr pane, in a worktree on another branch holding work
/// beyond its commits, with no git on the path that answers, the driver
/// names the harness's agent or else the branch, the branch as its work
/// and its head, its host, and no path or pane (record 0090; rule
/// core.50).
#[test]
fn named_records_no_worktree() {
    let s = Scratch::new("wt-named");
    let a = s.session("a", &[("a.txt", "a\n")]);
    std::fs::write(a.join("a.txt"), "changed\n").unwrap();
    std::fs::write(a.join("held.txt"), "held\n").unwrap();
    // A git that keeps each call and answers none, first on the path.
    let git = s.repo.dir().with_extension("named-git");
    std::fs::create_dir_all(&git).unwrap();
    let calls = git.join("calls");
    std::fs::write(
        git.join("git"),
        format!("#!/bin/sh\necho \"$*\" >> {}\nexit 1\n", calls.display()),
    )
    .unwrap();
    std::fs::set_permissions(
        git.join("git"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let socket = s.repo.dir().with_extension("herdr.sock");
    let named = |agent: Option<&str>| -> Submitter {
        let mut c = s.command_under(Command::new(std::env::current_exe().unwrap()), &a);
        c.args(["--exact", "name_a_branchs_submitter", "--nocapture"])
            .env("SLUICE_TEST_NAMED", "bolt/x")
            .env(
                "PATH",
                format!("{}:{}", git.display(), std::env::var("PATH").unwrap()),
            );
        for v in support::HERDR {
            c.env(v, "set");
        }
        c.env("HERDR_PANE_ID", "w1:p2")
            .env("HERDR_SOCKET_PATH", &socket);
        if let Some(agent) = agent {
            c.env("SLUICE_AGENT", agent);
        }
        let out = c.output().unwrap();
        assert!(out.status.success(), "{}", text(&out));
        let answer = String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|l| l.strip_prefix(NAMED).map(str::to_string))
            .unwrap_or_else(|| panic!("no answer: {}", text(&out)));
        serde_json::from_str(&answer).unwrap()
    };

    let from = named(None);
    assert_eq!(
        from,
        Submitter {
            agent: "bolt/x".into(),
            worktree: "bolt/x".into(),
            path: None,
            head: Some("bolt/x".into()),
            host: Some("box-1".into()),
            session: None,
        }
    );
    assert!(from.by_name());
    let from = named(Some("flume-base-1"));
    assert_eq!(from.agent, "flume-base-1");
    assert_eq!(
        (from.worktree.as_str(), from.head.as_deref()),
        ("bolt/x", Some("bolt/x"))
    );
    assert_eq!((from.path, from.session), (None, None));
    assert!(
        !calls.exists(),
        "git ran: {}",
        std::fs::read_to_string(&calls).unwrap()
    );
}

/// 51: a submit in a herdr pane records the pane on its server, and the
/// host as its peers name it; one in no pane records no session.
#[test]
fn a_submit_records_the_pane_it_ran_in_and_its_host() {
    let s = Scratch::new("wt-pane");
    std::fs::write(
        s.config.join("sluice/config.toml"),
        "[runs]\nmax = 2\n\n[peers]\nname = \"box-1\"\n",
    )
    .unwrap();
    let a = s.session("a", &[("a.txt", "a\n")]);
    let b = s.session("b", &[("b.txt", "b\n")]);
    let socket = s.repo.dir().with_extension("herdr.sock");
    let submit = |dir: &Path| {
        let mut c = s.command("wt", dir);
        c.args(["submit", "bolt/x", "--no-wait", "--json"]);
        c
    };
    let one = id(&submit(&a)
        .env("HERDR_PANE_ID", "w1:p2")
        .env("HERDR_SOCKET_PATH", &socket)
        .output()
        .unwrap());
    let from = &s.status(one)["from"];
    assert_eq!(
        from["session"],
        format!("herdr:w1:p2@{}", socket.display()).as_str(),
        "{from}"
    );
    assert_eq!(from["host"], "box-1");

    let two = id(&submit(&b)
        .env_remove("HERDR_PANE_ID")
        .env_remove("HERDR_SOCKET_PATH")
        .output()
        .unwrap());
    let from = &s.status(two)["from"];
    assert!(from.get("session").is_none(), "{from}");
    assert_eq!(from["host"], "box-1");
}

/// 51: a test's sluice is in no pane, whatever pane its parent is in.
#[test]
fn a_tests_submit_records_no_pane_its_parent_is_in() {
    let s = Scratch::new("wt-isolated");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let mut parent = Command::new("wt");
    for v in support::HERDR {
        parent.env(v, "set");
    }
    parent.env("HERDR_PANE_ID", "w1:p2");
    let out = s
        .command_under(parent, &a)
        .args(["submit", "bolt/x", "--no-wait", "--json"])
        .output()
        .unwrap();
    let from = &s.status(id(&out))["from"];
    assert!(from.get("session").is_none(), "{from}");
}

impl Scratch {
    /// A `--no-wait` submit from `dir` in herdr pane `pane`, on the
    /// scratch's own herdr socket; its number and the session it records.
    fn submit_from_pane(&self, dir: &Path, pane: &str) -> (u64, String) {
        let socket = self.repo.dir().with_extension("herdr.sock");
        let out = self
            .command("wt", dir)
            .args(["submit", "bolt/x", "--no-wait", "--json"])
            .env("HERDR_PANE_ID", pane)
            .env("HERDR_SOCKET_PATH", &socket)
            .output()
            .unwrap();
        (id(&out), format!("herdr:{pane}@{}", socket.display()))
    }

    /// Rounds of `bolt/x` until submission `id` is decided, at most a
    /// minute, and one more.
    fn decide(&self, id: u64) -> serde_json::Value {
        let until = Instant::now() + Duration::from_secs(60);
        loop {
            let out = self.sluice(&["step", "bolt/x"]);
            assert!(out.status.success(), "{}", text(&out));
            let v = self.status(id);
            if ["landed", "failed", "conflict"].contains(&v["state"].as_str().unwrap_or("")) {
                break;
            }
            assert!(Instant::now() < until, "{v}");
            std::thread::sleep(Duration::from_millis(200));
        }
        let out = self.sluice(&["step", "bolt/x"]);
        assert!(out.status.success(), "{}", text(&out));
        self.status(id)
    }

    /// The branch's log lines of `event`.
    fn logged(&self, event: &str) -> Vec<serde_json::Value> {
        let log = std::fs::read_to_string(self.repo.dir().join(".git/sluice/bolt/x/log.jsonl"))
            .unwrap_or_default();
        log.lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .filter(|v| v["event"] == event)
            .collect()
    }
}

/// 50, 51; outcome-delivery spec, "A landing is pushed into the
/// submitting pane" and "A notice taken into the pane": a landing
/// submitted from a pane is typed into that pane once, on the server the
/// submit ran under, ending with the key it is confirmed by, and is handed
/// over by `notice` to the pane, staying owed (record 0102, D10).
#[test]
fn a_landing_is_pushed_into_the_pane_it_was_submitted_from() {
    let s = Scratch::new("wt-push");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let (n, session) = s.submit_from_pane(&a, "w1:p2");
    let v = s.decide(n);
    assert_eq!(v["state"], "landed", "{v}");
    let landed = s.tip();
    let calls = s.herdr.calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(
        calls[0][0],
        format!(
            "call <agent> <prompt> <w1:p2> <[sluice] #{n} landed on bolt/x as {}. \
             sluice confirm {n}:landed once you have read it.>",
            &landed[..7]
        )
    );
    let socket = session.split_once('@').unwrap().1;
    assert_eq!(calls[0][1], format!("socket {socket}"));
    assert_eq!(calls[0][2], "pane unset");

    let out = s.sluice(&["wait", &n.to_string(), "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v.get("delivered").is_none(), "{v}");
    assert_eq!(v["delivery"]["state"], "owed", "{v}");
    assert_eq!(v["delivery"]["handed"][0]["by"], "notice", "{v}");
    let handed = s.logged("handed");
    assert_eq!(handed.len(), 1, "{handed:?}");
    assert_eq!(handed[0]["to"], session.as_str());
}

/// 50; outcome-delivery spec, "A failure is pushed with its failed checks":
/// a blamed submission's line names its failure, the check and the key it
/// is confirmed by.
#[test]
fn a_failure_is_pushed_with_its_check() {
    let s = Scratch::new("wt-push-failed");
    let b = s.session("b", &[("broken", "x\n")]);
    let (n, session) = s.submit_from_pane(&b, "w1:p3");
    let v = s.decide(n);
    assert_eq!(v["state"], "failed", "{v}");
    let calls = s.herdr.calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    let line = &calls[0][0];
    assert!(
        line.starts_with(&format!(
            "call <agent> <prompt> <w1:p3> <[sluice] #{n} failed on bolt/x: "
        )),
        "{line}"
    );
    assert!(line.contains("sh check.sh"), "{line}");
    assert!(
        line.contains(&format!("sluice confirm {n}:failed")),
        "{line}"
    );
    let handed = s.logged("handed");
    assert_eq!(handed.len(), 1, "{handed:?}");
    assert_eq!(handed[0]["to"], session.as_str());
}

/// 51; outcome-delivery spec, "A refused prompt records nothing": a prompt
/// herdr refuses records no hand-over, and the log says why once, in
/// herdr's words; no later round pushes it again.
#[test]
fn a_refused_prompt_leaves_the_outcome_pending() {
    let s = Scratch::new("wt-push-refused");
    s.herdr.refuses(
        1,
        r#"{"error":{"code":"agent_blocked","message":"agent is blocked"}}"#,
    );
    let a = s.session("a", &[("a.txt", "a\n")]);
    let (n, _) = s.submit_from_pane(&a, "w1:p2");
    let v = s.decide(n);
    assert_eq!(v["state"], "landed", "{v}");
    assert!(handed(&s, n).is_empty(), "{v}");
    for _ in 0..3 {
        let out = s.sluice(&["step", "bolt/x"]);
        assert!(out.status.success(), "{}", text(&out));
    }
    assert_eq!(s.herdr.calls().len(), 1, "never pushed again");
    let pending = s.logged("pending");
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert_eq!(pending[0]["id"], n);
    assert_eq!(pending[0]["why"], "agent is blocked");
}

/// 51; outcome-delivery spec, "A submission outside any multiplexer": a
/// submit in no pane lands with nothing pushed, owed with no hand-over, and
/// the log says once that it is pending.
#[test]
fn a_submit_in_no_pane_is_pushed_nothing() {
    let s = Scratch::new("wt-push-none");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let n = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    let v = s.decide(n);
    assert_eq!(v["state"], "landed", "{v}");
    assert!(s.herdr.calls().is_empty());
    assert_eq!(s.status(n)["delivery"]["state"], "owed");
    assert!(handed(&s, n).is_empty(), "{v}");
    let pending = s.logged("pending");
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert_eq!(pending[0]["id"], n);
    assert_eq!(pending[0]["why"], "no session recorded");
}

/// 51: a round on another host than the one a submission recorded types
/// into no pane of its own.
#[test]
fn a_submission_from_another_host_is_not_pushed() {
    let s = Scratch::new("wt-push-host");
    let peers = |name: &str| {
        std::fs::write(
            s.config.join("sluice/config.toml"),
            format!("[runs]\nmax = 2\n\n[peers]\nname = \"{name}\"\n"),
        )
        .unwrap();
    };
    peers("box-1");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let (n, _) = s.submit_from_pane(&a, "w1:p2");
    peers("box-2");
    // The host process the submit started keeps box-1's settings until it
    // ends; the next command's reads box-2's (135; rule core.54).
    support::kill_host_process(&mut s.command(env!("CARGO_BIN_EXE_sluice"), s.repo.dir()));
    let v = s.decide(n);
    assert_eq!(v["state"], "landed", "{v}");
    assert!(s.herdr.calls().is_empty());
    assert!(handed(&s, n).is_empty(), "{v}");
    let pending = s.logged("pending");
    assert!(
        pending[0]["why"].as_str().unwrap().contains("box-1"),
        "{pending:?}"
    );
}

/// 51: a round run from another pane reaches the server the submission
/// recorded, and carries none of its own pane.
#[test]
fn a_round_in_another_pane_reaches_the_recorded_server() {
    let s = Scratch::new("wt-push-other");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let (n, session) = s.submit_from_pane(&a, "w1:p2");
    let other = s.repo.dir().with_extension("other.sock");
    let until = Instant::now() + Duration::from_secs(60);
    while s.herdr.calls().is_empty() {
        assert!(Instant::now() < until, "{}", s.status(n));
        let out = s
            .command(env!("CARGO_BIN_EXE_sluice"), s.repo.dir())
            .args(["step", "bolt/x"])
            .env("HERDR_PANE_ID", "w9:p9")
            .env("HERDR_SOCKET_PATH", &other)
            .env("HERDR_SESSION", "other")
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        std::thread::sleep(Duration::from_millis(200));
    }
    let calls = s.herdr.calls();
    let socket = session.split_once('@').unwrap().1;
    assert!(calls[0][0].contains("<w1:p2>"), "{calls:?}");
    assert_eq!(calls[0][1], format!("socket {socket}"));
    assert_eq!(calls[0][2], "pane unset");
}

/// A submission names the worktree it came from by its path, and the
/// overview lists it, read from the records with no round run.
#[test]
fn a_submission_names_its_worktrees_path_and_the_overview_shows_it() {
    let s = Scratch::new("wt-path");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let one = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    let path = std::fs::canonicalize(&a).unwrap();
    assert_eq!(s.status(one)["from"]["path"], path.to_str().unwrap());

    let out = s.sluice(&["overview", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema"], "sluice/overview@1");
    let b = &v["branches"][0];
    assert_eq!(b["branch"], "bolt/x");
    assert_eq!(b["depth"], 1);
    assert_eq!(b["submissions"][0]["id"], one);
    assert_eq!(b["submissions"][0]["from"]["path"], path.to_str().unwrap());
    let common = std::fs::canonicalize(s.repo.dir().join(".git")).unwrap();
    assert_eq!(v["repository"], common.to_str().unwrap());
}

/// 67: a second submit from one worktree withdraws the first.
#[test]
fn a_resubmission_withdraws_the_first() {
    let s = Scratch::new("wt-again");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let one = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    std::fs::write(a.join("a.txt"), "a2\n").unwrap();
    s.repo
        .git(&["-C", a.to_str().unwrap(), "commit", "-qam", "again"]);
    let two = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    assert_ne!(one, two);
    assert_eq!(s.status(one)["state"], "withdrawn");
}

/// Record 0065, C.5 worktrunk: run again after a `--no-wait` submit,
/// `wt submit` makes no submission and withdraws none; it follows the one
/// standing to its landing, says so, then releases and removes the
/// worktree.
#[test]
fn run_again_follows_its_submission_and_removes_on_landing() {
    let (s, log) = Scratch::releasing("wt-again-lands");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &a)
        .args(["exec", "bolt/x"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let one = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    let a_canon = std::fs::canonicalize(&a).unwrap();
    let out = s.wt_submit(&a, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(&format!(
            "wt submit: following submission {one}, already submitted at this revision"
        )),
        "{}",
        text(&out)
    );
    // Its landing, then the release's own line.
    let tip = s.tip();
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .contains(&format!("passed · landed as {}\n", &tip[..7])),
        "{}",
        text(&out)
    );
    let subs = s.subs_from("a");
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0]["id"], one);
    assert_eq!(s.status(one)["state"], "landed");
    assert_eq!(released(&log, &a_canon), 1, "{}", text(&out));
    assert!(!a.exists(), "{}", text(&out));
    let _ = std::fs::remove_file(&log);
}

/// Already landed: a run again runs no round, which would start the
/// submission queued behind it, and releases and removes the worktree.
#[test]
fn run_again_once_landed_removes_at_once() {
    let (s, log) = Scratch::releasing("wt-again-landed");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &a)
        .args(["exec", "bolt/x"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let one = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    s.land(one);
    let b = s.session("b", &[("b.txt", "b\n")]);
    let two = id(&s.wt_submit(&b, &["--no-wait", "--json"]));
    let a_canon = std::fs::canonicalize(&a).unwrap();
    let out = s.wt_submit(&a, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(s.status(two)["state"], "queued", "{}", text(&out));
    assert_eq!(s.subs_from("a").len(), 1);
    assert_eq!(released(&log, &a_canon), 1, "{}", text(&out));
    assert!(!a.exists(), "{}", text(&out));
    let _ = std::fs::remove_file(&log);
}

/// Landed, kept: a run again under `--no-remove` exits 0 and neither
/// releases nor removes the worktree.
#[test]
fn run_again_once_landed_keeps_under_no_remove() {
    let (s, log) = Scratch::releasing("wt-again-kept");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &a)
        .args(["exec", "bolt/x"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let one = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    s.land(one);
    let out = s.wt_submit(&a, &["--no-remove"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(a.join("a.txt").exists(), "{}", text(&out));
    assert_eq!(released(&log, &a), 0, "released under --no-remove");
    assert_eq!(s.subs_from("a").len(), 1);
    let _ = std::fs::remove_file(&log);
}

/// A first submit under a time limit: `wt submit --timeout` submits
/// without waiting, then follows; out of time it exits 5 with only that
/// submission's record on stdout and keeps the worktree. A later run makes
/// no new submission, exits 0 once it lands, and the worktree is gone.
#[test]
fn a_first_submit_under_a_time_limit_submits_then_follows() {
    let s = Scratch::new("wt-first-timeout");
    let a = s.session("a", &[("slow", "x\n")]);
    let out = s.wt_submit(&a, &["--timeout", "2s", "--json"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let record: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("no JSON: {}", text(&out)));
    let subs = s.subs_from("a");
    assert_eq!(subs.len(), 1, "{subs:?}");
    let one = subs[0]["id"].as_u64().unwrap();
    assert_eq!(record["id"], one, "{record}");
    assert!(
        record["state"] == "queued" || record["state"] == "testing",
        "{record}"
    );
    assert!(a.join("slow").exists(), "{}", text(&out));

    let out = s.wt_submit(&a, &["--timeout", "9m", "--json"]);
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    let record: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("no JSON: {}", text(&out)));
    assert_eq!(record["id"], one, "{record}");
    assert_eq!(record["state"], "landed", "{record}");
    assert_eq!(s.subs_from("a").len(), 1);
    assert!(!a.exists(), "{}", text(&out));
}

/// Blamed while followed: a run again exits 1, as `sluice wait` does, and
/// the worktree stays.
#[test]
fn run_again_blamed_keeps_the_worktree() {
    let s = Scratch::new("wt-again-blamed");
    let b = s.session("b", &[("broken", "x\n")]);
    let one = id(&s.wt_submit(&b, &["--no-wait", "--json"]));
    let out = s.wt_submit(&b, &["--timeout", "9m"]);
    assert_eq!(out.status.code(), Some(sluice::FAILED), "{}", text(&out));
    assert!(
        text(&out).contains(&format!("following submission {one}")),
        "{}",
        text(&out)
    );
    assert_eq!(s.status(one)["state"], "failed");
    assert_eq!(s.subs_from("b").len(), 1);
    assert!(b.join("broken").exists(), "{}", text(&out));
}

/// Blamed, the same head submitted again: nothing stands, so a run again
/// is a new submission of that head, tested again (67).
#[test]
fn run_again_after_a_blame_submits_the_same_head() {
    let s = Scratch::new("wt-again-retried");
    let b = s.session("b", &[("broken", "x\n")]);
    let out = s.wt_submit(&b, &[]);
    assert_eq!(out.status.code(), Some(sluice::FAILED), "{}", text(&out));
    let out = s.wt_submit(&b, &[]);
    assert_eq!(out.status.code(), Some(sluice::FAILED), "{}", text(&out));
    assert!(!text(&out).contains("following"), "{}", text(&out));
    let subs = s.subs_from("b");
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[0]["head"], subs[1]["head"], "{subs:?}");
    assert_eq!(subs[1]["state"], "failed", "{subs:?}");
    assert!(b.join("broken").exists());
}

/// `--timeout` bounds a wait: with `--no-wait` it exits 64, saying so,
/// and submits nothing.
#[test]
fn a_time_limit_without_waiting_is_refused() {
    let s = Scratch::new("wt-nowait-timeout");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let out = s.wt_submit(&a, &["--no-wait", "--timeout", "9m"]);
    assert_eq!(out.status.code(), Some(sluice::USAGE), "{}", text(&out));
    assert!(
        text(&out).contains("--timeout is for a run that waits"),
        "{}",
        text(&out)
    );
    assert!(s.subs_from("a").is_empty());
    assert!(a.join("a.txt").exists());
}

/// Record 0065: a `--no-wait` run again where a submission stands answers
/// with its record and exits 0, submitting nothing and withdrawing nothing.
#[test]
fn a_no_wait_run_again_answers_with_what_stands() {
    let s = Scratch::new("wt-nowait-again");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let one = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    let out = s.wt_submit(&a, &["--no-wait", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let record: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("no JSON: {}", text(&out)));
    assert_eq!(record["id"], one, "{record}");
    assert_eq!(record["state"], "queued", "{record}");
    let subs = s.subs_from("a");
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(s.status(one)["state"], "queued");
}

/// Landed, then a `--no-wait` run again: it answers with the `landed`
/// record and keeps the worktree, since only a run that waits removes.
#[test]
fn a_no_wait_run_again_once_landed_keeps_the_worktree() {
    let s = Scratch::new("wt-nowait-landed");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let one = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    s.land(one);
    let out = s.wt_submit(&a, &["--no-wait", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let record: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("no JSON: {}", text(&out)));
    assert_eq!(record["id"], one, "{record}");
    assert_eq!(record["state"], "landed", "{record}");
    assert_eq!(s.subs_from("a").len(), 1);
    assert!(a.join("a.txt").exists(), "{}", text(&out));
}

/// Only a new head submits again: a run naming `--after` where a
/// submission stands exits 64, names `sluice amend <id>`, and withdraws
/// nothing.
#[test]
fn a_place_in_the_queue_where_one_stands_is_refused() {
    let s = Scratch::new("wt-again-after");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let b = s.session("b", &[("b.txt", "b\n")]);
    let one = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    let two = id(&s.wt_submit(&b, &["--no-wait", "--json"]));
    let out = s.wt_submit(&a, &["--no-wait", "--after", &two.to_string()]);
    assert_eq!(out.status.code(), Some(sluice::USAGE), "{}", text(&out));
    assert!(
        text(&out).contains(&format!("sluice amend {one}")),
        "{}",
        text(&out)
    );
    assert_eq!(s.status(one)["state"], "queued");
    assert_eq!(s.subs_from("a").len(), 1);
}

/// Where sluice's status cannot be read, a run that would submit submits
/// nothing and exits 3 with sluice's words: a submit at the revision
/// standing would withdraw it (67).
#[test]
fn an_unread_status_submits_nothing() {
    let s = Scratch::new("wt-unread");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let one = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    let version = s.repo.dir().join(".git/sluice/bolt/x/version");
    let kept = std::fs::read(&version).unwrap();
    std::fs::write(&version, "unreadable\n").unwrap();
    let out = s.wt_submit(&a, &["--timeout", "9m"]);
    assert_eq!(out.status.code(), Some(sluice::INTERNAL), "{}", text(&out));
    assert!(
        text(&out).contains("could not read whether a submission stands, so nothing was submitted"),
        "{}",
        text(&out)
    );
    std::fs::write(&version, kept).unwrap();
    assert_eq!(s.status(one)["state"], "queued");
    assert_eq!(s.subs_from("a").len(), 1);
    assert!(a.join("a.txt").exists());
}

/// A commit made while a run again follows: the submission lands, the
/// run exits 0, says sluice records no landing of the worktree's head,
/// and keeps it.
#[test]
fn a_commit_while_followed_keeps_the_worktree() {
    let s = Scratch::new("wt-again-commit");
    let a = s.session("a", &[("slow", "x\n")]);
    let one = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    let child = s
        .command("wt", &a)
        .args(["submit", "bolt/x", "--timeout", "9m"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Its run started, so the read of what stands is behind it.
    let runs = s.repo.dir().join(".git/sluice/.runs");
    let until = Instant::now() + Duration::from_secs(30);
    while std::fs::read_dir(&runs).map_or(true, |mut d| d.next().is_none()) {
        assert!(Instant::now() < until, "no run started");
        std::thread::sleep(Duration::from_millis(50));
    }
    std::fs::write(a.join("more.txt"), "more\n").unwrap();
    s.repo.git(&["-C", a.to_str().unwrap(), "add", "-A"]);
    s.repo
        .git(&["-C", a.to_str().unwrap(), "commit", "-qm", "more"]);
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    assert_eq!(s.status(one)["state"], "landed");
    assert!(
        text(&out).contains("sluice records no landing of this worktree's head, so it was kept"),
        "{}",
        text(&out)
    );
    assert!(a.join("more.txt").exists(), "{}", text(&out));
}

/// Record 0065, 25: `sluice wait` is the core's, and removes no worktree.
/// A worktree that ran `sluice exec` submits with `wt submit --no-wait`,
/// and `sluice wait <id>` follows it to its landing: it exits 0, and the
/// worktree is still there at its head, its ports still held.
#[test]
fn sluice_wait_removes_nothing() {
    let policy = format!("{POLICY}\n[checkout]\nports = [\"web\"]\nports_from = \"41600\"\n");
    let s = Scratch::with_policy("wt-wait-keeps", &policy);
    let a = s.session("a", &[("a.txt", "a\n")]);
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &a)
        .args(["exec", "bolt/x"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let canon = std::fs::canonicalize(&a).unwrap();
    let held = || {
        let out = s.sluice(&["status", "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        doc["worktrees"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|w| w["worktree"] == canon.to_str().unwrap())
            .cloned()
            .unwrap_or_else(|| panic!("not held: {doc}"))
    };
    let ports = held()["ports"].clone();
    assert!(ports["SLUICE_PORT_WEB"].is_string(), "{ports}");
    let head = s
        .repo
        .git(&["-C", a.to_str().unwrap(), "rev-parse", "HEAD"]);
    let one = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &a)
        .args(["wait", &one.to_string()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    assert_eq!(s.status(one)["state"], "landed");
    assert!(a.join("a.txt").exists(), "{}", text(&out));
    assert_eq!(
        s.repo
            .git(&["-C", a.to_str().unwrap(), "rev-parse", "HEAD"]),
        head
    );
    let after = held();
    assert_eq!(after["ports"], ports, "{after}");
    assert_eq!(after["removed"], false, "{after}");
}

/// 5, 60: Ctrl-C to a waiting submit ends the wait alone: the submission
/// stays queued and its run goes on, and later rounds land it.
#[test]
fn ctrl_c_ends_only_the_wait() {
    let s = Scratch::new("wt-interrupt");
    let a = s.session("a", &[("a.txt", "a\n")]);
    // A terminal's foreground group: wt and the sluice it runs.
    let child = s
        .command("wt", &a)
        .args(["submit", "bolt/x"])
        .env("SLOW", "3")
        .stdout(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    // Wait for its run to start.
    let runs = s.repo.dir().join(".git/sluice/.runs");
    let until = Instant::now() + Duration::from_secs(30);
    while std::fs::read_dir(&runs).map_or(true, |mut d| d.next().is_none()) {
        assert!(Instant::now() < until, "no run started");
        std::thread::sleep(Duration::from_millis(50));
    }
    // Ctrl-C: SIGINT to the whole foreground group.
    nix::sys::signal::killpg(
        nix::unistd::Pid::from_raw(child.id() as i32),
        nix::sys::signal::Signal::SIGINT,
    )
    .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("still waiting: sluice wait 1"),
        "{}",
        text(&out)
    );
    assert_eq!(s.status(1)["state"], "testing");
    let until = Instant::now() + Duration::from_secs(60);
    while s.status(1)["state"] != "landed" {
        assert!(Instant::now() < until, "{}", s.status(1));
        s.sluice(&["step", "bolt/x"]);
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Session-runs-1 clause 95, C.5 worktrunk: a worktree that ran `sluice
/// exec` is released before worktrunk removes it on landing, and left
/// unreleased under `--no-remove`. The release command writes the checkout
/// it ran for while that checkout still exists: the combination's own
/// checkout is released as well, as every place sluice made is.
#[test]
fn wt_submit_releases_before_it_removes() {
    let (s, log) = Scratch::releasing("wt-release");
    let released = |dir: &Path| released(&log, dir);
    let a = s.session("a", &[("a.txt", "a\n")]);
    let a_canon = std::fs::canonicalize(&a).unwrap();
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &a)
        .args(["exec", "bolt/x"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let out = s.wt_submit(&a, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(released(&a_canon), 1, "{}", text(&out));
    assert!(!a.exists(), "{}", text(&out));

    let k = s.session("k", &[("k.txt", "k\n")]);
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &k)
        .args(["exec", "bolt/x"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let out = s.wt_submit(&k, &["--no-remove"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(released(&k), 0, "released under --no-remove");
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &k)
        .arg("release")
        .output()
        .unwrap();
    assert!(text(&out).starts_with("released "), "{}", text(&out));
    assert_eq!(released(&k), 1);
    let _ = std::fs::remove_file(&log);
}

/// 51; outcome-delivery spec, "wt submit removes a landed worktree" and
/// "wt submit reports a blame": a waiting `wt submit` hands its landing
/// over by `wait` and confirms it on receipt, since it removes the
/// worktree, whose reader goes with it; a blame it reports, keeping the
/// worktree, is handed over and stays owed (record 0102, design D5).
#[test]
fn wt_submit_confirms_a_landing_it_removes_the_worktree_of() {
    let s = Scratch::new("wt-confirms-landing");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let out = s.wt_submit(&a, &[]);
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    assert!(!a.exists(), "{}", text(&out));
    let n = s.subs_from("a")[0]["id"].as_u64().unwrap();
    let key = format!("{n}:landed");
    let of = |event: &str| -> Vec<serde_json::Value> {
        s.logged(event)
            .into_iter()
            .filter(|l| l["key"] == key.as_str())
            .collect()
    };
    let handed_lines = of("handed");
    assert_eq!(handed_lines.len(), 1, "{handed_lines:?}");
    assert_eq!(handed_lines[0]["by"], "wait");
    let confirmed = of("confirmed");
    assert_eq!(confirmed.len(), 1, "{confirmed:?}");
    assert_eq!(confirmed[0]["by"], "wait");
    assert_eq!(confirmed[0]["reader"], "unit/a");
    assert_eq!(s.status(n)["delivery"]["state"], "done");

    // A blame it reports keeps the worktree, and stays owed.
    let b = s.session("b", &[("broken", "x\n")]);
    let out = s.wt_submit(&b, &[]);
    assert_eq!(out.status.code(), Some(sluice::FAILED), "{}", text(&out));
    assert!(b.exists());
    let m = s.subs_from("b")[0]["id"].as_u64().unwrap();
    assert_eq!(s.status(m)["delivery"]["state"], "owed");
    assert_eq!(handed(&s, m), ["wait"]);
}

/// 51, S77; owed-outcomes spec, "An expired blame of a removed worktree"
/// and "One owed landing": a branch's status shows, after its queue, what
/// its readers are owed. On a host whose span is a second, a blame the
/// round pushed into its pane, of a worktree removed before it confirmed
/// anything, is shown under its reader marked `never confirmed`, expired,
/// with its hand-over by `notice`; under `--json`, a landing handed over
/// by `inbox` to a worktree that confirmed an earlier outcome is its
/// reader's one delivery, owed, and the reader known; nothing owed shows
/// no section (weir 104 to 106; record 0102).
#[test]
fn status_shows_an_expired_blame_with_its_reader_never_confirmed() {
    let s = Scratch::new("wt-owed-expired");
    std::fs::write(
        s.config.join("sluice/config.toml"),
        "[runs]\nmax = 2\n\n[deliveries]\nexpire_after = \"1s\"\n",
    )
    .unwrap();
    let b = s.session("b", &[("broken", "x\n")]);
    let (n, _) = s.submit_from_pane(&b, "w1:p3");
    assert_eq!(s.decide(n)["state"], "failed");
    assert_eq!(handed(&s, n), ["notice"]);
    s.repo
        .git(&["worktree", "remove", "--force", b.to_str().unwrap()]);
    std::thread::sleep(Duration::from_millis(1500));

    let out = s.sluice(&["status", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    let said = text(&out);
    let owed = said
        .split_once("OWED\n")
        .unwrap_or_else(|| panic!("no OWED section: {said}"))
        .1;
    let mut lines = owed.lines();
    assert_eq!(lines.next(), Some("  unit/b · never confirmed"), "{said}");
    let row = lines.next().unwrap_or_default();
    let expires_at = s.status(n)["delivery"]["expires_at"]
        .as_str()
        .unwrap()
        .to_string();
    for word in [
        format!("{n}:failed"),
        format!("#{n}"),
        format!("expired at {expires_at}"),
        "notice at".to_string(),
    ] {
        assert!(row.contains(&word), "{word} not in {said}");
    }
    assert_eq!(s.status(n)["delivery"]["state"], "expired");

    // One owed landing, to a reader that confirmed an earlier outcome.
    let s = Scratch::new("wt-owed-landing");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let first = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    s.land(first);
    let out = confirm(&s, &a, &[&format!("{first}:landed")]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let out = s.sluice(&["status", "bolt/x"]);
    assert!(!text(&out).contains("OWED"), "{}", text(&out));
    let v: serde_json::Value =
        serde_json::from_slice(&s.sluice(&["status", "bolt/x", "--json"]).stdout).unwrap();
    assert_eq!(v["owed"], serde_json::json!([]), "{v}");

    std::fs::write(a.join("more.txt"), "more\n").unwrap();
    let dir = a.to_str().unwrap();
    s.repo.git(&["-C", dir, "add", "-A"]);
    s.repo.git(&["-C", dir, "commit", "-q", "-m", "more"]);
    let n = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    s.land(n);
    let out = inbox(&s, &a, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    for all in [false, true] {
        let mut args = vec!["status", "bolt/x", "--json"];
        if all {
            args.push("--all");
        }
        let v: serde_json::Value = serde_json::from_slice(&s.sluice(&args).stdout).unwrap();
        let owed = v["owed"].as_array().unwrap();
        assert_eq!(owed.len(), 1, "{v}");
        assert_eq!(owed[0]["reader"], "unit/a", "{v}");
        assert_eq!(owed[0]["known"], true, "{v}");
        let d = &owed[0]["deliveries"];
        assert_eq!(d.as_array().unwrap().len(), 1, "{v}");
        assert_eq!(d[0]["key"], format!("{n}:landed").as_str(), "{v}");
        assert_eq!(d[0]["id"], n, "{v}");
        assert_eq!(d[0]["state"], "owed", "{v}");
        assert!(d[0]["expires_at"].is_string(), "{v}");
        assert_eq!(d[0]["handed"][0]["by"], "inbox", "{v}");
    }
}

/// `sluice inbox` run in `dir`.
fn inbox(s: &Scratch, dir: &Path, args: &[&str]) -> Output {
    s.command(env!("CARGO_BIN_EXE_sluice"), dir)
        .arg("inbox")
        .args(args)
        .output()
        .unwrap()
}

/// The ways each hand-over of submission `n`'s outcome took, as its
/// `delivery` gives them.
fn handed(s: &Scratch, n: u64) -> Vec<String> {
    s.status(n)["delivery"]["handed"]
        .as_array()
        .map(|all| {
            all.iter()
                .map(|h| h["by"].as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// 51, 135; inbox spec, "Collecting twice hands over twice": a landing no
/// carrier handed over is collected by `sluice inbox` in the worktree,
/// opening with the round's notice, then as `sluice wait` reports it, and
/// ending with the line naming its key; it is handed over by `inbox`, the
/// log saying it reached the worktree where no pane names its session,
/// and stays owed, so a second collect prints it again and its delivery
/// holds two hand-overs by `inbox` (record 0102).
#[test]
fn inbox_hands_a_landing_over_until_it_is_confirmed() {
    let s = Scratch::new("wt-inbox");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let n = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    s.land(n);
    let landed_as = s.status(n)["landed_as"].as_str().unwrap().to_string();
    let key = format!("{n}:landed");
    assert_eq!(s.status(n)["delivery"]["key"], key.as_str());
    assert_eq!(s.status(n)["delivery"]["state"], "owed");
    assert!(handed(&s, n).is_empty(), "{}", s.status(n));

    let out = inbox(&s, &a, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let said = String::from_utf8_lossy(&out.stdout).to_string();
    let mut lines = said.lines();
    let first = lines.next().unwrap_or_default();
    for word in [format!("#{n}"), "landed".into(), landed_as[..7].to_string()] {
        assert!(first.contains(&word), "{word} not in {said}");
    }
    assert!(
        said.lines().any(|l| l.starts_with("passed · landed as")),
        "{said}"
    );
    assert_eq!(
        said.lines().last(),
        Some(format!("Confirm it once you have acted on it: sluice confirm {key}").as_str()),
        "{said}"
    );
    assert_eq!(handed(&s, n), ["inbox"]);
    let record = s.status(n);
    assert_eq!(record["delivery"]["state"], "owed", "{record}");
    assert!(record.get("delivered").is_none(), "{record}");
    let path = std::fs::canonicalize(&a).unwrap();
    let lines = s.logged("handed");
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["key"], key.as_str());
    assert_eq!(lines[0]["by"], "inbox");
    assert_eq!(lines[0]["to"], path.to_str().unwrap());

    let again = inbox(&s, &a, &[]);
    assert_eq!(again.status.code(), Some(0), "{}", text(&again));
    assert_eq!(String::from_utf8_lossy(&again.stdout), said);
    assert_eq!(handed(&s, n), ["inbox", "inbox"]);
}

/// `sluice confirm` with `args`, run in `dir`.
fn confirm(s: &Scratch, dir: &Path, args: &[&str]) -> Output {
    s.command(env!("CARGO_BIN_EXE_sluice"), dir)
        .arg("confirm")
        .args(args)
        .output()
        .unwrap()
}

/// 51, S77; outcome-confirm spec, "A landing confirmed", "Confirmed
/// twice", "Another worktree's blame" and "A removed worktree's blame": a
/// worktree confirms its landing by its key, and its inbox prints nothing
/// of it then; confirmed again, it says when it was and writes nothing;
/// another worktree's blame is refused, naming its reader, and stays owed
/// until the operator confirms it for its removed worktree (record 0102).
#[test]
fn confirm_confirms_a_landing_by_its_key_and_says_when_again() {
    let s = Scratch::new("wt-confirm");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let n = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    s.land(n);
    let key = format!("{n}:landed");

    let out = confirm(&s, &a, &[&key]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let said = text(&out);
    for word in [key.as_str(), &format!("#{n}"), "bolt/x", "landed"] {
        assert!(said.contains(word), "{word} not in {said}");
    }
    assert_eq!(text(&inbox(&s, &a, &[])), "");
    let record = s.status(n);
    assert_eq!(record["delivery"]["state"], "done", "{record}");
    assert!(record["delivery"]["confirmed"].is_string(), "{record}");
    let at = record["delivery"]["confirmed"]
        .as_str()
        .unwrap()
        .to_string();
    let lines = s.logged("confirmed");
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["key"], key.as_str());
    assert_eq!(lines[0]["by"], "confirm");

    // Confirmed twice.
    let before = std::fs::read(s.repo.dir().join(".git/sluice/bolt/x/version")).ok();
    let out = confirm(&s, &a, &[&key, "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["confirmed"][0]["key"], key.as_str(), "{v}");
    assert_eq!(v["confirmed"][0]["first"], false, "{v}");
    assert_eq!(v["confirmed"][0]["at"], at.as_str(), "{v}");
    let out = confirm(&s, &a, &[&key]);
    assert_eq!(
        text(&out).trim_end(),
        format!("{key} was confirmed at {at}"),
        "{}",
        text(&out)
    );
    assert_eq!(
        std::fs::read(s.repo.dir().join(".git/sluice/bolt/x/version")).ok(),
        before,
        "nothing written"
    );
    assert_eq!(s.logged("confirmed").len(), 1);

    // Another worktree's blame.
    let b = s.session("b", &[("broken", "x\n")]);
    let m = id(&s.wt_submit(&b, &["--no-wait", "--json"]));
    assert_eq!(s.decide(m)["state"], "failed");
    let blame = format!("{m}:failed");
    let out = confirm(&s, &a, &[&blame]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("unit/b"), "{}", text(&out));
    assert!(text(&out).contains("--as operator"), "{}", text(&out));
    assert_eq!(s.status(m)["delivery"]["state"], "owed");
    let out = confirm(&s, &a, &[&blame, "--json"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "refused", "{v}");
    assert_eq!(
        v["error"]["next"],
        serde_json::json!([format!("sluice confirm {blame} --as operator")]),
        "{v}"
    );

    // A removed worktree's blame, confirmed by the operator in the main
    // checkout.
    s.repo
        .git(&["worktree", "remove", "--force", b.to_str().unwrap()]);
    let out = confirm(&s, s.repo.dir(), &[&blame, "--as", "operator"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let record = s.status(m);
    assert_eq!(record["delivery"]["state"], "done", "{record}");
    let by: Vec<_> = s
        .logged("confirmed")
        .into_iter()
        .filter(|l| l["id"] == m)
        .map(|l| l["by"].clone())
        .collect();
    assert_eq!(by, ["operator"]);

    // A key no branch holds.
    let out = confirm(&s, &a, &["99:landed"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("99:landed"), "{}", text(&out));
    assert!(text(&out).contains("sluice inbox"), "{}", text(&out));
}

/// 51; outcome-confirm spec, "A confirmation's document" and "A key no
/// branch holds is not found": under `--json`, a first confirmation gives
/// its key, branch, reader, time and `first` true; a key no branch holds
/// is the error document, `not_found`, naming `sluice inbox`, exit 1;
/// several keys exit by the worst of them, a refusal's 2 over a key not
/// found; and while a branch's records do not read, a key no branch read
/// holds exits 3, the branch named in the reader's words, over a refusal,
/// since whether the key is there is not known (rule core.11; record
/// 0102, D7).
#[test]
fn confirm_json_gives_a_first_confirmation_and_the_worst_of_its_exits() {
    let s = Scratch::new("wt-confirm-json");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let n = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    s.land(n);
    let key = format!("{n}:landed");
    let out = confirm(&s, &a, &[&key, "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let confirmed = v["confirmed"].as_array().unwrap();
    assert_eq!(confirmed.len(), 1, "{v}");
    assert_eq!(confirmed[0]["key"], key.as_str(), "{v}");
    assert_eq!(confirmed[0]["branch"], "bolt/x", "{v}");
    assert_eq!(confirmed[0]["reader"], "unit/a", "{v}");
    assert!(confirmed[0]["at"].is_string(), "{v}");
    assert_eq!(confirmed[0]["first"], true, "{v}");

    // A key never recorded.
    let out = confirm(&s, &a, &["99:landed", "--json"]);
    assert_eq!(out.status.code(), Some(sluice::FAILED), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "not_found", "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("99:landed"),
        "{v}"
    );
    assert_eq!(
        v["error"]["next"],
        serde_json::json!(["sluice inbox"]),
        "{v}"
    );

    // Another worktree's blame beside a key not found, in either order.
    let b = s.session("b", &[("broken", "x\n")]);
    let m = id(&s.wt_submit(&b, &["--no-wait", "--json"]));
    assert_eq!(s.decide(m)["state"], "failed");
    let blame = format!("{m}:failed");
    for keys in [["99:landed", blame.as_str()], [blame.as_str(), "99:landed"]] {
        let out = confirm(&s, &a, &keys);
        assert_eq!(out.status.code(), Some(sluice::CONFLICT), "{}", text(&out));
        let out = confirm(&s, &a, &[keys[0], keys[1], "--json"]);
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["error"]["kind"], "refused", "{v}");
    }

    // A branch whose records do not read.
    s.repo.git(&["branch", "bolt/y", "main"]);
    let out = s.sluice(&["step", "bolt/y"]);
    assert!(out.status.success(), "{}", text(&out));
    let records = s.repo.dir().join(".git/sluice/bolt/y");
    assert!(records.join("version").exists(), "no records of bolt/y");
    std::fs::write(
        records.join("deliveries.jsonl"),
        "{\"format\":\"weir-delivery/2\"}\n",
    )
    .unwrap();
    let out = confirm(&s, &a, &["99:landed"]);
    assert_eq!(out.status.code(), Some(sluice::INTERNAL), "{}", text(&out));
    let said = String::from_utf8_lossy(&out.stderr);
    for word in ["bolt/y", "deliveries.jsonl"] {
        assert!(said.contains(word), "{word} not in {said}");
    }
    let out = confirm(&s, &a, &["99:landed", &blame, "--json"]);
    assert_eq!(out.status.code(), Some(sluice::INTERNAL), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "internal", "{v}");
    assert_eq!(s.status(m)["delivery"]["state"], "owed");
}

/// 51; owed-outcomes spec, "A landing pushed and not confirmed", "A
/// landing confirmed and let go of" and "A queued submission": `sluice
/// status <n> --json` gives a queued submission no `delivery`; landed and
/// pushed into its pane, its `delivery` with its key, reader, `owed`,
/// times, the hand-over by `notice`, `confirmed` null and its reader not
/// known, and no `delivered`; confirmed, `done` with its time; let go of
/// by the branch's next write, its key and `done` alone; and a withdrawn
/// submission none (weir 107; record 0102, D3, D13; rule core.50).
#[test]
fn status_json_gives_a_decided_submissions_delivery_and_none_before() {
    let s = Scratch::new("wt-status-delivery");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let (n, _) = s.submit_from_pane(&a, "w1:p2");
    let queued = s.status(n);
    assert_eq!(queued["state"], "queued", "{queued}");
    assert!(queued.get("delivery").is_none(), "{queued}");
    assert_eq!(s.decide(n)["state"], "landed");
    let key = format!("{n}:landed");
    let record = s.status(n);
    let d = &record["delivery"];
    assert_eq!(d["key"], key.as_str(), "{record}");
    assert_eq!(d["reader"], "unit/a", "{record}");
    assert_eq!(d["state"], "owed", "{record}");
    assert!(d["recorded_at"].is_string(), "{record}");
    assert!(d["expires_at"].is_string(), "{record}");
    let handed = d["handed"].as_array().unwrap();
    assert_eq!(handed.len(), 1, "{record}");
    assert_eq!(handed[0]["by"], "notice", "{record}");
    assert!(handed[0]["at"].is_string(), "{record}");
    assert_eq!(d["confirmed"], serde_json::Value::Null, "{record}");
    assert_eq!(d["known"], false, "{record}");
    assert!(record.get("delivered").is_none(), "{record}");
    assert!(record.get("delivered_to").is_none(), "{record}");

    let out = confirm(&s, &a, &[&key]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let done = s.status(n);
    assert_eq!(done["delivery"]["state"], "done", "{done}");
    assert!(done["delivery"]["confirmed"].is_string(), "{done}");
    assert_eq!(done["delivery"]["known"], true, "{done}");

    // The branch's next write, a submit's, lets the done delivery go, its
    // submission among the finished records already.
    let c = s.session("c", &[("c.txt", "c\n")]);
    let w = id(&s.wt_submit(&c, &["--no-wait", "--json"]));
    assert_eq!(
        s.status(n)["delivery"],
        serde_json::json!({ "key": key, "state": "done" })
    );

    // Withdrawn by its worktree's next submission while queued.
    std::fs::write(c.join("more.txt"), "more\n").unwrap();
    let dir = c.to_str().unwrap();
    s.repo.git(&["-C", dir, "add", "-A"]);
    s.repo.git(&["-C", dir, "commit", "-q", "-m", "more"]);
    let again = id(&s.wt_submit(&c, &["--no-wait", "--json"]));
    assert_ne!(again, w);
    let withdrawn = s.status(w);
    assert_eq!(withdrawn["state"], "withdrawn", "{withdrawn}");
    assert!(withdrawn.get("delivery").is_none(), "{withdrawn}");
}

/// 51, 104; outcome-deliveries spec, "A day's span": on a host whose
/// settings hold `[deliveries] expire_after = "24h"`, a landing's delivery
/// expires a day after it is recorded (weir 104; record 0102, D9).
#[test]
fn a_hosts_span_of_a_day_owes_a_landing_for_a_day() {
    let s = Scratch::new("wt-span-day");
    std::fs::write(
        s.config.join("sluice/config.toml"),
        "[runs]\nmax = 2\n\n[deliveries]\nexpire_after = \"24h\"\n",
    )
    .unwrap();
    let a = s.session("a", &[("a.txt", "a\n")]);
    let n = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    s.land(n);
    let d = s.status(n)["delivery"].clone();
    let at = |field: &str| {
        chrono::DateTime::parse_from_rfc3339(d[field].as_str().unwrap_or_default())
            .unwrap_or_else(|e| panic!("{field}: {e}: {d}"))
    };
    assert_eq!(
        at("expires_at") - at("recorded_at"),
        chrono::Duration::hours(24),
        "{d}"
    );
}

/// 51, rule core.11; outcome-deliveries spec, "A line that does not
/// read": a line of `deliveries.jsonl` whose `format` is
/// `weir-delivery/2` stops the branch's status, naming the file and
/// `format` (record 0102, D3).
#[test]
fn a_deliveries_line_that_does_not_read_stops_the_branchs_status() {
    let s = Scratch::new("wt-deliveries-unread");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let n = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    s.land(n);
    let file = s.repo.dir().join(".git/sluice/bolt/x/deliveries.jsonl");
    let held = std::fs::read_to_string(&file).unwrap();
    assert!(held.contains("weir-delivery/1"), "{held}");
    std::fs::write(&file, held.replace("weir-delivery/1", "weir-delivery/2")).unwrap();
    let out = s.sluice(&["status", "bolt/x"]);
    assert!(!out.status.success(), "{}", text(&out));
    let said = text(&out);
    for word in ["deliveries.jsonl", "`format`", "weir-delivery/2"] {
        assert!(said.contains(word), "{word} not in {said}");
    }
}

/// 51, 135; inbox spec, "Each outcome collected is handed over and stays
/// owed": under `--json`, one document of the records handed over, each
/// with its `delivery` as the run leaves it, and no `delivered`; collected
/// again, the same record, until it is confirmed, then none (record 0102).
#[test]
fn inbox_json_lists_each_record_until_it_is_confirmed() {
    let s = Scratch::new("wt-inbox-json");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let n = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    s.land(n);
    for times in 1..=2 {
        let out = inbox(&s, &a, &["--json"]);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out));
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let outcomes = v["outcomes"].as_array().unwrap();
        assert_eq!(outcomes.len(), 1, "{v}");
        assert_eq!(outcomes[0]["id"], n);
        assert_eq!(outcomes[0]["state"], "landed");
        assert!(outcomes[0].get("delivered").is_none(), "{v}");
        assert!(outcomes[0].get("delivered_to").is_none(), "{v}");
        let delivery = &outcomes[0]["delivery"];
        assert_eq!(delivery["key"], format!("{n}:landed").as_str(), "{v}");
        assert_eq!(delivery["state"], "owed", "{v}");
        assert_eq!(delivery["handed"].as_array().unwrap().len(), times, "{v}");
        assert_eq!(delivery["handed"], s.status(n)["delivery"]["handed"], "{v}");
        assert!(v.get("unread").is_none(), "{v}");
    }

    let out = confirm(&s, &a, &[&format!("{n}:landed")]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let again = inbox(&s, &a, &["--json"]);
    assert_eq!(again.status.code(), Some(0), "{}", text(&again));
    let v: serde_json::Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(v, serde_json::json!({ "outcomes": [] }));
}

/// 51, 104; inbox spec, "An expired outcome says so": on a host whose
/// span is a second, a blame no carrier handed over expires still owed,
/// and `sluice inbox` in its worktree hands it over all the same, its
/// last line saying when it expired and naming its key (weir 104, 105;
/// record 0102, D8).
#[test]
fn inbox_says_an_expired_outcome_is_still_owed() {
    let s = Scratch::new("wt-inbox-expired");
    std::fs::write(
        s.config.join("sluice/config.toml"),
        "[runs]\nmax = 2\n\n[deliveries]\nexpire_after = \"1s\"\n",
    )
    .unwrap();
    let b = s.session("b", &[("broken", "x\n")]);
    let n = id(&s.wt_submit(&b, &["--no-wait", "--json"]));
    assert_eq!(s.decide(n)["state"], "failed");
    std::thread::sleep(Duration::from_millis(1500));
    let record = s.status(n);
    assert_eq!(record["delivery"]["state"], "expired", "{record}");
    let expires_at = record["delivery"]["expires_at"].as_str().unwrap();

    let out = inbox(&s, &b, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let said = String::from_utf8_lossy(&out.stdout).to_string();
    let first = said.lines().next().unwrap_or_default();
    assert!(first.contains(&format!("#{n} failed")), "{said}");
    assert_eq!(
        said.lines().last(),
        Some(
            format!(
                "Expired at {expires_at}, still owed. Confirm it once you have acted on it: \
                 sluice confirm {n}:failed"
            )
            .as_str()
        ),
        "{said}"
    );
    assert_eq!(handed(&s, n), ["inbox"]);
    assert_eq!(s.status(n)["delivery"]["state"], "expired");
}

/// 51; inbox spec, "A notice already pushed" and "A new outcome with an
/// older one owed": `sluice inbox --new` hands over only what no way has
/// handed its reader: nothing of a landing the round pushed into the
/// worktree's pane, recording nothing; and, once a plain collect has
/// handed that landing over by `inbox`, the worktree's next blame alone
/// (record 0102, D8, D14).
#[test]
fn inbox_new_hands_over_only_what_no_way_has_handed_over() {
    let s = Scratch::new("wt-inbox-new");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let (n, _) = s.submit_from_pane(&a, "w1:p2");
    assert_eq!(s.decide(n)["state"], "landed");
    assert_eq!(handed(&s, n), ["notice"]);

    let out = inbox(&s, &a, &["--new"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(text(&out), "");
    assert_eq!(handed(&s, n), ["notice"]);
    assert_eq!(s.logged("handed").len(), 1);

    let out = inbox(&s, &a, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(handed(&s, n), ["notice", "inbox"]);

    std::fs::write(a.join("broken"), "x\n").unwrap();
    let dir = a.to_str().unwrap();
    s.repo.git(&["-C", dir, "add", "-A"]);
    s.repo.git(&["-C", dir, "commit", "-q", "-m", "broken"]);
    let m = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    assert_eq!(s.decide(m)["state"], "failed");
    assert!(handed(&s, m).is_empty(), "{}", s.status(m));
    let out = inbox(&s, &a, &["--new"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let said = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        said.lines()
            .next()
            .unwrap_or_default()
            .contains(&format!("#{m} failed")),
        "{said}"
    );
    assert!(!said.contains(&format!("#{n} landed")), "{said}");
    assert_eq!(
        said.lines().last(),
        Some(format!("Confirm it once you have acted on it: sluice confirm {m}:failed").as_str()),
        "{said}"
    );
    assert_eq!(handed(&s, m), ["inbox"]);
    assert_eq!(handed(&s, n), ["notice", "inbox"]);
}

/// 51, 135; inbox spec, "A blame is delivered with its failed checks and
/// logs": a blame is collected naming its failed check, then in the words
/// `sluice wait` reports it with, its check's log among them, then the line
/// naming its key. The submission is decided by rounds alone and its wait
/// is read from another worktree, so neither hands it over before the
/// collect.
#[test]
fn inbox_delivers_a_blame_as_sluice_wait_reports_it() {
    let s = Scratch::new("wt-inbox-blamed");
    let b = s.session("b", &[("broken", "x\n")]);
    let n = id(&s.wt_submit(&b, &["--no-wait", "--json"]));
    assert_eq!(s.decide(n)["state"], "failed");
    let waited = s.sluice(&["wait", &n.to_string()]);
    assert_eq!(
        waited.status.code(),
        Some(sluice::FAILED),
        "{}",
        text(&waited)
    );
    let waited = String::from_utf8_lossy(&waited.stdout).to_string();
    assert!(waited.contains("log of sh check.sh"), "{waited}");
    assert!(handed(&s, n).is_empty(), "{}", s.status(n));

    let out = inbox(&s, &b, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let said = String::from_utf8_lossy(&out.stdout).to_string();
    let (first, rest) = said.split_once('\n').unwrap();
    for word in [&format!("#{n}") as &str, "failed", "sh check.sh"] {
        assert!(first.contains(word), "{word} not in {said}");
    }
    assert_eq!(
        rest,
        format!("{waited}Confirm it once you have acted on it: sluice confirm {n}:failed\n")
    );
    assert_eq!(handed(&s, n), ["inbox"]);
}

/// 51; outcome-delivery spec, "A waiting submit hands over its landing": a
/// `sluice submit --json` that waits and reports its landing hands it over
/// by `wait`, in the record it prints and in the branch's records, and the
/// landing stays owed, so a later collect in that worktree hands it over
/// again (record 0102).
#[test]
fn a_waiting_submit_hands_its_landing_over_and_inbox_hands_it_over_again() {
    let s = Scratch::new("wt-wait-delivers");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &a)
        .args(["submit", "bolt/x", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    let record: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("no JSON: {}", text(&out)));
    assert_eq!(record["state"], "landed", "{record}");
    assert!(record.get("delivered").is_none(), "{record}");
    assert_eq!(record["delivery"]["state"], "owed", "{record}");
    assert_eq!(record["delivery"]["handed"][0]["by"], "wait", "{record}");
    let n = record["id"].as_u64().unwrap();
    assert_eq!(handed(&s, n), ["wait"]);
    let path = std::fs::canonicalize(&a).unwrap();
    let lines: Vec<_> = s
        .logged("handed")
        .into_iter()
        .filter(|l| l["id"] == n)
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["by"], "wait");
    assert_eq!(lines[0]["to"], path.to_str().unwrap());

    let e = collected_one(&s, &a);
    assert_eq!(e["id"], n, "{e}");
    assert_eq!(handed(&s, n), ["wait", "inbox"]);
}

/// 51; outcome-delivery spec, "A wait that confirms on receipt": `sluice
/// wait <id> --confirm` in the submitting worktree, reporting a blame the
/// rounds decided, hands it over and confirms it there, and a later collect
/// prints nothing (record 0102).
#[test]
fn a_wait_that_confirms_on_receipt_leaves_inbox_nothing() {
    let s = Scratch::new("wt-wait-own");
    let b = s.session("b", &[("broken", "x\n")]);
    let n = id(&s.wt_submit(&b, &["--no-wait", "--json"]));
    assert_eq!(s.decide(n)["state"], "failed");
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &b)
        .args(["wait", &n.to_string(), "--confirm", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::FAILED), "{}", text(&out));
    let record: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(record["delivery"]["state"], "done", "{record}");
    assert_eq!(handed(&s, n), ["wait"]);
    let confirmed: Vec<_> = s
        .logged("confirmed")
        .into_iter()
        .filter(|l| l["id"] == n)
        .collect();
    assert_eq!(confirmed.len(), 1, "{confirmed:?}");
    assert_eq!(confirmed[0]["by"], "wait");
    assert_eq!(confirmed[0]["reader"], "unit/b");

    let again = inbox(&s, &b, &[]);
    assert_eq!(again.status.code(), Some(0), "{}", text(&again));
    assert_eq!(text(&again), "");
}

/// 20, 51; outcome-delivery spec, "A wait on another's submission marks
/// nothing": a blame waited on from another worktree is reported and left
/// pending, for its own session to collect.
#[test]
fn a_wait_from_another_worktree_marks_nothing() {
    let s = Scratch::new("wt-wait-other");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let b = s.session("b", &[("broken", "x\n")]);
    let n = id(&s.wt_submit(&b, &["--no-wait", "--json"]));
    assert_eq!(s.decide(n)["state"], "failed");
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &a)
        .args(["wait", &n.to_string(), "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::FAILED), "{}", text(&out));
    let record: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(record.get("delivered").is_none(), "{record}");
    assert_eq!(record["delivery"]["state"], "owed", "{record}");
    assert!(handed(&s, n).is_empty(), "{}", s.status(n));
    assert!(s.logged("handed").is_empty());
    assert_eq!(collected_one(&s, &b)["id"], n);
}

/// 51; outcome-delivery spec, "A wait reads an outcome already
/// delivered": a landing the pane took and its worktree confirmed is
/// reported by a later wait in its worktree with its delivery as it
/// stood, and the wait records nothing (record 0102).
#[test]
fn a_wait_reports_an_outcome_the_pane_took_as_it_stands() {
    let s = Scratch::new("wt-wait-pushed");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let (n, session) = s.submit_from_pane(&a, "w1:p2");
    let v = s.decide(n);
    assert_eq!(v["state"], "landed", "{v}");
    assert_eq!(handed(&s, n), ["notice"]);
    let lines = s.logged("handed");
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["to"], session.as_str());
    let out = confirm(&s, &a, &[&format!("{n}:landed")]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let stood = s.status(n)["delivery"].clone();
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &a)
        .args(["wait", &n.to_string(), "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    let record: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(record["delivery"], stood, "{record}");
    assert_eq!(s.logged("handed").len(), 1);
    assert_eq!(s.logged("confirmed").len(), 1);
}

/// 51: a wait that runs out of time before its submission is decided
/// marks nothing, and the outcome, once decided, is collected still.
#[test]
fn a_wait_that_runs_out_of_time_marks_nothing() {
    let s = Scratch::new("wt-wait-timeout");
    let a = s.session("a", &[("slow", "x\n")]);
    let n = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &a)
        .args(["wait", &n.to_string(), "--timeout", "1s", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let record: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        record["state"] == "queued" || record["state"] == "testing",
        "{record}"
    );
    assert!(record.get("delivery").is_none(), "{record}");
    assert!(s.logged("handed").is_empty());
    assert_eq!(s.decide(n)["state"], "landed");
    assert!(handed(&s, n).is_empty(), "{}", s.status(n));
    assert_eq!(collected_one(&s, &a)["id"], n);
}

/// 51, 135; inbox spec, "Another worktree's outcomes are not delivered":
/// another worktree's outcome is not this session's to collect, and stays
/// owed with no hand-over.
#[test]
fn inbox_delivers_nothing_of_another_worktrees_outcome() {
    let s = Scratch::new("wt-inbox-other");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let b = s.session("b", &[("b.txt", "b\n")]);
    let n = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    s.land(n);
    let out = inbox(&s, &b, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(text(&out), "");
    assert_eq!(s.status(n)["delivery"]["state"], "owed");
    assert!(handed(&s, n).is_empty(), "{}", s.status(n));
}

/// A session `name` that submits `changes` to `bolt/x`, then a commit of
/// `more` on top of them: its first submission, which the second
/// withdrew, and the second, which carries the first's work (67).
fn resubmitted(
    s: &Scratch,
    name: &str,
    changes: &[(&str, &str)],
    more: &[(&str, &str)],
) -> (PathBuf, u64, u64) {
    let a = s.session(name, changes);
    let first = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    for (p, c) in more {
        std::fs::write(a.join(p), c).unwrap();
    }
    let dir = a.to_str().unwrap();
    s.repo.git(&["-C", dir, "add", "-A"]);
    s.repo.git(&["-C", dir, "commit", "-q", "-m", "more"]);
    let second = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    assert_eq!(s.status(first)["state"], "withdrawn");
    (a, first, second)
}

/// 67, 122; record 0100 D2, D3: a wait on a submission its session's next
/// submit withdrew follows the submission that carries its work, says so,
/// and ends with its own submission landed by that one's landing, never
/// with that one's record; the withdrawn one's record stands as it is.
#[test]
fn a_wait_follows_the_resubmission_that_carries_its_work() {
    let s = Scratch::new("wt-carried");
    let (_, first, second) = resubmitted(&s, "a", &[("a.txt", "a\n")], &[("more.txt", "more\n")]);
    let out = s.sluice(&["wait", &first.to_string(), "--timeout", "60s"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{said}");
    assert!(
        said.contains(&format!(
            "#{first} withdrawn by resubmission: #{second} carries its work, so this wait follows it"
        )),
        "{said}"
    );
    let landed_as = s.status(second)["landed_as"].as_str().unwrap().to_string();
    assert!(
        said.contains(&format!(
            "landed as {}: bolt/x holds its work, by #{second}'s landing",
            &landed_as[..7]
        )),
        "{said}"
    );
    assert_eq!(s.status(second)["state"], "landed");
    assert_eq!(s.status(first)["state"], "withdrawn");

    // Under --json, its own record, naming what carried its work.
    let (_, first, second) = resubmitted(&s, "b", &[("b.txt", "b\n")], &[("more-b.txt", "more\n")]);
    let out = s.sluice(&["wait", &first.to_string(), "--timeout", "60s", "--json"]);
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["id"], first, "{doc}");
    assert_eq!(doc["state"], "landed", "{doc}");
    assert_eq!(doc["landed_elsewhere"], true, "{doc}");
    assert_eq!(doc["landed_as"], s.status(second)["landed_as"], "{doc}");
    assert_eq!(
        doc["carried_by"],
        serde_json::json!({ "id": second, "state": "landed" }),
        "{doc}"
    );
    assert_eq!(s.status(first)["state"], "withdrawn");
}

/// 67, 122; record 0100 D2, D3: a wait on a withdrawn submission started
/// only once the resubmission carrying its work has landed ends with its
/// own submission landed by that one's landing and names it, as a wait
/// standing while it landed does, never with the commit a fast-forward
/// would give, its own head.
#[test]
fn a_wait_started_after_the_resubmission_landed_names_it() {
    let s = Scratch::new("wt-carried-landed");
    let (_, first, second) = resubmitted(&s, "a", &[("a.txt", "a\n")], &[("more.txt", "more\n")]);
    s.land(second);
    let landed_as = s.status(second)["landed_as"].as_str().unwrap().to_string();
    let out = s.sluice(&["wait", &first.to_string(), "--timeout", "60s"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{said}");
    assert!(
        said.contains(&format!(
            "landed as {}: bolt/x holds its work, by #{second}'s landing",
            &landed_as[..7]
        )),
        "{said}"
    );

    let out = s.sluice(&["wait", &first.to_string(), "--timeout", "60s", "--json"]);
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["id"], first, "{doc}");
    assert_eq!(doc["state"], "landed", "{doc}");
    assert_eq!(doc["landed_elsewhere"], true, "{doc}");
    assert_eq!(doc["landed_as"], landed_as.as_str(), "{doc}");
    assert_ne!(doc["landed_as"], doc["head"], "{doc}");
    assert_eq!(
        doc["carried_by"],
        serde_json::json!({ "id": second, "state": "landed" }),
        "{doc}"
    );
    assert_eq!(s.status(first)["state"], "withdrawn");
}

/// 67; record 0100 D2, D3: a wait on a submission whose resubmission, which
/// carried its work, is blamed ends with its own submission withdrawn,
/// exit 4, naming the resubmission and how it ended, never with the
/// resubmission's failure as its own.
#[test]
fn a_wait_exits_withdrawn_when_what_carried_its_work_failed() {
    let s = Scratch::new("wt-carried-failed");
    let (_, first, second) = resubmitted(&s, "a", &[("a.txt", "a\n")], &[("broken", "x\n")]);
    let out = s.sluice(&["wait", &first.to_string(), "--timeout", "60s", "--json"]);
    assert_eq!(out.status.code(), Some(sluice::WITHDRAWN), "{}", text(&out));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["id"], first, "{doc}");
    assert_eq!(doc["state"], "withdrawn", "{doc}");
    assert_eq!(
        doc["carried_by"],
        serde_json::json!({ "id": second, "state": "failed" }),
        "{doc}"
    );
    assert_eq!(s.status(second)["state"], "failed");

    let out = s.sluice(&["wait", &first.to_string(), "--timeout", "60s"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(sluice::WITHDRAWN), "{said}");
    assert!(
        said.contains(&format!(
            "#{first} withdrawn by resubmission: #{second}, which carried its work, failed\n\
             sluice status {second}"
        )),
        "{said}"
    );
}

/// 67; record 0100 D2, D3: a wait out of time while it follows the
/// resubmission that carries its work prints its own submission's record,
/// naming what it follows as it stands, and how to wait on its own again.
#[test]
fn a_wait_out_of_time_while_it_follows_prints_its_own_record() {
    let s = Scratch::new("wt-carried-timeout");
    let (_, first, second) = resubmitted(&s, "a", &[("a.txt", "a\n")], &[("slow", "x\n")]);
    let out = s.sluice(&["wait", &first.to_string(), "--timeout", "2s", "--json"]);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["id"], first, "{doc}");
    assert_eq!(doc["state"], "withdrawn", "{doc}");
    assert_eq!(doc["carried_by"]["id"], second, "{doc}");
    let state = &doc["carried_by"]["state"];
    assert!(state == "queued" || state == "testing", "{doc}");

    // Its words, in a scratch of its own, so no run of the first is under
    // way to end its round late.
    let s = Scratch::new("wt-carried-timeout-words");
    let (_, first, _) = resubmitted(&s, "a", &[("a.txt", "a\n")], &[("slow", "x\n")]);
    let out = s.sluice(&["wait", &first.to_string(), "--timeout", "2s"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{said}");
    assert!(
        said.contains(&format!("still waiting: sluice wait {first}")),
        "{said}"
    );
}

/// 51; record 0100 D3: a wait that followed the resubmission carrying its
/// work to its landing marks nothing of the resubmission's delivered: its
/// landing stays pending for its own delivery, which the worktree's inbox
/// then collects and marks.
#[test]
fn a_wait_leaves_the_resubmissions_landing_pending() {
    let s = Scratch::new("wt-carried-pending");
    let (a, first, second) = resubmitted(&s, "a", &[("a.txt", "a\n")], &[("more.txt", "more\n")]);
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &a)
        .args(["wait", &first.to_string(), "--timeout", "60s"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::LANDED), "{}", text(&out));
    let landed = s.status(second);
    assert_eq!(landed["state"], "landed", "{landed}");
    assert!(handed(&s, second).is_empty(), "{landed}");
    assert!(s.logged("handed").is_empty());
    assert_eq!(collected_one(&s, &a)["id"], second);
    assert_eq!(handed(&s, second), ["inbox"]);
}

/// 2, 3, 51: a folder with no files checked out, a bare clone or a
/// checkout's git directory, is read by the driver's `recipient` as the
/// worktree on the branch its `HEAD` names, with no path, and by its
/// `session` with nothing held beyond its commits, neither running git
/// status, which refuses to run there; `recipient` reads no `HEAD` commit,
/// so a branch with none yet is read all the same, and a `HEAD` on no
/// branch is no one's (record 0099; rule core.48).
#[test]
fn a_folder_with_no_files_checked_out_is_read_with_no_status_and_no_commit() {
    let s = Scratch::new("wt-bare-recipient");
    let bare = s.repo.dir().with_extension("bare");
    let _ = std::fs::remove_dir_all(&bare);
    s.repo.git(&[
        "clone",
        "-q",
        "--bare",
        s.repo.dir().to_str().unwrap(),
        bare.to_str().unwrap(),
    ]);
    s.repo.write("untracked.txt", "u\n");
    let driver = Worktrunk::new("h");
    let agent = |branch: &str| {
        std::env::var("SLUICE_AGENT")
            .ok()
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| branch.to_string())
    };
    for dir in [bare.clone(), s.repo.dir().join(".git")] {
        let me = driver.recipient(dir.to_str().unwrap()).unwrap();
        let me = me.unwrap_or_else(|| panic!("{} read as no one's", dir.display()));
        assert_eq!(me.worktree, "main", "{}", dir.display());
        assert_eq!(me.path, None, "{}", dir.display());
        assert_eq!(me.head, None);
        assert_eq!(me.agent, agent("main"));
        assert_eq!(me.host.as_deref(), Some("h"));
        let session = driver.session(dir.to_str().unwrap()).unwrap();
        assert!(session.dirty.is_empty(), "{:?}", session.dirty);
        assert_eq!(session.from.path, None);
    }

    let b = bare.to_str().unwrap();
    s.repo
        .git(&["--git-dir", b, "symbolic-ref", "HEAD", "refs/heads/nothing"]);
    let me = driver
        .recipient(b)
        .unwrap()
        .expect("a branch with no commit");
    assert_eq!(me.worktree, "nothing");
    assert_eq!(me.path, None);

    let tip = s.repo.rev("main");
    s.repo
        .git(&["--git-dir", b, "update-ref", "--no-deref", "HEAD", &tip]);
    assert_eq!(driver.recipient(b).unwrap(), None);

    let checkout = std::fs::canonicalize(s.repo.dir()).unwrap();
    let me = driver
        .recipient(s.repo.dir().to_str().unwrap())
        .unwrap()
        .expect("the checkout's");
    assert_eq!(me.worktree, "main");
    assert_eq!(me.path.as_deref(), checkout.to_str());
    let session = driver.session(s.repo.dir().to_str().unwrap()).unwrap();
    assert_eq!(session.dirty, vec!["?? untracked.txt".to_string()]);
}

/// 2, 51: a wait and an inbox in the repository's git directory, which has
/// no files checked out and whose `HEAD` names `main`, read as the
/// worktree on `main` would: worktree `a`'s landing is not theirs, so the
/// wait reports it and exits 0, saying nothing of it pending, of git status
/// or of files checked out, and marks nothing; the inbox collects nothing
/// and says nothing; `a`'s own inbox then collects it (record 0099; rule
/// core.48).
#[test]
fn a_wait_with_no_files_checked_out_reads_as_a_worktree_on_its_branch() {
    let s = Scratch::new("wt-bare-wait");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let n = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    s.land(n);
    let git_dir = s.repo.dir().join(".git");
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &git_dir)
        .args(["wait", &n.to_string()])
        .output()
        .unwrap();
    let said = text(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(said.contains("landed as"), "{said}");
    for word in ["pending", "owed", "git status", "checked out"] {
        assert!(!said.contains(word), "{word} in {said}");
    }
    assert!(handed(&s, n).is_empty(), "{}", s.status(n));
    assert!(s.logged("handed").is_empty());
    let out = inbox(&s, &git_dir, &["--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v, serde_json::json!({ "outcomes": [] }));
    // Inbox spec, "The git directory collects nothing of a worktree's":
    // the landing stays owed, with no hand-over.
    assert_eq!(s.status(n)["delivery"]["state"], "owed", "{}", s.status(n));
    assert!(handed(&s, n).is_empty(), "{}", s.status(n));
    assert!(s.logged("handed").is_empty());
    let e = collected_one(&s, &a);
    assert_eq!(e["id"], n);
}

/// `sluice` run in `dir` with the harness naming `agent-a`, in no herdr
/// pane.
fn as_agent_a(s: &Scratch, dir: &Path, args: &[&str]) -> Output {
    s.command(env!("CARGO_BIN_EXE_sluice"), dir)
        .env("SLUICE_AGENT", "agent-a")
        .args(args)
        .output()
        .unwrap()
}

/// The git directory of worktree `dir`, which has no files checked out and
/// whose `HEAD` is the worktree's own.
fn own_git_dir(s: &Scratch, dir: &Path) -> PathBuf {
    PathBuf::from(s.repo.git(&[
        "-C",
        dir.to_str().unwrap(),
        "rev-parse",
        "--absolute-git-dir",
    ]))
}

/// 51; outcome-delivery spec, "The git directory's own landing": worktree
/// `a`'s own git directory, whose `HEAD` names `a`'s branch, is that
/// worktree with no path: a wait there on `a`'s landing hands it over by
/// `wait`, the `handed` line reaching the agent the harness names, and its
/// inbox hands over `a`'s next submission, blamed, to the same agent
/// (record 0099's D3; record 0102; rule core.48).
#[test]
fn a_git_directory_delivers_its_branchs_own() {
    let s = Scratch::new("wt-gitdir-own");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let n = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    s.land(n);
    let own = own_git_dir(&s, &a);
    let out = as_agent_a(&s, &own, &["wait", &n.to_string()]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(said.contains("landed as"), "{said}");
    assert_eq!(handed(&s, n), ["wait"]);
    let to = |n: u64| -> Vec<serde_json::Value> {
        s.logged("handed")
            .into_iter()
            .filter(|l| l["id"] == n)
            .map(|l| l["to"].clone())
            .collect()
    };
    assert_eq!(to(n), ["agent-a"]);

    std::fs::write(a.join("broken"), "x\n").unwrap();
    let dir = a.to_str().unwrap();
    s.repo.git(&["-C", dir, "add", "-A"]);
    s.repo.git(&["-C", dir, "commit", "-q", "-m", "broken"]);
    let m = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    assert_eq!(s.decide(m)["state"], "failed");
    let out = as_agent_a(&s, &own, &["inbox", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    // The landing the wait handed over stays owed, so it comes again
    // beside the blame.
    let outcomes = v["outcomes"].as_array().unwrap();
    let ids: Vec<_> = outcomes.iter().map(|e| e["id"].clone()).collect();
    assert_eq!(ids, [serde_json::json!(n), serde_json::json!(m)], "{v}");
    assert_eq!(outcomes[1]["state"], "failed", "{v}");
    assert_eq!(outcomes[1]["delivery"]["handed"][0]["by"], "inbox", "{v}");
    assert_eq!(to(m), ["agent-a"]);
    assert_eq!(to(n), ["agent-a", "agent-a"]);
}

/// 2, 51: a git directory whose `HEAD` names a branch with no commit fails
/// no delivery read: a wait there reports the landing and exits 0, saying
/// nothing of what it could not read, and marks nothing; its inbox
/// collects nothing and says nothing (record 0099; rule core.48).
#[test]
fn a_head_with_no_commit_fails_no_delivery_read() {
    let s = Scratch::new("wt-gitdir-unborn");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let n = id(&s.wt_submit(&a, &["--no-wait", "--json"]));
    s.land(n);
    let own = own_git_dir(&s, &a);
    s.repo.git(&[
        "--git-dir",
        own.to_str().unwrap(),
        "symbolic-ref",
        "HEAD",
        "refs/heads/nothing",
    ]);
    let out = as_agent_a(&s, &own, &["wait", &n.to_string()]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(said.contains("landed as"), "{said}");
    for word in ["could not read", "pending", "owed", "no git", "commit:"] {
        assert!(!said.contains(word), "{word} in {said}");
    }
    assert!(handed(&s, n).is_empty(), "{}", s.status(n));
    assert!(s.logged("handed").is_empty());
    let out = as_agent_a(&s, &own, &["inbox", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v, serde_json::json!({ "outcomes": [] }));
}

/// 124, 51, 2; submit-by-name spec, "A wait in a bare clone reports it and
/// marks nothing": a branch sluice lands on submitted by name, from the
/// repository's git directory, is owed to no inbox: a wait on it there
/// reports it landed and exits 0, saying nothing of it staying owed, and
/// records nothing; the branch's deliveries hold nothing under its key, it
/// gives no `delivery`, and no inbox, there or in a worktree on the branch,
/// collects it (design D5; records 0085, 0102; rule core.50).
#[test]
fn a_wait_by_name_in_a_bare_clone_says_nothing_pending() {
    let s = Scratch::new("wt-named-bare");
    let head = s.repo.branch("bolt/w", "bolt/x", &[("w.txt", Some("w\n"))]);
    let git_dir = s.repo.dir().join(".git");
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &git_dir)
        .args([
            "submit",
            "bolt/x",
            "--head",
            "bolt/w",
            "--no-wait",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let n = id(&out);
    let record = s.status(n);
    assert_eq!(record["head"], head.as_str(), "{record}");
    assert_eq!(record["from"]["head"], "bolt/w", "{record}");
    assert_eq!(record["from"]["worktree"], "bolt/w", "{record}");
    s.land(n);

    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &git_dir)
        .args(["wait", &n.to_string()])
        .output()
        .unwrap();
    let said = text(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(said.contains("landed as"), "{said}");
    assert!(!said.contains("pending"), "{said}");
    assert!(!said.contains("owed"), "{said}");
    assert!(!said.contains("no files checked out"), "{said}");
    assert!(s.status(n).get("delivery").is_none(), "{}", s.status(n));
    for event in ["pending", "handed", "confirmed"] {
        assert!(s.logged(event).is_empty(), "a {event} line");
    }
    let deliveries =
        std::fs::read_to_string(s.repo.dir().join(".git/sluice/bolt/x/deliveries.jsonl"))
            .unwrap_or_default();
    assert!(
        !deliveries.contains(&format!("\"{n}:landed\"")),
        "{deliveries}"
    );

    let out = inbox(&s, &git_dir, &["--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v, serde_json::json!({ "outcomes": [] }));
    // A worktree that has checked out the branch collects nothing of it.
    let w = s.repo.dir().with_extension("s-w");
    let _ = std::fs::remove_dir_all(&w);
    s.repo
        .git(&["worktree", "add", "-q", w.to_str().unwrap(), "bolt/w"]);
    let out = inbox(&s, &w, &["--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v, serde_json::json!({ "outcomes": [] }));
    assert!(s.logged("handed").is_empty());
}

/// 3, 135: a worktree on no branch has submitted nothing, and collects
/// nothing.
#[test]
fn inbox_in_a_detached_worktree_delivers_nothing() {
    let s = Scratch::new("wt-inbox-detached");
    let a = s.session("a", &[("a.txt", "a\n")]);
    s.repo
        .git(&["-C", a.to_str().unwrap(), "switch", "-q", "--detach"]);
    let out = inbox(&s, &a, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(text(&out), "");
    let out = inbox(&s, &a, &["--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v, serde_json::json!({ "outcomes": [] }));
}

/// `sluice inbox --json` in `dir`: its one element, after asserting the
/// document holds one and no `unread`, and the fields every element holds
/// whatever its state (the contract a caller reads, which only gains
/// fields).
fn collected_one(s: &Scratch, dir: &Path) -> serde_json::Value {
    collected_one_by(s, dir, &["--json"])
}

/// `sluice inbox` with `args` in `dir`, as [`collected_one`] reads it.
fn collected_one_by(s: &Scratch, dir: &Path, args: &[&str]) -> serde_json::Value {
    let out = inbox(s, dir, args);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v.get("unread").is_none(), "{v}");
    let outcomes = v["outcomes"].as_array().unwrap();
    assert_eq!(outcomes.len(), 1, "{v}");
    let e = outcomes[0].clone();
    let path = std::fs::canonicalize(dir).unwrap();
    assert!(e["id"].is_u64(), "{e}");
    assert_eq!(e["branch"], "bolt/x", "{e}");
    assert!(e.get("delivered").is_none(), "{e}");
    assert!(e.get("delivered_to").is_none(), "{e}");
    let delivery = &e["delivery"];
    assert_eq!(
        delivery["key"],
        format!("{}:{}", e["id"], e["state"].as_str().unwrap()).as_str(),
        "{e}"
    );
    assert_eq!(delivery["reader"], e["from"]["worktree"], "{e}");
    let handed = delivery["handed"].as_array().unwrap();
    assert_eq!(handed.last().unwrap()["by"], "inbox", "{e}");
    assert!(e["from"]["worktree"].is_string(), "{e}");
    assert_eq!(e["from"]["path"], path.to_str().unwrap(), "{e}");
    let notice = e["notice"].as_str().unwrap();
    assert!(
        notice.starts_with(&format!("[sluice] #{} ", e["id"])),
        "{e}"
    );
    assert_eq!(notice.lines().count(), 1, "{e}");
    e
}

/// 51, 135; inbox spec, "The JSON document is a contract": a landing, a
/// blame and a conflict, each collected as its record with `notice`, and
/// by its state `landed_as`; `result` and `failed[].task`;
/// `conflict.paths` and `conflict.with[].id`.
#[test]
fn inbox_json_holds_the_contracts_fields_for_each_outcome() {
    let s = Scratch::new("wt-inbox-contract");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let b = s.session("b", &[("broken", "x\n")]);
    let d = s.session("d", &[("same.txt", "d\n")]);
    let c = s.session("c", &[("same.txt", "c\n")]);
    let ids: Vec<u64> = [&a, &b, &d, &c]
        .iter()
        .map(|w| id(&s.wt_submit(w, &["--no-wait", "--json"])))
        .collect();
    let (na, nb, nd, nc) = (ids[0], ids[1], ids[2], ids[3]);
    for n in &ids {
        s.decide(*n);
    }

    let e = collected_one(&s, &a);
    assert_eq!(
        (e["id"].as_u64(), &e["state"]),
        (Some(na), &"landed".into())
    );
    let landed_as = e["landed_as"].as_str().unwrap();
    assert_eq!(landed_as, s.status(na)["landed_as"], "{e}");
    assert!(e["notice"].as_str().unwrap().contains("landed"), "{e}");

    let e = collected_one(&s, &b);
    assert_eq!(
        (e["id"].as_u64(), &e["state"]),
        (Some(nb), &"failed".into())
    );
    assert_eq!(e["result"], "failed", "{e}");
    assert_eq!(e["failed"][0]["task"], "sh check.sh", "{e}");
    assert!(e["notice"].as_str().unwrap().contains("failed"), "{e}");

    let e = collected_one(&s, &c);
    assert_eq!(
        (e["id"].as_u64(), &e["state"]),
        (Some(nc), &"conflict".into())
    );
    assert_eq!(
        e["conflict"]["paths"],
        serde_json::json!(["same.txt"]),
        "{e}"
    );
    assert_eq!(e["conflict"]["with"][0]["id"], nd, "{e}");
    assert!(e["notice"].as_str().unwrap().contains("conflicts"), "{e}");
}

/// 51, 135; inbox spec, "The round does not push what the command
/// delivered": a session in a herdr pane whose push herdr refused collects
/// its outcome by `sluice inbox`, handed over to the pane it collects in;
/// no later round prompts any pane for it, and its delivery holds that one
/// hand-over (record 0102).
#[test]
fn inbox_in_a_herdr_pane_delivers_to_that_pane_and_no_round_pushes_it() {
    let s = Scratch::new("wt-inbox-pane");
    s.herdr.refuses(
        1,
        r#"{"error":{"code":"agent_blocked","message":"agent is blocked"}}"#,
    );
    let a = s.session("a", &[("a.txt", "a\n")]);
    let (n, _) = s.submit_from_pane(&a, "w1:p1");
    let v = s.decide(n);
    assert_eq!(v["state"], "landed", "{v}");
    assert!(v.get("delivered").is_none(), "{v}");
    assert!(handed(&s, n).is_empty(), "{v}");
    assert_eq!(s.herdr.calls().len(), 1, "the refused push");

    let socket = s.repo.dir().with_extension("herdr.sock");
    let out = s
        .command(env!("CARGO_BIN_EXE_sluice"), &a)
        .args(["inbox"])
        .env("HERDR_PANE_ID", "w1:p2")
        .env("HERDR_SOCKET_PATH", &socket)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(
        text(&out).starts_with(&format!("[sluice] #{n} landed")),
        "{}",
        text(&out)
    );
    let to = format!("herdr:w1:p2@{}", socket.display());
    let lines: Vec<_> = s
        .logged("handed")
        .into_iter()
        .filter(|l| l["id"] == n)
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["to"], to.as_str());

    for _ in 0..2 {
        let out = s.sluice(&["step", "bolt/x"]);
        assert!(out.status.success(), "{}", text(&out));
    }
    assert_eq!(s.herdr.calls().len(), 1, "no round pushed it");
    assert_eq!(handed(&s, n), ["inbox"]);
}

/// 50, 20; outcome-delivery spec, "No other pane is prompted": of two
/// submissions tested together, only the blamed one's pane is sent the
/// failure, and the other's pane nothing of it.
#[test]
fn of_two_tested_together_only_the_blamed_ones_pane_is_sent_the_failure() {
    let s = Scratch::new("wt-push-two");
    let a = s.session("a", &[("a.txt", "a\n")]);
    let b = s.session("b", &[("broken", "x\n")]);
    let (one, _) = s.submit_from_pane(&a, "w1:p2");
    let (two, _) = s.submit_from_pane(&b, "w1:p3");
    assert_eq!(s.decide(one)["state"], "landed");
    let v = s.decide(two);
    assert_eq!(v["state"], "failed", "{v}");
    let together = v["tested"]["submissions"].as_array().unwrap();
    assert!(
        together.contains(&one.into()) && together.contains(&two.into()),
        "tested together: {v}"
    );

    let calls = s.herdr.calls();
    let to = |pane: &str| -> Vec<String> {
        calls
            .iter()
            .filter(|c| c[0].contains(&format!("<{pane}>")))
            .map(|c| c[0].clone())
            .collect()
    };
    let p3 = to("w1:p3");
    assert_eq!(p3.len(), 1, "{calls:?}");
    assert!(p3[0].contains(&format!("#{two} failed")), "{p3:?}");
    let p2 = to("w1:p2");
    assert_eq!(p2.len(), 1, "{calls:?}");
    assert!(p2[0].contains(&format!("#{one} landed")), "{p2:?}");
    assert!(!p2[0].contains(&format!("#{two}")), "{p2:?}");
    assert_eq!(calls.len(), 2, "{calls:?}");
}

/// 51, 135; inbox spec, "A caller that never waits collects a failure" and
/// "A caller that never waits collects a landing": a caller that submits
/// with `sluice submit --no-wait --json` in no pane and never waits, rounds
/// run by `sluice step`, then `sluice inbox --json` twice, each handing the
/// blame over owed, then `sluice inbox --confirm --json`, which confirms
/// it, then `sluice inbox --json`, which gives nothing. `sluice step` is as
/// it was (record 0102).
#[test]
fn a_caller_that_never_waits_collects_a_failure_and_a_landing_by_inbox() {
    let s = Scratch::new("wt-inbox-caller");
    let submit = |dir: &Path| {
        let out = s
            .command(env!("CARGO_BIN_EXE_sluice"), dir)
            .args(["submit", "bolt/x", "--no-wait", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        id(&out)
    };
    let pending_for = |n: u64| s.logged("pending").iter().filter(|l| l["id"] == n).count();
    let none = |dir: &Path| {
        let again = inbox(&s, dir, &["--json"]);
        assert_eq!(again.status.code(), Some(0), "{}", text(&again));
        let v: serde_json::Value = serde_json::from_slice(&again.stdout).unwrap();
        assert_eq!(v, serde_json::json!({ "outcomes": [] }));
    };

    // A blame.
    let b = s.session("b", &[("broken", "x\n")]);
    let n = submit(&b);
    assert!(s.status(n)["from"].get("session").is_none());
    assert_eq!(s.decide(n)["state"], "failed");
    assert_eq!(pending_for(n), 1);
    for _ in 0..2 {
        let out = s.sluice(&["step", "bolt/x"]);
        assert!(out.status.success(), "{}", text(&out));
    }
    assert_eq!(pending_for(n), 1, "no further attempt");
    assert!(s.herdr.calls().is_empty());
    for _ in 0..2 {
        let e = collected_one(&s, &b);
        assert_eq!(e["id"], n, "{e}");
        assert_eq!(e["state"], "failed", "{e}");
        assert_eq!(e["failed"][0]["task"], "sh check.sh", "{e}");
        assert!(
            e["notice"].as_str().unwrap().contains(&format!("#{n}")),
            "{e}"
        );
        assert_eq!(e["delivery"]["state"], "owed", "{e}");
        assert_eq!(e["delivery"]["confirmed"], serde_json::Value::Null, "{e}");
    }
    let e = collected_one_by(&s, &b, &["--confirm", "--json"]);
    assert_eq!(e["id"], n, "{e}");
    assert_eq!(e["delivery"]["state"], "done", "{e}");
    assert!(e["delivery"]["confirmed"].is_string(), "{e}");
    none(&b);
    assert_eq!(pending_for(n), 1);

    // A landing.
    let a = s.session("a", &[("a.txt", "a\n")]);
    let n = submit(&a);
    assert_eq!(s.decide(n)["state"], "landed");
    assert_eq!(pending_for(n), 1);
    let e = collected_one(&s, &a);
    assert_eq!(e["id"], n, "{e}");
    assert_eq!(e["state"], "landed", "{e}");
    assert_eq!(e["landed_as"], s.tip().as_str(), "{e}");
    assert_eq!(e["delivery"]["state"], "owed", "{e}");
    let e = collected_one_by(&s, &a, &["--confirm", "--json"]);
    assert_eq!(e["delivery"]["state"], "done", "{e}");
    none(&a);
}

/// 124: a branch sluice lands on submitted by name, `sluice submit
/// <target> --head <branch>`, from wherever in the repository the command
/// runs, its head the branch's revision and its record no worktree's
/// (design D1, D2, D3, D9; record 0090; rule core.50).
mod by_name {
    use super::*;

    /// `sluice submit bolt/x --head bolt/w`, then `args`, run in `dir`.
    pub(super) fn submit(s: &Scratch, dir: &Path, args: &[&str]) -> Command {
        let mut c = s.command(env!("CARGO_BIN_EXE_sluice"), dir);
        c.args(["submit", "bolt/x", "--head", "bolt/w"]).args(args);
        c
    }

    /// From a bare clone's git directory, which has no files checked out
    /// and no worktree on `bolt/w`: answered as any submit that does not
    /// wait, its head `bolt/w`'s revision, and its record the branch as its
    /// work and its head, with no path and no session; no git status is
    /// run there (rule core.48).
    #[test]
    fn submits_from_a_bare_clone() {
        let s = Scratch::new("wt-by-name-bare");
        let head = s.repo.branch("bolt/w", "bolt/x", &[("w.txt", Some("w\n"))]);
        let bare = s.repo.dir().with_extension("bare");
        let _ = std::fs::remove_dir_all(&bare);
        s.repo.git(&[
            "clone",
            "-q",
            "--bare",
            s.repo.dir().to_str().unwrap(),
            bare.to_str().unwrap(),
        ]);
        let out = submit(&s, &bare, &["--no-wait", "--json"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{}", text(&out));
        assert!(!text(&out).contains("work tree"), "{}", text(&out));
        let n = id(&out);
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(
            v,
            serde_json::json!({ "id": n, "branch": "bolt/x", "ahead": [] })
        );
        let out = s
            .command(env!("CARGO_BIN_EXE_sluice"), &bare)
            .args(["status", &n.to_string(), "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        let record: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(record["head"], head.as_str(), "{record}");
        let from = &record["from"];
        assert_eq!(from["worktree"], "bolt/w", "{from}");
        assert_eq!(from["head"], "bolt/w", "{from}");
        assert_eq!(from["agent"], "bolt/w", "{from}");
        assert!(from.get("path").is_none(), "{from}");
        assert!(from.get("session").is_none(), "{from}");
    }

    /// From a worktree on another branch that holds work beyond its
    /// commits, in a herdr pane: the head is `bolt/w`'s revision, not the
    /// worktree's, nothing the worktree holds refuses it, and its record
    /// names the branch as its agent, its work and its head, and the host,
    /// with no path and no pane.
    #[test]
    fn submits_from_a_worktree_on_another_branch() {
        let s = Scratch::new("wt-by-name-elsewhere");
        std::fs::write(
            s.config.join("sluice/config.toml"),
            "[runs]\nmax = 2\n\n[peers]\nname = \"box-1\"\n",
        )
        .unwrap();
        let head = s.repo.branch("bolt/w", "bolt/x", &[("w.txt", Some("w\n"))]);
        let a = s.session("a", &[("a.txt", "a\n")]);
        std::fs::write(a.join("a.txt"), "changed\n").unwrap();
        std::fs::write(a.join("held.txt"), "held\n").unwrap();
        let socket = s.repo.dir().with_extension("herdr.sock");
        let out = submit(&s, &a, &["--no-wait", "--json"])
            .env("HERDR_PANE_ID", "w1:p2")
            .env("HERDR_SOCKET_PATH", &socket)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{}", text(&out));
        let record = s.status(id(&out));
        assert_eq!(record["head"], head.as_str(), "{record}");
        assert_ne!(record["head"], s.repo.rev("unit/a").as_str());
        assert_eq!(
            record["from"],
            serde_json::json!({
                "agent": "bolt/w",
                "worktree": "bolt/w",
                "head": "bolt/w",
                "host": "box-1",
            })
        );
    }

    /// 64, 67: a forecast of `bolt/w` and its submit by name read one head
    /// and one work: the forecast's `head` is the revision the submit then
    /// sends, and it leaves out the earlier submission by name, which the
    /// submit withdraws as a resubmission.
    #[test]
    fn the_forecast_names_what_the_submit_sends() {
        let s = Scratch::new("wt-by-name-forecast");
        s.repo.branch("bolt/w", "bolt/x", &[("w.txt", Some("w\n"))]);
        // A fix to land on bolt/w once it is submitted, made on a branch no
        // policy protects.
        let fix = s
            .repo
            .branch("next/w", "bolt/w", &[("w.txt", Some("fixed\n"))]);
        let a = s.session("a", &[("a.txt", "a\n")]);
        let first = id(&submit(&s, &a, &["--no-wait", "--json"]).output().unwrap());
        s.repo.git(&[
            "-c",
            "core.hooksPath=/dev/null",
            "update-ref",
            "refs/heads/bolt/w",
            &fix,
        ]);
        let out = s
            .command(env!("CARGO_BIN_EXE_sluice"), &a)
            .args(["submit", "bolt/x", "--check", "--head", "bolt/w", "--json"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{}", text(&out));
        let forecast: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(forecast["head"], fix.as_str(), "{forecast}");
        assert_eq!(forecast["ahead"], serde_json::json!([]), "{forecast}");
        let out = submit(&s, &a, &["--no-wait", "--json"]).output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{}", text(&out));
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["ahead"], serde_json::json!([]), "{v}");
        let n = id(&out);
        assert_eq!(s.status(n)["head"], forecast["head"]);
        let withdrawn = s.status(first);
        assert_eq!(withdrawn["state"], "withdrawn", "{withdrawn}");
        assert_eq!(withdrawn["withdrawn"]["by"], "resubmission", "{withdrawn}");
    }

    /// D9, 135: a submit by name that waits runs `bolt/x`'s rounds while it
    /// waits, with no `sluice run` on the host, says `submission <n> to
    /// <target> from <branch>`, reports the landing and exits 0. It
    /// hands over nothing: nothing is typed into its pane, no `pending`,
    /// `handed` or `confirmed` line is logged, and the submission gives no
    /// `delivery`.
    #[test]
    fn a_waiting_submit_by_name_lands() {
        let s = Scratch::new("wt-by-name-waits");
        s.repo.branch("bolt/w", "bolt/x", &[("w.txt", Some("w\n"))]);
        let socket = s.repo.dir().with_extension("herdr.sock");
        let out = submit(&s, s.repo.dir(), &[])
            .env("HERDR_PANE_ID", "w1:p2")
            .env("HERDR_SOCKET_PATH", &socket)
            .output()
            .unwrap();
        let said = text(&out);
        assert_eq!(out.status.code(), Some(0), "{said}");
        let first = said.lines().next().unwrap_or_default();
        let n: u64 = first
            .strip_prefix("submission ")
            .and_then(|l| l.strip_suffix(" to bolt/x from bolt/w · nothing queued ahead"))
            .unwrap_or_else(|| panic!("{said}"))
            .parse()
            .unwrap();
        let record = s.status(n);
        assert_eq!(record["state"], "landed", "{record}");
        let landed = record["landed_as"].as_str().unwrap();
        assert!(
            said.contains(&format!("passed · landed as {}", &landed[..7])),
            "{said}"
        );
        assert_eq!(s.tip(), landed);
        assert!(record.get("delivery").is_none(), "{record}");
        for event in ["pending", "handed", "confirmed"] {
            assert!(s.logged(event).is_empty(), "a {event} line");
        }
        assert!(s.herdr.calls().is_empty(), "{:?}", s.herdr.calls());
    }

    /// 26, core.43: a submit by name with `--subject`, run in the git
    /// directory, lands `bolt/w` with the subject as its landing commit's
    /// first line, as any submit's subject names its landing, never the
    /// branch's own commit message; the record keeps the subject.
    #[test]
    fn a_subject_names_the_landing_by_name() {
        let s = Scratch::new("wt-by-name-subject");
        s.repo.branch("bolt/w", "bolt/x", &[("w.txt", Some("w\n"))]);
        let made = s.repo.git(&["log", "-1", "--format=%s", "bolt/w"]);
        let out = submit(&s, &s.repo.dir().join(".git"), &["--subject", "The w work"])
            .output()
            .unwrap();
        let said = text(&out);
        assert_eq!(out.status.code(), Some(0), "{said}");
        let record = s.status(1);
        assert_eq!(record["state"], "landed", "{record}");
        assert_eq!(record["subject"], "The w work", "{record}");
        let landed = record["landed_as"].as_str().unwrap();
        assert_eq!(s.tip(), landed);
        let first = s.repo.git(&["log", "-1", "--format=%s", landed]);
        assert_eq!(first, "chore(land): #1 The w work");
        assert!(!first.contains(made.trim()), "{first}");
    }
}

/// 124, D10: the way on of a branch submitted by name that failed or
/// conflicts is its caller's, a fix landed on the branch and the submit by
/// name again, in a waiting submit's report and in its status; never an
/// agent working in the branch, where none works.
mod by_name_way_on {
    use super::*;

    /// A check failing on `bolt/w`'s work: the waiting submit by name exits
    /// 1 and names a fix landed on `bolt/w`, then `sluice submit bolt/x
    /// --head bolt/w`; `sluice status <n>` names the same.
    #[test]
    fn a_failure() {
        let s = Scratch::new("wt-by-name-fails");
        s.repo
            .branch("bolt/w", "bolt/x", &[("broken", Some("broken\n"))]);
        let out = by_name::submit(&s, s.repo.dir(), &[]).output().unwrap();
        let said = text(&out);
        assert_eq!(out.status.code(), Some(1), "{said}");
        let way = "once a fix lands on bolt/w, sluice submit bolt/x --head bolt/w submits it again";
        assert!(said.contains(way), "{said}");
        assert!(!said.contains("agent working"), "{said}");
        let n = s.subs_by_name()[0]["id"].as_u64().unwrap();
        let out = s.sluice(&["status", &n.to_string()]);
        let said = text(&out);
        assert!(out.status.success(), "{said}");
        assert!(said.contains(way), "{said}");
        assert!(!said.contains("agent working"), "{said}");
    }

    /// `bolt/w`, cut before a landing on `bolt/x` that changed the same
    /// file: the waiting submit by name exits 2, and its way on, found
    /// while its combination was formed and so never tested, merges
    /// `bolt/x` at its revision before the submit by name again.
    #[test]
    fn a_conflict() {
        let s = Scratch::new("wt-by-name-conflicts");
        s.repo
            .branch("bolt/w", "bolt/x", &[("same.txt", Some("w\n"))]);
        let a = s.session("a", &[("same.txt", "a\n")]);
        let out = s.wt_submit(&a, &["--no-remove"]);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out));
        let tip = s.tip();
        let out = by_name::submit(&s, s.repo.dir(), &[]).output().unwrap();
        let said = text(&out);
        assert_eq!(out.status.code(), Some(2), "{said}");
        assert!(said.contains("conflict: same.txt"), "{said}");
        let way = format!(
            "once a fix that merges bolt/x at {} lands on bolt/w, sluice submit bolt/x --head \
             bolt/w submits it again",
            &tip[..7]
        );
        assert!(said.contains(&way), "{said}");
        assert!(!said.contains("agent working"), "{said}");
        let n = s.subs_by_name()[0]["id"].as_u64().unwrap();
        let out = s.sluice(&["status", &n.to_string()]);
        assert!(text(&out).contains(&way), "{}", text(&out));
    }

    impl Scratch {
        /// Every submission `bolt/x` has had of `bolt/w` by name.
        pub(super) fn subs_by_name(&self) -> Vec<serde_json::Value> {
            let out = self.sluice(&["status", "bolt/x", "--json", "--all"]);
            assert!(out.status.success(), "{}", text(&out));
            let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            v["submissions"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|x| x["from"]["head"] == "bolt/w" && x["from"]["worktree"] == "bolt/w")
                .collect()
        }
    }
}

/// 51, 124, D8: a status gives each decided submission the words `sluice
/// inbox` gives it, beside its record as it is; a submission by name,
/// which no inbox takes, is read so.
mod status_gives_the_inbox_words {
    use super::*;

    /// `bolt/w` by name, blamed for `sh check.sh`: queued, it has no
    /// notice; decided, `sluice status bolt/x --all --json` and `sluice
    /// status <n> --json` give it the inbox's notice, its `failed` and the
    /// revision of `bolt/x` it was tested on. Submit-by-name spec, "Blamed,
    /// and no inbox takes it": the log holds its blame and no `pending`,
    /// `handed` or `confirmed` line, it gives no `delivery`, and `sluice
    /// inbox --json` in the git directory, the main checkout and a worktree
    /// on `bolt/w` each give nothing (record 0102; rule core.50).
    #[test]
    fn a_failure_by_name() {
        let s = Scratch::new("wt-status-words-fails");
        s.repo
            .branch("bolt/w", "bolt/x", &[("broken", Some("broken\n"))]);
        let tip = s.tip();
        let out = by_name::submit(&s, s.repo.dir(), &["--no-wait", "--json"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{}", text(&out));
        let n = id(&out);
        let queued = s.status(n);
        assert_eq!(queued["state"], "queued", "{queued}");
        assert!(queued.get("notice").is_none(), "{queued}");
        s.decide(n);
        let notice =
            format!("[sluice] #{n} failed on bolt/x: sh check.sh. sluice wait {n} shows why.");
        let e = s.subs_by_name().into_iter().find(|x| x["id"] == n).unwrap();
        assert_eq!(e["state"], "failed", "{e}");
        assert_eq!(e["from"]["head"], "bolt/w", "{e}");
        assert_eq!(e["notice"], notice.as_str(), "{e}");
        assert_eq!(e["failed"][0]["task"], "sh check.sh", "{e}");
        assert_eq!(e["tested"]["on"], tip.as_str(), "{e}");
        let one = s.status(n);
        assert_eq!(one["notice"], notice.as_str(), "{one}");
        assert_eq!(one["tested"]["on"], tip.as_str(), "{one}");
        assert!(one.get("delivery").is_none(), "{one}");
        assert!(s.logged("blamed").iter().any(|l| l["id"] == n));
        for event in ["pending", "handed", "confirmed"] {
            assert!(
                s.logged(event).iter().all(|l| l["id"] != n),
                "a {event} line"
            );
        }
        let w = s.repo.dir().with_extension("s-w");
        let _ = std::fs::remove_dir_all(&w);
        s.repo
            .git(&["worktree", "add", "-q", w.to_str().unwrap(), "bolt/w"]);
        for dir in [
            s.repo.dir().join(".git"),
            s.repo.dir().to_path_buf(),
            w.clone(),
        ] {
            let out = inbox(&s, &dir, &["--json"]);
            assert_eq!(out.status.code(), Some(0), "{}", text(&out));
            let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(
                v,
                serde_json::json!({ "outcomes": [] }),
                "{}",
                dir.display()
            );
        }
        // `sluice confirm` finds no outcome owed under its key, in the
        // worktree on `bolt/w` and as the operator.
        let key = format!("{n}:failed");
        for (dir, args) in [
            (w.clone(), vec![key.as_str()]),
            (
                s.repo.dir().to_path_buf(),
                vec![key.as_str(), "--as", "operator"],
            ),
        ] {
            let out = confirm(&s, &dir, &args);
            assert_eq!(out.status.code(), Some(sluice::FAILED), "{}", text(&out));
            assert!(
                text(&out).contains(&format!("no outcome is owed under {key}")),
                "{}",
                text(&out)
            );
        }
        assert!(s.logged("confirmed").iter().all(|l| l["id"] != n));
    }

    /// `bolt/w` by name, conflicting in `same.txt` with a session's landing:
    /// `sluice status <n> --json` gives it the inbox's notice and its
    /// `conflict.paths`; the session's landing has the inbox's notice too.
    #[test]
    fn a_conflict_by_name() {
        let s = Scratch::new("wt-status-words-conflicts");
        s.repo
            .branch("bolt/w", "bolt/x", &[("same.txt", Some("w\n"))]);
        let a = s.session("a", &[("same.txt", "a\n")]);
        let out = s.wt_submit(&a, &["--no-remove"]);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out));
        let out = by_name::submit(&s, s.repo.dir(), &[]).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{}", text(&out));
        let n = s.subs_by_name()[0]["id"].as_u64().unwrap();
        let e = s.status(n);
        assert_eq!(e["state"], "conflict", "{e}");
        assert_eq!(
            e["conflict"]["paths"],
            serde_json::json!(["same.txt"]),
            "{e}"
        );
        let notice = e["notice"]
            .as_str()
            .unwrap_or_else(|| panic!("no notice: {e}"));
        assert!(
            notice.starts_with(&format!("[sluice] #{n} conflicts on bolt/x")),
            "{e}"
        );
        assert!(notice.contains("in same.txt"), "{e}");
        let landed = &s.subs_from("a")[0];
        let words = format!("[sluice] #{} landed on bolt/x as ", landed["id"]);
        assert!(
            landed["notice"]
                .as_str()
                .is_some_and(|w| w.starts_with(&words)),
            "{landed}"
        );
    }
}
