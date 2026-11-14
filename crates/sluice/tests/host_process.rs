//! sluice's host process through the binary, on repositories on disk with
//! the command runner and real git (135, 129, 71, 150; rules core.31,
//! core.25, core.49; record 0093). A test binary of its own: a child a
//! test starts holds a copy of every lock the test process holds until it
//! execs.

mod support;

use std::collections::BTreeSet;
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
