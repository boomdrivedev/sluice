//! A watch ends where its repository has no sluice (129), through the
//! binary on scratch repositories: no records, and no branch, the host's
//! own or a remote's tracking branch as last fetched, holding the policy.
//! Where one does, the watch goes on. Needs git, so it is in the
//! check-tools tier.

#![cfg(feature = "tools")]

#[path = "support/mod.rs"]
mod support;

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use support::{OnDisk, Repo, Scratch};

const POLICY: &str = "[tests]\nrunner = \"command\"\ncommand = \"true\"\n";

/// How long a watch that ends at its first tick may take to.
const ENDS_WITHIN: Duration = Duration::from_secs(10);

/// `sluice overview --json --watch` run in a worktree, with the host's
/// state and config its own: ended when dropped.
struct Watch {
    child: Child,
    lines: mpsc::Receiver<String>,
    /// Where what it says on stderr is kept.
    stderr: PathBuf,
    _own: Option<(Scratch, Scratch)>,
}

impl Watch {
    fn start(dir: &Path) -> Watch {
        let config = Scratch::new("watch-config");
        let state = Scratch::new("watch-state");
        let mut c = Command::new(env!("CARGO_BIN_EXE_sluice"));
        support::isolated(&mut c)
            .current_dir(dir)
            .env("XDG_CONFIG_HOME", config.path())
            .env("XDG_STATE_HOME", state.path())
            .env_remove("SLUICE_AGENT")
            .env_remove("SLUICE_OTEL")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null());
        let stderr = state.path().join("stderr");
        Watch::spawn(c, stderr, Some((state, config)))
    }

    /// A watch of `d`'s repository, with its config, state and counting
    /// git.
    fn of(d: &OnDisk) -> Watch {
        let stderr = d.state.path().join("watch-stderr");
        Watch::spawn(d.command(d.repo.dir()), stderr, None)
    }

    fn spawn(c: Command, stderr: PathBuf, own: Option<(Scratch, Scratch)>) -> Watch {
        Watch::spawn_with(c, stderr, own, &[])
    }

    /// [`Watch::spawn`], with `more` after its arguments.
    fn spawn_with(
        mut c: Command,
        stderr: PathBuf,
        own: Option<(Scratch, Scratch)>,
        more: &[&str],
    ) -> Watch {
        let mut child = c
            .args(["overview", "--json", "--watch"])
            .args(more)
            .stdout(Stdio::piped())
            .stderr(File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        let (send, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in out.lines().map_while(Result::ok) {
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Watch {
            child,
            lines,
            stderr,
            _own: own,
        }
    }

    /// How it ended, once it has within `within`; `None` while it runs.
    fn ended_within(&mut self, within: Duration) -> Option<ExitStatus> {
        let until = Instant::now() + within;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return Some(status);
            }
            if Instant::now() >= until {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Every line it printed, once its output has closed.
    fn all_lines(&self) -> Vec<serde_json::Value> {
        self.lines.iter().map(|l| parse(&l)).collect()
    }

    /// Every line it printed within `within`, while it goes on.
    fn lines_within(&self, within: Duration) -> Vec<serde_json::Value> {
        let until = Instant::now() + within;
        let mut lines = Vec::new();
        while let Some(left) = until.checked_duration_since(Instant::now()) {
            match self.lines.recv_timeout(left) {
                Ok(l) => lines.push(parse(&l)),
                Err(_) => break,
            }
        }
        lines
    }

    /// The first line it printed.
    fn first_line(&self) -> serde_json::Value {
        let line = self
            .lines
            .recv_timeout(ENDS_WITHIN)
            .unwrap_or_else(|e| panic!("no line: {e}; {}", self.stderr()));
        parse(&line)
    }

    /// What it said on stderr so far.
    fn stderr(&self) -> String {
        std::fs::read_to_string(&self.stderr).unwrap_or_default()
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn parse(line: &str) -> serde_json::Value {
    serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}"))
}

/// The watch ends at its first tick, exit 0, on one line: the overview of
/// no branches with `"set_up": false`.
fn assert_ends_with_no_sluice(dir: &Path) {
    let mut watch = Watch::start(dir);
    let status = watch.ended_within(ENDS_WITHIN);
    assert!(
        status.is_some_and(|s| s.success()),
        "{status:?}: {}",
        watch.stderr()
    );
    let lines = watch.all_lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["schema"], "sluice/overview@1", "{}", lines[0]);
    assert_eq!(lines[0]["branches"], serde_json::json!([]), "{}", lines[0]);
    assert_eq!(lines[0]["set_up"], false, "{}", lines[0]);
}

/// The watch prints the overview with no `set_up` field, and is still
/// running three seconds on.
fn assert_goes_on(dir: &Path) {
    let mut watch = Watch::start(dir);
    let first = watch.first_line();
    assert_eq!(first["branches"], serde_json::json!([]), "{first}");
    assert!(first.get("set_up").is_none(), "{first}");
    let status = watch.ended_within(Duration::from_secs(3));
    assert!(status.is_none(), "{status:?}: {}", watch.stderr());
}

/// 129: a repository sluice was never set up in.
#[test]
fn a_watch_ends_where_no_branch_holds_the_policy() {
    let repo = Repo::new("no-sluice");
    assert_ends_with_no_sluice(repo.dir());
}

/// 129, rule core.27: the policy on `origin/main` as last fetched, and on
/// no branch of the host's own, is sluice set up; the remote is nowhere,
/// so nothing could be fetched.
#[test]
fn a_watch_goes_on_where_the_policy_is_only_on_a_remotes_branch() {
    let repo = Repo::new("remote-policy");
    let fetched = repo.branch("fetched", "main", &[(".config/sluice.toml", Some(POLICY))]);
    repo.git(&["switch", "-q", "main"]);
    repo.git(&["branch", "-q", "-D", "fetched"]);
    repo.git(&["remote", "add", "origin", "/nowhere/origin.git"]);
    repo.git(&["update-ref", "refs/remotes/origin/main", &fetched]);
    assert_goes_on(repo.dir());
}

/// 129: a policy written in a worktree and committed on no branch is no
/// policy a branch holds.
#[test]
fn a_watch_ends_where_the_policy_is_in_the_working_copy_alone() {
    let repo = Repo::new("working-copy");
    repo.write(".config/sluice.toml", POLICY);
    repo.git(&["add", "-A"]);
    assert_ends_with_no_sluice(repo.dir());
}

/// 129: a policy on a branch other than HEAD's, as on a bolt, is sluice
/// set up.
#[test]
fn a_watch_goes_on_where_the_policy_is_on_a_branch_other_than_heads() {
    let repo = Repo::new("other-branch");
    repo.branch("bolt/x", "main", &[(".config/sluice.toml", Some(POLICY))]);
    repo.git(&["switch", "-q", "main"]);
    assert_goes_on(repo.dir());
}

/// overview-reader, "A read that changes nothing" (129; the changes
/// `sluice-host-runs-on-a-tokio-edge` and `sluice-host-answers-every-read`,
/// design D6): over a repository where nothing moves, the watch prints
/// the document once and nothing more for three seconds; a ref that moves
/// is a file event, and `sluice host` reads the repository again, which
/// changes nothing of the document but its time, so the watch prints
/// nothing.
#[test]
fn a_readers_marks_move_while_its_document_stands() {
    let repo = Repo::new("marks");
    repo.branch("bolt/x", "main", &[(".config/sluice.toml", Some(POLICY))]);
    repo.git(&["switch", "-q", "main"]);
    let watch = Watch::start(repo.dir());
    let first = watch.first_line();
    assert_eq!(first["tick"], 1, "{first}");
    std::thread::sleep(Duration::from_millis(3500));
    assert!(
        watch.lines.try_recv().is_err(),
        "a line though nothing moved"
    );
    let head = repo.rev("HEAD");
    repo.git(&["update-ref", "refs/heads/moved", &head]);
    let again = watch.lines_within(Duration::from_secs(3));
    assert!(
        again.is_empty(),
        "a line for a read that changed nothing: {again:?}"
    );
}

/// A policy a branch's records can be kept under.
const MAIN: &str =
    "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\n[branches.main]\ntimeout = \"1m\"\n";

/// How long a watch is watched for further starts: two ticks at least,
/// the watch pausing a second between them.
const TICKS: Duration = Duration::from_millis(2500);

/// overview-reader, "Set up with nothing submitted, watched for many
/// ticks": no records, `main` holding the policy; the watch, the reader,
/// goes on, and asks git nothing once it has printed its first line
/// (129).
#[test]
fn a_set_up_repository_with_nothing_submitted_is_watched_with_no_git() {
    let d = OnDisk::new("watched-unsubmitted", MAIN);
    let mut watch = Watch::of(&d);
    let first = watch.first_line();
    assert_eq!(first["branches"], serde_json::json!([]), "{first}");
    assert!(first.get("set_up").is_none(), "{first}");
    let before = d.git.count();
    let status = watch.ended_within(TICKS);
    assert!(status.is_none(), "{status:?}: {}", watch.stderr());
    assert_eq!(d.git.count(), before, "{:?}", &d.git.starts()[before..]);
}

/// overview-reader, "The policy removed while a watch runs": `main`, the
/// one branch holding the policy, moves to a commit removing it; the
/// watch asks again at its next tick and ends on `"set_up": false` (129).
#[test]
fn a_watch_ends_once_the_policy_is_removed_while_it_runs() {
    let d = OnDisk::new("watched-removed", MAIN);
    let mut watch = Watch::of(&d);
    let first = watch.first_line();
    assert!(first.get("set_up").is_none(), "{first}");
    d.repo.git(&["rm", "-q", ".config/sluice.toml"]);
    d.repo.git(&["commit", "-q", "-m", "no sluice"]);
    let status = watch.ended_within(ENDS_WITHIN);
    assert!(
        status.is_some_and(|s| s.success()),
        "{status:?}: {}",
        watch.stderr()
    );
    let lines = watch.all_lines();
    let last = lines.last().expect("a line after the first");
    assert_eq!(last["branches"], serde_json::json!([]), "{last}");
    assert_eq!(last["set_up"], false, "{last}");
}

/// reader-tick, "A watch of a set-up repository with nothing queued" and
/// "Held worktrees are named without git": with one landed submission
/// and the repository's own worktree on `main`, every line the watch, the
/// reader, prints names that worktree under `sessions`, and it starts git
/// no more after its first line (129).
#[test]
fn a_watch_with_nothing_queued_names_held_worktrees_with_no_git() {
    let d = OnDisk::new("watched-idle", MAIN);
    d.land("a");
    let watch = Watch::of(&d);
    let first = watch.first_line();
    let before = d.git.count();
    let mut lines = vec![first];
    lines.extend(watch.lines_within(TICKS));
    let own = std::fs::canonicalize(d.repo.dir()).unwrap();
    for line in &lines {
        assert_eq!(line["branches"][0]["branch"], "main", "{line}");
        let names = line["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["branch"] == "main" && s["worktree"] == own.to_str().unwrap());
        assert!(names, "{line}");
    }
    assert_eq!(d.git.count(), before, "{:?}", &d.git.starts()[before..]);
}

/// reader-tick, "Policy read again once the branch moves": `main` lands a
/// revision whose policy sets `parallel = 4` while a watch, the reader,
/// runs; its next line shows `main` testing up to 4 at once (56, 129).
#[test]
fn a_watch_reads_the_policy_again_once_the_branch_moves() {
    let d = OnDisk::new("watched-moved", &format!("{MAIN}parallel = 2\n"));
    d.land("a");
    let watch = Watch::of(&d);
    let first = watch.first_line();
    assert_eq!(first["branches"][0]["branch"], "main", "{first}");
    assert_eq!(first["branches"][0]["parallel"], 2, "{first}");
    let policy = format!("{MAIN}parallel = 4\n");
    d.land_with("b", &[(".config/sluice.toml", &policy)]);
    let lines = watch.lines_within(ENDS_WITHIN);
    let last = lines.last().expect("a line once main moved");
    assert_eq!(last["branches"][0]["parallel"], 4, "{lines:?}");
}

/// 129, 148: `main` written by a command that is not sluice's, the
/// refusal counted in its state; a watch, and a second watch beside it,
/// both following `sluice host`: each document gives `main` the cause.
#[test]
fn a_followers_overview_carries_refused_writes() {
    let d = OnDisk::new("watched-refused", MAIN);
    d.land("a");
    let (old, new) = (d.repo.rev("main"), d.repo.rev("main~1"));
    let refused = support::git_command()
        .current_dir(d.repo.dir())
        .env("XDG_CONFIG_HOME", d.config.path())
        .env("XDG_STATE_HOME", d.state.path())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .args(["update-ref", "refs/heads/main", &new, &old])
        .output()
        .unwrap();
    assert!(!refused.status.success(), "the write went through");

    let reader = Watch::of(&d);
    let read = reader.first_line();
    let causes = &read["branches"][0]["refused_writes"];
    assert_eq!(causes.as_array().map(Vec::len), Some(1), "{read}");
    assert_eq!(causes[0]["times"], 1, "{read}");
    assert!(
        causes[0]["command"]
            .as_str()
            .is_some_and(|c| c.ends_with("update-ref refs/heads/main <revision> <revision>")),
        "{read}"
    );

    let follower = Watch::of(&d);
    let followed = follower.first_line();
    assert_eq!(followed["branches"][0]["branch"], "main", "{followed}");
    assert_eq!(
        followed["branches"][0]["refused_writes"], *causes,
        "{followed}"
    );
}

/// `sluice overview --json --watch --stdin` in `d`'s repository, its input
/// held by the test, ended when dropped (the change
/// `sluice-host-answers-every-read`, design D7).
struct Served {
    watch: Watch,
    input: Option<std::process::ChildStdin>,
}

impl Served {
    fn start(d: &OnDisk) -> Served {
        let stderr = d.state.path().join("served-stderr");
        let mut c = d.command(d.repo.dir());
        c.stdin(Stdio::piped());
        let mut watch = Watch::spawn_with(c, stderr, None, &["--stdin"]);
        let input = watch.child.stdin.take();
        Served { watch, input }
    }

    fn say(&mut self, line: &str) {
        use std::io::Write;
        let input = self.input.as_mut().expect("its input is open");
        writeln!(input, "{line}").unwrap();
        input.flush().unwrap();
    }

    /// Its input closed.
    fn close(&mut self) {
        self.input = None;
    }
}

/// resident-subscriptions, "Two folders from the input": the watch reads
/// `add <a>` and `add <b>`, for worktrees of two repositories, prints one
/// line for each with its `folder`, and holds one connection to `sluice
/// host`.
#[test]
fn the_watch_serves_the_folders_on_its_input() {
    let d = OnDisk::new("served-two", MAIN);
    let b = d.another("served-two-b", MAIN);
    let mut served = Served::start(&d);
    served.say(&format!("add {}", d.repo.dir().display()));
    served.say(&format!("add {}", b.dir().display()));
    let mut folders = std::collections::BTreeSet::new();
    let until = Instant::now() + Duration::from_secs(20);
    while folders.len() < 2 && Instant::now() < until {
        for line in served.watch.lines_within(Duration::from_millis(500)) {
            assert_eq!(line["schema"], "sluice/overview@1", "{line}");
            folders.insert(line["folder"].as_str().unwrap().to_string());
        }
    }
    let want: std::collections::BTreeSet<String> = [d.repo.dir(), b.dir()]
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert_eq!(folders, want, "{}", served.watch.stderr());
    // One connection: the host process accepted one for the watch, which
    // subscribed once.
    let log = std::fs::read_to_string(d.state.path().join("sluice/host/log")).unwrap_or_default();
    assert!(!log.contains("a connection:"), "{log}");
}

/// resident-subscriptions, "A forecast asked on the subscription": a line
/// `forecast main <worktree>` on the watch's input prints the worktree's
/// forecast, with its `folder` and `branch`, on the same connection, and
/// the watch starts no other process of sluice's for it. The test waits
/// for the line, and bounds only how long it waits to fail.
#[test]
fn the_watch_prints_a_forecast_asked_on_its_input() {
    let d = OnDisk::new("served-forecast", MAIN);
    let head = d
        .repo
        .branch("work", "main", &[("work.txt", Some("work\n"))]);
    let work = Scratch::new("served-forecast-work");
    let at = work.path().join("w");
    d.repo
        .git(&["worktree", "add", "-q", at.to_str().unwrap(), "work"]);
    let mut served = Served::start(&d);
    served.say(&format!("add {}", d.repo.dir().display()));
    assert_eq!(served.watch.first_line()["schema"], "sluice/overview@1");
    served.say(&format!("forecast main {}", at.display()));
    let until = Instant::now() + Duration::from_secs(60);
    let mut got = None;
    while got.is_none() && Instant::now() < until {
        got = served
            .watch
            .lines_within(Duration::from_millis(500))
            .into_iter()
            .find(|l| l.get("forecast").is_some());
    }
    let line = got.unwrap_or_else(|| panic!("no forecast: {}", served.watch.stderr()));
    assert_eq!(line["folder"], at.display().to_string(), "{line}");
    assert_eq!(line["branch"], "main", "{line}");
    assert_eq!(
        line["forecast"]["conflict"],
        serde_json::Value::Null,
        "{line}"
    );
    assert_eq!(line["forecast"]["head"], head, "{line}");
}

/// resident-subscriptions, "The watch command serves the folders named on
/// its input": its input ends while it serves no folder: it ends, 0.
#[test]
fn the_watch_ends_when_its_input_ends_with_no_folder() {
    let d = OnDisk::new("served-ends", MAIN);
    let mut served = Served::start(&d);
    served.say(&format!("add {}", d.repo.dir().display()));
    assert_eq!(served.watch.first_line()["schema"], "sluice/overview@1");
    served.say(&format!("remove {}", d.repo.dir().display()));
    served.close();
    let status = served.watch.ended_within(ENDS_WITHIN);
    assert!(
        status.is_some_and(|s| s.success()),
        "{status:?}: {}",
        served.watch.stderr()
    );
}

/// Its input ends while it serves a folder: the watch goes on.
#[test]
fn the_watch_goes_on_when_its_input_ends_with_folders() {
    let d = OnDisk::new("served-goes-on", MAIN);
    let mut served = Served::start(&d);
    served.say(&format!("add {}", d.repo.dir().display()));
    assert_eq!(served.watch.first_line()["schema"], "sluice/overview@1");
    served.close();
    assert_eq!(
        served.watch.ended_within(TICKS),
        None,
        "{}",
        served.watch.stderr()
    );
}

/// `sluice host` ends under the watch: the watch exits 3, as a read that
/// `sluice host` did not answer.
#[test]
fn the_watch_exits_3_when_sluice_host_ends() {
    let d = OnDisk::new("served-host-ends", MAIN);
    let mut served = Served::start(&d);
    served.say(&format!("add {}", d.repo.dir().display()));
    let first = served.watch.first_line();
    let pid = first["host_process"]["pid"]
        .as_i64()
        .expect("a host process") as i32;
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid),
        nix::sys::signal::Signal::SIGKILL,
    );
    let status = served.watch.ended_within(ENDS_WITHIN);
    assert_eq!(
        status.and_then(|s| s.code()),
        Some(3),
        "{}",
        served.watch.stderr()
    );
    assert!(
        served
            .watch
            .stderr()
            .contains("sluice's host process did not answer"),
        "{}",
        served.watch.stderr()
    );
}

/// resident-subscriptions, "A watch of one folder": with no `--stdin`,
/// each line is the overview as a watch printed it before this change,
/// with no `folder`.
#[test]
fn a_watch_of_one_folder_prints_as_before() {
    let d = OnDisk::new("watch-one-folder", MAIN);
    let watch = Watch::of(&d);
    let first = watch.first_line();
    assert_eq!(first["schema"], "sluice/overview@1", "{first}");
    assert!(first.get("folder").is_none(), "{first}");
    assert_eq!(first["tick"], 1, "{first}");
    assert!(first.get("set_up").is_none(), "{first}");
}
