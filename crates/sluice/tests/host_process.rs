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
