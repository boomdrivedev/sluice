//! The worktrunk driver end to end through the real binaries: `wt
//! submit` runs `wt-submit`, which runs `sluice submit` from the session's
//! worktree; section 7's acceptance tests in
//! `docs/plans/phase-1-integrations.md`. A scratch repository gates
//! `bolt/*` on one command, in git-worktree places. Needs git and wt on
//! the path, so it is in the check-tools tier.

#![cfg(feature = "tools")]

#[path = "../support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use sluice::profiles::worktrunk::Worktrunk;
use sluice_seams::Driver as _;
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

/// Its tests end while a run may go: its host process is stopped as it
/// ends, which ends the runs (`a-leaked-child-fails-its-test@9a82a5147ba6`).
impl Drop for Scratch {
    fn drop(&mut self) {
        support::guard::stop_host_processes(&self.state);
    }
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

/// The worktrunk driver against real git (2, 3, 51; record 0099; rule
/// core.48): what it reads of a session's worktree and of a folder with
/// no files checked out.
///
/// A worktree on a branch is its session: its head, the branch as its
/// work, its agent the harness names else the branch, its top level as
/// its path, this host, its herdr pane, and every path held beyond its
/// commits as git status gives it. A detached worktree's session is on no
/// branch, and it is no one's recipient. A folder with no files checked
/// out, a bare clone or a git directory, is the worktree on the branch its
/// `HEAD` names, with no path, and holds nothing beyond its commits,
/// git status never run; a branch with no commit yet is read all the
/// same, and a `HEAD` on no branch is no one's.
#[test]
fn the_driver_reads_a_repository_as_git_holds_it() {
    let s = Scratch::new("wt-driver");
    let driver = Worktrunk::new("h");
    let vars = |pairs: &[(&str, &str)]| -> std::collections::BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };
    let as_caller = |pairs: &[(&str, &str)], f: &dyn Fn() -> sluice_seams::Session| {
        sluice::caller::as_caller(vars(pairs), f)
    };

    // A session's worktree, held beyond its commits, in a pane.
    let a = s.session("a", &[("a.txt", "a\n"), ("b.txt", "b\n")]);
    std::fs::write(a.join("a.txt"), "changed\n").unwrap();
    std::fs::write(a.join("new.txt"), "n\n").unwrap();
    let at = a.to_str().unwrap();
    let in_pane = [
        ("HERDR_PANE_ID", "w1:p2"),
        ("HERDR_SOCKET_PATH", "/run/herdr.sock"),
    ];
    let session = as_caller(&in_pane, &|| driver.session(at).unwrap());
    assert_eq!(session.head.0, s.repo.rev("unit/a"));
    assert!(!session.detached);
    assert_eq!(session.dirty, [" M a.txt", "?? new.txt"]);
    let from = &session.from;
    let top = std::fs::canonicalize(&a).unwrap();
    assert_eq!(from.worktree, "unit/a");
    assert_eq!(from.agent, "unit/a");
    assert_eq!(from.path.as_deref(), top.to_str());
    assert_eq!(from.host.as_deref(), Some("h"));
    assert_eq!(from.session.as_deref(), Some("herdr:w1:p2@/run/herdr.sock"));
    let named = as_caller(&[("SLUICE_AGENT", "crw-1-unit-1")], &|| {
        driver.session(at).unwrap()
    });
    assert_eq!(named.from.agent, "crw-1-unit-1");
    assert_eq!(named.from.session, None);

    // Its own git directory: the worktree with no path.
    let own = s.repo.git(&["-C", at, "rev-parse", "--absolute-git-dir"]);
    let me = driver.recipient(&own).unwrap().expect("a's own");
    assert_eq!((me.worktree.as_str(), me.path.as_deref()), ("unit/a", None));

    // Detached: on no branch, and no one's.
    let d = s.session("d", &[("d.txt", "d\n")]);
    s.repo
        .git(&["-C", d.to_str().unwrap(), "switch", "-q", "--detach"]);
    assert!(driver.session(d.to_str().unwrap()).unwrap().detached);
    assert_eq!(driver.recipient(d.to_str().unwrap()).unwrap(), None);

    // A bare clone and the repository's git directory.
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
    for dir in [bare.clone(), s.repo.dir().join(".git")] {
        let me = sluice::caller::as_caller(vars(&[]), || driver.recipient(dir.to_str().unwrap()))
            .unwrap()
            .unwrap_or_else(|| panic!("{} read as no one's", dir.display()));
        assert_eq!(me.worktree, "main", "{}", dir.display());
        assert_eq!(me.path, None, "{}", dir.display());
        assert_eq!(me.head, None);
        assert_eq!(me.agent, "main");
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
    assert_eq!(
        (me.worktree.as_str(), me.path.as_deref()),
        ("nothing", None)
    );
    let tip = s.repo.rev("main");
    s.repo
        .git(&["--git-dir", b, "update-ref", "--no-deref", "HEAD", &tip]);
    assert_eq!(driver.recipient(b).unwrap(), None);

    // The checkout on main: its path, and what it holds beyond.
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
