//! The guard that fails a test that leaves a process
//! (`a-leaked-child-fails-its-test@9a82a5147ba6`; design D6 to D8 of the
//! change `sluice-host-owns-its-workers`).
//!
//! A process belongs to a test where its environment names one of the
//! test's folders as `XDG_STATE_HOME`, or where its working folder is in
//! one of the test's folders. A detached process is found so too.
//!
//! When the test ends, the guard first notes each process of the test that
//! is no `sluice host` and no git or credential worker of one. Then it
//! stops each `sluice host` of the test, and waits for it to exit. It gives
//! the processes that it noted one second to end by themselves. Then it
//! lists the test's processes again. It ends each process that it found
//! still running, and the test fails where it found any.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// How long the guard waits for a test's `sluice host` to exit.
const HOST_STOP: Duration = Duration::from_secs(30);

/// How long a process of the test has to end by itself after the test
/// ends.
const SETTLE: Duration = Duration::from_secs(1);

/// How long a process that the guard asks to end has before it is killed.
const GRACE: Duration = Duration::from_secs(2);

/// A process of a test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub pid: u32,
    pub command: String,
    /// Whether it is a `sluice host` or `sluice daemon` of the test.
    pub host: bool,
    /// Its parent's pid, its process group and the seconds it has run, as
    /// the guard found it: a failure names what started the process.
    pub parent: Option<u32>,
    pub group: Option<i32>,
    pub ran: u64,
}

impl std::fmt::Display for Found {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let parent = self.parent.map_or("none".to_string(), |p| p.to_string());
        let group = self.group.map_or("none".to_string(), |g| g.to_string());
        write!(
            f,
            "pid {} (parent {parent}, group {group}, {} s): {}",
            self.pid, self.ran, self.command
        )
    }
}

/// The folders of one test.
#[derive(Clone, Debug, Default)]
pub struct Guard {
    pub folders: Vec<PathBuf>,
}

impl Guard {
    pub fn new(folders: &[&Path]) -> Guard {
        Guard {
            folders: folders.iter().map(|p| real(p)).collect(),
        }
    }

    /// The processes of the test now.
    pub fn find(&self) -> Vec<Found> {
        find(&self.folders)
    }

    /// Stop the test's host processes, end what is left, and say what was
    /// left: the processes that ran before the stop and were no host
    /// process or a worker that a host process ends as it stops, and each
    /// process that ran after it.
    pub fn check(&self) -> Vec<Found> {
        if self.folders.is_empty() {
            return Vec::new();
        }
        let before = self.find();
        if before.is_empty() {
            return Vec::new();
        }
        let mut left: Vec<Found> = before
            .iter()
            .filter(|p| !p.host && !host_worker(p.pid))
            .cloned()
            .collect();
        let hosts: Vec<&Found> = before.iter().filter(|p| p.host).collect();
        for h in &hosts {
            signal(h.pid, libc::SIGTERM);
        }
        let until = Instant::now() + HOST_STOP;
        while hosts.iter().any(|h| runs(h.pid)) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
        for p in self.find() {
            if !left.iter().any(|l| l.pid == p.pid) && !host_worker(p.pid) {
                left.push(p);
            }
        }
        // A process that ends by itself within [`SETTLE`] does not continue
        // to run: the far side of a fetch that just ended, for example, or
        // of one that a host process of another test's folder made.
        let until = Instant::now() + SETTLE;
        while left.iter().any(|p| runs(p.pid)) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        left.retain(|p| runs(p.pid));
        end(&left);
        left
    }

    /// [`Guard::check`], and a failure that names each process left,
    /// unless the test failed already.
    pub fn judge(&self) {
        let left = self.check();
        if left.is_empty() || std::thread::panicking() {
            return;
        }
        let names: Vec<String> = left.iter().map(Found::to_string).collect();
        panic!(
            "the test left {} process(es) running, now ended:\n  {}",
            left.len(),
            names.join("\n  ")
        );
    }
}

/// A folder as the system names it: a process's working folder is read
/// through every link.
fn real(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// The processes whose working folder is in one of `folders`, or whose
/// environment names one of them as `XDG_STATE_HOME`.
pub fn find(folders: &[PathBuf]) -> Vec<Found> {
    let me = std::process::id();
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cwd(UpdateKind::Always)
            .with_cmd(UpdateKind::Always),
    );
    // A process with no working folder in the test's folders is read for
    // its environment only where it may be a host process: one whose
    // command is sluice's.
    let maybe: Vec<Pid> = system
        .processes()
        .iter()
        .filter(|(pid, p)| {
            pid.as_u32() != me
                && !p.cwd().is_some_and(|c| inside(c, folders))
                && p.cmd()
                    .first()
                    .is_some_and(|c| Path::new(c).file_name().is_some_and(|n| n == "sluice"))
        })
        .map(|(pid, _)| *pid)
        .collect();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&maybe),
        false,
        ProcessRefreshKind::nothing()
            .with_cwd(UpdateKind::Always)
            .with_cmd(UpdateKind::Always)
            .with_environ(UpdateKind::Always),
    );
    let mut out = Vec::new();
    for (pid, p) in system.processes() {
        if pid.as_u32() == me || matches!(p.status(), sysinfo::ProcessStatus::Zombie) {
            continue;
        }
        let state = p.environ().iter().find_map(|e| {
            e.to_str()
                .and_then(|e| e.strip_prefix("XDG_STATE_HOME="))
                .map(|s| real(Path::new(s)))
        });
        let by_state = state.as_ref().is_some_and(|s| inside(s, folders));
        let by_cwd = p.cwd().is_some_and(|c| inside(c, folders));
        if !by_state && !by_cwd {
            continue;
        }
        let words: Vec<String> = p
            .cmd()
            .iter()
            .map(|c| c.to_string_lossy().into_owned())
            .collect();
        let host = by_state && words.get(1).is_some_and(|w| w == "host" || w == "daemon");
        out.push(Found {
            pid: pid.as_u32(),
            command: words.join(" "),
            host,
            parent: p.parent().map(|q| q.as_u32()),
            group: nix::unistd::getpgid(Some(nix::unistd::Pid::from_raw(pid.as_u32() as i32)))
                .ok()
                .map(|g| g.as_raw()),
            ran: p.run_time(),
        });
    }
    out.sort_by_key(|f| f.pid);
    out
}

fn inside(dir: &Path, folders: &[PathBuf]) -> bool {
    folders.iter().any(|f| dir.starts_with(f))
}

/// Stop each host process that the state folder `state` names, with
/// `SIGTERM`, and wait for it to exit: it ends its runs as it stops. A
/// test harness whose tests end while a run goes calls it as it ends.
pub fn stop_host_processes(state: &Path) {
    let version = state.join("sluice").join("host").join("version");
    let Some(pid) = std::fs::read(version)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["pid"].as_u64())
    else {
        return;
    };
    let pid = pid as u32;
    if !runs(pid) {
        return;
    }
    signal(pid, libc::SIGTERM);
    let until = Instant::now() + Duration::from_secs(90);
    while runs(pid) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Whether `pid` is in the group of a git or credential worker of a host
/// process: a group whose leader runs git, or `sluice credential`, as a
/// child of `sluice host` or `sluice daemon`. Such a worker is the host's,
/// which its stop waits for; git's own children, the other side of a local
/// fetch among them, are in its group.
fn host_worker(pid: u32) -> bool {
    let Ok(group) = nix::unistd::getpgid(Some(nix::unistd::Pid::from_raw(pid as i32))) else {
        return false;
    };
    let leader = group.as_raw();
    let Some(parent) = sluice::workers::parent(leader) else {
        return false;
    };
    let words = |pid: i32| -> Vec<String> {
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[Pid::from_u32(pid as u32)]),
            false,
            ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
        );
        system
            .process(Pid::from_u32(pid as u32))
            .map(|p| {
                p.cmd()
                    .iter()
                    .map(|c| c.to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    let named = |w: &[String], n: &str| {
        w.first()
            .is_some_and(|c| Path::new(c).file_name().is_some_and(|f| f == n))
    };
    let leads = words(leader);
    let git = named(&leads, "git") || leads.get(1).is_some_and(|w| w == "credential");
    let host = words(parent);
    git && named(&host, "sluice") && host.get(1).is_some_and(|w| w == "host" || w == "daemon")
}

fn runs(pid: u32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_ok()
        && sluice::workers::started(pid as i32).is_some()
}

fn signal(pid: u32, signal: libc::c_int) {
    // SAFETY: kill reads its two integer arguments.
    unsafe {
        libc::kill(pid as i32, signal);
    }
}

/// End each of `found`: asked first, killed after [`GRACE`].
fn end(found: &[Found]) {
    for p in found {
        signal(p.pid, libc::SIGTERM);
    }
    let until = Instant::now() + GRACE;
    while found.iter().any(|p| runs(p.pid)) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    for p in found.iter().filter(|p| runs(p.pid)) {
        signal(p.pid, libc::SIGKILL);
    }
    let until = Instant::now() + Duration::from_secs(5);
    while found.iter().any(|p| runs(p.pid)) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
}
