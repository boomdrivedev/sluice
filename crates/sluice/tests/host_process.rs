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

/// Stop the process `pid` with `SIGTERM`, as a service manager does, and
/// wait for it to exit: a host process ends its runs as it stops.
fn stop(pid: u32) {
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid as i32),
        nix::sys::signal::Signal::SIGTERM,
    );
    within(90, "the process stops", || !alive(pid));
}

/// Stop the host process that answers for `d`, where one does: the runs
/// that it holds end with it, so the test leaves none.
fn stop_host(d: &OnDisk) {
    if let Some(pid) = answering(d) {
        stop(pid);
    }
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

/// `pid` is the one host process, within ten seconds: it answers, or it has
/// ended. A host process ends once its queues are empty and nothing follows
/// it, and a command after that, a status among them, starts one of its own
/// (135; design D6), so another answering once `pid` is gone is no second
/// host process. Another answering while `pid` lives is asked again, as a
/// process that let its lock go lives a moment as it ends.
fn the_one_host_process(d: &OnDisk, pid: u32, what: &str) {
    within(10, what, || answering(d) == Some(pid) || !alive(pid));
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

/// `host-process`'s "An older sluice run keeps the repository it drives"
/// (135; design D7, D10): `sluice step` of a repository an older `sluice
/// run` drives runs no round of it, saying that the older run drives it,
/// and its submission stays queued for that run.
#[test]
fn a_step_of_a_repository_an_older_sluice_run_drives_runs_no_round() {
    let d = OnDisk::new("host-older-step", POLICY);
    let id = queued(&d, "a");
    let a = d.repository(d.repo.dir());
    let readers = d.state.path().join("sluice/readers");
    let older = sluice::reader::Driver::take(&readers, &a)
        .unwrap()
        .expect("the older run's lock");
    live_from_here(&d);
    let out = d
        .command(d.repo.dir())
        .args(["step", "main"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains(&format!(
            "an older sluice run drives {a}, so sluice's host process runs no round of it while \
             that run lives"
        )),
        "{}",
        text(&out)
    );
    assert_eq!(state(&d, d.repo.dir(), id), "queued", "a is left to it");
    drop(older);
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
    // Its socket went with it: none is left at its path, or a host process
    // a later command started answers there, the status that read the
    // landing starting one where it read after the first had ended (D6).
    within(10, "its socket went with it", || {
        match client.answering().unwrap() {
            Some(other) => other.named.pid != pid,
            None => !client.paths().socket.exists(),
        }
    });
}

/// reader-tick, "An idle tick starts no process" (129, 135; design D4): a
/// host process that lives on, here for an agent's held room, looks at
/// each repository in its index as an idle tick reads it, and starts no
/// task for one with nothing in play. On a host whose config names a
/// system it is not, a task's start is refused and logged: nothing is
/// logged while the repository is idle, and once work is in play there
/// the refusal is logged once, however many looks pass the repository by.
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
            None,
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

/// reader-tick, "A remote's records are asked for once a minute where
/// nothing is in play" (129 as amended 2026-11-19): a host process that
/// lives on, here for an agent's held room, asks origin for the records of
/// a repository with nothing in play once as it starts, and not again
/// within the minute, however many times it goes through its index; a
/// watch's opening asks origin at once, and the watch's passes, a second
/// apart, ask nothing more within the minute.
#[test]
fn an_idle_repositorys_remote_is_asked_once_a_minute() {
    let (d, _origin) = on_origin("host-idle-asks");
    let repository = d.repository(d.repo.dir());
    within(15, "the host process ends once idle", || {
        answering(&d).is_none()
    });
    let before = d.git.count();
    let client = d.client(d.repo.dir());
    let mut held = client
        .hold(
            sluice_seams::RunKind::Session,
            Path::new(&repository),
            "an-agent",
            None,
        )
        .expect("the host process starts");
    let answer = held.next(Some(Duration::from_secs(10))).unwrap();
    assert!(
        matches!(answer, Some(sluice::host_process::protocol::Answer::Held)),
        "{answer:?}"
    );
    let pid = answering(&d).expect("the host process answers");
    let asks = || {
        d.git.parented()[before..]
            .iter()
            .filter(|(p, args)| *p == pid && asks_a_remote(args))
            .count()
    };
    let said = || {
        format!(
            "{:?}\n{}",
            &d.git.parented()[before..],
            std::fs::read_to_string(client.paths().log()).unwrap_or_default()
        )
    };
    let until = Instant::now() + Duration::from_secs(10);
    while asks() != 1 {
        assert!(
            Instant::now() < until,
            "its first look asks origin: {}",
            said()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_secs(7));
    assert_eq!(
        asks(),
        1,
        "a look within the minute asked origin: {}",
        said()
    );

    let watch = client
        .follow(Path::new(&repository), FollowKind::Watch)
        .expect("the watch follows");
    let until = Instant::now() + Duration::from_secs(10);
    while asks() != 2 {
        assert!(
            Instant::now() < until,
            "the watch's opening asks origin: {}",
            said()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(
        asks(),
        2,
        "a watched pass within the minute asked origin: {}",
        said()
    );
    drop(watch);
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

/// 135 (design D8; D3 of `sluice-host-owns-its-workers`): a command of a
/// newer sluice has an older host process hand over, which starts afresh
/// in place: the same pid with a later `since`.
#[test]
fn a_newer_client_has_an_older_host_process_hand_over() {
    let d = OnDisk::new("host-newer", POLICY);
    let client = d.client(d.repo.dir());
    let repository = d.repository(d.repo.dir());
    let mut follow = client
        .follow(Path::new(&repository), FollowKind::Wait)
        .unwrap();
    let older = follow.named.clone();
    let newer = client.clone().with_version("0.72.0");
    let reached = newer.reach().expect("the newer command's own");
    assert_eq!(reached.named.pid, older.pid);
    assert_ne!(reached.named.since, older.since);
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

/// 135 (design D8; `never-break-an-existing-client@ed293965fc5c`): a
/// command of an older sluice uses a newer host process as it is, and a
/// request the process does not know is answered with an error naming
/// both versions.
#[test]
fn an_older_command_uses_a_newer_host_process() {
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
    stop_host(&d);
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

/// submission-wait, "Out of time as it starts" (1; design D9): beside a
/// host process that passes on its own tick, a wait's first pass is the
/// one its follow asked for, whatever its timeout: `--timeout 0s` finds
/// its submission as that pass left it, under test, never as it was
/// queued.
#[test]
fn a_waits_first_pass_beside_a_live_host_process_is_the_one_it_asked_for() {
    let slow = "[tests]\nrunner = \"command\"\ncommand = \"sleep 20\"\n\n[branches.main]\ntimeout = \"1m\"\n";
    let d = OnDisk::new("host-wait-first", slow);
    let id = queued(&d, "a");
    live_from_here(&d);
    let out = d
        .command(d.repo.dir())
        .args(["wait", &id.to_string(), "--timeout", "0s", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(sluice::WAITING), "{}", text(&out));
    let doc: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", text(&out)));
    assert_eq!(doc["state"], "testing", "{doc}");
    stop_host(&d);
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
    stop_host(&d);
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

/// Task 3.4 (design D12): a check profile in any process but the host
/// process holds its runs' parts of the host process's room over its
/// socket, deciding none by itself: with the host's bound at one, a second
/// repository's hold is refused while the first holds the run, and held
/// once the first gives its part back.
#[test]
fn a_profile_beside_the_host_process_holds_through_its_socket() {
    use sluice::profiles::checks::{Checks, RoomHandle};
    use sluice_seams::{RunKind, Tests};
    let d = OnDisk::new("host-room-socket", POLICY);
    bound(&d, 1);
    let b = d.another("host-room-socket-b", POLICY);
    let client = std::sync::Arc::new(d.client(d.repo.dir()));
    let at = |dir: &Path| {
        Checks::new(
            Path::new(&d.repository(dir)),
            std::path::PathBuf::from(env!("CARGO_BIN_EXE_sluice")),
            sluice::instance::Instance {
                bound: 1,
                reserved: 0,
                processors: 8,
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
        .with_room(RoomHandle::Socket(Some(client.clone())))
    };
    let (first, second) = (at(d.repo.dir()), at(b.dir()));
    let part = first
        .hold(RunKind::Queue, "main")
        .unwrap()
        .expect("the host's one run");
    assert!(
        second.hold(RunKind::Queue, "main").unwrap().is_none(),
        "the host's one run is the first repository's"
    );
    drop(part);
    within(10, "the run given back is the second repository's", || {
        second.hold(RunKind::Queue, "main").unwrap().is_some()
    });
    let (_, room) = client.room().unwrap().expect("it answers");
    assert_eq!(room.waiting.sessions, 0, "no session waits: {room:?}");
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
    // The overview that named the repository started a host process that
    // passes only when asked (design D14, D16): this one passes on its own.
    live_from_here(&d);
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
    stop_host(&d);
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
/// Whether a run of `d`'s repository wrote its result.
fn run_ended(d: &OnDisk) -> bool {
    let runs = d.repo.dir().join(".git/sluice/.runs");
    std::fs::read_dir(runs)
        .map(|e| e.flatten().any(|e| e.path().join("result.json").exists()))
        .unwrap_or(false)
}

fn status_live(d: &OnDisk) {
    let out = d
        .command(d.repo.dir())
        .args(["status", "main"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
}

/// host-process, "A result read before the kill" (135, S10): a run whose
/// checks end with a result after its host process was killed, before the
/// next host process starts, is landed by the one a status starts, which
/// reads its result from the run and runs no check again.
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
    within(10, "its result is written", || run_ended(&d));
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

/// Dropped, the git that waits goes on at once, so the host process can
/// stop.
impl Drop for Slowed {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.flag);
    }
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
                "#!/bin/sh\nif [ -f '{flag}' ] && [ \"$PPID\" = \"$(cat '{flag}')\" ]; then i=0; while [ -f '{flag}' ] && [ $i -lt 600 ]; do sleep 0.1; i=$((i+1)); done; fi\nexec '{real}' \"$@\"\n",
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

    /// From now on, every git the host process starts waits a minute, or
    /// until the slowing is dropped.
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
        let _ = client.pass(&repository, None);
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

/// 135, record 0107 (design D19): with #1's run going under a check that
/// waits for a mark, a watch following the host process, and the host's
/// settings changed meanwhile, the host process takes the change up and
/// its next passes run under a binding made afresh: the watch prints on,
/// the run's part and its repository are kept, and #1 lands once its
/// check ends.
#[test]
fn the_host_process_keeps_its_repositories_across_a_new_binding() {
    let marks = support::Scratch::new("host-rebind-marks");
    let go = marks.path().join("go");
    // The run waits for the mark, which each wait below bounds, or for the
    // test's marks to go; its limit is no part of what is tested, and under
    // load the take-up between its start and the mark may outlast a minute.
    let policy = format!(
        "[tests]\nrunner = \"command\"\n\
         command = \"while [ ! -f {} ] && [ -d {} ]; do sleep 0.1; done\"\n\n\
         [branches.main]\ntimeout = \"10m\"\n",
        go.display(),
        marks.path().display()
    );
    let d = OnDisk::new("host-rebind", &policy);
    let config = d.config.path().join("sluice/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, "[runs]\nmax = 2\n").unwrap();
    let id = queued(&d, "a");

    let mut watch = d
        .command(d.repo.dir())
        .env("SLUICE_ROUND_SLEEP", "record")
        .args(["overview", "--json", "--watch"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let lines = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = lines.clone();
    let stdout = watch.stdout.take().unwrap();
    std::thread::spawn(move || {
        for _ in BufReader::new(stdout).lines() {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    });
    within(60, "#1 is under test", || {
        state(&d, d.repo.dir(), id) == "testing"
    });

    std::fs::write(&config, "[runs]\nmax = 3\n").unwrap();
    let log = d.client(d.repo.dir()).paths().log();
    let until = Instant::now() + Duration::from_secs(30);
    while !std::fs::read_to_string(&log)
        .unwrap_or_default()
        .contains("took up the host's settings in")
    {
        assert!(
            Instant::now() < until,
            "the host process takes the change up: {}",
            host_logs(&d)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let before = lines.load(std::sync::atomic::Ordering::SeqCst);
    std::fs::write(&go, "").unwrap();
    within(60, "#1 lands", || state(&d, d.repo.dir(), id) == "landed");
    within(10, "the watch prints on", || {
        lines.load(std::sync::atomic::Ordering::SeqCst) > before
    });
    assert!(watch.try_wait().unwrap().is_none(), "the watch goes on");
    let _ = watch.kill();
    let _ = watch.wait();
    let said = std::fs::read_to_string(&log).unwrap();
    assert!(!said.contains("do not bind here"), "{said}");
}

/// The host's settings file in `d`'s config: written as `text`.
fn settings(d: &OnDisk, text: &str) -> std::path::PathBuf {
    let file = d.config.path().join("sluice/config.toml");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let next = file.with_extension("next");
    std::fs::write(&next, text).unwrap();
    std::fs::rename(&next, &file).unwrap();
    file
}

/// 135, record 0107 as amended 2026-11-13 (design D20; D3 of
/// `sluice-host-owns-its-workers`): a submit made as the host process
/// starts afresh in place for a changed `[telemetry]` reaches the image
/// started afresh, exactly one host process runs, and the submission lands
/// with no other command moving it.
#[test]
fn a_submit_during_a_hand_over_reaches_the_successor() {
    let marks = support::Scratch::new("host-hand-over-submit-marks");
    let d = OnDisk::new("host-hand-over-submit", &held_policy(&marks));
    settings(&d, "[telemetry]\nservice = \"one\"\n");
    let held = d.session("a");
    let dir = d.session("b");
    live_from_here(&d);
    // A host process that passes at its own tick, its first submission's
    // check held, so it has work in play across the hand-over.
    let first_id = queued_live(&d, &held);
    within(60, "the first submission's check runs", || {
        check_of(&marks).is_some()
    });
    let first = named(&d).expect("a host process");

    settings(&d, "[telemetry]\nservice = \"two\"\n");
    within(30, "the host process hands over", || {
        host_logs(&d).contains("takes up by starting afresh in place")
    });
    let id = queued_live(&d, &dir);

    within(15, "the image started afresh answers", || {
        named(&d).is_some_and(|n| n.since != first.since)
    });
    let successor = answering(&d).expect("a host process answers");
    assert_eq!(successor, first.pid, "the same pid answers");
    std::fs::write(marks.path().join("go"), "").unwrap();
    lands(&d, first_id);
    lands(&d, id);
    the_one_host_process(&d, successor, "one host process throughout");
}

/// settings-take-up, "A successor that does not answer" (135, record 0107
/// as amended 2026-11-13; D3 of `sluice-host-owns-its-workers`): where the
/// executable the host process starts afresh as for a changed
/// `[telemetry]` is gone, it takes its lock back, its socket answers with
/// its own pid and `since`, its run goes on, and its log says once that it
/// could not start afresh.
#[test]
fn a_successor_that_does_not_start_leaves_the_host_process_as_it_was() {
    let marks = support::Scratch::new("host-no-successor-marks");
    let (began, go) = (marks.path().join("began"), marks.path().join("go"));
    // The run marks that its check began, then waits for the mark, which
    // each wait below bounds, or for the test's marks to go. Its limit is
    // no part of what is tested: the hand-over tried between its start and
    // the mark waits ten seconds for a successor after its tasks end their
    // passes, and under load a minute may pass before the mark.
    let policy = format!(
        "[tests]\nrunner = \"command\"\n\
         command = \"touch {}; while [ ! -f {} ] && [ -d {} ]; do sleep 0.1; done\"\n\n\
         [branches.main]\ntimeout = \"10m\"\n",
        began.display(),
        go.display(),
        marks.path().display()
    );
    let d = OnDisk::new("host-no-successor", &policy);
    settings(&d, "[telemetry]\nservice = \"one\"\n");
    let dir = d.session("a");
    let repository = d.repository(d.repo.dir());
    live_from_here(&d);
    // The host process started from a copy of sluice, which is gone when
    // it starts afresh.
    let bin = support::Scratch::new("host-no-successor-bin");
    let copy = bin.path().join("sluice");
    std::fs::copy(env!("CARGO_BIN_EXE_sluice"), &copy).unwrap();
    let client =
        sluice::host_process::client::Client::new(&copy, support::env_of(&d.command(d.repo.dir())))
            .unwrap();
    let kept = client
        .follow(Path::new(&repository), FollowKind::Wait)
        .expect("a host process");
    let first = kept.named.clone();
    let id = queued_live(&d, &dir);
    within(60, "#1 is under test", || {
        state(&d, d.repo.dir(), id) == "testing"
    });
    // The round marks #1 testing before it makes the run's place and
    // starts its supervisor from the host process's own executable, so the
    // copy goes only once the run's check has begun: gone sooner, the
    // supervisor cannot start and #1 fails.
    within(60, "#1's check begins", || began.exists());
    std::fs::remove_file(&copy).unwrap();

    settings(&d, "[telemetry]\nservice = \"two\"\n");
    let log = client.paths().log();
    let said = || std::fs::read_to_string(&log).unwrap_or_default();
    within(
        40,
        "the host process says it could not start afresh",
        || said().contains("it could not start afresh"),
    );
    assert!(alive(first.pid), "{}", said());
    let now = named(&d).expect("it answers");
    assert_eq!(
        (now.pid, &now.since),
        (first.pid, &first.since),
        "{}",
        said()
    );
    let warned: Vec<String> = said()
        .lines()
        .filter(|l| l.contains("it could not start afresh"))
        .map(str::to_string)
        .collect();
    assert_eq!(warned.len(), 1, "{}", said());
    assert!(
        warned[0]
            .contains("changed [telemetry], which the host process takes up only by handing over"),
        "{}",
        warned[0]
    );
    std::fs::write(&go, "").unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        let now = state(&d, d.repo.dir(), id);
        if now == "landed" {
            break;
        }
        let status = d
            .command(d.repo.dir())
            .args(["status", "main", "--json"])
            .output()
            .unwrap();
        assert!(
            Instant::now() < until,
            "#1 lands: {now} after 60s\n{}\n{}",
            text(&status),
            said()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    the_one_host_process(&d, first.pid, "the host process as it was");
    drop(kept);
}

/// `sluice daemon`, as a machine that keeps it running starts it, in a
/// folder of `d`'s with `d`'s environment, passing at its own tick (design
/// D1 of `sluice-runs-as-a-daemon`). It never ends for being idle: dropped,
/// it is killed and waited for.
struct Daemon {
    child: std::process::Child,
    /// What it says on stderr before it holds the host.
    err: std::path::PathBuf,
    _scratch: support::Scratch,
}

impl Daemon {
    fn start(d: &OnDisk, dir: &Path) -> Daemon {
        Daemon::start_with(d.command(dir))
    }

    fn start_with(mut command: std::process::Command) -> Daemon {
        let scratch = support::Scratch::new("daemon-err");
        let err = scratch.path().join("stderr");
        let child = command
            .arg("daemon")
            .stdout(std::process::Stdio::null())
            .stderr(std::fs::File::create(&err).unwrap())
            .spawn()
            .expect("sluice daemon starts");
        Daemon {
            child,
            err,
            _scratch: scratch,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// What it said on stderr.
    fn said(&self) -> String {
        std::fs::read_to_string(&self.err).unwrap_or_default()
    }

    /// Whether it still runs.
    fn runs(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }

    /// Signal it with `signal`, and wait at most ninety seconds for its
    /// exit, as a service manager stops it (design D7).
    fn stop(&mut self, signal: nix::sys::signal::Signal) -> std::process::ExitStatus {
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(self.pid() as i32), signal).unwrap();
        let until = Instant::now() + Duration::from_secs(90);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < until, "the daemon did not stop");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// A daemon dropped is stopped as a service manager stops it, so it ends
/// its workers, and killed where it has not exited within thirty seconds.
impl Drop for Daemon {
    fn drop(&mut self) {
        if self.runs() {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(self.pid() as i32),
                nix::sys::signal::Signal::SIGTERM,
            );
            let until = Instant::now() + Duration::from_secs(30);
            while self.runs() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The host process that answers now, as it names itself.
fn named(d: &OnDisk) -> Option<sluice::host_process::protocol::Named> {
    d.client(d.repo.dir()).answering().unwrap().map(|c| c.named)
}

/// `daemon` answers as the host process, within thirty seconds.
fn answers(d: &OnDisk, daemon: &Daemon) {
    within(30, "the daemon answers", || {
        answering(d) == Some(daemon.pid())
    });
}

/// The host process's tick (64, 129).
const TICK: Duration = Duration::from_secs(3);

/// host-daemon, "Idle, it keeps running", and host-process, "A daemon's
/// queues empty" (135, 137; design D1 of `sluice-runs-as-a-daemon`):
/// `sluice daemon` with nothing queued and nothing following it still runs
/// three ticks on, the host process that answers is it, and its `hello`
/// and `host/version` say it is a daemon; and once the one submission
/// queued lands, it still runs three ticks after the landing.
#[test]
fn an_idle_daemon_keeps_running() {
    let d = OnDisk::new("daemon-idle", POLICY);
    let dir = d.session("a");
    let mut daemon = Daemon::start(&d, d.repo.dir());
    answers(&d, &daemon);
    let hello = named(&d).expect("it answers");
    assert!(hello.daemon, "{hello:?}");
    let version = d
        .client(d.repo.dir())
        .paths()
        .named()
        .expect("host/version");
    assert!(version.daemon, "{version:?}");
    assert_eq!(version.pid, daemon.pid());

    std::thread::sleep(TICK * 3);
    assert!(daemon.runs(), "idle three ticks: {}", daemon.said());
    assert_eq!(answering(&d), Some(daemon.pid()));

    let id = queued_live(&d, &dir);
    within(60, "the submission lands", || {
        state(&d, d.repo.dir(), id) == "landed"
    });
    std::thread::sleep(TICK * 3);
    assert!(
        daemon.runs(),
        "three ticks after the landing: {}",
        daemon.said()
    );
    assert_eq!(answering(&d), Some(daemon.pid()));
}

/// The lines of the host process's log and the one before it that say a
/// host process started, or that one found the host held and ended.
fn starts_in_log(d: &OnDisk) -> Vec<String> {
    let paths = d.client(d.repo.dir()).paths().clone();
    [paths.previous_log(), paths.log()]
        .iter()
        .flat_map(|p| {
            std::fs::read_to_string(p)
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|l| l.contains(" started") || l.contains("another host process holds"))
        .collect()
}

/// host-daemon, "A submission to an idle daemon" (135, 137; design D1 of
/// `sluice-runs-as-a-daemon`): a submit that does not wait, in a repository
/// whose `main` holds a policy, reaches the idle daemon, which lands it,
/// and no other host process starts.
#[test]
fn a_daemon_lands_a_submission_with_no_other_host_process() {
    let d = OnDisk::new("daemon-lands", POLICY);
    let dir = d.session("a");
    let mut daemon = Daemon::start(&d, d.repo.dir());
    answers(&d, &daemon);
    std::thread::sleep(TICK);
    let id = queued_live(&d, &dir);
    within(60, "the submission lands", || {
        state(&d, d.repo.dir(), id) == "landed"
    });
    assert!(daemon.runs(), "{}", daemon.said());
    assert_eq!(answering(&d), Some(daemon.pid()));
    let starts = starts_in_log(&d);
    assert_eq!(starts.len(), 1, "one host process started: {starts:#?}");
    assert!(
        starts[0].contains(&format!("sluice daemon {} started", sluice::VERSION)),
        "{starts:#?}"
    );
}

/// host-daemon, "Started in a repository" and "Started in no repository"
/// (135; design D8 of `sluice-runs-as-a-daemon`): `sluice daemon` started
/// in a repository whose `main` holds a policy, in no index, adds it to the
/// host's index before it runs as the host process; started in `/`, it
/// runs as the host process all the same, and the index is as it was.
#[test]
fn a_daemon_indexes_the_repository_it_starts_in() {
    let d = OnDisk::new("daemon-index", POLICY);
    let index = d.client(d.repo.dir()).paths().index();
    assert_eq!(index.list().unwrap(), Vec::<std::path::PathBuf>::new());
    let repository = std::fs::canonicalize(d.repo.dir().join(".git")).unwrap();
    let daemon = Daemon::start(&d, d.repo.dir());
    answers(&d, &daemon);
    assert_eq!(index.list().unwrap(), vec![repository]);
    drop(daemon);

    let d = OnDisk::new("daemon-index-none", POLICY);
    let index = d.client(d.repo.dir()).paths().index();
    let daemon = Daemon::start(&d, Path::new("/"));
    answers(&d, &daemon);
    assert_eq!(index.list().unwrap(), Vec::<std::path::PathBuf>::new());
}

/// host-process, "A daemon" (150; design D1 of `sluice-runs-as-a-daemon`):
/// with `sluice daemon` running, `sluice overview --json` gives
/// `host_process.daemon` true and its pid; with a host process a wait
/// started, `host_process.daemon` is false.
#[test]
fn the_overview_names_a_daemon() {
    let overview = |d: &OnDisk| {
        let out = d
            .command(d.repo.dir())
            .args(["overview", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let d = OnDisk::new("daemon-overview", POLICY);
    let daemon = Daemon::start(&d, d.repo.dir());
    answers(&d, &daemon);
    let doc = overview(&d);
    assert_eq!(doc["host_process"]["daemon"], true, "{doc}");
    assert_eq!(doc["host_process"]["pid"], daemon.pid(), "{doc}");
    drop(daemon);

    let d = OnDisk::new("daemon-overview-wait", POLICY);
    let repository = d.repository(d.repo.dir());
    let follow = d
        .client(d.repo.dir())
        .follow(Path::new(&repository), FollowKind::Wait)
        .unwrap();
    let doc = overview(&d);
    assert_eq!(doc["host_process"]["daemon"], false, "{doc}");
    assert_eq!(doc["host_process"]["pid"], follow.named.pid, "{doc}");
}

/// A policy whose check marks that it began in `marks`, then waits for
/// `go` there, or for the marks to go: its run goes on until the host
/// process that owns it stops.
fn held_policy(marks: &support::Scratch) -> String {
    format!(
        "[tests]\nrunner = \"command\"\n\
         command = \"echo run >> {began}; while [ ! -f {go} ] && [ -d {dir} ]; do sleep 0.1; done\"\n\n\
         [branches.main]\ntimeout = \"10m\"\n",
        began = marks.path().join("began").display(),
        go = marks.path().join("go").display(),
        dir = marks.path().display()
    )
}

/// host-daemon, "A host process a wait started" (135; design D2 of
/// `sluice-runs-as-a-daemon`): with a host process that ends when idle,
/// started by `sluice wait`, running a combination's checks, `sluice
/// daemon` has it hand over and takes its place; the wait follows on to
/// its submission's landing.
#[test]
fn a_daemon_takes_the_place_of_a_host_process_that_ends_when_idle() {
    let marks = support::Scratch::new("daemon-takes-marks");
    let d = OnDisk::new("daemon-takes", &held_policy(&marks));
    let id = queued(&d, "a");
    live_from_here(&d);
    let wait = d
        .command(d.repo.dir())
        .args(["wait", &id.to_string()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    within(60, "the wait's host process runs the check", || {
        marks.path().join("began").exists()
    });
    let first = answering(&d).expect("the wait's host process");
    assert!(!named(&d).unwrap().daemon);

    let mut daemon = Daemon::start(&d, d.repo.dir());
    within(90, "the first host process ends", || !alive(first));
    answers(&d, &daemon);
    std::fs::write(marks.path().join("go"), "").unwrap();
    let out = wait.wait_with_output().unwrap();
    assert!(out.status.success(), "{}\n{}", text(&out), daemon.said());
    assert!(text(&out).contains("landed"), "{}", text(&out));
    assert!(daemon.runs(), "{}", daemon.said());
    assert_eq!(answering(&d), Some(daemon.pid()));
}

/// A process standing for a host process that is no daemon and never
/// ends: the test holds the host's lock and answers at its socket as it,
/// `host/version` naming it, for as long as the returned guard lives.
struct Standing {
    _lock: std::fs::File,
    sleeper: std::process::Child,
}

impl Drop for Standing {
    fn drop(&mut self) {
        let _ = self.sleeper.kill();
        let _ = self.sleeper.wait();
    }
}

fn standing(d: &OnDisk, daemon: bool, version: &str) -> Standing {
    let paths = d.client(d.repo.dir()).paths().clone();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(paths.lock())
        .unwrap();
    lock.lock().unwrap();
    let sleeper = std::process::Command::new("sleep")
        .arg("600")
        .spawn()
        .unwrap();
    let named = serde_json::json!({
        "version": version,
        "pid": sleeper.id(),
        "since": "2026-11-18T00:00:00.000Z",
        "daemon": daemon,
    });
    std::fs::write(paths.version(), named.to_string()).unwrap();
    let _ = std::fs::remove_file(&paths.socket);
    let listener = std::os::unix::net::UnixListener::bind(&paths.socket).unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let named = named.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut writer = stream;
                let mut hello = String::new();
                if reader.read_line(&mut hello).unwrap_or(0) == 0 {
                    return;
                }
                let _ = writeln!(writer, "{named}");
                let mut request = String::new();
                if reader.read_line(&mut request).unwrap_or(0) > 0 {
                    let _ = writeln!(writer, r#""ok""#);
                }
            });
        }
    });
    Standing {
        _lock: lock,
        sleeper,
    }
}

/// host-daemon, "A host process that does not hand over" (135; design D2
/// of `sluice-runs-as-a-daemon`): where a process holds the host's lock
/// and answers as a host process that is no daemon, and never ends when
/// asked to hand over, `sluice daemon` exits 3 once the hand-over's wait
/// has passed, naming the log.
#[test]
fn a_daemon_whose_host_process_does_not_hand_over_exits_3() {
    let d = OnDisk::new("daemon-held", POLICY);
    let _standing = standing(&d, false, sluice::VERSION);
    let began = Instant::now();
    let mut daemon = Daemon::start(&d, d.repo.dir());
    let wait = sluice::host_process::client::HAND_OVER_WAIT;
    let until = Instant::now() + wait + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = daemon.child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < until, "it never exited: {}", daemon.said());
        std::thread::sleep(Duration::from_millis(200));
    };
    assert!(began.elapsed() >= wait, "{:?}", began.elapsed());
    assert_eq!(status.code(), Some(3), "{}", daemon.said());
    let log = d.client(d.repo.dir()).paths().log();
    assert!(
        daemon.said().contains(&log.display().to_string()),
        "{}",
        daemon.said()
    );
}

/// host-daemon, "A second daemon" (design D2 of
/// `sluice-runs-as-a-daemon`): a second `sluice daemon` asks the first
/// nothing, says once on stderr that it waits for it, naming its pid and
/// version, and answers once the first is stopped.
#[test]
fn a_second_daemon_waits_and_takes_the_place_of_the_first() {
    let d = OnDisk::new("daemon-second", POLICY);
    let mut first = Daemon::start(&d, d.repo.dir());
    answers(&d, &first);
    let mut second = Daemon::start(&d, d.repo.dir());
    let naming = format!("daemon {} of sluice {}", first.pid(), sluice::VERSION);
    within(30, "the second says which daemon it waits for", || {
        second.said().contains(&naming)
    });
    std::thread::sleep(TICK);
    assert!(second.runs(), "{}", second.said());
    assert_eq!(answering(&d), Some(first.pid()));
    let log = std::fs::read_to_string(d.client(d.repo.dir()).paths().log()).unwrap();
    assert!(!log.contains("asked this one to hand over"), "{log}");

    let status = first.stop(nix::sys::signal::Signal::SIGTERM);
    assert!(status.success(), "{status:?}");
    within(30, "the second answers", || {
        answering(&d) == Some(second.pid())
    });
    let said = second.said();
    assert_eq!(
        said.lines().filter(|l| l.contains(&naming)).count(),
        1,
        "{said}"
    );
}

/// What the host process said, in its log and the one before it.
fn host_logs(d: &OnDisk) -> String {
    let paths = d.client(d.repo.dir()).paths().clone();
    [paths.previous_log(), paths.log()]
        .iter()
        .map(|p| std::fs::read_to_string(p).unwrap_or_default())
        .collect()
}

/// Ask the host process that answers to hand over, naming `program`.
fn ask_hand_over(d: &OnDisk, program: &Path) {
    let mut c = d
        .client(d.repo.dir())
        .answering()
        .unwrap()
        .expect("a host process answers");
    let answer = c
        .ask(&sluice::host_process::protocol::Request::HandOver {
            program: Some(program.to_path_buf()),
            ends: false,
        })
        .unwrap();
    assert_eq!(answer, sluice::host_process::protocol::Answer::Ok);
}

/// host-daemon, "The daemon is asked to hand over" (135; design D3, D4
/// of `sluice-runs-as-a-daemon`): a daemon running a combination's checks,
/// asked to hand over naming an executable, starts afresh in place as that
/// executable: a host process with its pid and a later `since` answers as
/// a daemon, its log says it started afresh as that executable, and the
/// submission lands.
#[test]
fn a_daemon_asked_to_hand_over_starts_afresh_in_place() {
    let marks = support::Scratch::new("daemon-afresh-marks");
    let d = OnDisk::new("daemon-afresh", &held_policy(&marks));
    let dir = d.session("a");
    let daemon = Daemon::start(&d, d.repo.dir());
    answers(&d, &daemon);
    let first = named(&d).expect("it answers");
    let id = queued_live(&d, &dir);
    within(60, "the combination's check runs", || {
        marks.path().join("began").exists()
    });
    // The executable named: a copy of sluice's, so the image started
    // afresh is the one the ask named.
    let bin = support::Scratch::new("daemon-afresh-bin");
    let copy = bin.path().join("sluice");
    std::fs::copy(env!("CARGO_BIN_EXE_sluice"), &copy).unwrap();
    let copy = std::fs::canonicalize(&copy).unwrap();
    ask_hand_over(&d, &copy);
    within(90, "the daemon started afresh answers", || {
        named(&d).is_some_and(|n| n.pid == first.pid && n.since != first.since && n.daemon)
    });
    let logs = host_logs(&d);
    assert!(
        logs.contains(&format!("started afresh in place as {}", copy.display())),
        "{logs}"
    );
    std::fs::write(marks.path().join("go"), "").unwrap();
    within(60, "the submission lands", || {
        state(&d, d.repo.dir(), id) == "landed"
    });
    let ran = std::fs::read_to_string(marks.path().join("began")).unwrap();
    assert_eq!(ran.lines().count(), 1, "no check ran again: {ran}");
}

/// A child killed and waited for once dropped.
struct Kept(std::process::Child);

impl Drop for Kept {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// host-daemon, "A newer command reaches an older daemon" (135; design D3,
/// D4 of `sluice-runs-as-a-daemon`): with a process holding the host's
/// lock and answering as a daemon of sluice 0.71.0, a command of sluice
/// 0.72.0 asks it to hand over naming the command's own executable, and
/// reaches the host process started in its place.
#[test]
fn a_newer_client_has_an_older_daemon_start_afresh() {
    let d = OnDisk::new("daemon-newer", POLICY);
    let paths = d.client(d.repo.dir()).paths().clone();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(paths.lock())
        .unwrap();
    lock.lock().unwrap();
    let mut sleeper = std::process::Command::new("sleep")
        .arg("600")
        .spawn()
        .unwrap();
    let older = serde_json::json!({
        "version": "0.71.0",
        "pid": sleeper.id(),
        "since": "2026-11-18T00:00:00.000Z",
        "daemon": true,
    });
    std::fs::write(paths.version(), older.to_string()).unwrap();
    let _ = std::fs::remove_file(&paths.socket);
    let listener = std::os::unix::net::UnixListener::bind(&paths.socket).unwrap();
    let mut start = d.command(d.repo.dir());
    start
        .arg("daemon")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let socket = paths.socket.clone();
    // The older daemon: asked to hand over, it lets its lock and socket go
    // and starts `<the executable named> daemon`, standing for its start
    // afresh in place.
    let handed = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut hello = String::new();
        reader.read_line(&mut hello).unwrap();
        writeln!(writer, "{older}").unwrap();
        let mut request = String::new();
        reader.read_line(&mut request).unwrap();
        writeln!(writer, r#""ok""#).unwrap();
        drop(listener);
        let _ = std::fs::remove_file(&socket);
        drop(lock);
        let _ = sleeper.kill();
        let _ = sleeper.wait();
        let asked: serde_json::Value = serde_json::from_str(request.trim()).unwrap();
        let program = asked["program"].as_str().expect("a program").to_string();
        let started = std::process::Command::new(&program)
            .args(start.get_args().collect::<Vec<_>>())
            .env_clear()
            .envs(support::env_of(&start))
            .current_dir(start.get_current_dir().unwrap())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        (asked, Kept(started))
    });
    let reached = d
        .client(d.repo.dir())
        .with_version("0.72.0")
        .reach()
        .expect("the host process started in the older daemon's place");
    let (asked, started) = handed.join().unwrap();
    assert_eq!(asked["ask"], "hand-over", "{asked}");
    let own = std::fs::canonicalize(env!("CARGO_BIN_EXE_sluice")).unwrap();
    assert_eq!(asked["program"], own.display().to_string(), "{asked}");
    assert_eq!(reached.named.pid, started.0.id());
    assert!(reached.named.daemon, "{:?}", reached.named);
}

/// host-daemon, "An executable that is not there" (design D3 of
/// `sluice-runs-as-a-daemon`): a daemon asked to hand over naming an
/// executable that is not there goes on as it was: the host process that
/// answers has its pid and its first `since`, its log names the executable
/// once, and it goes on passing.
#[test]
fn a_daemon_whose_start_afresh_fails_goes_on() {
    let d = OnDisk::new("daemon-afresh-fails", POLICY);
    let dir = d.session("a");
    let daemon = Daemon::start(&d, d.repo.dir());
    answers(&d, &daemon);
    let first = named(&d).expect("it answers");
    let none = support::Scratch::new("daemon-afresh-none");
    let missing = none.path().join("no-such-sluice");
    ask_hand_over(&d, &missing);
    within(90, "it says it could not start afresh", || {
        host_logs(&d).contains("could not start afresh")
    });
    let now = named(&d).expect("it answers");
    assert_eq!((now.pid, &now.since), (first.pid, &first.since));
    let logs = host_logs(&d);
    assert_eq!(
        logs.matches(&missing.display().to_string()).count(),
        1,
        "{logs}"
    );
    let id = queued_live(&d, &dir);
    within(60, "the submission lands", || {
        state(&d, d.repo.dir(), id) == "landed"
    });
    assert_eq!(answering(&d), Some(daemon.pid()));
}

/// host-daemon, "A submit during the start afresh", and settings-take-up,
/// "A submit during a daemon's start afresh" (135; record 0107 as amended
/// 2026-11-13; design D5, D6 of `sluice-runs-as-a-daemon`): a submit made
/// as the daemon starts afresh for a changed `[telemetry]` reaches the
/// daemon started afresh, no other host process starts, and the
/// submission lands.
#[test]
fn a_submit_during_a_daemons_start_afresh_reaches_it() {
    let d = OnDisk::new("daemon-afresh-submit", POLICY);
    settings(&d, "[telemetry]\nservice = \"one\"\n");
    let dir = d.session("a");
    let daemon = Daemon::start(&d, d.repo.dir());
    answers(&d, &daemon);
    let first = named(&d).expect("it answers");

    settings(&d, "[telemetry]\nservice = \"two\"\n");
    within(30, "the daemon starts afresh", || {
        host_logs(&d).contains("it starts afresh in its own process")
    });
    let id = queued_live(&d, &dir);
    within(30, "the daemon started afresh answers", || {
        named(&d).is_some_and(|n| n.pid == first.pid && n.since != first.since)
    });
    within(60, "the submission lands", || {
        state(&d, d.repo.dir(), id) == "landed"
    });
    let starts = starts_in_log(&d);
    assert!(
        starts.iter().all(|l| l.contains("sluice daemon ")),
        "no other host process: {starts:#?}"
    );
    assert_eq!(answering(&d), Some(daemon.pid()));
}

/// host-daemon, "Stopped while a run goes on" (135; design D7 of
/// `sluice-runs-as-a-daemon`; `sluices-runs-end-with-its-host-process@e814b716f3b1`):
/// `sluice daemon` running a combination's checks, sent `SIGTERM`, exits 0,
/// its socket goes and no process of the checks runs; once `sluice wait`
/// runs, the combination is tested again and the submission lands.
#[test]
fn a_stopped_daemon_ends_its_runs() {
    let marks = support::Scratch::new("daemon-stopped-marks");
    let d = OnDisk::new("daemon-stopped", &held_policy(&marks));
    let dir = d.session("a");
    let mut daemon = Daemon::start(&d, d.repo.dir());
    answers(&d, &daemon);
    let id = queued_live(&d, &dir);
    within(60, "the combination's checks run", || {
        check_of(&marks).is_some()
    });
    // Once its round has recorded the run started (S10).
    std::thread::sleep(Duration::from_secs(1));
    let status = daemon.stop(nix::sys::signal::Signal::SIGTERM);
    assert!(status.success(), "{status:?}: {}", host_logs(&d));
    assert!(!d.client(d.repo.dir()).paths().socket.exists());
    assert_eq!(check_of(&marks), None, "{}", host_logs(&d));
    std::fs::write(marks.path().join("go"), "").unwrap();
    let out = d
        .command(d.repo.dir())
        .args(["wait", &id.to_string()])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(state(&d, d.repo.dir(), id), "landed");
    let runs = std::fs::read_to_string(marks.path().join("began")).unwrap();
    assert_eq!(runs.lines().count(), 2, "tested again: {runs}");
}

/// The pid of the run's check that `marks` holds: the shell whose command
/// names its marks, where one runs.
fn check_of(marks: &support::Scratch) -> Option<u32> {
    use sysinfo::{ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};
    let began = marks.path().join("began").display().to_string();
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    system
        .processes()
        .iter()
        .filter(|(_, p)| !matches!(p.status(), ProcessStatus::Zombie))
        .find(|(_, p)| {
            let words: Vec<String> = p
                .cmd()
                .iter()
                .map(|c| c.to_string_lossy().into_owned())
                .collect();
            let line = words.join(" ");
            line.contains(&began) && line.contains("while")
        })
        .map(|(pid, _)| pid.as_u32())
}

/// The process groups that `d`'s host state names as workers.
fn recorded_groups(d: &OnDisk) -> Vec<i32> {
    let dir = d
        .state
        .path()
        .join("sluice/host")
        .join(sluice::workers::DIR);
    sluice::workers::records(&dir)
        .into_iter()
        .map(|r| r.pid)
        .collect()
}

/// The process group of `pid`.
fn group_of(pid: u32) -> i32 {
    nix::unistd::getpgid(Some(nix::unistd::Pid::from_raw(pid as i32)))
        .unwrap()
        .as_raw()
}

/// The submission `id` lands within ninety seconds, under whatever host
/// process a status starts.
fn lands(d: &OnDisk, id: u64) {
    let until = Instant::now() + Duration::from_secs(90);
    while state(d, d.repo.dir(), id) != "landed" {
        assert!(
            Instant::now() < until,
            "the submission lands: {}",
            host_logs(d)
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// resident-workers, "A combination's checks"
/// (`workers-end-with-their-owner@292e869158d6`): the checks' parent chain
/// reaches `sluice host`, their process group is not its group, and the
/// host's state names that group.
#[test]
fn a_runs_checks_are_a_child_group_of_sluice_host() {
    let marks = support::Scratch::new("host-child-group-marks");
    let d = OnDisk::new("host-child-group", &held_policy(&marks));
    let dir = d.session("a");
    let id = queued_live(&d, &dir);
    within(60, "the combination's check runs", || {
        check_of(&marks).is_some()
    });
    let host = answering(&d).expect("the host process runs");
    let check = check_of(&marks).expect("the check runs");
    let supervisor = sluice::workers::parent(check as i32).expect("its parent");
    assert_eq!(
        sluice::workers::parent(supervisor),
        Some(host as i32),
        "the check's parent chain reaches sluice host"
    );
    assert_ne!(group_of(check), group_of(host));
    assert_ne!(group_of(supervisor as u32), group_of(host));
    let groups = recorded_groups(&d);
    assert!(groups.contains(&group_of(check)), "{groups:?}");
    assert!(groups.contains(&supervisor), "{groups:?}");
    std::fs::write(marks.path().join("go"), "").unwrap();
    lands(&d, id);
}

/// resident-workers, "Stopped while a combination is under test", and "The
/// run after a stop" (`sluices-runs-end-with-its-host-process@e814b716f3b1`):
/// a host process that a command started, sent `SIGTERM` while a
/// combination's checks run, has ended them when it exits; the run has no
/// outcome, and the next host process tests the combination again.
#[test]
fn a_stopped_host_process_ends_its_runs() {
    let marks = support::Scratch::new("host-stop-ends-marks");
    let d = OnDisk::new("host-stop-ends", &held_policy(&marks));
    let dir = d.session("a");
    let id = queued_live(&d, &dir);
    within(60, "the combination's check runs", || {
        check_of(&marks).is_some()
    });
    // Once its round has recorded the run started (S10).
    std::thread::sleep(Duration::from_secs(1));
    let host = answering(&d).expect("the host process runs");
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(host as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    within(60, "the host process exits", || !alive(host));
    assert_eq!(check_of(&marks), None, "{}", host_logs(&d));
    assert!(recorded_groups(&d).is_empty(), "{:?}", recorded_groups(&d));
    std::fs::write(marks.path().join("go"), "").unwrap();
    status_live(&d);
    lands(&d, id);
    let ran = std::fs::read_to_string(marks.path().join("began")).unwrap();
    assert_eq!(ran.lines().count(), 2, "tested again: {ran}");
}

/// resident-workers, "Killed with SIGKILL", and host-process, "Killed while
/// a combination is under test" (`nothing-detached@f807c7f5fcea`): a host
/// process killed with `SIGKILL` leaves its checks running; the next host
/// process ends their group before it starts a worker, the run has no
/// outcome, and the combination is tested again.
#[test]
fn a_killed_host_processs_runs_end_at_the_next_start() {
    let marks = support::Scratch::new("host-killed-ends-marks");
    let d = OnDisk::new("host-killed-ends", &held_policy(&marks));
    let dir = d.session("a");
    let id = queued_live(&d, &dir);
    within(60, "the combination's check runs", || {
        check_of(&marks).is_some()
    });
    std::thread::sleep(Duration::from_secs(1));
    let host = answering(&d).expect("the host process runs");
    kill(host);
    within(10, "it is gone", || !alive(host));
    let check = check_of(&marks).expect("the check outlives a killed host process");
    status_live(&d);
    within(30, "the next host process ends the check", || !alive(check));
    let logs = host_logs(&d);
    assert!(logs.contains("which a stopped host process left"), "{logs}");
    std::fs::write(marks.path().join("go"), "").unwrap();
    lands(&d, id);
    let ran = std::fs::read_to_string(marks.path().join("began")).unwrap();
    assert_eq!(ran.lines().count(), 2, "tested again: {ran}");
}

/// resident-workers, "An upgrade while a combination is under test", and
/// host-process, "The host's sluice is upgraded"
/// (`sluices-runs-end-with-its-host-process@e814b716f3b1`): a host process
/// that a command started, running a combination's checks, asked to hand
/// over by a newer command, starts afresh in place: the newer command
/// reaches a host process with the same pid and a later `since`, the
/// checks go on as its children, and their result lands the combination
/// with no check run again.
#[test]
fn an_upgrade_keeps_its_runs_as_children() {
    let marks = support::Scratch::new("host-upgrade-marks");
    let d = OnDisk::new("host-upgrade", &held_policy(&marks));
    let dir = d.session("a");
    let id = queued_live(&d, &dir);
    within(60, "the combination's check runs", || {
        check_of(&marks).is_some()
    });
    let first = named(&d).expect("the host process answers");
    let check = check_of(&marks).unwrap();
    let reached = d
        .client(d.repo.dir())
        .with_version("99.0.0")
        .reach()
        .expect("the newer command's own");
    assert_eq!(reached.named.pid, first.pid, "{}", host_logs(&d));
    assert_ne!(reached.named.since, first.since);
    drop(reached);
    assert!(alive(check), "the check goes on");
    let supervisor = sluice::workers::parent(check as i32).unwrap();
    assert_eq!(sluice::workers::parent(supervisor), Some(first.pid as i32));
    std::fs::write(marks.path().join("go"), "").unwrap();
    lands(&d, id);
    let ran = std::fs::read_to_string(marks.path().join("began")).unwrap();
    assert_eq!(ran.lines().count(), 1, "no check ran again: {ran}");
    // A stop of the image started afresh ends what it took over.
    let logs = host_logs(&d);
    assert!(
        logs.contains("it starts afresh in its own process, keeping its pid"),
        "{logs}"
    );
}

/// daemon-service, "A new package" (D5 of `sluice-host-owns-its-workers`):
/// `sluice hand-over` from another sluice has the host process that runs a
/// combination's checks start afresh as that sluice, with the same pid, and
/// its checks go on; run again, it asks nothing and says nothing; with no
/// host process, it starts none.
#[test]
fn a_hand_over_starts_the_host_process_afresh_as_this_sluice() {
    let marks = support::Scratch::new("host-hand-over-cmd-marks");
    let d = OnDisk::new("host-hand-over-cmd", &held_policy(&marks));
    let bin = support::Scratch::new("host-hand-over-cmd-bin");
    let copy = bin.path().join("sluice");
    std::fs::copy(env!("CARGO_BIN_EXE_sluice"), &copy).unwrap();
    let copy = std::fs::canonicalize(&copy).unwrap();
    let hand_over = || {
        let mut c = std::process::Command::new(&copy);
        c.current_dir(d.repo.dir())
            .env_clear()
            .envs(support::env_of(&d.command(d.repo.dir())))
            .arg("hand-over")
            .output()
            .unwrap()
    };

    let none = hand_over();
    assert!(none.status.success(), "{}", text(&none));
    assert_eq!(text(&none), "");
    assert_eq!(answering(&d), None, "it started a host process");

    let dir = d.session("a");
    let id = queued_live(&d, &dir);
    within(60, "the combination's check runs", || {
        check_of(&marks).is_some()
    });
    let first = named(&d).expect("the host process answers");
    let check = check_of(&marks).unwrap();
    let out = hand_over();
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains(&format!("started afresh as {}", copy.display())),
        "{}",
        text(&out)
    );
    let now = named(&d).expect("it answers");
    assert_eq!(now.pid, first.pid);
    assert_ne!(now.since, first.since);
    assert_eq!(now.program.as_deref(), Some(copy.as_path()));
    assert!(alive(check), "the check goes on");

    let again = hand_over();
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(text(&again), "", "it asked again");
    assert_eq!(named(&d).unwrap().since, now.since);
    std::fs::write(marks.path().join("go"), "").unwrap();
    lands(&d, id);
}

/// resident-workers, "A stop after a hand-over": a host process that took
/// a run in a hand-over, sent `SIGTERM`, has ended the run's group when it
/// exits.
#[test]
fn a_stop_after_a_hand_over_ends_the_run() {
    let marks = support::Scratch::new("host-stop-after-marks");
    let d = OnDisk::new("host-stop-after", &held_policy(&marks));
    let dir = d.session("a");
    let _id = queued_live(&d, &dir);
    within(60, "the combination's check runs", || {
        check_of(&marks).is_some()
    });
    let first = named(&d).expect("the host process answers");
    let reached = d
        .client(d.repo.dir())
        .with_version("99.0.0")
        .reach()
        .expect("the newer command's own");
    assert_eq!(reached.named.pid, first.pid);
    drop(reached);
    let check = check_of(&marks).expect("the check goes on");
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(first.pid as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    within(60, "the host process exits", || !alive(first.pid));
    assert!(!alive(check), "{}", host_logs(&d));
    assert_eq!(check_of(&marks), None);
}

/// test-children, "A test that leaves a run"
/// (`a-leaked-child-fails-its-test@9a82a5147ba6`): a test that ends while a
/// combination's checks run fails; the guard stops its `sluice host`, and
/// its words name the pid and the command of the checks.
#[test]
fn a_test_that_leaves_a_run_fails() {
    let marks = support::Scratch::new("guard-run-marks");
    let d = OnDisk::new("guard-run", &held_policy(&marks));
    let dir = d.session("a");
    let _id = queued_live(&d, &dir);
    within(60, "the combination's check runs", || {
        check_of(&marks).is_some()
    });
    let check = check_of(&marks).unwrap();
    let host = answering(&d).unwrap();
    let guard = d.guard();
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| guard.judge()))
        .expect_err("the test fails");
    let words = failed.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(words.contains(&format!("pid {check}")), "{words}");
    assert!(words.contains("while"), "{words}");
    assert!(!alive(host), "the guard stopped the host process");
    assert!(!alive(check));
    assert!(d.guard().find().is_empty());
}

/// test-children, "A clean test": a test that lands its submission, its
/// checks and git commands ended, passes; the guard stops its `sluice host`
/// and finds no other process.
#[test]
fn a_clean_test_passes() {
    let d = OnDisk::new("guard-clean", POLICY);
    let dir = d.session("a");
    let id = queued_live(&d, &dir);
    lands(&d, id);
    let host = answering(&d);
    assert_eq!(d.guard().check(), Vec::new());
    if let Some(host) = host {
        assert!(!alive(host), "the guard stopped the host process");
    }
}

/// test-children, "A detached host process": the guard finds a `sluice
/// host` that a command started in a session of its own by the test's
/// state folder, and stops it, which is no leak.
#[test]
fn the_guard_finds_a_detached_host_process() {
    let d = OnDisk::new("guard-detached", POLICY);
    let reached = d.client(d.repo.dir()).reach().expect("started");
    let pid = reached.named.pid;
    drop(reached);
    let found = d.guard().find();
    assert!(found.iter().any(|f| f.pid == pid && f.host), "{found:?}");
    assert_eq!(d.guard().check(), Vec::new());
    assert!(!alive(pid), "the guard stopped it");
}

/// test-children, "Another test's process": the guard of one test neither
/// names nor ends the `sluice host` of another test that runs at the same
/// time.
#[test]
fn the_guard_passes_over_another_tests_process() {
    let first = OnDisk::new("guard-first", POLICY);
    let second = OnDisk::new("guard-second", POLICY);
    let reached = second.client(second.repo.dir()).reach().expect("started");
    let pid = reached.named.pid;
    assert!(first.guard().find().iter().all(|f| f.pid != pid));
    assert_eq!(first.guard().check(), Vec::new());
    assert!(alive(pid), "the other test's host process runs on");
    drop(reached);
}

/// test-children, "A check that ignores SIGTERM": the guard ends a process
/// of the test that ignores `SIGTERM`; none of it runs three seconds after
/// the guard is done.
#[test]
fn the_guard_ends_a_process_that_ignores_sigterm() {
    let scratch = support::Scratch::new("guard-stubborn");
    let mut child = std::process::Command::new("/bin/sh")
        .args(["-c", "trap '' TERM; while true; do sleep 1; done"])
        .current_dir(scratch.path())
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let guard = support::guard::Guard::new(&[scratch.path()]);
    let began = Instant::now();
    let left = guard.check();
    assert!(left.iter().any(|f| f.pid == child.id()), "{left:?}");
    let _ = child.wait();
    assert!(began.elapsed() < Duration::from_secs(10));
    std::thread::sleep(Duration::from_secs(3));
    assert!(guard.find().is_empty(), "{:?}", guard.find());
}

/// The edge of the host process on tokio (the change
/// `sluice-host-runs-on-a-tokio-edge`): its schedulers on a clock a test
/// pauses, with a fake worker and a fake look.
mod edge {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use sluice::host_process::pass::Report;
    use sluice::host_process::tasks::{Bind, InPlay, Looked, Tasks, Worker};

    /// Each look and pass the fakes saw, by repository.
    #[derive(Clone, Default)]
    pub struct Seen {
        pub looks: Arc<Mutex<Vec<PathBuf>>>,
        pub passes: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl Seen {
        pub fn looks(&self, r: &Path) -> usize {
            self.looks
                .lock()
                .unwrap()
                .iter()
                .filter(|p| *p == r)
                .count()
        }

        pub fn passes(&self, r: &Path) -> usize {
            self.passes
                .lock()
                .unwrap()
                .iter()
                .filter(|p| *p == r)
                .count()
        }
    }

    struct Fake {
        repository: PathBuf,
        seen: Seen,
        takes: Duration,
    }

    impl Worker for Fake {
        fn in_play(&mut self) -> bool {
            false
        }

        fn pass(
            &mut self,
            _: bool,
            _: Option<chrono::DateTime<chrono::Utc>>,
            _: Duration,
        ) -> Result<Report, String> {
            std::thread::sleep(self.takes);
            self.seen
                .passes
                .lock()
                .unwrap()
                .push(self.repository.clone());
            Ok(Report::default())
        }

        fn step(&mut self, _: &str) -> (Result<serde_json::Value, String>, bool) {
            (Ok(serde_json::Value::Null), false)
        }

        fn gone(&mut self) -> bool {
            false
        }
    }

    /// Tasks whose looks find nothing in play, and whose workers' passes
    /// take `takes`.
    pub fn tasks(seen: &Seen, takes: Duration) -> Tasks {
        let bound = seen.clone();
        let bind: Bind = Arc::new(move |r: &Path| {
            Ok(Box::new(Fake {
                repository: r.to_path_buf(),
                seen: bound.clone(),
                takes,
            }) as Box<dyn Worker>)
        });
        let looked = seen.clone();
        let look: InPlay = Arc::new(move |r: &Path, _: bool| {
            looked.looks.lock().unwrap().push(r.to_path_buf());
            Looked::default()
        });
        Tasks::new(bind, look)
    }

    /// Let every task run that can, and every blocking call end, the
    /// paused clock not moved.
    pub async fn settle() {
        for _ in 0..20 {
            for _ in 0..50 {
                tokio::task::yield_now().await;
            }
            let _ = tokio::task::spawn_blocking(|| ()).await;
        }
    }

    /// `check` holds within ten seconds.
    pub async fn soon(what: &str, check: impl Fn() -> bool) {
        let until = tokio::time::Instant::now() + Duration::from_secs(10);
        while !check() {
            assert!(
                tokio::time::Instant::now() < until,
                "{what}: not within 10s"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub fn load(n: &std::sync::atomic::AtomicU64) -> u64 {
        n.load(Ordering::SeqCst)
    }
}

/// host-events, "An idle host process on a paused clock"
/// (`tokio-streams-before-rxrust@0b9fc11cda8b`): a scheduler with nothing
/// in play, its clock paused and moved ten minutes, woke once a minute for
/// the backstop's timer and once for the end of the reading it started,
/// and for no poll and no job.
#[tokio::test(start_paused = true)]
async fn an_idle_host_process_wakes_only_for_its_timers() {
    let seen = edge::Seen::default();
    let tasks = edge::tasks(&seen, Duration::ZERO);
    let repo = std::path::PathBuf::from("/host/idle.git");
    tasks.look(&repo);
    edge::soon("its first look", || seen.looks(&repo) == 1).await;
    edge::settle().await;
    let wakes = tasks.wakes();
    let (timers, jobs, landed) = (
        edge::load(&wakes.timers),
        edge::load(&wakes.jobs),
        edge::load(&wakes.landed),
    );
    for minute in 1..=10u64 {
        tokio::time::advance(Duration::from_secs(60)).await;
        edge::settle().await;
        assert_eq!(edge::load(&wakes.timers) - timers, minute, "its timers");
        assert_eq!(edge::load(&wakes.landed) - landed, minute, "its readings");
        assert_eq!(edge::load(&wakes.jobs) - jobs, 0, "no job came");
    }
    assert_eq!(seen.looks(&repo), 11);
    assert!(!tasks.any(), "nothing keeps the host process");
}

/// host-events, "The minute's backstop on a paused clock": no backstop
/// reading starts in the first 59 seconds after the last ask, and one
/// starts after the 60th.
#[tokio::test(start_paused = true)]
async fn the_backstop_falls_due_at_the_minute_on_a_paused_clock() {
    let seen = edge::Seen::default();
    let tasks = edge::tasks(&seen, Duration::ZERO);
    let repo = std::path::PathBuf::from("/host/backstop.git");
    tasks.look(&repo);
    edge::soon("its first look", || seen.looks(&repo) == 1).await;
    edge::settle().await;
    tokio::time::advance(Duration::from_secs(59)).await;
    edge::settle().await;
    assert_eq!(seen.looks(&repo), 1, "no reading within 59 seconds");
    tokio::time::advance(Duration::from_secs(1)).await;
    edge::settle().await;
    assert_eq!(seen.looks(&repo), 2, "the backstop's reading at the 60th");
}

/// host-events, "Three idle repositories for a minute": with no event and
/// no command, each repository the host process holds is read once in the
/// minute after its start, at the minute's backstop.
#[tokio::test(start_paused = true)]
async fn three_idle_repositories_are_read_once_a_minute() {
    let seen = edge::Seen::default();
    let tasks = edge::tasks(&seen, Duration::ZERO);
    let repos: Vec<std::path::PathBuf> = ["a", "b", "c"]
        .iter()
        .map(|n| std::path::PathBuf::from(format!("/host/{n}.git")))
        .collect();
    for r in &repos {
        tasks.look(r);
    }
    edge::soon("the looks at the start", || {
        repos.iter().all(|r| seen.looks(r) == 1)
    })
    .await;
    edge::settle().await;
    for second in 0..60 {
        tokio::time::advance(Duration::from_secs(1)).await;
        edge::settle().await;
        let want = if second < 59 { 1 } else { 2 };
        for r in &repos {
            assert_eq!(seen.looks(r), want, "{} at {}s", r.display(), second + 1);
        }
    }
    assert_eq!(seen.passes.lock().unwrap().len(), 0, "no pass");
}

/// host-events, "Five serves during a reading" (`one-read-in-flight@e99acda774d0`):
/// five commands' `serve` while a pass is in flight make one more pass
/// after it, and no third.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn five_serves_during_a_reading_make_one_more() {
    let seen = edge::Seen::default();
    let tasks = edge::tasks(&seen, Duration::from_millis(300));
    let repo = std::path::PathBuf::from("/host/five.git");
    let _run = tasks
        .handle(&repo)
        .follow(sluice::host_process::protocol::FollowKind::Run);
    let first = tasks.handle(&repo);
    first.wake();
    edge::soon("the worker holds it", || tasks.holds(&repo)).await;
    // The pass the wake asked is in flight for 300 ms.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let before = seen.passes(&repo);
    for _ in 0..5 {
        tasks.told(&repo);
        tasks.handle(&repo).wake();
    }
    edge::soon("one more pass", || seen.passes(&repo) == before + 2).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(seen.passes(&repo), before + 2, "no third pass");
}

/// host-events, "A burst of ref changes": twenty refs of a repository
/// moved within a moment are one burst of the watch, and one reading.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_burst_of_ref_moves_makes_one_reading() {
    let repo = support::Repo::new("host-burst");
    let common = std::fs::canonicalize(repo.dir().join(".git")).unwrap();
    let seen = edge::Seen::default();
    let tasks = edge::tasks(&seen, Duration::ZERO);
    let (mut watch, mut bursts) = sluice::host_process::watch::Watch::new().unwrap();
    watch.repositories(std::slice::from_ref(&common));
    let watched = tasks.clone();
    let read = tokio::spawn(async move {
        while let Some(Ok(events)) = bursts.next().await {
            let seen = watch.seen(
                events
                    .iter()
                    .flat_map(|e| e.paths.iter().map(|p| p.as_path())),
            );
            for r in &seen.repositories {
                watched.event(r);
            }
        }
    });
    // The watch is in place before the burst.
    tokio::time::sleep(Duration::from_millis(500)).await;
    // One script moves twenty refs within a moment: one git process, each
    // ref its own file, so a loaded host spreads no burst out.
    let head = repo.rev("HEAD");
    let script: String = (0..20)
        .map(|n| format!("update refs/heads/burst-{n} {head}\n"))
        .collect();
    let mut moving = std::process::Command::new("git")
        .current_dir(repo.dir())
        .env_remove("GIT_DIR")
        .args(["update-ref", "--no-deref", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(moving.stdin.as_mut().unwrap(), script.as_bytes()).unwrap();
    drop(moving.stdin.take());
    assert!(moving.wait().unwrap().success());
    edge::soon("the burst's reading", || seen.looks(&common) >= 1).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(seen.looks(&common), 1, "one reading for the burst");
    read.abort();
}

/// host-events, "A connection to an idle host process": an idle daemon
/// answers each `hello` as the connection comes, not at a poll's next
/// turn: half of thirty-one hellos are answered within 4 ms, where a poll
/// of 10 ms would answer half of them after 5 ms.
#[test]
fn a_connection_is_answered_with_no_poll() {
    let d = OnDisk::new("host-no-poll", POLICY);
    let daemon = Daemon::start(&d, d.repo.dir());
    answers(&d, &daemon);
    let client = d.client(d.repo.dir());
    let mut times: Vec<Duration> = (0..31)
        .map(|_| {
            std::thread::sleep(Duration::from_millis(37));
            let began = Instant::now();
            client.answering().unwrap().expect("it answers");
            began.elapsed()
        })
        .collect();
    times.sort();
    assert!(times[15] < Duration::from_millis(4), "{times:?}");
}

/// host-events, "SIGTERM during a round": a host process that a command
/// started, running a combination's checks, stops on `SIGTERM` as a
/// hand-over stops it: it exits 0 and its socket is gone, and the
/// submission lands under the host process the next command starts.
#[test]
fn sigterm_stops_a_host_process_after_its_round() {
    let marks = support::Scratch::new("host-sigterm-marks");
    let d = OnDisk::new("host-sigterm", &held_policy(&marks));
    let dir = d.session("a");
    // The submission is queued first, under a host process that passes
    // only when asked, which then ends: the host process the test starts
    // finds it in play as it starts, so it lives on to test it.
    let id = queued(&d, "b");
    live_from_here(&d);
    drop(dir);
    let mut host = d
        .command(d.repo.dir())
        .arg("host")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    within(30, "the host process answers", || {
        answering(&d) == Some(host.id())
    });
    within(60, "the combination's check runs", || {
        marks.path().join("began").exists()
    });
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(host.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    let until = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = host.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < until, "the host process did not stop");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(status.code(), Some(0), "{}", host_logs(&d));
    assert!(!d.client(d.repo.dir()).paths().socket.exists());
    assert!(
        host_logs(&d).contains("sluice host: signalled to stop; it ends"),
        "{}",
        host_logs(&d)
    );
    std::fs::write(marks.path().join("go"), "").unwrap();
    // A host process started afresh, at its own tick, lands it.
    let mut again = d
        .command(d.repo.dir())
        .arg("host")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    within(30, "the host process started afresh answers", || {
        answering(&d) == Some(again.id())
    });
    let until = Instant::now() + Duration::from_secs(90);
    while state(&d, d.repo.dir(), id) != "landed" {
        assert!(
            Instant::now() < until,
            "the submission lands: {}",
            host_logs(&d)
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = again.kill();
    let _ = again.wait();
}

/// host-daemon, "The daemon is asked to hand over", on tokio's edge
/// (design D6 of `sluice-host-runs-on-a-tokio-edge`): a daemon that starts
/// afresh in place answers on the socket it held, the same file, so a
/// command that connects meanwhile waits in its queue.
#[test]
fn a_daemon_started_afresh_answers_on_its_socket() {
    use std::os::unix::fs::MetadataExt;
    let d = OnDisk::new("daemon-socket", POLICY);
    let daemon = Daemon::start(&d, d.repo.dir());
    answers(&d, &daemon);
    let first = named(&d).expect("it answers");
    let socket = d.client(d.repo.dir()).paths().socket.clone();
    let inode = std::fs::metadata(&socket).unwrap().ino();
    let bin = support::Scratch::new("daemon-socket-bin");
    let copy = bin.path().join("sluice");
    std::fs::copy(env!("CARGO_BIN_EXE_sluice"), &copy).unwrap();
    let copy = std::fs::canonicalize(&copy).unwrap();
    ask_hand_over(&d, &copy);
    within(60, "the daemon started afresh answers", || {
        named(&d).is_some_and(|n| n.pid == first.pid && n.since != first.since && n.daemon)
    });
    assert_eq!(
        std::fs::metadata(&socket).unwrap().ino(),
        inode,
        "the same socket: {}",
        host_logs(&d)
    );
}

/// host-events, "Another process writes the records": with a watch
/// following a host process and nothing in play, the repository is not
/// read each second; a submission another process writes to `main`'s
/// records, with no command telling this host process, is read once the
/// file events stop, and the watch is told.
#[test]
fn a_records_write_reaches_a_watch_by_its_file_event() {
    let d = OnDisk::new("host-records-event", POLICY);
    d.land("a");
    let dir = d.session("b");
    let repository = d.repository(d.repo.dir());
    within(15, "the host process ends once idle", || {
        answering(&d).is_none()
    });
    let client = d.client(d.repo.dir());
    let mut watch = client
        .follow(Path::new(&repository), FollowKind::Watch)
        .expect("the watch follows");
    let first = watch.next(Some(Duration::from_secs(30))).unwrap();
    assert!(
        matches!(first, Some(sluice::host_process::protocol::Answer::Pass(_))),
        "{first:?}"
    );
    // Idle: no pass in three seconds.
    let quiet = watch.next(Some(Duration::from_secs(3))).unwrap();
    assert!(quiet.is_none(), "a pass of an idle repository: {quiet:?}");
    // Another host's state folder: its submit tells another host process,
    // which passes only when asked, and writes `main`'s records here.
    let other = support::Scratch::new("host-records-event-other");
    let out = d
        .command(&dir)
        .env("XDG_STATE_HOME", other.path())
        .env("SLUICE_ROUND_SLEEP", "record")
        .args(["submit", "main", "--no-wait", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let written = Instant::now();
    let told = watch.next(Some(Duration::from_secs(5))).unwrap();
    assert!(
        matches!(told, Some(sluice::host_process::protocol::Answer::Pass(_))),
        "no pass after the write: {told:?}\n{}",
        host_logs(&d)
    );
    assert!(
        written.elapsed() < sluice::host_process::watch::DEBOUNCE + Duration::from_secs(2),
        "{:?}",
        written.elapsed()
    );
    drop(watch);
    let state = other.path().join("sluice").join("host").join("version");
    if let Ok(bytes) = std::fs::read(state) {
        if let Ok(n) = serde_json::from_slice::<sluice::host_process::protocol::Named>(&bytes) {
            stop(n.pid);
        }
    }
}
