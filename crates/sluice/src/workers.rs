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
//! In `sluice host`, each worker starts on its tokio edge through
//! process-wrap, with `ProcessGroup` and `KillOnDrop`, and a task of the
//! edge owns it and waits on it ([`edge`], [`output`], [`start`];
//! `each-worker-starts-through-process-wrap@025fae162990`). Its core waits
//! on that task from the blocking pool, and builds no runtime and starts
//! no thread. A drop of a worker that runs kills its whole group, which
//! tokio's kill on drop does not, through process-wrap's group kill; and
//! once its leader has ended, what is left of its group, which no wait of
//! this process reaches, is looked at within its grace, then killed. A
//! process with no runtime, a command, a run's supervisor or the git hook,
//! starts its processes as std's `Command` does, and weir-bounded bounds
//! them.
//!
//! When `sluice host` stops, it ends each worker's group ([`stop`]). When it
//! starts, before its first worker, it reaps each child of its earlier
//! image that has ended, by nix's `waitpid` on any child, waits again on
//! each recorded child of that image that runs, and ends each group that a
//! killed host process left ([`sweep`]). A process that is not `sluice
//! host` has no owner, and [`take`] does nothing in it.

use std::collections::BTreeSet;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop, ProcessGroup};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt as _;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

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
    // The change `the-git-hook-asks-sluice-host`, design D3: a git command
    // of `sluice host` carries its name, so the hook lets its transaction
    // go with no ask; no other worker does, a check, a session's command,
    // a preview's command, a release command or an equip step among them,
    // so a write of theirs to a protected branch is judged.
    match what {
        "git" => command.env(
            crate::protect::HOST_VAR,
            crate::protect::host_name(owner.pid, owner.start),
        ),
        _ => command.env_remove(crate::protect::HOST_VAR),
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

/// The edge of `sluice host`, on which each of its workers starts and is
/// waited on (`each-worker-starts-through-process-wrap@025fae162990`,
/// `a-residents-core-stays-synchronous-behind-a-tokio-edge@914dd32ed8dd`).
struct Edge {
    /// The runtime of the host process, which its `main` built.
    handle: tokio::runtime::Handle,
    /// The owner task of each worker.
    tracker: TaskTracker,
    /// Cancelled as the host process stops: each owner task then ends its
    /// worker's group.
    stop: CancellationToken,
}

static EDGE: OnceLock<Edge> = OnceLock::new();

/// Start each worker of this process on the runtime of `handle`, through
/// process-wrap, owned by a task that the edge tracks. `sluice host` calls
/// it once, inside its runtime, before [`sweep`]. A process that never
/// calls it, a command, a run's supervisor or the git hook, has no
/// runtime, and starts its processes as std's `Command` does.
pub fn edge(handle: tokio::runtime::Handle) {
    let _ = EDGE.set(Edge {
        handle,
        tracker: TaskTracker::new(),
        stop: CancellationToken::new(),
    });
}

/// How often a group whose leader has ended is looked at within its grace.
const LOOK: Duration = Duration::from_millis(50);

/// A worker as its owner task holds it: the leader of a process group of
/// its own, started through process-wrap's `ProcessGroup` with its
/// `KillOnDrop`. tokio's kill on drop reaches the leader alone, so a drop
/// before the leader was waited on kills the whole group first, through
/// process-wrap's group kill: git, nix and a check start children of
/// their own.
struct Owned {
    child: Box<dyn ChildWrapper>,
    group: i32,
    waited: bool,
}

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.waited {
            // The leader is not reaped, so the group is still its own.
            let _ = self.child.start_kill();
        }
    }
}

impl Owned {
    /// Start `command`, which [`command`] or [`take`] made, in a process
    /// group of its own.
    fn spawn(command: Command) -> std::io::Result<Owned> {
        let mut wrapped = CommandWrap::from(tokio::process::Command::from(command));
        wrapped.wrap(ProcessGroup::leader()).wrap(KillOnDrop);
        let child = wrapped.spawn()?;
        let group = child
            .id()
            .ok_or_else(|| std::io::Error::other("a worker ended before its pid was read"))?
            as i32;
        Ok(Owned {
            child,
            group,
            waited: false,
        })
    }

    /// Wait on the leader.
    async fn wait(&mut self) -> std::io::Result<ExitStatus> {
        let status = self.child.wait().await?;
        self.waited = true;
        Ok(status)
    }

    /// End the worker: its group is asked to end, given `grace`, then
    /// killed, and the leader is waited on.
    async fn end(&mut self, grace: Duration) -> std::io::Result<ExitStatus> {
        let _ = self.child.signal(libc::SIGTERM);
        if let Ok(status) = tokio::time::timeout(grace, self.wait()).await {
            return status;
        }
        let _ = self.child.start_kill();
        self.wait().await
    }

    /// Once the leader has ended: a process of its group that is no child
    /// of this process, which no wait reaches, is looked at within
    /// `grace`, then killed.
    async fn settle(&self, grace: Duration) {
        let until = tokio::time::Instant::now() + grace;
        let mut look = tokio::time::interval(LOOK);
        while is_there(self.group) {
            if tokio::time::Instant::now() >= until {
                signal_group(self.group, libc::SIGKILL);
                return;
            }
            look.tick().await;
        }
    }
}

/// Run `future` as an owner task on the edge, and wait for its answer. A
/// caller of the core waits so on the blocking pool, and builds no runtime
/// and starts no thread.
fn on_edge<T: Send + 'static>(
    edge: &'static Edge,
    future: impl std::future::Future<Output = std::io::Result<T>> + Send + 'static,
) -> std::io::Result<T> {
    let (answer, answered) = std::sync::mpsc::sync_channel(1);
    edge.tracker.spawn_on(
        async move {
            let _ = answer.send(future.await);
        },
        &edge.handle,
    );
    answered
        .recv()
        .map_err(|_| std::io::Error::other("a worker's owner ended before it answered"))?
}

/// A refusal to start, once the host process stops.
fn stopping() -> std::io::Error {
    std::io::Error::from_raw_os_error(libc::ECANCELED)
}

/// `command` run to its end, its standard output and error kept, its
/// standard input none, as std's `Command::output` gives it. In `sluice
/// host` it runs as a worker of the edge: its group is ended once its
/// leader has, given its grace, and at once where the host stops.
pub fn output(command: &mut Command) -> std::io::Result<Output> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run(command, None, None).map(|o| o.expect("no limit"))
}

/// `command` run as [`output`] runs it, to `limit`: `None` where the limit
/// came first and its group was ended. In a process with no edge,
/// weir-bounded runs it.
pub fn output_within(command: &mut Command, limit: Duration) -> std::io::Result<Option<Output>> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run(command, None, Some(limit))
}

/// `command` run as [`output`] runs it, with `input` written to its
/// standard input beside the read of its output, so a long input never
/// waits on a pipe it has not read. A write that failed is the answer only
/// where the command ended well: one that failed says why in what it
/// printed.
pub fn output_with(command: &mut Command, input: Vec<u8>) -> std::io::Result<Output> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run(command, Some(input), None).map(|o| o.expect("no limit"))
}

/// `command` run to its end, its standard streams as it set them, and its
/// exit.
pub fn status(command: &mut Command) -> std::io::Result<ExitStatus> {
    run(command, None, None).map(|o| o.expect("no limit").status)
}

/// `command` run to its end, or to `limit`, where `None` answers that the
/// limit came first.
fn run(
    command: &mut Command,
    input: Option<Vec<u8>>,
    limit: Option<Duration>,
) -> std::io::Result<Option<Output>> {
    let command = std::mem::replace(command, Command::new("/"));
    let Some(edge) = EDGE.get() else {
        return run_here(command, input, limit);
    };
    let grace = weir_bounded::GRACE;
    on_edge(edge, async move {
        if edge.stop.is_cancelled() {
            return Err(stopping());
        }
        let mut held = Owned::spawn(command)?;
        let stdin = held.child.stdin().take();
        let mut stdout = held.child.stdout().take();
        let mut stderr = held.child.stderr().take();
        let written = async move {
            match (stdin, input) {
                (Some(mut pipe), Some(input)) => {
                    let wrote = pipe.write_all(&input).await;
                    drop(pipe);
                    wrote
                }
                _ => Ok(()),
            }
        };
        let (out, err) = (read_all(stdout.take()), read_all(stderr.take()));
        let limited = async {
            match limit {
                Some(limit) => tokio::time::sleep(limit).await,
                None => std::future::pending().await,
            }
        };
        let ended = async {
            tokio::select! {
                ended = async { tokio::join!(held.wait(), out, err, written) } => Ok(ended),
                () = limited => Err(None),
                () = edge.stop.cancelled() => Err(Some(stopping())),
            }
        };
        let (status, out, err, written) = match ended.await {
            Ok(ended) => ended,
            Err(why) => {
                held.end(grace).await?;
                forget(held.group);
                return match why {
                    Some(stopped) => Err(stopped),
                    None => Ok(None),
                };
            }
        };
        let status = status?;
        held.settle(grace).await;
        forget(held.group);
        let output = Output {
            status,
            stdout: out?,
            stderr: err?,
        };
        match written {
            Err(e) if output.status.success() => Err(e),
            _ => Ok(Some(output)),
        }
    })
}

/// [`output`], [`output_with`] and [`status`] as methods of a command
/// that [`command`] made, so a call site reads as std's does.
pub trait Run {
    /// [`output`] of this command.
    fn worker_output(&mut self) -> std::io::Result<Output>;
    /// [`output_with`] of this command.
    fn worker_output_with(&mut self, input: Vec<u8>) -> std::io::Result<Output>;
    /// [`status`] of this command.
    fn worker_status(&mut self) -> std::io::Result<ExitStatus>;
}

impl Run for Command {
    fn worker_output(&mut self) -> std::io::Result<Output> {
        output(self)
    }

    fn worker_output_with(&mut self, input: Vec<u8>) -> std::io::Result<Output> {
        output_with(self, input)
    }

    fn worker_status(&mut self) -> std::io::Result<ExitStatus> {
        status(self)
    }
}

/// All that `pipe` gives until it ends.
async fn read_all(pipe: Option<impl tokio::io::AsyncRead + Unpin>) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    if let Some(mut pipe) = pipe {
        tokio::io::AsyncReadExt::read_to_end(&mut pipe, &mut bytes).await?;
    }
    Ok(bytes)
}

/// `command` run in this process, which has no edge, as std runs it.
fn run_here(
    mut command: Command,
    input: Option<Vec<u8>>,
    limit: Option<Duration>,
) -> std::io::Result<Option<Output>> {
    if let Some(limit) = limit {
        let ended = weir_bounded::output(&mut command, limit)?;
        return Ok((!ended.stopped).then_some(Output {
            status: ended.status,
            stdout: ended.stdout,
            stderr: ended.stderr,
        }));
    }
    let Some(input) = input else {
        return command.output().map(Some);
    };
    let mut child = command.spawn()?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    std::thread::scope(|s| {
        let writer = s.spawn(move || {
            use std::io::Write as _;
            stdin.write_all(&input)
        });
        let out = child.wait_with_output();
        let written = writer.join().expect("the writer of a command's stdin");
        let out = out?;
        match written {
            Err(e) if out.status.success() => Err(e),
            _ => Ok(Some(out)),
        }
    })
}

/// How a worker that [`start`] started ended.
#[derive(Clone, Copy, Debug)]
pub struct Ended {
    /// Its leader's exit.
    pub status: ExitStatus,
    /// Whether its limit came first, and its group was ended.
    pub stopped: bool,
}

/// What a worker's owner task tells its handle.
#[derive(Default)]
struct Told {
    ended: Mutex<Option<std::io::Result<Ended>>>,
    changed: std::sync::Condvar,
}

/// A worker that outlives the call that starts it: a run's supervisor, or
/// a release command. Dropped while it runs, its group ends, unless
/// [`Worker::keep`] gave it to its owner.
pub struct Worker {
    pid: u32,
    inner: Inner,
}

enum Inner {
    /// Owned by a task of the edge.
    Edge {
        told: Arc<Told>,
        end: CancellationToken,
        kept: bool,
    },
    /// Started in a process with no edge, as std starts it.
    Here {
        child: std::process::Child,
        limit: Option<Duration>,
        grace: Duration,
    },
}

/// Start `command` as a worker. Its group is ended at `limit`, where one
/// is given, its `grace` given first. In `sluice host` it counts among the
/// host's children until it ends, when the hook of [`own`] is called.
pub fn start(
    command: &mut Command,
    limit: Option<Duration>,
    grace: Duration,
) -> std::io::Result<Worker> {
    let mut command = std::mem::replace(command, Command::new("/"));
    let Some(edge) = EDGE.get() else {
        let child = command.spawn()?;
        return Ok(Worker {
            pid: child.id(),
            inner: Inner::Here {
                child,
                limit,
                grace,
            },
        });
    };
    let told = Arc::new(Told::default());
    let end = edge.stop.child_token();
    let (started, began) = std::sync::mpsc::sync_channel(1);
    let owned = (told.clone(), end.clone());
    edge.tracker.spawn_on(
        async move {
            let (told, end) = owned;
            if end.is_cancelled() {
                let _ = started.send(Err(stopping()));
                return;
            }
            let mut held = match Owned::spawn(command) {
                Ok(h) => h,
                Err(e) => {
                    let _ = started.send(Err(e));
                    return;
                }
            };
            let pid = held.group;
            CHILDREN.lock().expect("the children").insert(pid);
            let _ = started.send(Ok(pid as u32));
            let limited = async {
                match limit {
                    Some(limit) => tokio::time::sleep(limit).await,
                    None => std::future::pending().await,
                }
            };
            let ended = tokio::select! {
                status = held.wait() => status.map(|status| Ended { status, stopped: false }),
                () = limited => held.end(grace).await.map(|status| Ended { status, stopped: true }),
                () = end.cancelled() => held.end(grace).await.map(|status| Ended { status, stopped: true }),
            };
            held.settle(grace).await;
            ended_child(pid);
            *told.ended.lock().expect("the worker's end") = Some(ended);
            told.changed.notify_all();
        },
        &edge.handle,
    );
    let pid = began
        .recv()
        .map_err(|_| std::io::Error::other("a worker's owner ended before it started"))??;
    Ok(Worker {
        pid,
        inner: Inner::Edge {
            told,
            end,
            kept: false,
        },
    })
}

impl Worker {
    /// Its leader's pid, which is also its process group.
    pub fn id(&self) -> u32 {
        self.pid
    }

    /// How it ended, where it has.
    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        match &mut self.inner {
            Inner::Edge { told, .. } => match &*told.ended.lock().expect("the worker's end") {
                None => Ok(None),
                Some(Ok(e)) => Ok(Some(e.status)),
                Some(Err(e)) => Err(std::io::Error::new(e.kind(), e.to_string())),
            },
            Inner::Here { child, .. } => child.try_wait(),
        }
    }

    /// Wait for it to end: by itself, or at its limit, its group ended.
    pub fn wait(mut self) -> std::io::Result<Ended> {
        match &mut self.inner {
            Inner::Edge { told, .. } => {
                let mut ended = told.ended.lock().expect("the worker's end");
                while ended.is_none() {
                    ended = told.changed.wait(ended).expect("the worker's end");
                }
                ended.take().expect("an end")
            }
            Inner::Here {
                child,
                limit,
                grace,
            } => wait_here(child, *limit, *grace),
        }
    }

    /// End it now: its group is asked to end, given its grace, then
    /// killed. Its owner waits on it.
    pub fn kill(&mut self) {
        match &mut self.inner {
            Inner::Edge { end, .. } => end.cancel(),
            Inner::Here { child, .. } => {
                let _ = child.kill();
            }
        }
    }

    /// Give it to its owner: it then ends when it exits, or when its owner
    /// stops. In a process with no edge, a thread waits on it, so it is
    /// never left a zombie (`nothing-detached@f807c7f5fcea`).
    pub fn keep(mut self) {
        match &mut self.inner {
            Inner::Edge { kept, .. } => *kept = true,
            Inner::Here { .. } => {
                let Inner::Here { mut child, .. } =
                    std::mem::replace(&mut self.inner, Inner::kept())
                else {
                    unreachable!()
                };
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
        }
    }
}

impl Inner {
    /// What a handle holds once its worker went to its owner.
    fn kept() -> Inner {
        Inner::Edge {
            told: Arc::new(Told::default()),
            end: CancellationToken::new(),
            kept: true,
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Inner::Edge {
            end, kept: false, ..
        } = &self.inner
        {
            end.cancel();
        }
    }
}

/// Wait on `child`, started in a process with no edge, to `limit`, then
/// end its group, given `grace` (weir-bounded's `end_group`, for code that
/// has no runtime).
fn wait_here(
    child: &mut std::process::Child,
    limit: Option<Duration>,
    grace: Duration,
) -> std::io::Result<Ended> {
    let group = child.id() as i32;
    let deadline = limit.map(|l| Instant::now() + l);
    let mut status = None;
    while status.is_none() && deadline.is_none_or(|d| Instant::now() < d) {
        status = child.try_wait()?;
        if status.is_none() {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    let stopped = status.is_none();
    let mut reap = || {
        if status.is_none() {
            status = child.try_wait().ok().flatten();
        }
        status.is_some()
    };
    weir_bounded::end_group(group, grace, &mut reap);
    let status = match status {
        Some(s) => s,
        None => child.wait()?,
    };
    forget(group);
    Ok(Ended { status, stopped })
}

/// Wait again, on the edge, on `pid`, a child that an earlier image of
/// this process started and that runs: at each `SIGCHLD`, nix's `waitpid`
/// asks it without blocking. When it ends, its record in `dir` goes.
fn adopt(edge: &'static Edge, dir: &Path, pid: i32) {
    CHILDREN.lock().expect("the children").insert(pid);
    let dir = dir.to_path_buf();
    edge.tracker.spawn_on(
        async move {
            use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
            let raw = nix::unistd::Pid::from_raw(pid);
            let signal = tokio::signal::unix::SignalKind::child();
            let mut children = tokio::signal::unix::signal(signal).ok();
            while let Ok(WaitStatus::StillAlive) | Err(nix::errno::Errno::EINTR) =
                waitpid(raw, Some(WaitPidFlag::WNOHANG))
            {
                match children.as_mut() {
                    Some(c) => {
                        c.recv().await;
                    }
                    None => tokio::time::sleep(LOOK).await,
                }
            }
            forget_in(&dir, pid);
            ended_child(pid);
        },
        &edge.handle,
    );
}

/// Reap each child of this process that has ended, by nix's `waitpid` on
/// any child without blocking. Only before the first worker of this image
/// starts, so it takes no child that a handle of this image waits on.
fn reap_ended() -> Vec<i32> {
    use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
    let mut reaped = Vec::new();
    loop {
        match waitpid(None, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) | Err(nix::errno::Errno::ECHILD) => return reaped,
            Err(nix::errno::Errno::EINTR) => {}
            Err(_) => return reaped,
            Ok(status) => {
                if let Some(pid) = status.pid() {
                    reaped.push(pid.as_raw());
                }
            }
        }
    }
}

/// The child `pid` ended.
fn ended_child(pid: i32) {
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
    /// The children that an earlier image of this process started and
    /// that still run, now waited on.
    pub kept: Vec<i32>,
    /// The children that an earlier image of this process started and
    /// that had ended unwaited on, now reaped.
    pub reaped: Vec<i32>,
}

/// As `sluice host` starts, before it claims a run or starts a worker:
/// each child that an earlier image of this process started is waited on
/// again, a recorded worker or not, one that ended already among them
/// (`nothing-detached@f807c7f5fcea`); each record whose worker no longer
/// runs goes; and each group of another owner is ended.
pub fn sweep(owner: &Owner) -> Swept {
    // No worker of this image has started yet, so each child that has
    // ended is the earlier image's, and no handle of this image waits on
    // it.
    let mut swept = Swept {
        reaped: reap_ended(),
        ..Swept::default()
    };
    // Each process of `sluice host` records itself before its exec, so a
    // child of the earlier image that still runs has a record.
    let me = std::process::id() as i32;
    if let Some(edge) = EDGE.get() {
        for r in records(&owner.dir) {
            if r.of(owner) && r.live() && parent(r.pid) == Some(me) {
                adopt(edge, &owner.dir, r.pid);
                swept.kept.push(r.pid);
            }
        }
    }
    for pid in &swept.reaped {
        forget_in(&owner.dir, *pid);
    }
    let mut ending = Vec::new();
    for r in records(&owner.dir) {
        if swept.kept.contains(&r.pid) {
            // Its waiter removes its record when it ends.
        } else if !r.live() {
            forget_in(&owner.dir, r.pid);
        } else if !r.of(owner) {
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
    // Each owner task of the edge ends its worker's group and waits on it.
    if let Some(edge) = EDGE.get() {
        edge.stop.cancel();
    }
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
    // A child its owner task did not reap yet is reaped here: the edge's
    // tracker is waited on, within a second.
    if let Some(edge) = EDGE.get() {
        edge.tracker.close();
        edge.handle.block_on(async {
            let _ = tokio::time::timeout(Duration::from_secs(1), edge.tracker.wait()).await;
        });
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

    /// Whether the process `pid` is gone, a zombie that no one reaped
    /// being no gone process.
    fn gone(pid: i32) -> bool {
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None)
            == Err(nix::errno::Errno::ESRCH)
    }

    /// `nothing-detached@f807c7f5fcea`: as `sluice host` starts, each
    /// child that its earlier image left is waited on, one that has ended
    /// already, recorded as a worker or not, among them, so none of them
    /// stays a zombie of the host process (deneb, 2026-11-27: pid 64735, a
    /// child of the daemon's image before its hand-over in place, a zombie
    /// for 16 hours).
    #[test]
    fn a_sweep_reaps_each_child_its_earlier_image_left() {
        let dir = scratch("sweep-reaps");
        let me = std::process::id();
        let owner = Owner {
            dir: dir.clone(),
            pid: me,
            start: started(me as i32).unwrap(),
        };
        let held = serde_json::to_vec(&Held {
            owner: owner.pid,
            owner_start: owner.start,
            what: "git".into(),
        })
        .unwrap();
        let mut prefix = dir.as_os_str().as_bytes().to_vec();
        prefix.push(b'/');
        let mut recorded = Command::new("/bin/sh");
        recorded.args(["-c", "exit 0"]).process_group(0);
        // SAFETY: as in `take`.
        unsafe {
            recorded.pre_exec(move || record_self(&prefix, &held));
        }
        // Their handles go as the earlier image's did at its exec: no one
        // waits on them.
        let recorded = recorded.spawn().unwrap().id() as i32;
        let unrecorded = Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap()
            .id() as i32;
        let until = Instant::now() + Duration::from_secs(10);
        while [recorded, unrecorded].iter().any(|p| started(*p).is_some()) {
            assert!(Instant::now() < until, "the children never ended");
            std::thread::sleep(Duration::from_millis(20));
        }
        let swept = sweep(&owner);
        let until = Instant::now() + Duration::from_secs(5);
        while !(gone(recorded) && gone(unrecorded)) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(gone(recorded), "the recorded worker {recorded} is a zombie");
        assert!(gone(unrecorded), "the child {unrecorded} is a zombie");
        assert!(swept.ended.is_empty(), "{swept:?}");
        assert!(records(&dir).is_empty(), "{:?}", records(&dir));
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

    /// The pid that `sh` prints first on `out`: a child it started in the
    /// worker's group.
    async fn first_pid(out: &mut Option<tokio::process::ChildStdout>) -> i32 {
        use tokio::io::AsyncBufReadExt as _;
        let pipe = out.take().expect("stdout is piped");
        let mut line = String::new();
        tokio::io::BufReader::new(pipe)
            .read_line(&mut line)
            .await
            .unwrap();
        line.trim().parse().unwrap()
    }

    /// A command whose leader starts a child of its own in its group,
    /// prints the child's pid and waits for it.
    fn a_leader_with_a_child(also: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", &format!("sleep 600 & echo $!; {also}")])
            .stdin(Stdio::null())
            .stdout(Stdio::piped());
        command
    }

    /// Waits, with tokio's clock, until no process of `group` is left.
    async fn until_gone(group: i32) {
        while is_there(group) {
            tokio::time::sleep(LOOK).await;
        }
    }

    /// `each-worker-starts-through-process-wrap@025fae162990`: a worker
    /// dropped while it runs ends with its whole group, a child that its
    /// leader started among it, which tokio's kill on drop alone leaves
    /// running, and its leader is reaped. No time is asserted: the test
    /// waits until the group is gone.
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn a_dropped_worker_ends_its_whole_group() {
        let mut held = Owned::spawn(a_leader_with_a_child("wait")).unwrap();
        let child = first_pid(held.child.stdout()).await;
        let group = held.group;
        assert_eq!(
            nix::unistd::getpgid(Some(nix::unistd::Pid::from_raw(child)))
                .unwrap()
                .as_raw(),
            group
        );
        drop(held);
        until_gone(group).await;
        assert!(gone(child), "{child} outlived its worker");
    }

    /// A child that outlives its leader in the worker's group, which no
    /// wait of this process reaches, is ended once the worker's grace
    /// has passed.
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn a_worker_ends_what_its_leader_left_in_its_group() {
        edge(tokio::runtime::Handle::current());
        let out = tokio::task::spawn_blocking(|| {
            output(Command::new("/bin/sh").args(["-c", "sleep 600 & echo $!"]))
        })
        .await
        .unwrap()
        .unwrap();
        assert!(out.status.success(), "{out:?}");
        let child: i32 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
        // Ended, it is reaped by the process it was handed to.
        while !gone(child) {
            tokio::time::sleep(LOOK).await;
        }
    }

    /// A worker at its limit is ended with its group, and said stopped.
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn a_worker_at_its_limit_ends_with_its_group() {
        edge(tokio::runtime::Handle::current());
        let worker = tokio::task::spawn_blocking(|| {
            let mut command = a_leader_with_a_child("wait");
            command.stdout(Stdio::null());
            let worker = start(
                &mut command,
                Some(Duration::from_secs(60)),
                weir_bounded::GRACE,
            )
            .unwrap();
            let group = worker.id() as i32;
            (worker.wait().unwrap(), group)
        });
        let (ended, group) = worker.await.unwrap();
        assert!(ended.stopped, "{ended:?}");
        until_gone(group).await;
    }
}
