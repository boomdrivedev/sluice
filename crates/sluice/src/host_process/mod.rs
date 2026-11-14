//! sluice's host process (135, 129, 71, 150; rules core.31, core.25,
//! core.49; records 0093, 0123): the one process for each user on a host that
//! moves every repository's queues, each apart, holds the host's bound on
//! runs, and reads each repository it holds for the watches and waits
//! that follow it. A command that needs the host's queues moved reaches it
//! or starts it; it ends once nothing is in play and nothing follows it,
//! and keeps nothing the records and the host's state do not hold.
//!
//! ```text
//! $XDG_STATE_HOME/sluice/host/        (default ~/.local/state)
//!   lock           held by the host process while it lives
//!   start.lock     held by a command starting it, and by the process as
//!                  it decides to end
//!   version        its version, pid and start time, written whole
//!   log            what its passes say, its warnings and failures; the
//!                  previous process's kept as log.1
//!   repositories   the index of repositories, written whole
//! <the user's directory>/host-<digest>.sock
//! ```
//!
//! The socket is in the user's own short directory (record 0063), named by
//! a digest of the state folder's real path, so each state folder has its
//! own process and a socket's path stays within the 104 bytes macOS binds,
//! however deep the state folder (design D1).

pub mod ask;
pub mod client;
pub mod environment;
pub mod index;
pub mod pass;
pub mod protocol;
pub mod room;
pub mod tasks;

use std::fs::{File, OpenOptions};
use std::io::{BufReader, ErrorKind, Read};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::instance::Instance;
use crate::profiles::places::{repository_key, user_scratch, USER_SCRATCH_VAR};
pub use ask::{Following, Here, Holding, Holds, HostProcess, Socketed};
use protocol::{receive, send, Answer, Hello, Named, Request, RoomView, Started};
use room::{Asked, Asker, Room, Shared};

/// Set to `record` in tests: the host process takes no pass of its own,
/// and passes only when a command asks (design D14).
pub const ROUND_SLEEP_VAR: &str = "SLUICE_ROUND_SLEEP";

/// How often the host process looks at whether it should end, and at
/// whether its state folder is still there (design D6).
const LOOK: Duration = Duration::from_millis(100);

/// How often it looks at each repository in its index that it holds no
/// task for, as an idle tick reads it (129; design D4).
const SCAN: Duration = Duration::from_secs(3);

/// Where one state folder's host process keeps its files and its socket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    /// sluice's state folder, `$XDG_STATE_HOME/sluice`.
    pub state: PathBuf,
    /// Its `host/`.
    pub dir: PathBuf,
    pub socket: PathBuf,
}

impl Paths {
    /// The host process's of the state folder `state`, its socket in the
    /// user's directory `user`; `host/` is made where it is not there.
    pub fn of(state: &Path, user: &Path) -> Result<Paths> {
        let dir = state.join("host");
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let real = std::fs::canonicalize(state)
            .with_context(|| format!("resolving {}", state.display()))?;
        crate::profiles::places::make_own(user)?;
        Ok(Paths {
            state: state.to_path_buf(),
            dir,
            socket: user.join(format!("host-{}.sock", repository_key(&real))),
        })
    }

    /// As a process whose environment `var` reads finds them: its state
    /// folder `$XDG_STATE_HOME/sluice`, and the user's own short directory
    /// (record 0063), or the one the tests name in its stead.
    pub fn from_env(var: impl Fn(&str) -> Option<std::ffi::OsString>) -> Result<Paths> {
        let state = match var("XDG_STATE_HOME").filter(|d| !d.is_empty()) {
            Some(d) => PathBuf::from(d),
            None => PathBuf::from(var("HOME").context("HOME is not set")?)
                .join(".local")
                .join("state"),
        }
        .join("sluice");
        let user = match var(USER_SCRATCH_VAR).filter(|d| !d.is_empty()) {
            Some(d) => PathBuf::from(d),
            None => user_scratch()?,
        };
        Paths::of(&state, &user)
    }

    /// This process's.
    pub fn host() -> Result<Paths> {
        Paths::from_env(|k| std::env::var_os(k))
    }

    pub fn lock(&self) -> PathBuf {
        self.dir.join("lock")
    }

    pub fn start_lock(&self) -> PathBuf {
        self.dir.join("start.lock")
    }

    pub fn version(&self) -> PathBuf {
        self.dir.join("version")
    }

    pub fn log(&self) -> PathBuf {
        self.dir.join("log")
    }

    /// The process before's log.
    pub fn previous_log(&self) -> PathBuf {
        self.dir.join("log.1")
    }

    pub fn index(&self) -> index::Index {
        index::Index::at(&self.dir)
    }

    /// The host process as `host/version` names it, where one has.
    pub fn named(&self) -> Option<Named> {
        let bytes = std::fs::read(self.version()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }
}

/// The lock on `path` taken now, or `None` where another process holds it.
pub(crate) fn try_lock(path: &Path) -> Result<Option<File>> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => {
            Err(e).with_context(|| format!("locking {}", path.display()))
        }
    }
}

/// Whether passes are taken only when asked (design D14).
pub fn asked_only() -> bool {
    std::env::var(ROUND_SLEEP_VAR).as_deref() == Ok("record")
}

/// A line in the host process's log, which is its stderr.
pub(crate) fn log(line: &str) {
    eprintln!(
        "{} {line}",
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    );
}

/// What every thread of the host process shares.
pub(crate) struct Hosting {
    pub paths: Paths,
    pub named: Named,
    pub room: Shared,
    pub tasks: tasks::Tasks,
    /// Connections accepted and not yet closed.
    pub connections: AtomicUsize,
    /// A newer sluice asked it to end, or its state folder is gone.
    pub stopping: AtomicBool,
    /// Its folder, `host/`, as it found it when it started: the device and
    /// inode, so a folder made again at that path is not its own.
    pub folder: (u64, u64),
}

impl Hosting {
    /// 135 (design D6): whether its folder is gone, or is another made
    /// again at its path since it started.
    fn folder_gone(&self) -> bool {
        use std::os::unix::fs::MetadataExt;
        !std::fs::metadata(&self.paths.dir)
            .is_ok_and(|m| m.is_dir() && (m.dev(), m.ino()) == self.folder)
    }
}

/// `sluice host`: run as the host process of this user's state folder,
/// or, where another holds its lock, end at once and move nothing (135;
/// design D1). Its exit.
pub fn run() -> Result<i32> {
    let paths = Paths::host()?;
    let Some(lock) = try_lock(&paths.lock())? else {
        log("sluice host: another host process holds this host's lock; this one ends");
        return Ok(0);
    };
    run_holding(paths, lock)
}

fn run_holding(paths: Paths, _lock: File) -> Result<i32> {
    take_log(&paths)?;
    let named = Named {
        version: crate::VERSION.to_string(),
        pid: std::process::id(),
        since: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        asked_only: asked_only(),
    };
    let tmp = paths.dir.join(".version.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&named)?)?;
    std::fs::rename(&tmp, paths.version())?;
    // A socket a dead process left: nothing answers on it while this
    // process holds the lock.
    match std::fs::remove_file(&paths.socket) {
        Err(e) if e.kind() != ErrorKind::NotFound => {
            return Err(e).with_context(|| format!("removing {}", paths.socket.display()))
        }
        _ => {}
    }
    let listener = UnixListener::bind(&paths.socket)
        .with_context(|| format!("binding {}", paths.socket.display()))?;
    listener.set_nonblocking(true)?;
    let folder = {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(&paths.dir)
            .with_context(|| format!("reading {}", paths.dir.display()))?;
        (m.dev(), m.ino())
    };
    let instance = Instance::read()?;
    let room = Shared::new(Room::over(
        crate::host_count::HostCount::host()?,
        instance.bound,
        instance.reserved,
    ));
    let hosting = Arc::new(Hosting {
        tasks: tasks::Tasks::new(
            tasks::repository_body(room.clone(), asked_only(), named.clone()),
            Arc::new(|r: &Path| index::in_play(r).unwrap_or(true)),
        ),
        paths,
        named,
        room,
        connections: AtomicUsize::new(0),
        stopping: AtomicBool::new(false),
        folder,
    });
    log(&format!(
        "sluice host {} started, pid {}{}",
        hosting.named.version,
        hosting.named.pid,
        if asked_only() {
            "; it passes only when asked"
        } else {
            ""
        }
    ));
    // 135 (design D4): the repositories it holds, found again; a gone one,
    // or one with no sluice, leaves the index.
    let index = hosting.paths.index();
    match index.prune(|r| !index::has_sluice(r).unwrap_or(true)) {
        Ok(gone) => {
            for r in gone {
                log(&format!(
                    "{}: no sluice here; out of the index",
                    r.display()
                ));
            }
        }
        Err(e) => log(&format!("sluice host: reading the index: {e:#}")),
    }
    for repository in index.list().unwrap_or_default() {
        hosting.tasks.look(&repository);
    }
    let code = serve(&hosting, &listener);
    let _ = std::fs::remove_file(&hosting.paths.socket);
    log(&format!("sluice host {} ends", hosting.named.pid));
    code
}

/// Keep the previous process's log as `log.1`, and send this one's output
/// to `log`. A log the command that started it made, which holds nothing
/// yet, is this process's own.
fn take_log(paths: &Paths) -> Result<()> {
    let log = paths.log();
    if std::fs::metadata(&log).is_ok_and(|m| m.len() > 0) {
        let _ = std::fs::rename(&log, paths.previous_log());
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .with_context(|| format!("opening {}", log.display()))?;
    nix::unistd::dup2_stdout(&file).context("sending the host process's output to its log")?;
    nix::unistd::dup2_stderr(&file).context("sending the host process's output to its log")?;
    Ok(())
}

/// Accept connections, each answered on a thread of its own, until the
/// process should end (design D6, D8).
fn serve(hosting: &Arc<Hosting>, listener: &UnixListener) -> Result<i32> {
    let mut looked = Instant::now();
    let mut scanned = Instant::now();
    loop {
        accept_all(hosting, listener);
        if looked.elapsed() < LOOK {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        looked = Instant::now();
        // 135: a state folder removed ends it, its socket with it, though
        // another made a folder at its path again since; it reads no
        // repository after, so nothing of it writes there again.
        if !hosting.stopping.load(Ordering::SeqCst) && hosting.folder_gone() {
            log("sluice host: its state folder is gone; it ends");
            hosting.stopping.store(true, Ordering::SeqCst);
        }
        if !hosting.stopping.load(Ordering::SeqCst) {
            hosting.tasks.tick();
            if scanned.elapsed() >= SCAN {
                scanned = Instant::now();
                scan(hosting);
            }
        }
        if hosting.stopping.load(Ordering::SeqCst) {
            hosting.tasks.stop_all();
            return Ok(0);
        }
        if hosting.tasks.any()
            || hosting.connections.load(Ordering::SeqCst) > 0
            || hosting.room.lock().busy(Instant::now())
        {
            continue;
        }
        // Idle: it decides under the start lock, which it keeps until it
        // ends, after taking every connection already queued (design D6).
        let Ok(Some(start)) = try_lock(&hosting.paths.start_lock()) else {
            continue;
        };
        if accept_all(hosting, listener) > 0 {
            drop(start);
            continue;
        }
        let _ = std::fs::remove_file(&hosting.paths.socket);
        // The start lock goes with the process.
        std::mem::forget(start);
        return Ok(0);
    }
}

/// 135 (design D4): each repository in the index whose directory is gone
/// leaves it, and each other it holds no task for is looked at as an idle
/// tick reads it, and given a task only where work is in play there, so
/// work another host wrote to its records is found and an idle repository
/// costs that read alone (129). Each read runs on a thread of its own
/// ([`tasks::Tasks::look`]), so the loop that accepts commands' connections
/// never waits on a remote.
fn scan(hosting: &Hosting) {
    let index = hosting.paths.index();
    match index.prune(|r| !r.is_dir()) {
        Ok(gone) => {
            for r in gone {
                log(&format!("{}: gone; out of the index", r.display()));
            }
        }
        Err(e) => log(&format!("sluice host: reading the index: {e:#}")),
    }
    for repository in index.list().unwrap_or_default() {
        hosting.tasks.look(&repository);
    }
}

/// Take every connection queued, each answered on a thread of its own:
/// how many.
fn accept_all(hosting: &Arc<Hosting>, listener: &UnixListener) -> usize {
    let mut n = 0;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                n += 1;
                hosting.connections.fetch_add(1, Ordering::SeqCst);
                let hosting = hosting.clone();
                std::thread::spawn(move || {
                    if let Err(e) = answer(&hosting, stream) {
                        log(&format!("sluice host: a connection: {e:#}"));
                    }
                    hosting.connections.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => return n,
            Err(e) => {
                log(&format!("sluice host: accepting: {e}"));
                return n;
            }
        }
    }
}

/// Answer one connection: its hello, then its one request.
fn answer(hosting: &Arc<Hosting>, stream: UnixStream) -> Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let hello: Hello = receive(&mut reader)?;
    send(&mut writer, &hosting.named)?;
    let mut line = String::new();
    if std::io::BufRead::read_line(&mut reader, &mut line)? == 0 {
        return Ok(());
    }
    let request: Request = match serde_json::from_str(line.trim_end()) {
        Ok(r) => r,
        Err(_) => {
            let words = format!(
                "sluice {} asked what this host process, sluice {}, does not know: {}",
                hello.hello,
                hosting.named.version,
                line.trim_end()
            );
            return send(&mut writer, &Answer::Error(words));
        }
    };
    stream.set_read_timeout(None)?;
    match request {
        Request::Serve { repository } => {
            note(hosting, &repository);
            // Where passes are taken only when asked, the repository is
            // held for the next ask; otherwise it is passed at once.
            let task = hosting.tasks.handle(&repository);
            if !asked_only() {
                task.wake();
            }
            send(&mut writer, &Answer::Ok)
        }
        Request::Pass { repository } => {
            note(hosting, &repository);
            let answer = match hosting.tasks.handle(&repository).pass() {
                Ok(report) => Answer::Pass(report),
                Err(e) => Answer::Error(e),
            };
            send(&mut writer, &answer)
        }
        Request::Step { repository, branch } => {
            note(hosting, &repository);
            let answer = match hosting.tasks.handle(&repository).step(&branch) {
                Ok(doc) => Answer::Step(doc),
                Err(e) => Answer::Error(e),
            };
            send(&mut writer, &answer)
        }
        Request::Follow { repository, kind } => {
            note(hosting, &repository);
            let task = hosting.tasks.handle(&repository);
            let follower = task.follow(kind);
            loop {
                if hosting.stopping.load(Ordering::SeqCst) || closed(&stream) {
                    return Ok(());
                }
                match follower.next(Duration::from_millis(200)) {
                    tasks::Next::Report(r) => {
                        if send(&mut writer, &Answer::Pass(r)).is_err() {
                            return Ok(());
                        }
                    }
                    tasks::Next::Nothing => {}
                    // The task ended under it: one is held again.
                    tasks::Next::Ended => return Ok(()),
                }
            }
        }
        Request::Hold {
            kind,
            repository,
            place: _,
        } => hold(
            hosting,
            kind,
            &repository,
            &stream,
            &mut writer,
            &mut reader,
        ),
        Request::Room => send(&mut writer, &Answer::Room(view(hosting)?)),
        Request::HandOver => {
            log(&format!(
                "sluice host: sluice {} asked this one to hand over; it ends",
                hello.hello
            ));
            hosting.stopping.store(true, Ordering::SeqCst);
            send(&mut writer, &Answer::Ok)
        }
    }
}

/// Note `repository` in the index (design D4).
fn note(hosting: &Hosting, repository: &Path) {
    if let Err(e) = hosting.paths.index().note(repository) {
        log(&format!(
            "sluice host: noting {} in the index: {e:#}",
            repository.display()
        ));
    }
}

/// Whether the other end of `stream` closed it: a read that finds its end
/// or fails; nothing waiting is open.
fn closed(stream: &UnixStream) -> bool {
    if stream.set_nonblocking(true).is_err() {
        return true;
    }
    let mut buf = [0u8; 1];
    let gone = match (&*stream).read(&mut buf) {
        Ok(0) => true,
        Ok(_) => false,
        Err(e) => e.kind() != ErrorKind::WouldBlock,
    };
    let _ = stream.set_nonblocking(false);
    gone
}

/// The room as a person reads it (150).
fn view(hosting: &Hosting) -> Result<RoomView> {
    view_of(&hosting.room)
}

/// `room` as a person reads it (150).
pub(crate) fn view_of(room: &Shared) -> Result<RoomView> {
    let mut room = room.lock();
    let running = room.running()?;
    let now = Instant::now();
    Ok(RoomView {
        bound: room.bound(),
        reserve: room.reserved(),
        queue: running.queue,
        sessions: running.sessions,
        waiting: room.waiting(now),
        turn: room.turn(),
        repositories: room.repositories(now)?,
    })
}

/// A session's or a preview's room (design D12): it waits in the room's
/// order while its connection is open, saying why once for each change,
/// and once held, the room is its until it says its run began, or gives it
/// back when the connection closes.
fn hold(
    hosting: &Arc<Hosting>,
    kind: sluice_seams::RunKind,
    repository: &Path,
    stream: &UnixStream,
    writer: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
) -> Result<()> {
    let waiter = hosting.room.lock().wait(repository);
    let asker = match kind {
        sluice_seams::RunKind::Session => Asker::Session(waiter),
        sluice_seams::RunKind::Queue => Asker::Queue(repository.to_path_buf()),
    };
    let mut said = None;
    let id = loop {
        if hosting.stopping.load(Ordering::SeqCst) || closed(stream) {
            hosting.room.lock().let_go(waiter);
            return Ok(());
        }
        let asked = hosting.room.lock().ask(&asker, repository, Instant::now());
        match asked {
            Ok(Asked::Held(id)) => break id,
            Ok(Asked::Refused(why)) => {
                if said != Some(why) {
                    said = Some(why);
                    if send(writer, &Answer::Waiting(why)).is_err() {
                        hosting.room.lock().let_go(waiter);
                        return Ok(());
                    }
                }
            }
            Err(e) => {
                hosting.room.lock().let_go(waiter);
                return send(writer, &Answer::Error(format!("{e:#}")));
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    hosting.room.lock().let_go(waiter);
    let part = hosting.room.part(kind, id);
    if send(writer, &Answer::Held).is_err() {
        return Ok(());
    }
    // The run's start, or a connection closed: the room given back.
    let Ok(started) = receive::<Started>(reader) else {
        return Ok(());
    };
    let Ok(held) = part.take::<room::Holding>() else {
        return Ok(());
    };
    held.begin(&started.place, &started.run_dir)?;
    // Its supervisor holds the run directory before the command says so.
    held.started();
    Ok(())
}
