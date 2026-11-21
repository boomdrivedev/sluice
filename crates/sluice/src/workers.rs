//! The workers of `sluice host` (the change `sluice-host-owns-its-workers`;
//! `workers-end-with-their-owner@292e869158d6`,
//! `sluices-runs-end-with-its-host-process@e814b716f3b1`,
//! `nothing-detached@f807c7f5fcea`).
//!
//! A worker is a process that `sluice host` starts to do its work: the
//! supervisor of a run, a git command, a credential command, an equip step
//! or a release command. Each worker starts in a process group of its own,
//! as a child of `sluice host`. A run's checks are a group that the run's
//! supervisor starts. They are a worker of the same owner.
//!
//! The worker records itself in `host/workers/` before it does its work:
//! the child writes the record between its fork and its exec, so no work
//! starts unrecorded. The record's name is the worker's pid, which is also
//! its process group. The record holds the owner's pid and the owner's
//! start time.
//!
//! ```text
//! $XDG_STATE_HOME/sluice/host/workers/
//!   <pid>    {"owner":<pid>,"owner_start":<seconds>,"what":"<kind>"}
//! ```
//!
//! A record is of a live worker only where its pid runs and started within
//! two seconds of the record's write. So a pid that the system gave to
//! another process is never ended.
//!
//! When `sluice host` stops, it ends each worker's group ([`stop`]). When it
//! starts, it ends each group that a killed host process left, and it keeps
//! each worker that an earlier image of its own process started ([`sweep`]).
//! A process that is not `sluice host` has no owner, and [`take`] does
//! nothing in it.

use std::collections::BTreeSet;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The folder of the records, in `host/`.
pub const DIR: &str = "workers";

/// The variable that tells a run's supervisor where to record its checks,
/// and for which owner: `<owner pid> <owner start> <folder>`.
pub const VAR: &str = "SLUICE_WORKERS";

/// How long a supervisor has to end its checks after a stop asks it to.
/// A run's own grace is ten seconds unless its request says less.
pub const STOP_GRACE: Duration = Duration::from_secs(15);

/// How far apart a worker's start and its record's write can be, in
/// seconds.
const SAME_START: u64 = 2;

/// The owner of the workers that this process starts: `sluice host`, or
/// the run's supervisor on behalf of it.
#[derive(Clone, Debug)]
pub struct Owner {
    /// The folder of the records.
    pub dir: PathBuf,
    /// The pid of `sluice host`.
    pub pid: u32,
    /// The start time of `sluice host`, in seconds since the epoch.
    pub start: u64,
}

/// What a record holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    pub owner: u32,
    pub owner_start: u64,
    pub what: String,
}

/// A record, as a reader finds it.
#[derive(Clone, Debug)]
pub struct Record {
    /// The worker's pid, and its process group.
    pub pid: i32,
    /// What the record holds. `None` where it cannot be read.
    pub held: Option<Held>,
    /// When the record was written, in seconds since the epoch.
    pub written: u64,
}

impl Record {
    /// Whether its pid runs, and is the process that wrote the record.
    pub fn live(&self) -> bool {
        started(self.pid).is_some_and(|s| s.abs_diff(self.written) <= SAME_START)
    }

    /// Whether `owner` owns it.
    pub fn of(&self, owner: &Owner) -> bool {
        self.held
            .as_ref()
            .is_some_and(|h| h.owner == owner.pid && h.owner_start == owner.start)
    }
}

static OWNER: OnceLock<Owner> = OnceLock::new();

/// The direct children of this process that it waits on.
static CHILDREN: Mutex<BTreeSet<i32>> = Mutex::new(BTreeSet::new());

type Hook = Box<dyn Fn() + Send + Sync>;

/// What this process calls when a worker that it waits on ends.
static ENDED: OnceLock<Hook> = OnceLock::new();

/// Whether this process stops: from then, no worker of it starts.
static STOPPING: AtomicBool = AtomicBool::new(false);

/// How long [`stop`] waits after it ends the workers, for a worker that
/// started just before the stop to write its record.
const SETTLE: Duration = Duration::from_millis(100);

/// Make `sluice host` the owner of each worker that this process starts
/// from now. `ended` is called when a child worker ends. Its records are
/// in `<host>/workers/`.
pub fn own(host: &Path, ended: impl Fn() + Send + Sync + 'static) -> std::io::Result<Owner> {
    let dir = host.join(DIR);
    std::fs::create_dir_all(&dir)?;
    let pid = std::process::id();
    let start = started(pid as i32)
        .ok_or_else(|| std::io::Error::other("the start time of this process could not be read"))?;
    let owner = Owner { dir, pid, start };
    let _ = OWNER.set(owner.clone());
    let _ = ENDED.set(Box::new(ended));
    Ok(owner)
}

/// Make the owner that [`VAR`] names the owner of the workers that this
/// process starts, and take the variable out of the environment. A run's
/// supervisor calls it first.
pub fn own_from_env() -> Option<Owner> {
    let text = std::env::var_os(VAR)?;
    std::env::remove_var(VAR);
    let text = text.to_str()?;
    let mut parts = text.splitn(3, ' ');
    let pid = parts.next()?.parse().ok()?;
    let start = parts.next()?.parse().ok()?;
    let dir = PathBuf::from(parts.next()?);
    let owner = Owner { dir, pid, start };
    let _ = OWNER.set(owner.clone());
    Some(owner)
}

/// The owner of this process's workers, where it has one.
pub fn owner() -> Option<&'static Owner> {
    OWNER.get()
}

/// The value of [`VAR`] for a supervisor of `owner`.
pub fn var(owner: &Owner) -> String {
    format!("{} {} {}", owner.pid, owner.start, owner.dir.display())
}

/// Make `command` a worker of this process's owner: it starts in a process
/// group of its own, and it records itself before it does its work. A
/// command whose record cannot be written does not start, and a command
/// does not start once [`stop`] has started. Where this process has no
/// owner, `command` is not changed.
pub fn take(command: &mut Command, what: &str) {
    let Some(owner) = owner() else {
        return;
    };
    let held = Held {
        owner: owner.pid,
        owner_start: owner.start,
        what: what.to_string(),
    };
    let body = serde_json::to_vec(&held).expect("a record is JSON");
    let mut dir = owner.dir.as_os_str().as_bytes().to_vec();
    dir.push(b'/');
    command.process_group(0);
    // SAFETY: the closure runs in the child between fork and exec. It
    // reads an atomic and calls only getpid, open, write and close, which
    // are async-signal-safe, and it allocates nothing: the path is put
    // together in a buffer on the stack.
    unsafe {
        command.pre_exec(move || start_unless(&STOPPING, &dir, &body));
    }
}

/// In the child, between fork and exec: refuse to start where `stopping`
/// is set, else write the record. The child reads `stopping` as its
/// parent held it at the fork.
fn start_unless(stopping: &AtomicBool, dir: &[u8], body: &[u8]) -> std::io::Result<()> {
    if stopping.load(Ordering::SeqCst) {
        return Err(std::io::Error::from_raw_os_error(libc::ECANCELED));
    }
    record_self(dir, body)
}

/// A command for `program` that is a worker of this process's owner of the
/// kind `what`, as [`take`] makes it.
pub fn command(program: impl AsRef<std::ffi::OsStr>, what: &str) -> Command {
    let mut command = Command::new(program);
    take(&mut command, what);
    command
}

/// Write this process's record: `<dir><pid>`, holding `body`.
fn record_self(dir: &[u8], body: &[u8]) -> std::io::Result<()> {
    let mut path = [0u8; 4096];
    let mut digits = [0u8; 12];
    // SAFETY: getpid reads nothing and cannot fail.
    let mut pid = unsafe { libc::getpid() } as u32;
    let mut n = 0;
    loop {
        digits[n] = b'0' + (pid % 10) as u8;
        n += 1;
        pid /= 10;
        if pid == 0 {
            break;
        }
    }
    if dir.len() + n + 1 > path.len() {
        return Err(std::io::Error::from_raw_os_error(libc::ENAMETOOLONG));
    }
    path[..dir.len()].copy_from_slice(dir);
    for i in 0..n {
        path[dir.len() + i] = digits[n - 1 - i];
    }
    // SAFETY: `path` ends with a NUL, as the buffer is zeroed past what
    // was copied; open, write and close read only the memory given.
    unsafe {
        let fd = libc::open(
            path.as_ptr().cast(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC,
            0o600 as libc::c_uint,
        );
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let wrote = libc::write(fd, body.as_ptr().cast(), body.len());
        libc::close(fd);
        if wrote != body.len() as isize {
            return Err(std::io::Error::other("a worker's record was not written"));
        }
    }
    Ok(())
}

/// Wait on `child`, a worker of this process, on a thread of its own: when
/// it ends, its record goes, and the hook of [`own`] is called.
pub fn wait(mut child: std::process::Child) {
    let pid = child.id() as i32;
    CHILDREN.lock().expect("the children").insert(pid);
    std::thread::spawn(move || {
        let _ = child.wait();
        ended(pid);
    });
}

/// Wait on `pid`, a child that an earlier image of this process started,
/// on a thread of its own, as [`wait`] does.
fn adopt(pid: i32) {
    CHILDREN.lock().expect("the children").insert(pid);
    std::thread::spawn(move || {
        let pid = nix::unistd::Pid::from_raw(pid);
        while let Err(nix::errno::Errno::EINTR) = nix::sys::wait::waitpid(pid, None) {}
        ended(pid.as_raw());
    });
}

/// The child `pid` ended.
fn ended(pid: i32) {
    if let Some(owner) = owner() {
        forget_in(&owner.dir, pid);
    }
    CHILDREN.lock().expect("the children").remove(&pid);
    if let Some(hook) = ENDED.get() {
        hook();
    }
}

/// How many child workers run now.
pub fn children() -> usize {
    CHILDREN.lock().expect("the children").len()
}

/// Remove the record of the worker `pid` of this process's owner.
pub fn forget(pid: i32) {
    if let Some(owner) = owner() {
        forget_in(&owner.dir, pid);
    }
}

fn forget_in(dir: &Path, pid: i32) {
    let _ = std::fs::remove_file(dir.join(pid.to_string()));
}

/// The records in `dir`.
pub fn records(dir: &Path) -> Vec<Record> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<i32>().ok()) else {
            continue;
        };
        let path = e.path();
        let written = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        let held = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        out.push(Record { pid, held, written });
    }
    out.sort_by_key(|r| r.pid);
    out
}

/// Remove each record in `dir` whose worker no longer runs.
pub fn prune(dir: &Path) {
    for r in records(dir) {
        if !r.live() {
            forget_in(dir, r.pid);
        }
    }
}

/// What a sweep at start found.
#[derive(Debug, Default)]
pub struct Swept {
    /// The groups that a killed host process left, now ended.
    pub ended: Vec<Record>,
    /// The child workers that an earlier image of this process started,
    /// now waited on.
    pub kept: Vec<i32>,
}

/// As `sluice host` starts, before it claims a run or starts a worker:
/// each record whose worker no longer runs goes, each group of another
/// owner is ended, and each child that an earlier image of this process
/// started is waited on again.
pub fn sweep(owner: &Owner) -> Swept {
    let mut swept = Swept::default();
    let mut ending = Vec::new();
    for r in records(&owner.dir) {
        if !r.live() {
            forget_in(&owner.dir, r.pid);
        } else if r.of(owner) {
            if is_child(r.pid) {
                adopt(r.pid);
                swept.kept.push(r.pid);
            }
        } else {
            ending.push(r);
        }
    }
    end_groups(
        &ending.iter().map(|r| r.pid).collect::<Vec<_>>(),
        weir_bounded::GRACE,
    );
    for r in &ending {
        forget_in(&owner.dir, r.pid);
    }
    swept.ended = ending;
    swept
}

/// As `sluice host` stops for a reason other than a hand-over: each child
/// worker's group is asked to end, and given [`STOP_GRACE`]. A run's
/// supervisor ends its checks and writes no result. Then each group that a
/// record of `owner` still names is ended, its grace given first. From
/// the start of the stop, no worker starts, and after [`SETTLE`] the stop
/// ends each group that a record written meanwhile names. Each record
/// goes. The groups it ended.
pub fn stop(owner: &Owner) -> Vec<Record> {
    // A thread of this process that is still at work starts no worker
    // after this point. A worker that it forked before this point writes
    // its record at once, and the second look below finds it.
    STOPPING.store(true, Ordering::SeqCst);
    let mut mine: Vec<Record> = records(&owner.dir)
        .into_iter()
        .filter(|r| r.live() && r.of(owner))
        .collect();
    let waited: Vec<i32> = CHILDREN
        .lock()
        .expect("the children")
        .iter()
        .copied()
        .collect();
    for pid in &waited {
        signal_group(*pid, libc::SIGTERM);
    }
    let until = Instant::now() + STOP_GRACE;
    while children() > 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    let left: Vec<Record> = records(&owner.dir)
        .into_iter()
        .filter(|r| r.live() && r.of(owner))
        .collect();
    end_groups(
        &left.iter().map(|r| r.pid).collect::<Vec<_>>(),
        weir_bounded::GRACE,
    );
    std::thread::sleep(SETTLE);
    let late: Vec<Record> = records(&owner.dir)
        .into_iter()
        .filter(|r| r.live() && r.of(owner))
        .collect();
    end_groups(
        &late.iter().map(|r| r.pid).collect::<Vec<_>>(),
        weir_bounded::GRACE,
    );
    for r in late {
        if !mine.iter().any(|m| m.pid == r.pid) {
            mine.push(r);
        }
    }
    // A child its waiter did not reap yet is reaped here.
    let until = Instant::now() + Duration::from_secs(1);
    while children() > 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    for r in records(&owner.dir) {
        if r.of(owner) || !r.live() {
            forget_in(&owner.dir, r.pid);
        }
    }
    mine
}

/// End each of `groups`: each is asked to end, given `grace`, then killed.
fn end_groups(groups: &[i32], grace: Duration) {
    let there = |g: &i32| is_there(*g);
    let live: Vec<i32> = groups.iter().copied().filter(there).collect();
    if live.is_empty() {
        return;
    }
    for g in &live {
        signal_group(*g, libc::SIGTERM);
    }
    let until = Instant::now() + grace;
    while live.iter().any(there) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    for g in live.iter().filter(|g| there(g)) {
        signal_group(*g, libc::SIGKILL);
    }
    let until = Instant::now() + Duration::from_secs(5);
    while live.iter().any(there) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Whether any process of `group` is left.
fn is_there(group: i32) -> bool {
    // SAFETY: killpg reads its two integer arguments; signal 0 sends
    // nothing.
    let asked = unsafe { libc::killpg(group, 0) };
    asked == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

fn signal_group(group: i32, signal: libc::c_int) {
    // SAFETY: killpg reads its two integer arguments.
    unsafe {
        libc::killpg(group, signal);
    }
}

/// Whether `pid` is a child of this process.
fn is_child(pid: i32) -> bool {
    parent(pid) == Some(std::process::id() as i32)
}

/// The start time of the process `pid`, in seconds since the epoch, where
/// it runs.
pub fn started(pid: i32) -> Option<u64> {
    platform::started(pid)
}

/// The parent of the process `pid`, where it runs.
pub fn parent(pid: i32) -> Option<i32> {
    platform::parent(pid)
}

#[cfg(target_os = "macos")]
mod platform {
    fn info(pid: i32) -> Option<libc::proc_bsdinfo> {
        // SAFETY: proc_bsdinfo is plain data; zeroed is a valid value.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        // SAFETY: the buffer is `size` bytes of this process's memory.
        let n = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        (n == size).then_some(info)
    }

    pub fn started(pid: i32) -> Option<u64> {
        // A zombie keeps its info, but it runs no more.
        info(pid)
            .filter(|i| i.pbi_status != libc::SZOMB)
            .map(|i| i.pbi_start_tvsec)
    }

    pub fn parent(pid: i32) -> Option<i32> {
        info(pid).map(|i| i.pbi_ppid as i32)
    }
}

#[cfg(target_os = "linux")]
mod platform {
    /// The fields of `/proc/<pid>/stat` after the command's name.
    fn stat(pid: i32) -> Option<Vec<String>> {
        let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let rest = &text[text.rfind(')')? + 2..];
        Some(rest.split_whitespace().map(str::to_string).collect())
    }

    fn boot() -> Option<u64> {
        let text = std::fs::read_to_string("/proc/stat").ok()?;
        text.lines()
            .find_map(|l| l.strip_prefix("btime "))
            .and_then(|s| s.trim().parse().ok())
    }

    pub fn started(pid: i32) -> Option<u64> {
        let fields = stat(pid)?;
        // A zombie runs no more.
        if fields.first().map(String::as_str) == Some("Z") {
            return None;
        }
        let ticks: u64 = fields.get(19)?.parse().ok()?;
        // SAFETY: sysconf reads its integer argument.
        let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        let hz = u64::try_from(hz).ok().filter(|h| *h > 0)?;
        Some(boot()? + ticks / hz)
    }

    pub fn parent(pid: i32) -> Option<i32> {
        stat(pid)?.get(1)?.parse().ok()
    }
}

/// Now, in seconds since the epoch.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(what: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("sluice-workers-{what}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// This process's own start time is read, and is no later than now.
    #[test]
    fn a_process_start_time_is_read() {
        let s = started(std::process::id() as i32).expect("its own start");
        assert!(s <= now() && s + 3600 > now(), "{s}");
        assert_eq!(
            parent(std::process::id() as i32),
            Some(std::os::unix::process::parent_id() as i32)
        );
    }

    /// A record whose pid runs with another start time is not live, and a
    /// prune removes it.
    #[test]
    fn a_record_of_a_reused_pid_is_not_live() {
        let dir = scratch("reused");
        let me = std::process::id();
        std::fs::write(
            dir.join(me.to_string()),
            br#"{"owner":1,"owner_start":1,"what":"git"}"#,
        )
        .unwrap();
        let old = std::fs::File::options()
            .write(true)
            .open(dir.join(me.to_string()))
            .unwrap();
        old.set_modified(UNIX_EPOCH + Duration::from_secs(1_000_000))
            .unwrap();
        let r = records(&dir);
        assert_eq!(r.len(), 1);
        assert!(!r[0].live());
        prune(&dir);
        assert!(records(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The record a child writes before its exec names its pid and its
    /// owner, and the child is in a process group of its own.
    #[test]
    fn a_child_records_itself_before_it_runs() {
        let dir = scratch("child");
        let held = serde_json::to_vec(&Held {
            owner: 7,
            owner_start: 8,
            what: "test".into(),
        })
        .unwrap();
        let mut prefix = dir.as_os_str().as_bytes().to_vec();
        prefix.push(b'/');
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 5"]).process_group(0);
        // SAFETY: as in `take`.
        unsafe {
            command.pre_exec(move || record_self(&prefix, &held));
        }
        let mut child = command.spawn().unwrap();
        let pid = child.id() as i32;
        let r = records(&dir);
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].pid, pid);
        assert!(r[0].live());
        assert_eq!(r[0].held.as_ref().unwrap().owner, 7);
        let group = nix::unistd::getpgid(Some(nix::unistd::Pid::from_raw(pid))).unwrap();
        assert_eq!(group.as_raw(), pid);
        end_groups(&[pid], Duration::from_secs(1));
        let _ = child.wait();
        assert!(!is_there(pid));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Once its owner stops, a worker does not start, and writes no
    /// record.
    #[test]
    fn a_worker_does_not_start_once_its_owner_stops() {
        static STOPS: AtomicBool = AtomicBool::new(false);
        let dir = scratch("stopping");
        let mut prefix = dir.as_os_str().as_bytes().to_vec();
        prefix.push(b'/');
        let spawn = |prefix: Vec<u8>| {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", "exit 0"]).process_group(0);
            // SAFETY: as in `take`.
            unsafe {
                command.pre_exec(move || start_unless(&STOPS, &prefix, b"{}"));
            }
            command.spawn()
        };
        let mut started = spawn(prefix.clone()).unwrap();
        started.wait().unwrap();
        assert_eq!(records(&dir).len(), 1);
        std::fs::remove_file(dir.join(started.id().to_string())).unwrap();
        STOPS.store(true, Ordering::SeqCst);
        let refused = spawn(prefix).unwrap_err();
        assert_eq!(refused.raw_os_error(), Some(libc::ECANCELED), "{refused}");
        assert!(records(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
