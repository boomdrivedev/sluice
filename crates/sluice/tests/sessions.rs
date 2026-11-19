//! The host's session runs on a real filesystem (session-runs-1 clauses
//! 89 to 91): records beside the host's blocks of ports, live while a
//! supervisor holds their run directory, begun under the host's lock. It
//! needs no tool, so it is in the fast tier.

mod support;

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use sluice::sessions::{SessionRun, Sessions};
use support::Scratch;

fn run(worktree: &str, run_dir: &Path) -> SessionRun {
    SessionRun {
        worktree: worktree.into(),
        place: format!("session/{worktree}"),
        branch: "bolt/x".into(),
        pid: std::process::id(),
        started: "2026-10-14T09:00:00Z".into(),
        run_dir: run_dir.to_path_buf(),
        command: None,
        tasks: Vec::new(),
    }
}

/// A run directory whose lock is held, as a supervisor holds it, until
/// the returned file is dropped.
fn supervised(dir: &Path, name: &str) -> (PathBuf, File) {
    let run_dir = dir.join(name);
    std::fs::create_dir_all(&run_dir).unwrap();
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(run_dir.join("lock"))
        .unwrap();
    lock.lock().unwrap();
    (run_dir, lock)
}

/// A run directory whose lock a supervisor in another process holds,
/// until the returned [`Supervisor`] is dropped. A lock dropped in this
/// process may still be held a moment by a child another test is
/// spawning, which has a copy of every descriptor until it execs; a
/// supervisor's lock ends with its process, as this one's does.
fn supervisor(dir: &Path, name: &str) -> (PathBuf, Supervisor) {
    let run_dir = dir.join(name);
    std::fs::create_dir_all(&run_dir).unwrap();
    let child = support::isolated(&mut Command::new(std::env::current_exe().unwrap()))
        .args([
            "--exact",
            "hold_a_run_directory_until_killed",
            "--nocapture",
        ])
        .env("SLUICE_TEST_RUN_DIR", &run_dir)
        .spawn()
        .unwrap();
    let held = Supervisor(child);
    let start = Instant::now();
    while !run_dir.join("held").exists() {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(20));
    }
    (run_dir, held)
}

/// A supervisor's process, killed and reaped when dropped, and with it
/// its lock.
struct Supervisor(Child);

impl Drop for Supervisor {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Held by a child process of this test binary until it is killed.
#[test]
fn hold_a_run_directory_until_killed() {
    let Ok(dir) = std::env::var("SLUICE_TEST_RUN_DIR") else {
        return;
    };
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(Path::new(&dir).join("lock"))
        .unwrap();
    lock.lock().unwrap();
    std::fs::write(Path::new(&dir).join("held"), "").unwrap();
    std::thread::sleep(Duration::from_secs(60));
}

/// 89, core.11: a session run is a file a reader finds with no sluice
/// process running, counted while its supervisor holds its run directory;
/// its end removes it.
#[test]
fn a_session_run_is_written_read_and_ended() {
    let dir = Scratch::new("sessions");
    let s = Sessions::new(dir.path().join("sessions"));
    assert!(s.running().unwrap().is_empty());
    let (da, _la) = supervised(dir.path(), "run-a");
    let (db, _lb) = supervised(dir.path(), "run-b");
    let a = run("/work/a", &da);
    let b = run("/work/b", &db);
    {
        let held = s.lock().unwrap();
        s.begin(&held, &a).unwrap();
        s.begin(&held, &b).unwrap();
    }
    assert_eq!(
        Sessions::new(dir.path().join("sessions"))
            .running()
            .unwrap(),
        vec![a.clone(), b.clone()]
    );
    let file = dir.path().join("sessions/%2Fwork%2Fa.json");
    let on_disk: SessionRun = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(on_disk, a);
    s.end("/work/a").unwrap();
    assert!(!file.exists());
    assert_eq!(s.running().unwrap(), vec![b]);
    // Ending one never recorded is nothing.
    s.end("/work/none").unwrap();
}

/// ADR 0035: a running session run's tasks are kept on its record, the
/// rest of it as it was; keeping them for a run not recorded is nothing.
#[test]
fn a_session_runs_tasks_are_kept_on_its_record() {
    use sluice_seams::{TaskProgress, TaskState};
    let dir = Scratch::new("sessions");
    let s = Sessions::new(dir.path().join("sessions"));
    let (da, _la) = supervised(dir.path(), "run-a");
    let mut a = run("/work/a", &da);
    a.command = Some("make e2e".into());
    {
        let held = s.lock().unwrap();
        s.begin(&held, &a).unwrap();
    }
    let tasks = vec![TaskProgress {
        target: "web:test".into(),
        state: TaskState::Running,
        started: None,
        ended: None,
        exit: None,
    }];
    s.keep_tasks("/work/a", &tasks).unwrap();
    a.tasks = tasks;
    assert_eq!(s.running().unwrap(), vec![a]);
    s.keep_tasks("/work/none", &[]).unwrap();
    assert_eq!(s.running().unwrap().len(), 1);
}

/// 89, core.14: a session run whose supervisor let go of its run
/// directory no longer counts against the bound, and the next start
/// drops its record.
#[test]
fn a_session_run_whose_supervisor_is_gone_is_dropped() {
    let dir = Scratch::new("sessions-dead");
    let s = Sessions::new(dir.path().join("sessions"));
    let (dead_dir, dead) = supervisor(dir.path(), "run-dead");
    s.begin(&s.lock().unwrap(), &run("/work/dead", &dead_dir))
        .unwrap();
    assert_eq!(s.running().unwrap().len(), 1);
    drop(dead);
    assert!(s.running().unwrap().is_empty());
    // A run directory that never had a supervisor is no run either.
    let never = dir.path().join("run-never");
    s.begin(&s.lock().unwrap(), &run("/work/never", &never))
        .unwrap();
    assert!(s.running().unwrap().is_empty());

    let (live_dir, _live_lock) = supervised(dir.path(), "run-live");
    let live = run("/work/live", &live_dir);
    s.begin(&s.lock().unwrap(), &live).unwrap();
    assert!(!dir.path().join("sessions/%2Fwork%2Fdead.json").exists());
    assert!(!dir.path().join("sessions/%2Fwork%2Fnever.json").exists());
    assert_eq!(s.running().unwrap(), vec![live]);
}

/// Held by a child process of this test binary until it is killed.
#[test]
fn hold_the_sessions_lock_until_killed() {
    let Ok(dir) = std::env::var("SLUICE_TEST_SESSIONS") else {
        return;
    };
    let s = Sessions::new(PathBuf::from(&dir).join("sessions"));
    let _held = s.lock().unwrap();
    std::fs::write(Path::new(&dir).join("held"), "").unwrap();
    std::thread::sleep(Duration::from_secs(60));
}

/// 89, 91: a starter holds the host's lock across reading the count and
/// recording its run, so a second starter, in any process, waits.
#[test]
fn a_session_run_is_begun_under_the_hosts_lock() {
    let dir = Scratch::new("sessions-lock");
    let mut child = support::isolated(&mut Command::new(std::env::current_exe().unwrap()))
        .args([
            "--exact",
            "hold_the_sessions_lock_until_killed",
            "--nocapture",
        ])
        .env("SLUICE_TEST_SESSIONS", dir.path())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while !dir.path().join("held").exists() {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(20));
    }
    let path = dir.path().to_path_buf();
    let waiter = std::thread::spawn(move || {
        let (run_dir, lock) = supervised(&path, "run-b");
        let s = Sessions::new(path.join("sessions"));
        s.begin(&s.lock().unwrap(), &run("/work/b", &run_dir))
            .unwrap();
        let n = s.running().unwrap().len();
        drop(lock);
        n
    });
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        !waiter.is_finished(),
        "began a session run under a lock another process holds"
    );
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(waiter.join().unwrap(), 1);
}

/// 71, 89: the check seam reports the host's running checks by kind,
/// every repository's, from the host's registry of them: the count the
/// core subtracts from the bound.
#[test]
fn the_check_seam_counts_the_hosts_running_checks() {
    use sluice::host_count::{Entry, Kind};
    use sluice::instance::Instance;
    use sluice::profiles::checks::Checks;
    use sluice_seams::{Running, Tests};

    let dir = Scratch::new("sessions-checks");
    let checks = Checks::new(
        dir.path(),
        PathBuf::from("sluice"),
        Instance {
            bound: 2,
            reserved: 0,
            processors: 2,
            forge: None,
            signing: None,
            peers: Default::default(),
            systems: None,
            caches: Vec::new(),
            expire_after: sluice_core::EXPIRE_AFTER,
            telemetry: None,
            aws: Default::default(),
        },
    )
    .with_sessions(dir.path().join("sessions"))
    .with_running(dir.path().join("running"));
    assert_eq!(checks.running().unwrap(), Running::default());
    let (run_dir, a) = supervisor(dir.path(), "run-a");
    let entry = Entry {
        kind: Kind::Session,
        repository: dir.path().join("another.git"),
        place: "session//work/a".into(),
        run_dir,
        pid: std::process::id(),
        admitted: "2026-10-23T09:00:00Z".into(),
        branch: None,
    };
    checks
        .host_count()
        .unwrap()
        .admit(&entry, 2)
        .unwrap()
        .expect("room under the bound")
        .started();
    assert_eq!(
        checks.running().unwrap(),
        Running {
            queue: 0,
            sessions: 1
        }
    );
    drop(a);
    assert_eq!(checks.running().unwrap(), Running::default());
}
