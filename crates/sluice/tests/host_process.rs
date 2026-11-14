//! sluice's host process through the binary, on repositories on disk with
//! the command runner and real git (135, 129, 71, 150; rules core.31,
//! core.25, core.49; record 0093). A test binary of its own: a child a
//! test starts holds a copy of every lock the test process holds until it
//! execs.

mod support;

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use sluice::host_process::protocol::FollowKind;
use support::OnDisk;

const POLICY: &str =
    "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\n[branches.main]\ntimeout = \"1m\"\n";

fn text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// `check` holds within `secs`, asked every tenth of a second.
fn within(secs: u64, what: &str, mut check: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(secs);
    while !check() {
        assert!(Instant::now() < until, "{what}: not within {secs}s");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Whether the process `pid` lives.
fn alive(pid: u32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_ok()
}

/// Kill the process `pid` at once.
fn kill(pid: u32) {
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid as i32),
        nix::sys::signal::Signal::SIGKILL,
    );
}

/// Submission `id`'s state on `main`, read through the binary in `dir`.
fn state(d: &OnDisk, dir: &Path, id: u64) -> String {
    let out = d
        .command(dir)
        .env("SLUICE_ROUND_SLEEP", "record")
        .args(["status", &id.to_string(), "--json"])
        .output()
        .unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|_| panic!("status {id}: {}", text(&out)));
    doc["state"].as_str().unwrap_or_default().to_string()
}

/// A session of `d`'s on `unit/<name>`, submitted to `main` without
/// waiting: its number.
fn queued(d: &OnDisk, name: &str) -> u64 {
    let dir = d.session(name);
    queued_from(d, &dir)
}

/// The worktree `dir` submitted to `main` without waiting: its number.
fn queued_from(d: &OnDisk, dir: &Path) -> u64 {
    let out = d
        .command(dir)
        .env("SLUICE_ROUND_SLEEP", "record")
        .args(["submit", "main", "--no-wait", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let doc: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", text(&out)));
    doc["id"].as_u64().expect("a number")
}

/// The host process the test's queueing commands started, which passes
/// only when asked, since they run with the tests' setting (design D13),
/// ended, so that the test's own reach starts one from its environment,
/// which passes at its own tick, as a host's does: the test then reads
/// what a live host process does with what was queued.
fn live_from_here(d: &OnDisk) {
    if let Some(pid) = answering(d) {
        kill(pid);
        within(
            10,
            "the host process that passes when asked is gone",
            || !alive(pid),
        );
    }
}

/// 135 (design D1): a second host process started while the first lives
/// ends at once and moves nothing; the first goes on.
#[test]
fn a_second_host_process_ends_at_once() {
    let d = OnDisk::new("host-second", POLICY);
    let client = d.client(d.repo.dir());
    let repository = d.repository(d.repo.dir());
    let follow = client
        .follow(Path::new(&repository), FollowKind::Wait)
        .expect("the host process starts");
    let first = follow.named.pid;
    let out = d.command(d.repo.dir()).arg("host").output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let again = client.answering().unwrap().expect("the first answers");
    assert_eq!(again.named.pid, first);
    assert!(alive(first));
    assert!(
        text(&out).contains("another host process holds"),
        "{}",
        text(&out)
    );
}

/// 135 (design D1): a host process killed leaves its socket, which the
/// next one replaces, at the same path.
#[test]
fn a_socket_a_killed_host_process_left_is_replaced() {
    let d = OnDisk::new("host-killed", POLICY);
    let client = d.client(d.repo.dir());
    let repository = d.repository(d.repo.dir());
    let follow = client
        .follow(Path::new(&repository), FollowKind::Wait)
        .unwrap();
    let first = follow.named.pid;
    kill(first);
    within(10, "the killed process is gone", || !alive(first));
    drop(follow);
    assert!(client.paths().socket.exists(), "the socket it left");
    let next = client.reach().expect("another starts");
    assert_ne!(next.named.pid, first);
    assert!(client.paths().socket.exists());
    assert!(client.paths().version().exists());
}

/// 135 (design D2): ten commands reaching at once start one host
/// process.
#[test]
fn ten_reaches_at_once_start_one_host_process() {
    let d = OnDisk::new("host-ten", POLICY);
    let client = d.client(d.repo.dir());
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(10));
    let reached: Vec<_> = (0..10)
        .map(|_| {
            let (client, barrier) = (client.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                client.reach().expect("reached")
            })
        })
        .collect();
    let connections: Vec<_> = reached.into_iter().map(|t| t.join().unwrap()).collect();
    let pids: BTreeSet<u32> = connections.iter().map(|c| c.named.pid).collect();
    assert_eq!(pids.len(), 1, "{pids:?}");
}

/// 135 (design D2): a host process that does not answer within ten
/// seconds of its start is an error naming its log.
#[test]
fn a_host_process_that_cannot_start_names_its_log() {
    let d = OnDisk::new("host-cannot", POLICY);
    let env = support::env_of(&d.command(d.repo.dir()));
    let client = sluice::host_process::client::Client::new(Path::new("/usr/bin/false"), env)
        .expect("its paths");
    let err = match client.reach() {
        Ok(_) => panic!("nothing answers"),
        Err(e) => format!("{e:#}"),
    };
    let log = client.paths().log();
    assert!(err.contains(&log.display().to_string()), "{err}");
}

/// The worktree `dir` submitted to `main` without waiting, the host
/// process it starts or reaches passing at its own tick: its number.
fn queued_live(d: &OnDisk, dir: &Path) -> u64 {
    let out = d
        .command(dir)
        .args(["submit", "main", "--no-wait", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let doc: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", text(&out)));
    doc["id"].as_u64().expect("a number")
}

/// The host process that answers now: its pid.
fn answering(d: &OnDisk) -> Option<u32> {
    d.client(d.repo.dir())
        .answering()
        .unwrap()
        .map(|c| c.named.pid)
}

/// 26, 129, 135 (design D5): a repository served to the host process is
/// moved by it, with no other command run.
#[test]
fn a_served_repository_is_moved_by_the_host_process() {
    let d = OnDisk::new("host-served", POLICY);
    let id = queued(&d, "a");
    let client = d.client(d.repo.dir());
    let repository = d.repository(d.repo.dir());
    live_from_here(&d);
    client.serve(Path::new(&repository)).expect("served");
    within(60, "the submission lands", || {
        state(&d, d.repo.dir(), id) == "landed"
    });
}

/// 26 (design D5): one repository's refused policy holds no round of
/// another's, and its overview says the refusal.
#[test]
fn a_refused_policy_in_one_repository_holds_no_others_round() {
    let d = OnDisk::new("host-refused-a", POLICY);
    let b = d.another("host-refused-b", POLICY);
    let id_a = queued(&d, "a");
    // `main`'s policy refused once a's submission is queued.
    d.repo
        .write(".config/sluice.toml", &format!("{POLICY}bogus = 1\n"));
    // By hand, past sluice's protection of the branch.
    d.repo.git(&[
        "-c",
        "core.hooksPath=/dev/null",
        "commit",
        "-q",
        "-am",
        "a policy sluice refuses",
    ]);
    let id_b = queued_from(&d, b.dir());
    let client = d.client(d.repo.dir());
    let (a, b_repo) = (d.repository(d.repo.dir()), d.repository(b.dir()));
    live_from_here(&d);
    client.serve(Path::new(&a)).expect("a served");
    client.serve(Path::new(&b_repo)).expect("b served");
    within(60, "b's submission lands", || {
        state(&d, b.dir(), id_b) == "landed"
    });
    assert_ne!(state(&d, d.repo.dir(), id_a), "landed");
    let out = d
        .command(d.repo.dir())
        .env("SLUICE_ROUND_SLEEP", "record")
        .args(["overview", "--json"])
        .output()
        .unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let main = doc["branches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["branch"] == "main")
        .expect("main");
    assert!(
        main["unread"].as_str().is_some_and(|u| u.contains("bogus")),
        "{main}"
    );
}

/// 135 (design D7): a repository an older `sluice run` drives is left to
/// it while it lives, every other moved, and taken at the first tick after
/// it ends.
#[test]
fn a_repository_an_older_sluice_run_drives_is_left_to_it() {
    let d = OnDisk::new("host-older-a", POLICY);
    let b = d.another("host-older-b", POLICY);
    let id_a = queued(&d, "a");
    let id_b = queued_from(&d, b.dir());
    let (a, b_repo) = (d.repository(d.repo.dir()), d.repository(b.dir()));
    // An older `sluice run` holds a's driver's lock.
    let readers = d.state.path().join("sluice/readers");
    let older = sluice::reader::Driver::take(&readers, &a)
        .unwrap()
        .expect("the older run's lock");
    let client = d.client(d.repo.dir());
    live_from_here(&d);
    client.serve(Path::new(&a)).unwrap();
    client.serve(Path::new(&b_repo)).unwrap();
    within(60, "b's submission lands", || {
        state(&d, b.dir(), id_b) == "landed"
    });
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(state(&d, d.repo.dir(), id_a), "queued", "a is left to it");
    drop(older);
    within(60, "a's submission lands once the older run ends", || {
        state(&d, d.repo.dir(), id_a) == "landed"
    });
}

/// 135 (design D6): the host process ends once its queues are empty and
/// nothing follows it.
#[test]
fn the_host_process_ends_once_its_queues_are_empty() {
    let d = OnDisk::new("host-ends", POLICY);
    let id = queued(&d, "a");
    let client = d.client(d.repo.dir());
    let repository = d.repository(d.repo.dir());
    live_from_here(&d);
    client.serve(Path::new(&repository)).unwrap();
    let pid = client.answering().unwrap().expect("it runs").named.pid;
    within(60, "the submission lands", || {
        state(&d, d.repo.dir(), id) == "landed"
    });
    within(10, "the host process ends", || !alive(pid));
    assert!(!client.paths().socket.exists(), "its socket went with it");
}

/// reader-tick, "An idle tick starts no process" (129, 135; design D4): a
/// host process that lives on, here for an agent's held room, looks at
/// each repository in its index every three seconds as an idle tick
/// reads it, and starts no task for one with nothing in play. On a host
/// whose config names a system it is not, a task's start is refused and
/// logged: nothing is logged while the repository is idle, and once work
/// is in play there the refusal is logged once, however many looks pass
/// the repository by.
#[test]
fn the_host_processs_look_at_an_idle_repository_starts_nothing() {
    let policy = format!("{POLICY}\n[systems.elsewhere]\nos = \"windows\"\narch = \"x86_64\"\n");
    let d = OnDisk::new("host-look-idle", &policy);
    d.land("a");
    let repository = d.repository(d.repo.dir());
    within(15, "the host process ends once idle", || {
        answering(&d).is_none()
    });
    let dir = d.config.path().join("sluice");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        "[runs]\nsystems = [\"elsewhere\"]\n",
    )
    .unwrap();
    let client = d.client(d.repo.dir());
    let mut held = client
        .hold(
            sluice_seams::RunKind::Session,
            Path::new(&repository),
            "an-agent",
        )
        .expect("the host process starts");
    let answer = held.next(Some(Duration::from_secs(10))).unwrap();
    assert!(
        matches!(answer, Some(sluice::host_process::protocol::Answer::Held)),
        "{answer:?}"
    );
    let refused = || {
        std::fs::read_to_string(client.paths().log())
            .unwrap()
            .lines()
            .filter(|l| l.contains("this host claims no run of it"))
            .count()
    };
    std::thread::sleep(Duration::from_secs(7));
    assert_eq!(refused(), 0, "two looks at the idle repository");

    queued(&d, "b");
    within(10, "the start refused", || refused() == 1);
    std::thread::sleep(Duration::from_secs(7));
    assert_eq!(refused(), 1, "two looks pass it by");
    drop(held);
}

/// 135 (design D6): the host process ends at its next tick once its state
/// folder is gone, and its socket with it.
#[test]
fn the_host_process_ends_with_its_state_folder() {
    let d = OnDisk::new("host-folder", POLICY);
    let client = d.client(d.repo.dir());
    let repository = d.repository(d.repo.dir());
    let follow = client
        .follow(Path::new(&repository), FollowKind::Wait)
        .unwrap();
    let pid = follow.named.pid;
    // The process may write into the folder as it goes: removed again
    // until it is gone.
    within(5, "the state folder is removed", || {
        std::fs::remove_dir_all(d.state.path()).is_ok() || !d.state.path().exists()
    });
    within(5, "the host process ends", || !alive(pid));
    assert!(!client.paths().socket.exists());
    assert!(
        !client.paths().dir.exists(),
        "the host process made its folder again"
    );
}

/// 135 (design D6): a state folder removed and made again at its path by
/// another is not the host process's: it ends all the same, and leaves
/// what is there now alone.
#[test]
fn a_state_folder_made_again_ends_the_host_process() {
    let d = OnDisk::new("host-folder-again", POLICY);
    let client = d.client(d.repo.dir());
    let repository = d.repository(d.repo.dir());
    let follow = client
        .follow(Path::new(&repository), FollowKind::Wait)
        .unwrap();
    let pid = follow.named.pid;
    let dir = client.paths().dir.clone();
    // The folder that takes its place, made beside it first and moved in
    // as the old one goes, so no tick finds the path empty.
    let again = d.state.path().with_extension("again");
    std::fs::create_dir_all(&again).unwrap();
    let aside = d.state.path().with_extension("aside");
    std::fs::rename(&dir, &aside).unwrap();
    std::fs::rename(&again, &dir).unwrap();
    within(5, "the host process ends", || !alive(pid));
    let left: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(left.is_empty(), "it wrote into the new folder: {left:?}");
    let _ = std::fs::remove_dir_all(&aside);
}

/// 135 (design D6): a command that meets the host process ending starts
/// another: none is lost between the two.
#[test]
fn a_reach_while_it_ends_starts_another() {
    let d = OnDisk::new("host-ending", POLICY);
    let client = d.client(d.repo.dir());
    let mut pids = BTreeSet::new();
    for n in 0..40u64 {
        let c = client
            .reach()
            .unwrap_or_else(|e| panic!("reach {n}: {e:#}"));
        pids.insert(c.named.pid);
        drop(c);
        // Each idle process ends a tick after its last connection closed.
        std::thread::sleep(Duration::from_millis(40 + (n * 37) % 160));
    }
    assert!(pids.len() > 1, "it ended and was started again: {pids:?}");
}

/// 135 (design D8): a command of a newer sluice has an older host
/// process hand over, and starts its own.
#[test]
fn a_newer_client_has_an_older_host_process_hand_over() {
    let d = OnDisk::new("host-newer", POLICY);
    let client = d.client(d.repo.dir());
    let repository = d.repository(d.repo.dir());
    let mut follow = client
        .follow(Path::new(&repository), FollowKind::Wait)
        .unwrap();
    let older = follow.named.pid;
    let newer = client.clone().with_version("0.72.0");
    let reached = newer.reach().expect("the newer command's own");
    assert_ne!(reached.named.pid, older);
    within(10, "the older process ends", || !alive(older));
    // What followed the older one is closed, and reaches the new one.
    assert!(follow.next(Some(Duration::from_secs(5))).is_err());
}

/// 135 (design D8): an older host process whose tasks take longer than a
/// start's ten seconds to finish their rounds is waited out, its lock
/// held until it ends, and the newer command's own process then answers.
#[test]
fn a_hand_over_longer_than_a_start_is_waited_out() {
    let d = OnDisk::new("host-slow-hand-over", POLICY);
    let client = d.client(d.repo.dir());
    let paths = client.paths().clone();
    // The older process: `sleep` stands for it, and the test holds its lock
    // and answers at its socket as sluice 0.71.0.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(paths.lock())
        .unwrap();
    lock.lock().unwrap();
    let mut older = std::process::Command::new("sleep")
        .arg("600")
        .spawn()
        .unwrap();
    let pid = older.id();
    let _ = std::fs::remove_file(&paths.socket);
    let listener = std::os::unix::net::UnixListener::bind(&paths.socket).unwrap();
    let socket = paths.socket.clone();
    let standing = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut hello = String::new();
        reader.read_line(&mut hello).unwrap();
        writeln!(
            writer,
            r#"{{"version":"0.71.0","pid":{pid},"since":"2026-11-07T00:00:00Z"}}"#
        )
        .unwrap();
        let mut request = String::new();
        reader.read_line(&mut request).unwrap();
        writeln!(writer, r#""ok""#).unwrap();
        // It accepts no more, and finishes its tasks' rounds for twelve
        // seconds before its socket and its lock go as it ends.
        drop(listener);
        std::thread::sleep(Duration::from_secs(12));
        let _ = std::fs::remove_file(&socket);
        drop(lock);
        let _ = older.kill();
        let _ = older.wait();
        request
    });
    let began = Instant::now();
    let reached = client
        .with_version("0.72.0")
        .reach()
        .expect("the newer command's own, once the older ended");
    assert!(began.elapsed() >= Duration::from_secs(12));
    assert_ne!(reached.named.pid, pid);
    let request = standing.join().unwrap();
    assert!(request.contains("hand-over"), "{request}");
}

/// 135 (design D8): a command of an older sluice uses a newer host
/// process as it is, and a request the process does not know is answered
/// with an error naming both versions.
#[test]
fn an_older_client_uses_a_newer_host_process() {
    let d = OnDisk::new("host-older-client", POLICY);
    let client = d.client(d.repo.dir());
    // A newer process, standing at the socket: it answers `hello` as sluice
    // 9.0.0, and is asked what the command came for, never to hand over.
    let socket = client.paths().socket.clone();
    let _ = std::fs::remove_file(&socket);
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let asked = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut hello = String::new();
        reader.read_line(&mut hello).unwrap();
        writeln!(
            writer,
            r#"{{"version":"9.0.0","pid":1,"since":"2026-11-07T00:00:00Z"}}"#
        )
        .unwrap();
        let mut request = String::new();
        reader.read_line(&mut request).unwrap();
        writeln!(writer, r#""ok""#).unwrap();
        (hello, request)
    });
    client
        .serve(Path::new("/nowhere/repo.git"))
        .expect("used as it is");
    let (hello, request) = asked.join().unwrap();
    assert!(hello.contains("\"hello\""), "{hello}");
    assert!(request.contains("\"serve\""), "{request}");
    std::fs::remove_file(&socket).unwrap();

    // The real process answers a request it does not know.
    let c = client.reach().expect("started");
    drop(c);
    let stream = UnixStream::connect(&socket).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut writer = stream;
    writeln!(writer, r#"{{"hello":"0.80.0"}}"#).unwrap();
    let mut named = String::new();
    reader.read_line(&mut named).unwrap();
    writeln!(writer, r#"{{"ask":"teleport"}}"#).unwrap();
    let mut answer = String::new();
    reader.read_line(&mut answer).unwrap();
    assert!(
        answer.contains("0.80.0") && answer.contains(sluice::VERSION),
        "{answer}"
    );
}

/// host-process, "A submit with no host process running": the submit
/// starts it, and it lands the submission with no other command run (135).
#[test]
fn a_submit_with_no_host_process_running_is_landed_by_it() {
    let d = OnDisk::new("host-submit", POLICY);
    let dir = d.session("a");
    assert_eq!(answering(&d), None);
    let id = queued_live(&d, &dir);
    assert!(answering(&d).is_some(), "a host process runs");
    within(60, "the submission lands", || {
        state(&d, d.repo.dir(), id) == "landed"
    });
}

/// host-process, "Ten submits at once": each has its own number, and one
/// host process starts (135; design D2).
#[test]
fn ten_submits_at_once_start_one_host_process() {
    let d = OnDisk::new("host-ten-submits", POLICY);
    let dirs: Vec<_> = (0..10).map(|n| d.session(&format!("s{n}"))).collect();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(10));
    let started: Vec<_> = dirs
        .iter()
        .map(|dir| {
            let mut c = d.command(dir);
            c.args(["submit", "main", "--no-wait", "--json"]);
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                c.output().unwrap()
            })
        })
        .collect();
    let mut ids = BTreeSet::new();
    for t in started {
        let out = t.join().unwrap();
        assert!(out.status.success(), "{}", text(&out));
        let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        ids.insert(doc["id"].as_u64().unwrap());
    }
    assert_eq!(ids.len(), 10, "{ids:?}");
    let client = d.client(d.repo.dir());
    let log = std::fs::read_to_string(client.paths().log()).unwrap_or_default();
    assert_eq!(log.matches(" started, pid ").count(), 1, "{log}");
    assert!(!log.contains("another host process holds"), "{log}");
    assert!(!client.paths().previous_log().exists(), "one process's log");
}

/// host-process, "A withdrawal with no host process running": the
/// withdrawal starts it, and its next pass ends the run and releases the
/// combination's checkout (135; design D14).
#[test]
fn a_withdrawal_starts_the_host_process() {
    let slow = "[tests]\nrunner = \"command\"\ncommand = \"sleep 30\"\n\n[branches.main]\ntimeout = \"5m\"\n";
    let d = OnDisk::new("host-withdraw", slow);
    let id = queued(&d, "a");
    let out = d.sluice(&["step", "main"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(state(&d, d.repo.dir(), id), "testing");
    let checkouts = || {
        d.repo
            .git(&["worktree", "list", "--porcelain"])
            .lines()
            .filter(|l| l.starts_with("worktree ") && l.contains("try+2Fmain"))
            .count()
    };
    assert_eq!(checkouts(), 1, "the combination's checkout");
    kill(answering(&d).expect("the host process the submit started"));
    within(10, "it is gone", || answering(&d).is_none());
    let out = d
        .command(d.repo.dir())
        .args(["withdraw", &id.to_string()])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    assert!(answering(&d).is_some(), "a host process runs");
    within(60, "the checkout is released", || checkouts() == 0);
}

/// host-process, "Two repositories, one process" (135).
#[test]
fn two_repositories_one_host_process() {
    let d = OnDisk::new("host-two-a", POLICY);
    let b = d.another("host-two-b", POLICY);
    let a_dir = d.session("a");
    let id_a = queued_live(&d, &a_dir);
    let pid = answering(&d).expect("a host process runs");
    let id_b = queued_live(&d, b.dir());
    assert_eq!(answering(&d), Some(pid), "the same process");
    within(60, "both land", || {
        state(&d, d.repo.dir(), id_a) == "landed" && state(&d, b.dir(), id_b) == "landed"
    });
}

/// host-process, "Started from another repository": the process a submit
/// in B starts finds A in its index and lands both (135; design D4).
#[test]
fn started_from_another_repository_lands_both() {
    let d = OnDisk::new("host-from-a", POLICY);
    let b = d.another("host-from-b", POLICY);
    let id_a = queued(&d, "a");
    kill(answering(&d).expect("the host process the submit started"));
    within(10, "it is gone", || answering(&d).is_none());
    let id_b = queued_live(&d, b.dir());
    within(60, "both land", || {
        state(&d, d.repo.dir(), id_a) == "landed" && state(&d, b.dir(), id_b) == "landed"
    });
}

/// host-process, "Started from inside a hook and a pane": the checks the
/// host process runs carry nothing of the submit that started it, and the
/// process works in `/` (135, 147; design D3).
#[test]
fn a_run_holds_nothing_of_the_command_that_started_the_host_process() {
    let seen = support::Scratch::new("host-env-seen");
    let file = seen.path().join("env");
    let policy = format!(
        "[tests]\nrunner = \"command\"\ncommand = \"env > '{}'\"\n\n[branches.main]\ntimeout = \"1m\"\n",
        file.display()
    );
    let d = OnDisk::new("host-env", &policy);
    let dir = d.session("a");
    let git_dir = d.repo.git(&[
        "-C",
        dir.to_str().unwrap(),
        "rev-parse",
        "--absolute-git-dir",
    ]);
    let started_from = [
        ("GIT_DIR", git_dir.as_str()),
        ("GIT_WORK_TREE", dir.to_str().unwrap()),
        (support::HERDR[3], "p-1"),
        (support::HERDR[2], "/tmp/m.sock"),
    ];
    let out = d
        .command(&dir)
        .envs(started_from)
        .args(["submit", "main", "--no-wait"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    // Followed, it lives on once the submission lands.
    let _follow = d
        .client(d.repo.dir())
        .follow(Path::new(&d.repository(d.repo.dir())), FollowKind::Wait)
        .unwrap();
    within(60, "the checks ran", || file.exists());
    let env = std::fs::read_to_string(&file).unwrap();
    for (name, _) in started_from {
        assert!(!env.contains(&format!("{name}=")), "{name} in\n{env}");
    }
    let pid = answering(&d).expect("the host process runs");
    let cwd = std::process::Command::new("lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
        .output()
        .unwrap();
    let cwd = String::from_utf8_lossy(&cwd.stdout);
    assert!(cwd.lines().any(|l| l == "n/"), "{cwd}");
}

/// What a round of a combination's runs git for: its checkout, its
/// landing's commit and the branch's move.
fn a_rounds(args: &str) -> bool {
    ["worktree add", "commit-tree", "update-ref", "merge-tree"]
        .iter()
        .any(|r| args.contains(r))
}

/// submission-wait, "It lands": the wait follows the host process, which
/// runs the rounds; the wait runs none (135, 129; rule core.31; design D9).
#[test]
fn a_wait_follows_the_host_process_and_runs_no_round() {
    let d = OnDisk::new("host-wait-follows", POLICY);
    let id = queued(&d, "a");
    let host = answering(&d).expect("the host process the submit started");
    let child = d
        .command(d.repo.dir())
        .env("SLUICE_ROUND_SLEEP", "record")
        .args(["wait", &id.to_string()])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let wait = child.id();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("passed · landed as "), "{}", text(&out));
    let starts = d.git.parented();
    let own: Vec<&String> = starts
        .iter()
        .filter(|(p, args)| *p == wait && a_rounds(args))
        .map(|(_, a)| a)
        .collect();
    assert!(own.is_empty(), "the wait ran a round: {own:?}");
    assert!(
        starts.iter().any(|(p, args)| *p == host && a_rounds(args)),
        "the host process ran them: {starts:?}"
    );
}

/// overview-reader, "The host process is killed under a wait": the wait
/// reaches it again at its next tick, starting it, and reports the outcome
/// the new one decides (129).
#[test]
fn a_wait_starts_a_killed_host_process_again() {
    let slow = "[tests]\nrunner = \"command\"\ncommand = \"sleep 3\"\n\n[branches.main]\ntimeout = \"1m\"\n";
    let d = OnDisk::new("host-wait-killed", slow);
    let id = queued(&d, "a");
    let first = answering(&d).expect("the host process the submit started");
    let child = d
        .command(d.repo.dir())
        .env("SLUICE_ROUND_SLEEP", "record")
        .args(["wait", &id.to_string()])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    within(30, "the combination is under test", || {
        state(&d, d.repo.dir(), id) == "testing"
    });
    kill(first);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("passed · landed as "), "{}", text(&out));
    let again = answering(&d);
    assert!(again.is_none_or(|p| p != first), "{again:?}");
}

/// submission-wait, "The branch cannot be read for a while": a round of
/// the host process's that fails is said once on stderr in its words, and
/// the wait goes on to the landing (133; design D9).
#[test]
fn a_wait_says_a_rounds_failure_once_and_goes_on() {
    use std::io::Read;
    let d = OnDisk::new("host-wait-fails", POLICY);
    let id = queued(&d, "a");
    let by_hand = |content: &str| {
        d.repo.write(".config/sluice.toml", content);
        d.repo.git(&[
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "-q",
            "-am",
            "the policy by hand",
        ]);
    };
    by_hand(&format!("{POLICY}bogus = 1\n"));
    let mut child = d
        .command(d.repo.dir())
        .env("SLUICE_ROUND_SLEEP", "record")
        .args(["wait", &id.to_string()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let said = std::thread::spawn(move || {
        let mut all = String::new();
        let _ = stderr.read_to_string(&mut all);
        all
    });
    // Its rounds fail with the same words for a while.
    std::thread::sleep(Duration::from_secs(3));
    by_hand(POLICY);
    let out = child.wait_with_output().unwrap();
    let err = said.join().unwrap();
    assert!(out.status.success(), "{}{err}", text(&out));
    assert!(text(&out).contains("passed · landed as "), "{}", text(&out));
    let failures = err
        .lines()
        .filter(|l| l.starts_with("sluice: main: main's policy: "))
        .count();
    assert_eq!(failures, 1, "{err}");
}

/// host-process, "A step with a submission queued": `sluice step` asks the
/// host process for one round of the branch, which runs there, and prints
/// its document (rule core.14; design D10).
#[test]
fn a_step_is_the_host_processs_round() {
    let d = OnDisk::new("host-step", POLICY);
    let id = queued(&d, "a");
    let host = answering(&d).expect("the host process the submit started");
    let child = d
        .command(d.repo.dir())
        .env("SLUICE_ROUND_SLEEP", "record")
        .args(["step", "main", "--json"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let step = child.id();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let lines: Vec<&str> = std::str::from_utf8(&out.stdout).unwrap().lines().collect();
    assert_eq!(lines.len(), 1, "one document: {lines:?}");
    let doc: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(doc["outcome"], "stepped", "{doc}");
    assert_eq!(doc["branch"], "main", "{doc}");
    assert_eq!(state(&d, d.repo.dir(), id), "testing");
    let starts = d.git.parented();
    assert!(
        starts
            .iter()
            .any(|(p, a)| *p == host && a.contains("worktree add")),
        "the round ran in the host process: {starts:?}"
    );
    assert!(
        !starts.iter().any(|(p, a)| *p == step && a_rounds(a)),
        "the step ran none of it: {starts:?}"
    );
}

/// This setup's host bound: `[runs] max` in its config.
fn bound(d: &OnDisk, max: usize) {
    let dir = d.config.path().join("sluice");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        format!("[runs]\nmax = {max}\nreserved = 0\n"),
    )
    .unwrap();
}

/// An agent's `sluice exec main -- sh -c <script>` in a session of its
/// own, `name`, started now.
fn agent(d: &OnDisk, name: &str, script: &str) -> std::process::Child {
    let dir = d.session(name);
    d.command(&dir)
        .args(["exec", "main", "--", "sh", "-c", script])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap()
}

/// run-bound, "Turns on a bound of two" (150, S75; rule core.49; record
/// 0087): two agents' checks hold both runs, a third waits and a
/// combination falls due; the combination takes the first run to end, and
/// the third agent the second, while the combination's run goes on.
#[test]
fn s75_a_combination_starts_before_a_third_agents_check() {
    let marks = support::Scratch::new("host-s75-marks");
    let m = |name: &str| marks.path().join(name);
    let policy = format!(
        "[tests]\nrunner = \"command\"\ncommand = \"touch '{}'; sleep 15\"\n\n[branches.main]\ntimeout = \"2m\"\n",
        m("q").display()
    );
    let d = OnDisk::new("host-s75", &policy);
    bound(&d, 2);
    let repository = d.repository(d.repo.dir());
    let _follow = d
        .client(d.repo.dir())
        .follow(Path::new(&repository), FollowKind::Wait)
        .unwrap();
    let a = agent(&d, "a", &format!("touch '{}'; sleep 3", m("a").display()));
    let b = agent(&d, "b", &format!("touch '{}'; sleep 9", m("b").display()));
    within(30, "two agents' checks hold both runs", || {
        m("a").exists() && m("b").exists()
    });
    let c = agent(&d, "c", &format!("touch '{}'", m("c").display()));
    let id = queued_live(&d, &d.session("q"));
    within(60, "the combination's run starts", || m("q").exists());
    assert!(!m("c").exists(), "the third agent's check still waits");
    within(60, "the third agent's check starts", || m("c").exists());
    assert_eq!(
        state(&d, d.repo.dir(), id),
        "testing",
        "while the combination's run goes on"
    );
    for agent in [a, b, c] {
        let out = agent.wait_with_output().unwrap();
        assert!(out.status.success(), "{}", text(&out));
    }
}

/// run-bound, "An agent's check stopped while it waits" (150; rule
/// core.49): an agent's `sluice exec` stopped with Ctrl-C while it waits
/// is counted nowhere, and the next agent's check that waits starts in the
/// next free run.
#[test]
fn an_agents_check_stopped_while_it_waits_is_counted_nowhere() {
    let marks = support::Scratch::new("host-stopped-marks");
    let m = |name: &str| marks.path().join(name);
    let d = OnDisk::new("host-stopped", POLICY);
    bound(&d, 1);
    let repository = d.repository(d.repo.dir());
    let _follow = d
        .client(d.repo.dir())
        .follow(Path::new(&repository), FollowKind::Wait)
        .unwrap();
    let a = agent(&d, "a", &format!("touch '{}'; sleep 5", m("a").display()));
    within(30, "the first agent's check holds the run", || {
        m("a").exists()
    });
    let b = agent(&d, "b", &format!("touch '{}'", m("b").display()));
    let waiting = |d: &OnDisk| {
        let out = d
            .command(d.repo.dir())
            .env("SLUICE_ROUND_SLEEP", "record")
            .args(["overview", "--json"])
            .output()
            .unwrap();
        let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        doc["room"]["waiting"]["sessions"].as_u64().unwrap_or(0)
    };
    within(20, "the second agent's check waits", || waiting(&d) == 1);
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(b.id() as i32),
        nix::sys::signal::Signal::SIGINT,
    );
    let out = b.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(sluice::STOPPED), "{}", text(&out));
    within(10, "the stopped check waits no more", || waiting(&d) == 0);
    let c = agent(&d, "c", &format!("touch '{}'", m("c").display()));
    within(60, "the next agent's check starts", || m("c").exists());
    assert!(!m("b").exists(), "the stopped one never ran");
    for agent in [a, c] {
        let out = agent.wait_with_output().unwrap();
        assert!(out.status.success(), "{}", text(&out));
    }
}

/// `sluice status main` through the binary, the host process it may start
/// passing at its own tick.
fn status_live(d: &OnDisk) {
    let out = d
        .command(d.repo.dir())
        .args(["status", "main"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
}

/// host-process, "Killed while a combination is under test" (135, S10): a
/// run outlives the host process killed under it; the one a status starts
/// reads its result from the run, runs no check again, and lands it.
#[test]
fn a_killed_host_process_loses_nothing() {
    let marks = support::Scratch::new("host-killed-marks");
    let ran = marks.path().join("ran");
    let done = marks.path().join("done");
    let policy = format!(
        "[tests]\nrunner = \"command\"\ncommand = \"echo run >> '{}'; sleep 3; touch '{}'\"\n\n[branches.main]\ntimeout = \"1m\"\n",
        ran.display(),
        done.display()
    );
    let d = OnDisk::new("host-killed-run", &policy);
    let dir = d.session("a");
    let id = queued_live(&d, &dir);
    within(30, "the combination's checks run", || ran.exists());
    // Once its round has recorded the run started: a run marked starting
    // that a stopped round left is opened again (S10).
    std::thread::sleep(Duration::from_secs(1));
    kill(answering(&d).expect("the host process runs"));
    within(30, "the checks end, outliving it", || done.exists());
    within(10, "it is gone", || answering(&d).is_none());
    status_live(&d);
    within(60, "the submission lands", || {
        state(&d, d.repo.dir(), id) == "landed"
    });
    let runs = std::fs::read_to_string(&ran).unwrap();
    assert_eq!(runs.lines().count(), 1, "no check ran again: {runs}");
}

/// host-process, "The overview names the host process and the runs
/// waiting" (150; design D12): `host_process` is the process that answers,
/// its version, pid and start, and null where none answers.
#[test]
fn the_overview_names_the_host_process() {
    let d = OnDisk::new("host-overview", POLICY);
    let overview = |d: &OnDisk| {
        let out = d
            .command(d.repo.dir())
            .env("SLUICE_ROUND_SLEEP", "record")
            .args(["overview", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    // 129 (design D16): the first overview starts the host process for its
    // reading, whichever answers once it has read.
    overview(&d);
    let repository = d.repository(d.repo.dir());
    let follow = d
        .client(d.repo.dir())
        .follow(Path::new(&repository), FollowKind::Wait)
        .unwrap();
    let doc = overview(&d);
    assert_eq!(doc["host_process"]["pid"], follow.named.pid, "{doc}");
    assert_eq!(doc["host_process"]["version"], sluice::VERSION, "{doc}");
    assert!(doc["host_process"]["since"].is_string(), "{doc}");
    assert_eq!(
        doc["room"]["waiting"],
        serde_json::json!({ "queue": 0, "sessions": 0 }),
        "{doc}"
    );
    assert_eq!(doc["room"]["turn"], "queue", "{doc}");
    // The host's name stays where it was.
    assert!(doc["host"].is_string(), "{doc}");
}

/// `main`'s records on a bare origin, its runs signed by `a`, whose key
/// `d`'s host holds: the host, and the remote.
fn on_origin(name: &str) -> (OnDisk, support::Scratch) {
    let (d, mut remotes) = on_remotes(name, &["origin"]);
    (d, remotes.remove(0))
}

/// `main`'s records on a bare `remotes[0]`, and a branch of each other
/// remote's name on that remote, their runs signed by `a`, whose key
/// `d`'s host holds: the host, and the remotes in order.
fn on_remotes(name: &str, remotes: &[&str]) -> (OnDisk, Vec<support::Scratch>) {
    use ssh_key::{Algorithm, PrivateKey};
    let k = PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap();
    let mut policy = format!(
        "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\n\
         [signers.a]\nssh = \"a {}\"\n\n\
         [branches.main]\ntimeout = \"1m\"\nremote = \"{}\"\ntrust = [\"a\"]\n",
        k.public_key().to_openssh().unwrap(),
        remotes[0]
    );
    for r in &remotes[1..] {
        policy.push_str(&format!(
            "\n[branches.{r}]\ntimeout = \"1m\"\nremote = \"{r}\"\ntrust = [\"a\"]\n"
        ));
    }
    let d = OnDisk::new(name, &policy);
    for r in &remotes[1..] {
        d.repo.git(&["branch", r]);
    }
    let mut bare = vec![d.repo.bare_origin()];
    for r in &remotes[1..] {
        let other = support::Scratch::new(&format!("{r}.git"));
        let at = other.path().to_str().unwrap();
        d.repo.git(&["clone", "-q", "--bare", ".", at]);
        d.repo.git(&["remote", "add", r, at]);
        d.repo.git(&["fetch", "-q", r]);
        bare.push(other);
    }
    if remotes[0] != "origin" {
        d.repo.git(&["remote", "rename", "origin", remotes[0]]);
    }
    signing(&d, &k);
    (d, bare)
}

/// `d`'s host signs as `a` with `k`.
fn signing(d: &OnDisk, k: &ssh_key::PrivateKey) {
    let path = d.config.path().join("signing");
    k.write_openssh_file(&path, ssh_key::LineEnding::LF)
        .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::create_dir_all(d.config.path().join("sluice")).unwrap();
    std::fs::write(
        d.config.path().join("sluice/config.toml"),
        format!(
            "[signing]\nsigner = \"a\"\nkey = {:?}\n",
            path.display().to_string()
        ),
    )
    .unwrap();
}

/// Whether `start`, a git command's arguments, asks a remote.
fn asks_a_remote(start: &str) -> bool {
    [" fetch ", " ls-remote ", " push "]
        .iter()
        .any(|a| format!(" {start} ").contains(a))
}

/// 129 (design D16): with no host process running and `main` on origin
/// holding a submission another host queued, which this host has never
/// fetched, `sluice status <id> --json` starts the host process and shows
/// the submission as its first reading found it; the status itself asks
/// origin nothing.
#[test]
fn a_status_with_no_host_process_starts_it_and_reads_its_first_reading() {
    let (d, origin) = on_origin("host-first-reading");
    let other = OnDisk {
        repo: support::Repo::clone_of(origin.path(), "host-first-reading-other"),
        git: support::CountingGit::new(),
        config: support::Scratch::new("host-first-reading-other-config"),
        state: support::Scratch::new("host-first-reading-other-state"),
        sessions: support::Scratch::new("host-first-reading-other-sessions"),
    };
    std::fs::create_dir_all(other.config.path().join("sluice")).unwrap();
    std::fs::copy(
        d.config.path().join("sluice/config.toml"),
        other.config.path().join("sluice/config.toml"),
    )
    .unwrap();
    let id = queued(&other, "theirs");
    assert_eq!(
        answering(&d),
        None,
        "this host's host process is not running"
    );

    let before = d.git.count();
    let child = d
        .command(d.repo.dir())
        .env("SLUICE_ROUND_SLEEP", "record")
        .args(["status", &id.to_string(), "--json"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let status_pid = child.id();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let doc: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", text(&out)));
    assert_eq!(doc["id"].as_u64(), Some(id), "{doc}");
    assert!(doc["state"].is_string(), "{doc}");

    let starts = d.git.parented()[before..].to_vec();
    let own: Vec<&String> = starts
        .iter()
        .filter(|(pid, args)| *pid == status_pid && asks_a_remote(args))
        .map(|(_, args)| args)
        .collect();
    assert!(own.is_empty(), "the status asked origin: {own:?}");
    assert!(
        starts
            .iter()
            .any(|(pid, args)| *pid != status_pid && asks_a_remote(args)),
        "no host process read origin: {starts:?}"
    );
}

/// A host process of `d`'s, started afresh in the test, which passes only
/// when asked and whose every git, once `flag` names its pid, waits a
/// minute before it runs: one whose looks and passes are slow while a
/// command's own git is not. The repository, and the host process's
/// connection that keeps its task, its pid written to `flag` by the
/// caller.
struct Slowed {
    repository: std::path::PathBuf,
    client: sluice::host_process::client::Client,
    _follow: sluice::host_process::client::Connection,
    pid: u32,
    flag: std::path::PathBuf,
    _bin: support::Scratch,
}

impl Slowed {
    fn start(d: &OnDisk, name: &str) -> Slowed {
        let repository = std::path::PathBuf::from(d.repository(d.repo.dir()));
        live_from_here(d);
        let bin = support::Scratch::new(&format!("{name}-slow-git"));
        let flag = bin.path().join("flag");
        let real = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|p| p.join("git"))
            .find(|p| p.is_file())
            .expect("git on the path");
        let script = bin.path().join("git");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nif [ -f '{flag}' ] && [ \"$PPID\" = \"$(cat '{flag}')\" ]; then sleep 60; fi\nexec '{real}' \"$@\"\n",
                flag = flag.display(),
                real = real.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut c = d.command(d.repo.dir());
        c.env("SLUICE_ROUND_SLEEP", "record");
        let mut env = support::env_of(&c);
        for (k, v) in env.iter_mut() {
            if k == "PATH" {
                let mut path = vec![bin.path().to_path_buf()];
                path.extend(std::env::split_paths(v));
                *v = std::env::join_paths(path).unwrap();
            }
        }
        let client =
            sluice::host_process::client::Client::new(Path::new(env!("CARGO_BIN_EXE_sluice")), env)
                .expect("its paths");
        let follow = client
            .follow(&repository, FollowKind::Wait)
            .expect("the host process starts");
        let pid = follow.named.pid;
        Slowed {
            repository,
            client,
            _follow: follow,
            pid,
            flag,
            _bin: bin,
        }
    }

    /// From now on, every git the host process starts waits a minute.
    fn slow(&self) {
        std::fs::write(&self.flag, self.pid.to_string()).unwrap();
    }
}

/// `sluice status main --json` in `d`'s repository: its output, how long
/// it took, and whether its own process asked a remote.
fn timed_status(d: &OnDisk) -> (std::process::Output, Duration, bool) {
    let before = d.git.count();
    let began = Instant::now();
    let child = d
        .command(d.repo.dir())
        .env("SLUICE_ROUND_SLEEP", "record")
        .args(["status", "main", "--json"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let out = child.wait_with_output().unwrap();
    let took = began.elapsed();
    let asked = d.git.parented()[before..]
        .iter()
        .any(|(p, args)| *p == pid && asks_a_remote(args));
    (out, took, asked)
}

/// 129 (design D16): a status whose reading does not come within the
/// round's retry bound and a tick, the host process's look held by two
/// remotes that answer nothing, each for a try's whole limit, reads the
/// records as the host holds them, fetching nothing, and says on stderr
/// when the host process last read the repository.
#[test]
fn a_status_beside_a_slow_look_reads_as_held_and_says_so() {
    let (d, _remotes) = on_remotes("host-slow-look", &["origin", "mirror"]);
    queued(&d, "mine");
    let host = Slowed::start(&d, "host-slow-look");
    let (out, _, asked) = timed_status(&d);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!asked, "the status asked origin");
    let first = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(!first.contains("host process"), "{first}");

    host.slow();
    std::thread::sleep(sluice::host_process::READ_TICK + Duration::from_millis(500));
    let (out, took, asked) = timed_status(&d);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!asked, "the status asked origin");
    let bound = sluice::host_process::read_for();
    assert!(took >= bound, "answered in {took:?}, within {bound:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("sluice's host process last read it at"),
        "{err}"
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", text(&out)));
    assert_eq!(doc["branch"], "main", "{doc}");
}

/// 129 (design D16): a status while the host process's pass of the
/// repository is long under way, begun within a tick of its last reading's
/// end, answers at once from the records as the host holds them, with
/// nothing fetched and nothing said.
#[test]
fn a_status_during_a_long_round_answers_at_once() {
    let (d, _origin) = on_origin("host-long-round");
    queued(&d, "mine");
    let host = Slowed::start(&d, "host-long-round");
    let (out, _, _) = timed_status(&d);
    assert!(out.status.success(), "{}", text(&out));

    host.slow();
    let (client, repository) = (host.client.clone(), host.repository.clone());
    std::thread::spawn(move || {
        let _ = client.pass(&repository);
    });
    std::thread::sleep(sluice::host_process::READ_TICK + Duration::from_secs(1));
    let (out, took, asked) = timed_status(&d);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!asked, "the status asked origin");
    assert!(took < Duration::from_secs(10), "answered in {took:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("host process"), "{err}");
}

/// 69, 129 (design D17): with the results log holding results of two
/// hundred other combinations, `sluice status <n> --json` for a submission
/// whose combination has three shows those three, and the command reads no
/// line of the results log itself: lines rewritten in place after the host
/// process's index read them are still answered as that index read them.
#[test]
fn a_status_reads_only_the_results_it_asks_for() {
    let d = OnDisk::new("host-asks", POLICY);
    let dir = d.session("a");
    let id = queued_live(&d, &dir);
    within(60, "the submission lands", || {
        state(&d, d.repo.dir(), id) == "landed"
    });
    // One host process answers every status after, its index kept.
    let repository = d.repository(d.repo.dir());
    let _kept = d
        .client(d.repo.dir())
        .follow(Path::new(&repository), FollowKind::Wait)
        .expect("the host process");
    let status = || {
        let out = d
            .command(d.repo.dir())
            .env("SLUICE_ROUND_SLEEP", "record")
            .args(["status", &id.to_string(), "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        assert!(!text(&out).contains("reads them itself"), "{}", text(&out));
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let tasks = |doc: &serde_json::Value| -> BTreeSet<String> {
        doc["checks"]
            .as_array()
            .unwrap_or_else(|| panic!("checks in {doc}"))
            .iter()
            .map(|c| c["task"].as_str().unwrap().to_string())
            .collect()
    };
    let doc = status();
    let combination = doc["tested"]["combination"]
        .as_str()
        .unwrap_or_else(|| panic!("a combination in {doc}"))
        .to_string();
    let own = tasks(&doc);
    assert_eq!(own.len(), 1, "{doc}");

    let line = |task: &str, combination: &str| {
        format!(
            "{{\"at\":\"2026-11-13T00:00:00Z\",\"kind\":\"ran\",\"task\":\"{task}\",\
             \"hash\":\"h-{task}\",\"outcome\":\"pass\",\"branch\":\"main\",\
             \"combination\":\"{combination}\"}}\n"
        )
    };
    let mut more = String::new();
    for i in 0..200 {
        more.push_str(&line("other:check", &format!("{i:040x}")));
    }
    more.push_str(&line("b:check", &combination));
    more.push_str(&line("c:check", &combination));
    let log = d.repo.dir().join(".git/sluice/results.jsonl");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&log)
        .unwrap()
        .write_all(more.as_bytes())
        .unwrap();
    let mut three = own.clone();
    three.extend(["b:check".to_string(), "c:check".to_string()]);
    assert_eq!(tasks(&status()), three);

    // Rewritten in place, its length and file the same: a command reading
    // the log itself would read `q:check`.
    let bytes = std::fs::read(&log).unwrap();
    let rewritten = String::from_utf8(bytes)
        .unwrap()
        .replace("b:check", "q:check");
    let mut f = std::fs::OpenOptions::new().write(true).open(&log).unwrap();
    f.write_all(rewritten.as_bytes()).unwrap();
    drop(f);
    assert_eq!(tasks(&status()), three);
}
