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
//!   newest         the newest sluice that has run as its host process,
//!                  written whole, never lowered
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
//!
//! The host process runs a tokio runtime at its edge (the change
//! `sluice-host-runs-on-a-tokio-edge`; `tokio-streams-before-rxrust@0b9fc11cda8b`):
//! the socket, the connections, the signals, the timers, the file events
//! and each repository's scheduler are tokio tasks. The core and its seams
//! stay synchronous: the host process calls them on tokio's pool for
//! blocking work. It waits on events and timers, on tokio's clock, and no
//! wait of it sleeps and looks again. `SIGTERM` and `SIGINT` stop each host
//! process.
//!
//! Each worker of the host process, a run's supervisor, a git command, a
//! credential command, an equip step or a release command, is its child in
//! a process group of its own, recorded in `host/workers/`
//! (`crate::workers`; the change `sluice-host-owns-its-workers`). A stop
//! ends the workers, and a run that ends so has no outcome. A hand-over
//! starts the newer image afresh in place, with the same pid, so the
//! workers stay its children. A host process that starts ends the groups
//! that a killed host process left.
//!
//! `sluice daemon` is the same process with its idle end off, on a machine
//! that chooses to keep it running (137; record 0093 as amended 2026-11-08
//! and 2026-11-18). It takes the host's place from a host process that ends
//! when idle, which ends for it, and waits behind another daemon. Every
//! host process hands over by starting afresh in its own process, keeping
//! its pid and its socket, so whatever keeps it running keeps the new image
//! (design D1 to D7 of `sluice-runs-as-a-daemon`).

pub mod ask;
pub mod changes;
pub mod client;
pub mod clock;
pub mod commands;
pub mod environment;
pub mod folder;
pub mod index;
pub mod joined;
pub mod pass;
pub mod protection;
pub mod protocol;
pub mod reads;
pub mod results;
pub mod room;
pub mod subscription;
pub mod tasks;
pub mod transaction;
pub mod version;
pub mod watch;

use std::ffi::OsString;
use std::fs::File;
use std::io::ErrorKind;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, Notify};
use tokio::time::Instant;
use tokio_util::codec::{FramedRead, LinesCodec};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::exec::Host;
use crate::host_settings::{HostSettings, Look};
use crate::instance::Instance;
use crate::profiles::places::{repository_key, user_scratch, USER_SCRATCH_VAR};
pub use ask::{Here, Holding, Holds, HostProcess, Socketed, Subscription};
use folder::{Folder, How};
use protocol::{Answer, FollowKind, Hello, Named, Request, RoomView};
use room::{Room, Shared};

/// Set to `record` in tests: the host process takes no pass of its own,
/// and passes only when a command asks (design D14).
pub const ROUND_SLEEP_VAR: &str = "SLUICE_ROUND_SLEEP";

/// The variable a daemon starting afresh in place names its start lock's
/// descriptor in, so the image started afresh holds it until its socket
/// answers (design D6 of `sluice-runs-as-a-daemon`).
pub const START_LOCK_FD_VAR: &str = "SLUICE_START_LOCK_FD";

/// The variable a daemon starting afresh in place names its socket's
/// descriptor in, so the image started afresh answers on the same socket,
/// and a command that connects meanwhile waits in its queue (design D6 of
/// `sluice-host-runs-on-a-tokio-edge`).
pub const SOCKET_FD_VAR: &str = "SLUICE_SOCKET_FD";

/// The variable a host process starting afresh in place names the host's
/// lock's descriptor in, so the image started afresh holds the lock its
/// last image held, with no moment between the two in which another
/// process may take it.
pub const LOCK_FD_VAR: &str = "SLUICE_LOCK_FD";

/// The variable a host process that hands over names a descriptor in, to
/// which its successor writes once its socket answers (design D20).
pub const READY_FD_VAR: &str = "SLUICE_READY_FD";

/// How often a repository with nothing in play has its remote asked, at
/// most, by the host process's look or a pass of its task, a watched one's
/// among them; a submit or another command that tells of a change, a
/// watch's opening and a forecast's second ask have the next reading ask
/// at once, and a repository with work in play is asked at every pass
/// (129 as amended 2026-11-19). It is also the minute's backstop: a
/// reading of each idle repository at least this often, for a change no
/// event reported.
pub const IDLE_ASK: Duration = Duration::from_secs(60);

/// The tick a reading is current within for a one-shot read (64, 129;
/// design D16).
pub const READ_TICK: Duration = Duration::from_secs(3);

/// How long a one-shot read waits for a current reading: the round's retry
/// bound and a tick (design D16).
pub fn read_for() -> Duration {
    crate::retry::COMMAND_FOR + READ_TICK
}

/// The threads of tokio's pool for blocking work, at most: a worker for
/// each repository in play, the looks, and the connections that read.
const BLOCKING_THREADS: usize = 256;

/// The longest line a connection reads.
const LINE_MAX: usize = 1 << 20;

/// How long a host process that is no daemon stays idle before it ends:
/// a command that closes one connection and opens the next within it
/// meets the same process. It is the time the host process looked at
/// whether it was idle before it ran on tokio.
const IDLE_LINGER: Duration = Duration::from_millis(100);

/// How long a connection waits for its hello and its request.
const ASK_WAIT: Duration = Duration::from_secs(10);

/// The files of `host/`, by their names in it.
const LOCK: &str = "lock";
const START_LOCK: &str = "start.lock";
const VERSION: &str = "version";
const NEWEST: &str = "newest";
const LOG: &str = "log";
const PREVIOUS_LOG: &str = "log.1";

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

    /// As [`Paths::from_env`] finds them, where a host process has had
    /// them, making nothing: `None` where the state folder's `host/` or the
    /// user's directory is not there (record 0120 as amended 2026-11-17).
    pub fn found_from_env(
        var: impl Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Result<Option<Paths>> {
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
        let dir = state.join("host");
        if !dir.is_dir() || !user.is_dir() {
            return Ok(None);
        }
        let real = std::fs::canonicalize(&state)
            .with_context(|| format!("resolving {}", state.display()))?;
        Ok(Some(Paths {
            state,
            dir,
            socket: user.join(format!("host-{}.sock", repository_key(&real))),
        }))
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
        self.dir.join(LOCK)
    }

    pub fn start_lock(&self) -> PathBuf {
        self.dir.join(START_LOCK)
    }

    pub fn version(&self) -> PathBuf {
        self.dir.join(VERSION)
    }

    pub fn log(&self) -> PathBuf {
        self.dir.join(LOG)
    }

    /// The process before's log.
    pub fn previous_log(&self) -> PathBuf {
        self.dir.join(PREVIOUS_LOG)
    }

    /// A command's index, of the folder at `host/` as each call finds it.
    pub fn index(&self) -> index::Index {
        index::Index::at(&self.dir)
    }

    /// The host process as `host/version` names it, where one has.
    pub fn named(&self) -> Option<Named> {
        let bytes = std::fs::read(self.version()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// `host/newest`: the newest sluice that has run as this state
    /// folder's host process (the change
    /// `an-older-client-starts-no-older-host`, design D1).
    pub fn newest(&self) -> PathBuf {
        self.dir.join(NEWEST)
    }

    /// The newest sluice that has run as the host process, as
    /// `host/newest` names it; `None` where none has, or the file cannot
    /// be read.
    pub fn newest_run(&self) -> Option<Newest> {
        Newest::from_bytes(&std::fs::read(self.newest()).ok()?)
    }

    /// Why sluice `version` starts no host process: the words of design D2
    /// of `an-older-client-starts-no-older-host`, where a newer sluice has
    /// run as the host process; `None` where none has.
    pub fn refused_start(&self, version: &str) -> Option<String> {
        let newest = self.newest_run()?;
        version::older(version, &newest.version).then(|| {
            format!(
                "sluice {version} starts no host process: sluice {} has run as this host's \
                 process{}. Run sluice {} or newer, or remove {} to run sluice {version}'s host \
                 process.",
                newest.version,
                newest
                    .program
                    .as_ref()
                    .map(|p| format!(" ({})", p.display()))
                    .unwrap_or_default(),
                newest.version,
                self.newest().display()
            )
        })
    }
}

/// The newest sluice that has run as a state folder's host process, as
/// `host/newest` keeps it (the change `an-older-client-starts-no-older-host`,
/// design D1).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Newest {
    pub version: String,
    /// Its executable, where it knew it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<PathBuf>,
}

impl Newest {
    fn from_bytes(bytes: &[u8]) -> Option<Newest> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Design D1 of `an-older-client-starts-no-older-host`: `host/newest`
/// raised to `own` where it names no version, or one older than `own`;
/// never lowered.
fn raise_newest(folder: &Folder, own: &Newest) -> Result<()> {
    let held = folder
        .read(NEWEST)
        .ok()
        .and_then(|b| Newest::from_bytes(&b));
    if held
        .as_ref()
        .is_none_or(|n| version::older(&n.version, &own.version))
    {
        folder.write(NEWEST, &serde_json::to_vec(own)?)?;
    }
    Ok(())
}

/// Design D3 of `an-older-client-starts-no-older-host`: why a host
/// process of sluice `own`, in `folder`, hands over to no sluice `by`: the
/// newest that has run, or its own, is newer; `None` where neither is.
fn refused_hand_over(folder: &Folder, own: &str, by: &str) -> Option<String> {
    let recorded = folder
        .read(NEWEST)
        .ok()
        .and_then(|b| Newest::from_bytes(&b))
        .map(|n| n.version)
        .filter(|v| !version::older(v, own))
        .unwrap_or_else(|| own.to_string());
    version::older(by, &recorded).then(|| {
        format!(
            "sluice {by} asked this host process, sluice {own}, to hand over, and a host process \
             hands over to no sluice older than {recorded}, which has run as this host's \
             process. Run sluice {recorded} or newer."
        )
    })
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

/// What every task of the host process shares.
pub(crate) struct Hosting {
    pub paths: Paths,
    pub named: Named,
    pub room: Shared,
    pub tasks: tasks::Tasks,
    /// Connections accepted and not yet closed.
    pub connections: AtomicUsize,
    /// A connection closed: the host process looks at whether it is idle.
    pub nudge: Notify,
    /// The host process ends: every connection ends.
    pub stop: CancellationToken,
    /// What a connection asks of the host process itself: a hand-over.
    pub control: mpsc::UnboundedSender<Control>,
    /// The connections' tasks.
    pub connected: TaskTracker,
    /// Its folder, `host/`, held open since it started: every file it
    /// opens or moves there is named through it, and its device and inode
    /// tell a folder made again at that path, which is not its own
    /// (design D6).
    pub folder: Arc<Folder>,
    /// The host's settings it holds, which every binding it makes is of
    /// (record 0107; design D19).
    pub settings: Arc<tasks::Settings>,
    /// The tokens of sluice's identity it holds, one for each repository,
    /// which every binding it makes holds its own in, on every thread, so
    /// no git call or API call of its gets a repository's token while one
    /// it holds lasts (record 0108 as amended 2026-11-19).
    pub tokens: crate::profiles::Tokens,
    /// The reads joined: a folder's repository, and each read of a
    /// repository (the change `sluice-host-answers-every-read`, design D5).
    pub reads: reads::Reads,
    /// What the pull request driver remembers of the requests each
    /// repository's waits follow, from one step to the next (136; the
    /// change `sluice-host-makes-each-change`, design D2).
    pub followed: std::sync::Mutex<
        std::collections::BTreeMap<PathBuf, crate::profiles::github::pulls::Remembered>,
    >,
    /// The hooks folder of each folder a request named, which branch
    /// protection's hook is put back in (the change
    /// `the-git-hook-asks-sluice-host`, design D4).
    pub hooks: protection::Hooks,
}

impl Hosting {
    /// What the waits on `repository` last remembered of the requests they
    /// follow.
    pub(crate) fn followed(&self, repository: &Path) -> crate::profiles::github::pulls::Remembered {
        self.followed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(repository)
            .cloned()
            .unwrap_or_default()
    }

    /// Keep what a wait on `repository` remembers for its next step.
    pub(crate) fn keep_followed(
        &self,
        repository: &Path,
        remembered: crate::profiles::github::pulls::Remembered,
    ) {
        self.followed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(repository.to_path_buf(), remembered);
    }
}

/// What a connection asks of the host process itself.
pub(crate) enum Control {
    /// A newer command asked it to hand over: it starts afresh in place as
    /// the executable the asker named, or as its own where it named none
    /// (design D3 of `sluice-runs-as-a-daemon` and of
    /// `sluice-host-owns-its-workers`). Where the asker takes the host's
    /// place itself, as a daemon does, it ends.
    HandOver {
        program: Option<PathBuf>,
        /// The asker's version, as its `hello` said it.
        by: String,
        /// The asker takes the host's place itself.
        ends: bool,
    },
    /// A command named a repository: the edge watches it, where it does
    /// not yet.
    Noted(PathBuf),
}

impl Hosting {
    /// 135 (design D6): whether its folder is gone, or is another made
    /// again at its path since it started.
    fn folder_gone(&self) -> bool {
        use std::os::unix::fs::MetadataExt;
        !std::fs::metadata(&self.paths.dir)
            .is_ok_and(|m| m.is_dir() && (m.dev(), m.ino()) == self.folder.id())
    }

    /// Its index, of the folder it holds (design D4, D6).
    fn index(&self) -> index::Index {
        index::Index::held(self.folder.clone())
    }
}

/// The host process as it started, which its successor starts as
/// (record 0107 as amended 2026-11-13; design D20): its environment
/// before telemetry took anything out of it, and the executable it
/// started as resolved through every link, or why that could not be read.
#[derive(Clone)]
struct Successor {
    env: Vec<(OsString, OsString)>,
    program: Result<PathBuf, String>,
}

impl Successor {
    /// This process as it starts.
    fn now() -> Successor {
        Successor {
            env: std::env::vars_os().collect(),
            program: std::env::current_exe()
                .and_then(std::fs::canonicalize)
                .map_err(|e| format!("the executable the host process started as: {e}")),
        }
    }
}

/// How the host process's loop ended: no work in play, a signal or its
/// state folder gone. A hand-over does not end it: the process starts
/// afresh in place.
enum Ending {
    Ended,
}

/// What an earlier image of this process, or the process that started it,
/// handed it by descriptor, each taken out of the environment and set to
/// close on exec, so no child inherits it. Taken first in `main`, before
/// any thread or child starts.
#[derive(Default)]
pub struct Carried {
    /// The start lock a daemon started afresh in place was handed, held
    /// until its socket answers (design D6 of `sluice-runs-as-a-daemon`).
    pub start: Option<File>,
    /// The socket a daemon started afresh in place was handed, which it
    /// answers on as it is (design D6 of `sluice-host-runs-on-a-tokio-edge`).
    pub socket: Option<std::os::unix::net::UnixListener>,
    /// Where a successor says that its socket answers (design D20).
    pub ready: Option<File>,
    /// The host's lock a host process started afresh in place was handed,
    /// which it holds as its last image held it.
    pub lock: Option<File>,
}

/// The descriptor `var` names, taken out of the environment and set to
/// close on exec again; `None` where none was handed, or the variable
/// names no open descriptor.
fn take_fd(var: &str) -> Option<OwnedFd> {
    let named = std::env::var_os(var)?;
    std::env::remove_var(var);
    let fd: RawFd = named.to_str()?.parse().ok()?;
    // SAFETY: the descriptor is this process's from its last image, or the
    // process that started it, which named it for this process alone; only
    // one that is open is taken.
    let held = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
    nix::fcntl::fcntl(held, nix::fcntl::FcntlArg::F_GETFD).ok()?;
    nix::fcntl::fcntl(
        held,
        nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::FD_CLOEXEC),
    )
    .ok()?;
    // SAFETY: as above; nothing else of this process owns it.
    Some(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// What this process was handed by descriptor ([`Carried`]).
pub fn take_carried() -> Carried {
    Carried {
        start: take_fd(START_LOCK_FD_VAR).map(File::from),
        socket: take_fd(SOCKET_FD_VAR).map(std::os::unix::net::UnixListener::from),
        ready: take_fd(READY_FD_VAR).map(File::from),
        lock: take_fd(LOCK_FD_VAR).map(File::from),
    }
}

/// Close each descriptor that this process inherited and keeps for no use
/// of its own: each at 3 or above that is not set to close on exec. sluice
/// sets each descriptor that it opens to close on exec, and
/// [`take_carried`] sets so each that it takes. A host process runs long,
/// and a descriptor that its starter's caller left open, such as the write
/// end of a test runner's pipe, would stay open as long as it runs, and in
/// each worker that it starts (`nothing-detached@f807c7f5fcea`). Called in
/// `main` after [`take_carried`], before any thread starts. The
/// descriptors that it closed.
pub fn close_inherited() -> Vec<RawFd> {
    let open: Vec<RawFd> = match std::fs::read_dir("/dev/fd") {
        Ok(entries) => entries
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
            .collect(),
        Err(_) => return Vec::new(),
    };
    let mut closed = Vec::new();
    for fd in open.into_iter().filter(|fd| *fd > 2) {
        // SAFETY: fcntl and close read only their integer arguments; a
        // descriptor of the listing that is closed already answers EBADF.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if flags >= 0 && flags & libc::FD_CLOEXEC == 0 && unsafe { libc::close(fd) } == 0 {
            closed.push(fd);
        }
    }
    closed
}

/// The host process's runtime: many threads, and a bounded pool for
/// blocking work (design D1 of `sluice-host-runs-on-a-tokio-edge`).
fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .max_blocking_threads(BLOCKING_THREADS)
        .thread_name("sluice-host")
        .build()
        .context("starting the host process's runtime")
}

/// `sluice host`: run as the host process of this user's state folder,
/// or, where another holds its lock, end at once and move nothing (135;
/// design D1). `choices` are the telemetry its own command line and
/// sluice's variables choose (151; rule core.53). Its exit.
pub fn run(choices: crate::telemetry::Choices, mut carried: Carried) -> Result<i32> {
    let successor = Successor::now();
    let paths = Paths::host()?;
    // An image started afresh in place holds the start lock that its last
    // image handed it, until its socket answers.
    let start = carried.start.take();
    let afresh = start.is_some();
    // Design D2 of `an-older-client-starts-no-older-host`: an older sluice
    // than the newest that has run starts no host process; an image
    // started afresh in place was asked for (D3).
    if !afresh {
        if let Some(why) = paths.refused_start(crate::VERSION) {
            anyhow::bail!(why);
        }
    }
    let folder =
        Folder::open(&paths.dir).with_context(|| format!("opening {}", paths.dir.display()))?;
    // An image started afresh in place holds the lock its last image
    // handed it.
    let lock = match carried.lock.take() {
        Some(lock) => lock,
        None => match folder.try_lock(LOCK)? {
            Some(lock) => lock,
            None => {
                log("sluice host: another host process holds this host's lock; this one ends");
                return Ok(0);
            }
        },
    };
    let runtime = runtime()?;
    run_holding(
        &runtime,
        Holder {
            paths,
            folder: Arc::new(folder),
            lock,
            choices,
            successor,
            daemon: false,
            start,
            afresh,
            carried,
        },
    )
}

/// `sluice daemon`: run as the host process of this user's state folder,
/// in the foreground, never ending for being idle (135, 137; record 0093
/// as amended 2026-11-08; design D1 of `sluice-runs-as-a-daemon`). It
/// notes the repository it is started in where that has sluice (D8),
/// takes the host's place from a host process that ends when idle, or
/// waits behind another daemon (D2), and stops on `SIGTERM` or `SIGINT`
/// (D7). The start lock its last image handed it is held until its socket
/// answers (D6). Its exit.
pub fn daemon(choices: crate::telemetry::Choices, mut carried: Carried) -> Result<i32> {
    let runtime = runtime()?;
    let successor = Successor::now();
    let paths = Paths::host()?;
    // Its last image noted it: one started afresh notes nothing, so it
    // takes no lock on the start lock it holds (D6).
    let start = carried.start.take();
    let afresh = start.is_some();
    if !afresh {
        // Design D2 of `an-older-client-starts-no-older-host`: before it
        // takes the host's lock or asks a host process to hand over.
        if let Some(why) = paths.refused_start(crate::VERSION) {
            anyhow::bail!(why);
        }
        note_here(&paths);
    }
    let folder = Arc::new(
        Folder::open(&paths.dir).with_context(|| format!("opening {}", paths.dir.display()))?,
    );
    // An image started afresh in place holds the lock its last image
    // handed it.
    let handed = carried.lock.take();
    let (lock, start) = match handed.map_or_else(|| folder.try_lock(LOCK), |l| Ok(Some(l)))? {
        Some(lock) => (lock, start),
        None => {
            // D6: a start lock it was handed is let go before it waits, so
            // the host process that holds the host can start afresh itself;
            // a socket it was handed is no longer the host's.
            drop(start);
            drop(carried.socket.take());
            let placed = runtime.block_on(async {
                let stop = CancellationToken::new();
                let signalled = stop.clone();
                tokio::spawn(async move {
                    signalled_to_stop().await;
                    signalled.cancel();
                });
                let (paths, folder, successor) = (paths.clone(), folder.clone(), successor.clone());
                let waiting = tokio::task::spawn_blocking(move || {
                    take_place(&paths, &folder, &successor, &stop)
                });
                waiting.await.context("waiting for the host's place")?
            })?;
            match placed {
                Some(placed) => placed,
                None => return Ok(0),
            }
        }
    };
    run_holding(
        &runtime,
        Holder {
            paths,
            folder,
            lock,
            choices,
            successor,
            daemon: true,
            start,
            afresh,
            carried,
        },
    )
}

/// The first of `SIGTERM` and `SIGINT`, from now.
async fn signalled_to_stop() {
    use tokio::signal::unix::{signal, SignalKind};
    let (Ok(mut term), Ok(mut int)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
    }
}

/// D8 of `sluice-runs-as-a-daemon`: the repository of the folder the
/// daemon is started in noted in the host's index where it has sluice, as
/// every command notes its own (135). A folder in no repository, or one
/// with no sluice, notes nothing, and the daemon starts all the same.
fn note_here(paths: &Paths) {
    let noted = std::env::current_dir()
        .map_err(anyhow::Error::from)
        .and_then(|dir| crate::profiles::local_refs::LocalRefs::open(&dir))
        .and_then(|refs| refs.common_dir())
        .and_then(|repository| {
            let index = paths.index();
            if !index.list()?.contains(&repository) && index::has_sluice(&repository)? {
                index.note(&repository)?;
            }
            Ok(())
        });
    drop(noted);
}

/// How often a daemon looks again at the host's lock while another holds
/// it (design D2 of `sluice-runs-as-a-daemon`).
const PLACE_LOOK: Duration = Duration::from_millis(100);

/// D2 of `sluice-runs-as-a-daemon`: the host's place, where another holds
/// its lock as the daemon starts: its lock, and the start lock where it
/// holds that too, until its socket answers; `None` where it was stopped
/// first. A holder that is no daemon, whatever its version, is asked to
/// hand over, as a newer command asks, and waited for as long; the start
/// lock is held meanwhile, so a command that meets neither reaches the
/// daemon. Where the holder still lives then, the daemon fails, naming the
/// log. A holder that is a daemon is asked nothing: this one says once on
/// stderr which daemon it waits for, and takes the host's place once its
/// lock goes, never while a start holds the start lock, so a daemon
/// starting afresh in place keeps its place (D6). The daemon is not the
/// host process while it waits: it looks again every 100 ms on a thread of
/// tokio's pool for blocking work, and `SIGTERM` or `SIGINT` ends the wait
/// through `stop`.
fn take_place(
    paths: &Paths,
    folder: &Folder,
    successor: &Successor,
    stop: &CancellationToken,
) -> Result<Option<(File, Option<File>)>> {
    let client = client::Client::new(
        successor
            .program
            .as_deref()
            .unwrap_or_else(|_| Path::new("sluice")),
        successor.env.clone(),
    )?;
    let mut said = None;
    loop {
        if stop.is_cancelled() {
            return Ok(None);
        }
        if let Some(lock) = folder.try_lock(LOCK)? {
            return Ok(Some((lock, None)));
        }
        let holder = client
            .answering()?
            .map(|c| c.named)
            .or_else(|| paths.named());
        match holder {
            Some(named) if named.daemon => {
                if said != Some(named.pid) {
                    eprintln!(
                        "sluice daemon: the daemon {} of sluice {} holds this host; this one waits \
                         for it to end",
                        named.pid, named.version
                    );
                    said = Some(named.pid);
                }
                // Until its lock goes, or `host/version` names another.
                loop {
                    std::thread::sleep(PLACE_LOOK);
                    if stop.is_cancelled() {
                        return Ok(None);
                    }
                    if let Some(lock) = folder.try_lock(LOCK)? {
                        match folder.try_lock(START_LOCK)? {
                            Some(start) => {
                                drop(start);
                                return Ok(Some((lock, None)));
                            }
                            None => drop(lock),
                        }
                    }
                    match paths.named() {
                        Some(n) if n.daemon && n.pid == named.pid => {}
                        _ => break,
                    }
                }
            }
            _ => match handed_over(paths, folder, &client, successor, stop)? {
                Handed::Placed(placed) => return Ok(Some(placed)),
                Handed::Daemon => {}
                Handed::Stopped => return Ok(None),
            },
        }
    }
}

/// How a host process that is no daemon came to hand over to a daemon.
enum Handed {
    /// Its place is the daemon's: the host's lock and the start lock.
    Placed((File, Option<File>)),
    /// A daemon holds the host now.
    Daemon,
    /// The daemon was stopped first.
    Stopped,
}

/// D2 of `sluice-runs-as-a-daemon`: under the start lock, have each host
/// process that is no daemon and holds the host hand over, and wait for
/// its lock as long as a newer command waits ([`client::HAND_OVER_WAIT`]).
fn handed_over(
    paths: &Paths,
    folder: &Folder,
    client: &client::Client,
    successor: &Successor,
    stop: &CancellationToken,
) -> Result<Handed> {
    let start = folder.lock(START_LOCK)?;
    let until = std::time::Instant::now() + client::HAND_OVER_WAIT;
    let mut asked = std::collections::BTreeSet::new();
    let mut last = None;
    loop {
        if stop.is_cancelled() {
            return Ok(Handed::Stopped);
        }
        if let Some(lock) = folder.try_lock(LOCK)? {
            return Ok(Handed::Placed((lock, Some(start))));
        }
        // A holder already asked is waited for, with no connection made to
        // it again; one `host/version` names afresh, or none, is reached.
        let known = paths.named().map(|n| n.pid);
        if known.is_none_or(|pid| !asked.contains(&pid)) {
            if let Some(mut c) = client.answering()? {
                if c.named.daemon {
                    return Ok(Handed::Daemon);
                }
                if asked.insert(c.named.pid) {
                    let program = successor.program.as_ref().ok().cloned();
                    let _ = c.ask(&Request::HandOver {
                        program,
                        ends: true,
                    });
                }
                last = Some(c.named);
            }
        }
        if std::time::Instant::now() >= until {
            let holder = match last.or_else(|| paths.named()) {
                Some(n) => format!("sluice's host process {} (pid {})", n.version, n.pid),
                None => "sluice's host process".to_string(),
            };
            anyhow::bail!(
                "{holder} holds this host, was asked to hand over and has not ended within {} \
                 seconds; see {}",
                client::HAND_OVER_WAIT.as_secs(),
                paths.log().display()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A host process that holds the host's lock, as it starts.
struct Holder {
    paths: Paths,
    folder: Arc<Folder>,
    lock: File,
    choices: crate::telemetry::Choices,
    successor: Successor,
    daemon: bool,
    /// The start lock, held until its socket answers.
    start: Option<File>,
    /// Whether it is a daemon started afresh in place.
    afresh: bool,
    carried: Carried,
}

fn run_holding(runtime: &tokio::runtime::Runtime, holder: Holder) -> Result<i32> {
    let Holder {
        paths,
        folder,
        lock,
        choices,
        successor,
        daemon,
        start,
        afresh,
        carried,
    } = holder;
    let mut lock = Some(lock);
    take_log(&folder)?;
    let named = Named {
        version: crate::VERSION.to_string(),
        pid: std::process::id(),
        since: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        asked_only: asked_only(),
        daemon,
        program: successor.program.as_ref().ok().cloned(),
    };
    folder.write(VERSION, &serde_json::to_vec(&named)?)?;
    // Design D1 of `an-older-client-starts-no-older-host`.
    raise_newest(
        &folder,
        &Newest {
            version: named.version.clone(),
            program: named.program.clone(),
        },
    )?;
    // D6 of `sluice-host-runs-on-a-tokio-edge`: a daemon started afresh in
    // place answers on the socket it was handed, so a command that
    // connected meanwhile is answered.
    let listener = match carried.socket {
        Some(socket) => socket,
        None => bind(&paths.socket)?,
    };
    listener.set_nonblocking(true)?;
    // D6 of `sluice-runs-as-a-daemon`: its socket answers, so a command
    // that waited on the start lock its last image or its taking of the
    // host's place held reaches it.
    drop(start);
    // D20: a host process that handed over to this one is told that its
    // socket answers.
    if let Some(mut ready) = carried.ready {
        let _ = std::io::Write::write_all(&mut ready, b"ready\n");
    }
    // Record 0107 (design D19): the host's settings, held with the sign of
    // their file, which it looks at when the file's folder changes.
    let host_settings = HostSettings::read()?;
    let instance = host_settings.held().clone();
    // 151 (record 0106; rule core.53): its rounds' telemetry, as the host's
    // settings and its own choices say, started before any thread of its
    // own; a change of `[telemetry]` goes to a successor (design D20).
    let chosen = crate::telemetry::Settings::choose(
        choices,
        instance.telemetry.as_ref(),
        &crate::profiles::checks::hostname(),
        std::env::vars(),
    );
    let telemetry = match chosen.send {
        true => match crate::telemetry::Telemetry::start(&chosen) {
            Ok(t) => Some(t),
            Err(e) => {
                log(&format!("sluice host: warning: telemetry off: {e:#}"));
                None
            }
        },
        false => None,
    };
    let settings = tasks::Settings::new(instance.clone());
    let room = Shared::new(Room::over(
        crate::host_count::HostCount::host()?,
        instance.bound,
        instance.reserved,
    ));
    // Record 0108 as amended 2026-11-19: one token for each repository,
    // held for the process's life and shared by its workers and its looks.
    let tokens = crate::profiles::Tokens::default();
    // Each worker that it starts is its child, in a process group of its
    // own, recorded in `host/workers/` (`workers-end-with-their-owner@292e869158d6`).
    // A worker's end is an event at its edge.
    let (worker_ended, workers_ended) = mpsc::unbounded_channel();
    let owner = crate::workers::own(&paths.dir, move || {
        let _ = worker_ended.send(());
    })
    .context("taking the host's workers")?;
    let ending = runtime.block_on(async {
        let looked_with = settings.clone();
        let looked_holding = tokens.clone();
        let tasks = tasks::Tasks::new(
            tasks::repository_bind(
                room.clone(),
                asked_only(),
                named.clone(),
                settings.clone(),
                tokens.clone(),
            ),
            Arc::new(move |r: &Path, asks: bool| {
                index::in_play(r, looked_with.instance(), &looked_holding, asks).unwrap_or(
                    tasks::Looked {
                        in_play: true,
                        refused: None,
                        ..Default::default()
                    },
                )
            }),
        );
        let tasks = match asked_only() {
            true => tasks.passes_when_asked(),
            false => tasks,
        };
        // D13: a freed part has the repository it is owed to pass at once.
        let told = tasks.clone();
        room.on_free(move |r| told.owed(r));
        let (control, controls) = mpsc::unbounded_channel();
        let connected = TaskTracker::new();
        let hosting = Arc::new(Hosting {
            reads: reads::Reads::new(connected.clone()),
            followed: Default::default(),
            hooks: Default::default(),
            tasks,
            paths,
            named,
            room,
            connections: AtomicUsize::new(0),
            nudge: Notify::new(),
            stop: CancellationToken::new(),
            control,
            connected,
            folder,
            settings,
            tokens,
        });
        // Design D6: each repository's document is the one-shot overview's
        // read, joined with it.
        let documenting = Arc::downgrade(&hosting);
        hosting
            .tasks
            .documenting(Arc::new(move |repository, after| {
                let hosting = documenting.clone();
                Box::pin(async move {
                    let hosting = hosting.upgrade()?;
                    reads::document(&hosting, repository, after).await
                })
            }));
        log(&format!(
            "sluice {} {} started{}, pid {}{}",
            if daemon { "daemon" } else { "host" },
            hosting.named.version,
            match (&successor.program, afresh) {
                (Ok(program), true) => format!(" afresh in place as {}", program.display()),
                (Err(_), true) => " afresh in place".to_string(),
                (_, false) => String::new(),
            },
            hosting.named.pid,
            if asked_only() {
                "; it passes only when asked"
            } else {
                ""
            }
        ));
        // Before it claims a run or starts a worker: the groups that a
        // killed host process left are ended, and the workers that an
        // earlier image of this process started are its own again
        // (`nothing-detached@f807c7f5fcea`).
        let swept = tokio::task::block_in_place(|| crate::workers::sweep(&owner));
        for r in &swept.ended {
            log(&format!(
                "sluice host: ended the process group {} of {}, which a stopped host process \
                 left",
                r.pid,
                r.held.as_ref().map_or("a worker", |h| h.what.as_str())
            ));
        }
        if !swept.kept.is_empty() {
            log(&format!(
                "sluice host: it keeps the workers {:?} of its last image",
                swept.kept
            ));
        }
        if !swept.reaped.is_empty() {
            log(&format!(
                "sluice host: it reaped the children {:?} of its last image, which had ended",
                swept.reaped
            ));
        }
        // 135 (design D4): the repositories it holds, found again; a gone
        // one, or one with no sluice, leaves the index.
        let index = hosting.index();
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
        let listener = UnixListener::from_std(listener)
            .with_context(|| format!("serving {}", hosting.paths.socket.display()))?;
        let ending = serve(
            &hosting,
            listener,
            TakeUp::new(host_settings),
            &mut lock,
            &successor,
            telemetry.as_ref(),
            controls,
            workers_ended,
            &owner,
        )
        .await;
        let _ = std::fs::remove_file(&hosting.paths.socket);
        // A stop ends its workers: a run that ended so has no outcome
        // (`sluices-runs-end-with-its-host-process@e814b716f3b1`).
        let ended = tokio::task::block_in_place(|| crate::workers::stop(&owner));
        for r in &ended {
            log(&format!(
                "sluice host: ended the process group {} of {} as it stops",
                r.pid,
                r.held.as_ref().map_or("a worker", |h| h.what.as_str())
            ));
        }
        hosting.connected.close();
        let _ = tokio::time::timeout(Duration::from_secs(1), hosting.connected.wait()).await;
        log(&format!(
            "sluice {} {} ends",
            if hosting.named.daemon {
                "daemon"
            } else {
                "host"
            },
            hosting.named.pid
        ));
        ending
    });
    if let Some(t) = telemetry {
        t.finish();
    }
    ending.map(|_| 0)
}

/// The host process's socket at `path`, bound afresh: a socket a dead
/// process left is removed first, as nothing answers on it while this
/// process holds the lock.
fn bind(path: &Path) -> Result<std::os::unix::net::UnixListener> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != ErrorKind::NotFound => {
            return Err(e).with_context(|| format!("removing {}", path.display()))
        }
        _ => {}
    }
    std::os::unix::net::UnixListener::bind(path)
        .with_context(|| format!("binding {}", path.display()))
}

/// Keep the previous process's log as `log.1`, and send this one's output
/// to `log`. A log the command that started it made, which holds nothing
/// yet, is this process's own.
fn take_log(folder: &Folder) -> Result<()> {
    let held = folder
        .file(LOG, How::Read)
        .and_then(|f| f.metadata())
        .is_ok_and(|m| m.len() > 0);
    if held {
        let _ = folder.rename(LOG, PREVIOUS_LOG);
    }
    let file = folder
        .file(LOG, How::Append)
        .with_context(|| format!("opening {}", folder.path().join(LOG).display()))?;
    nix::unistd::dup2_stdout(&file).context("sending the host process's output to its log")?;
    nix::unistd::dup2_stderr(&file).context("sending the host process's output to its log")?;
    Ok(())
}

/// The host's settings as the host process holds them, and a change being
/// bound on tokio's pool for blocking work (record 0107; rule core.54;
/// design D19). It looks at the file when its folder changes, and at the
/// minute's backstop.
struct TakeUp {
    settings: HostSettings,
    /// When it looks once more after a file event: a look that met the
    /// file mid-write finds nothing, and no further event may come.
    again: Option<Instant>,
    /// A change being bound: its settings, the tables it moves, and the
    /// binding's answer.
    binding: Option<(
        Instance,
        Vec<&'static str>,
        tokio::task::JoinHandle<Result<(), String>>,
    )>,
}

/// What a look at the host's settings asks of the host process.
enum Taken {
    /// Nothing more: taken up in place, refused, or nothing changed.
    Done,
    /// A change it takes up only by handing over, bound: the tables it
    /// moves (design D20).
    HandOver(Vec<&'static str>),
}

impl TakeUp {
    fn new(settings: HostSettings) -> TakeUp {
        TakeUp {
            settings,
            again: None,
            binding: None,
        }
    }

    /// The settings file, as its log names it.
    fn file(&self) -> String {
        self.settings
            .path()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    }

    /// Never within a pass: a change found is bound on tokio's pool for
    /// blocking work against the repositories it holds, the host's systems
    /// checked. A file gone, unreadable or refused leaves it on what it
    /// holds, said once by the sign kept (design D19).
    fn look(&mut self, hosting: &Hosting) {
        if self.binding.is_some() {
            return;
        }
        match self.settings.look() {
            Look::Same => {}
            Look::Refused(why) => self.refused(&why),
            Look::Changed(instance, _text) => {
                let tables = self.settings.held().changed_tables(&instance);
                let mut repositories = hosting.tasks.held();
                if repositories.is_empty() {
                    repositories = hosting.index().list().unwrap_or_default();
                }
                let checked = instance.clone();
                let tokens = hosting.tokens.clone();
                let binding = tokio::task::spawn_blocking(move || {
                    binds(&checked, &repositories, &tokens).map_err(|e| format!("{e:#}"))
                });
                self.binding = Some((instance, tables, binding));
            }
        }
    }

    /// The binding's answer, once it comes.
    async fn bound(&mut self) -> Result<(), String> {
        let Some((_, _, binding)) = &mut self.binding else {
            return std::future::pending().await;
        };
        binding
            .await
            .unwrap_or_else(|_| Err("binding them stopped short".to_string()))
    }

    /// A change bound: held for every worker's next pass and the room's
    /// next admission, said once in its log; or handed to a successor
    /// where it moves a table no binding takes up in place (design D19,
    /// D20).
    fn landed(&mut self, hosting: &Hosting, bound: Result<(), String>) -> Taken {
        let Some((instance, tables, _)) = self.binding.take() else {
            return Taken::Done;
        };
        let afresh = tables.iter().any(|t| crate::instance::AFRESH.contains(t));
        match bound {
            // D20: the successor reads the file itself; this process keeps
            // what it holds until it ends.
            Ok(()) if afresh => return Taken::HandOver(tables),
            Err(why) if afresh => self.not_handed(&tables, "its settings do not bind", &why),
            Ok(()) => {
                hosting
                    .room
                    .lock()
                    .bounds(instance.bound, instance.reserved);
                hosting.room.changed();
                hosting.settings.take_up(instance.clone());
                self.settings.accept(instance);
                log(&format!(
                    "sluice: took up the host's settings in {}: {} changed",
                    self.file(),
                    tables.join(", ")
                ));
            }
            Err(why) => self.refused(&why),
        }
        Taken::Done
    }

    /// A change it takes up only by handing over that it could not hand
    /// over for, said once with the tables (rule core.54; design D20).
    fn not_handed(&self, tables: &[&str], cause: &str, why: &str) {
        log(&format!(
            "sluice: warning: {} changed {}, which the host process takes up only by handing \
             over, and {cause}, so it keeps the settings it holds until the file changes: {why}",
            self.file(),
            tables.join(", ")
        ));
    }

    fn refused(&self, why: &str) {
        log(&format!(
            "sluice: warning: {} is refused, so the host process keeps the settings it holds \
             until the file changes: {why}",
            self.file()
        ));
    }
}

/// Whether `instance` binds: sluice's identity on the forge where it names
/// one, and each of `repositories` bound with it, holding its tokens in
/// `tokens`, the host's systems checked against its policies (87; design
/// D19; record 0108 as amended 2026-11-19).
fn binds(
    instance: &Instance,
    repositories: &[PathBuf],
    tokens: &crate::profiles::Tokens,
) -> Result<()> {
    let retry = std::rc::Rc::new(crate::retry::Policy::round().recording_where_set());
    if let Some(forge) = &instance.forge {
        crate::profiles::github::App::open(forge, retry.clone())?;
    }
    for r in repositories {
        let bound = crate::profiles::Bound::open_holding(
            &tasks::open_dir(r),
            std::env::current_exe()?,
            instance.clone(),
            retry.clone(),
            tokens,
        )?;
        sluice_core::Sluice::new(bound.seams(), &sluice_core::SystemClock)
            .as_peer(bound.peer())
            .check_host_systems()?;
    }
    Ok(())
}

/// The start lock, once whatever holds it lets go, waited for on tokio's
/// pool for blocking work.
async fn start_lock(folder: &Arc<Folder>) -> Result<File> {
    let folder = folder.clone();
    tokio::task::spawn_blocking(move || folder.lock(START_LOCK))
        .await
        .context("waiting for the start lock")?
}

/// Send what telemetry holds, on this thread, which may block.
fn flush(telemetry: Option<&crate::telemetry::Telemetry>) {
    if let Some(t) = telemetry {
        tokio::task::block_in_place(|| t.flush());
    }
}

/// D3, D5 and D6 of `sluice-runs-as-a-daemon`, and D3 of
/// `sluice-host-owns-its-workers`: a host process starts afresh in its own
/// process as `program`, running `daemon` or `host` as it does, with the
/// environment it started with. It keeps its pid, so its workers stay its
/// children, and whatever keeps it running keeps the image started afresh
/// (`sluices-runs-end-with-its-host-process@e814b716f3b1`). Every scheduler
/// finishes its reading and lets its worker go, and its telemetry sends
/// what it holds. Then, under the start lock, it replaces its image, the
/// start lock, its socket and the host's lock each handed to the image
/// started afresh by its descriptor, so no other process takes the host's
/// lock between the two images. A command that connects meanwhile waits in
/// the socket's queue for the new image (D6 of
/// `sluice-host-runs-on-a-tokio-edge`). It returns only where the start
/// failed: its schedulers going on, and why.
async fn start_afresh(
    hosting: &Arc<Hosting>,
    listener: &UnixListener,
    lock: &mut Option<File>,
    successor: &Successor,
    telemetry: Option<&crate::telemetry::Telemetry>,
    program: &Path,
) -> String {
    let start = match start_lock(&hosting.folder).await {
        Ok(f) => f,
        Err(e) => return format!("{e:#}"),
    };
    let role = if hosting.named.daemon {
        "daemon"
    } else {
        "host"
    };
    log(&format!(
        "sluice {role}: it starts afresh in its own process, keeping its pid"
    ));
    hosting.tasks.stop_all().await;
    flush(telemetry);
    let socket = listener.as_raw_fd();
    let inherit = |fd: RawFd, flag: nix::fcntl::FdFlag| {
        // SAFETY: both descriptors are this process's own, open for its
        // life.
        let held = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
        nix::fcntl::fcntl(held, nix::fcntl::FcntlArg::F_SETFD(flag))
    };
    let held = lock.as_ref().map(|l| l.as_raw_fd());
    let handed = || -> Result<std::process::Command> {
        inherit(start.as_raw_fd(), nix::fcntl::FdFlag::empty())?;
        inherit(socket, nix::fcntl::FdFlag::empty())?;
        if let Some(fd) = held {
            inherit(fd, nix::fcntl::FdFlag::empty())?;
        }
        let mut command = std::process::Command::new(program);
        command
            .arg(role)
            .env_clear()
            .envs(
                successor
                    .env
                    .iter()
                    .filter(|(name, _)| {
                        ![START_LOCK_FD_VAR, SOCKET_FD_VAR, LOCK_FD_VAR]
                            .iter()
                            .any(|var| *name == *var)
                    })
                    .cloned(),
            )
            .env(START_LOCK_FD_VAR, start.as_raw_fd().to_string())
            .env(SOCKET_FD_VAR, socket.to_string());
        if let Some(fd) = held {
            command.env(LOCK_FD_VAR, fd.to_string());
        }
        Ok(command)
    };
    let why = match handed() {
        Ok(command) => format!("{:#}", crate::hand_over::replace(&command)),
        Err(e) => format!("{e:#}"),
    };
    // The start failed: the process as it was, its socket and its lock
    // still its own.
    let _ = inherit(start.as_raw_fd(), nix::fcntl::FdFlag::FD_CLOEXEC);
    let _ = inherit(socket, nix::fcntl::FdFlag::FD_CLOEXEC);
    if let Some(fd) = held {
        let _ = inherit(fd, nix::fcntl::FdFlag::FD_CLOEXEC);
    }
    if lock.is_none() {
        match hosting.folder.try_lock(LOCK) {
            Ok(Some(f)) => *lock = Some(f),
            Ok(None) => log(&format!("sluice {role}: another holds the host's lock now")),
            Err(e) => log(&format!(
                "sluice {role}: taking the host's lock again: {e:#}"
            )),
        }
    }
    hosting.tasks.resume();
    scan(hosting);
    drop(start);
    why
}

/// The edge's own wakes, by what woke it: the measure of the claim that
/// the host process waits on events, not on a poll.
#[derive(Debug, Default)]
pub struct EdgeWakes {
    /// The minute's backstop, or the room's next lapse.
    pub timers: std::sync::atomic::AtomicU64,
    /// A connection, a signal, a file event, a binding's answer or a
    /// nudge.
    pub events: std::sync::atomic::AtomicU64,
}

/// Accept connections, each answered on a task of its own, and take file
/// events, signals and the room's changes, until the process should end
/// or hands over (design D6, D8, D19, D20; the change
/// `sluice-host-runs-on-a-tokio-edge`, design D1 to D6). It waits on each
/// of these at once: nothing sleeps and looks again.
async fn serve(
    hosting: &Arc<Hosting>,
    mut listener: UnixListener,
    mut take_up: TakeUp,
    lock: &mut Option<File>,
    successor: &Successor,
    telemetry: Option<&crate::telemetry::Telemetry>,
    mut controls: mpsc::UnboundedReceiver<Control>,
    mut workers_ended: mpsc::UnboundedReceiver<()>,
    owner: &crate::workers::Owner,
) -> Result<Ending> {
    let (mut watch, mut seen) = match watch::Watch::new() {
        Ok((w, bursts)) => (Some(w), Some(bursts)),
        Err(e) => {
            log(&format!(
                "sluice host: watching files: {e:#}; the minute's backstop reads each repository"
            ));
            (None, None)
        }
    };
    let registry = crate::host_count::HostCount::host()?;
    if let Some(w) = &mut watch {
        w.state(&hosting.paths.state, &hosting.paths.dir);
        w.settings(take_up.settings.path());
    }
    // 135 (design D4): each repository in its index is watched and looked
    // at as it starts.
    scan_with(hosting, watch.as_mut());
    watch_runs(&registry, watch.as_mut());
    // What the streams' starts may have missed, the scan above read.
    if let Some(w) = &mut watch {
        w.missed();
    }
    // When a stream the system would not start is started again.
    let mut revive_at: Option<Instant> = None;
    // 135 (design D6): the folder held tells of its own removal or move.
    let mut moves = hosting.folder.moves().unwrap_or_else(|e| {
        log(&format!(
            "sluice host: watching its folder: {e}; its file events and the minute's backstop read it"
        ));
        None
    });
    let mut signalled = Box::pin(signalled_to_stop());
    let mut activity = hosting.tasks.activity();
    let mut room = hosting.room.subscribe();
    let mut backstop = tokio::time::interval_at(Instant::now() + IDLE_ASK, IDLE_ASK);
    backstop.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // 154 (design D8 of `each-submission-shows-its-wait-and-its-test`): the
    // rows of the room last sent, and each repository's name as its
    // binding names it, read once.
    let mut sent = std::collections::BTreeSet::new();
    let mut names = std::collections::BTreeMap::new();
    let far = Instant::now() + Duration::from_secs(86_400 * 365);
    // When a host process that is no daemon, idle since, ends.
    let mut ends_at: Option<Instant> = None;
    loop {
        // D6: a host process that is no daemon ends once it has been idle
        // for [`IDLE_LINGER`], decided under the start lock after taking
        // every connection already queued. Where another holds the start
        // lock, the holder connects to it, and its connection's end has it
        // look again.
        let idle_now = !hosting.named.daemon && idle(hosting);
        ends_at = match (idle_now, ends_at) {
            (false, _) => None,
            (true, None) => Some(Instant::now() + IDLE_LINGER),
            (true, at) => at,
        };
        if ends_at.is_some_and(|at| at <= Instant::now()) {
            if let Ok(Some(start)) = hosting.folder.try_lock(START_LOCK) {
                if accept_queued(hosting, &listener) == 0 {
                    let _ = std::fs::remove_file(&hosting.paths.socket);
                    // The start lock goes with the process.
                    std::mem::forget(start);
                    return Ok(Ending::Ended);
                }
                drop(start);
            }
        }
        if let Some(w) = watch.as_mut() {
            let missed = w.missed();
            if !missed.is_empty() {
                if let Some(ending) = act(hosting, &mut take_up, w, &registry, missed).await {
                    return ending;
                }
            }
            revive_at = match (w.stalled(), revive_at) {
                (false, _) => None,
                (true, None) => Some(Instant::now() + watch::STALLED_FOR),
                (true, at) => at,
            };
        }
        let lapse = hosting.room.lock().next_lapse();
        let again = take_up.again;
        let ending = ends_at.filter(|at| *at > Instant::now());
        tokio::select! {
            () = tokio::time::sleep_until(ending.unwrap_or(far)), if ending.is_some() => {
                count(&EDGE.timers);
            }
            accepted = listener.accept() => {
                count(&EDGE.events);
                match accepted {
                    Ok((stream, _)) => connect(hosting, stream),
                    Err(e) => log(&format!("sluice host: accepting: {e}")),
                }
            }
            () = &mut signalled => {
                count(&EDGE.events);
                log(&format!(
                    "sluice {}: signalled to stop; it ends",
                    if hosting.named.daemon { "daemon" } else { "host" }
                ));
                return end(hosting).await;
            }
            control = controls.recv() => {
                count(&EDGE.events);
                let (program, by) = match control {
                    Some(Control::HandOver { by, ends: true, .. }) => {
                        // A daemon takes the host's place: this process
                        // ends, and its runs end with it.
                        log(&format!(
                            "sluice {}: sluice {by} takes the host's place; it ends",
                            if hosting.named.daemon { "daemon" } else { "host" }
                        ));
                        return end(hosting).await;
                    }
                    Some(Control::HandOver { program, by, .. }) => (program, by),
                    Some(Control::Noted(repository)) => {
                        // A repository the watch takes in now is read once
                        // more, for a change made before its watch began.
                        if watch.as_mut().is_some_and(|w| w.repository(&repository)) {
                            hosting.tasks.event(&repository);
                        }
                        continue;
                    }
                    None => continue,
                };
                // D3 of `sluice-runs-as-a-daemon` and of
                // `sluice-host-owns-its-workers`: a host process that a newer
                // command asked to hand over starts afresh as the executable
                // the asker named, or as its own, and keeps its workers;
                // where it cannot, it goes on as it was, said once.
                let role = if hosting.named.daemon { "daemon" } else { "host" };
                log(&format!(
                    "sluice {role}: sluice {by} asked this one to hand over; it starts afresh in \
                     place"
                ));
                let program = match (program, &successor.program) {
                    (Some(p), _) => Ok(p),
                    (None, Ok(p)) => Ok(p.clone()),
                    (None, Err(why)) => Err(why.clone()),
                };
                let (shown, why) = match &program {
                    Ok(p) => (
                        p.display().to_string(),
                        start_afresh(hosting, &listener, lock, successor, telemetry, p).await,
                    ),
                    Err(why) => ("its own executable".to_string(), why.clone()),
                };
                log(&format!(
                    "sluice {role}: warning: sluice {by} asked it to hand over, and it could not \
                     start afresh as {shown}, so it goes on as it was: {why}"
                ));
            }
            burst = async { seen.as_mut()?.next().await }, if seen.is_some() => {
                count(&EDGE.events);
                let Some(burst) = burst else {
                    seen = None;
                    continue;
                };
                if let Some(ending) = take_events(
                    hosting, &mut take_up, watch.as_mut(), &registry, burst,
                ).await {
                    return ending;
                }
            }
            bound = take_up.bound() => {
                count(&EDGE.events);
                if let Some(ending) = taken(
                    hosting, &mut listener, &mut take_up, lock, successor, telemetry, bound,
                ).await {
                    return Ok(ending);
                }
            }
            Some(()) = async {
                match moves.as_mut() {
                    Some(m) => m.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                count(&EDGE.events);
                if hosting.folder_gone() {
                    log("sluice host: its state folder is gone; it ends");
                    return end(hosting).await;
                }
            }
            () = tokio::time::sleep_until(revive_at.unwrap_or(far)), if revive_at.is_some() => {
                count(&EDGE.timers);
                revive_at = None;
                if let Some(w) = watch.as_mut() {
                    w.revive();
                }
            }
            () = tokio::time::sleep_until(again.unwrap_or(far)), if again.is_some() => {
                count(&EDGE.timers);
                take_up.again = None;
                take_up.look(hosting);
            }
            () = hosting.nudge.notified() => count(&EDGE.events),
            // A worker ended: a run's part of the bound is free whatever
            // way its run ended.
            Some(()) = workers_ended.recv() => {
                count(&EDGE.events);
                runs_changed(hosting, &registry, watch.as_mut());
            }
            _ = activity.changed() => count(&EDGE.events),
            _ = room.changed() => {
                count(&EDGE.events);
                watch_runs(&registry, watch.as_mut());
                send_room(hosting, telemetry, &mut sent, &mut names);
            }
            () = tokio::time::sleep_until(lapse.unwrap_or(far)), if lapse.is_some() => {
                count(&EDGE.timers);
                hosting.room.changed();
            }
            _ = backstop.tick() => {
                count(&EDGE.timers);
                // The minute's backstop: what no event reported.
                if hosting.folder_gone() {
                    log("sluice host: its state folder is gone; it ends");
                    return end(hosting).await;
                }
                take_up.look(hosting);
                scan_with(hosting, watch.as_mut());
                runs_changed(hosting, &registry, watch.as_mut());
                let dir = owner.dir.clone();
                let _ = tokio::task::spawn_blocking(move || crate::workers::prune(&dir)).await;
                send_room(hosting, telemetry, &mut sent, &mut names);
            }
        }
    }
}

/// The edge's wakes in this process.
pub static EDGE: EdgeWakes = EdgeWakes {
    timers: std::sync::atomic::AtomicU64::new(0),
    events: std::sync::atomic::AtomicU64::new(0),
};

fn count(n: &std::sync::atomic::AtomicU64) {
    n.fetch_add(1, Ordering::Relaxed);
}

/// Whether nothing keeps a host process that is no daemon: no scheduler
/// busy, no connection open, no part held or run waiting (D6), and no
/// worker of its own running, which would end with it.
fn idle(hosting: &Hosting) -> bool {
    crate::workers::children() == 0
        && !hosting.tasks.any()
        && hosting.connections.load(Ordering::SeqCst) == 0
        && !hosting.room.lock().busy(Instant::now())
}

/// The process ends: every connection ends, and every scheduler finishes
/// its reading and lets its worker go, as a hand-over ends it.
async fn end(hosting: &Arc<Hosting>) -> Result<Ending> {
    hosting.stop.cancel();
    hosting.tasks.stop_all().await;
    Ok(Ending::Ended)
}

/// A burst of file events: the state folder, the index, the settings, the
/// runs and each repository's refs and records. `Some` where the process
/// ends.
async fn take_events(
    hosting: &Arc<Hosting>,
    take_up: &mut TakeUp,
    watch: Option<&mut watch::Watch>,
    registry: &crate::host_count::HostCount,
    burst: notify_debouncer_full::DebounceEventResult,
) -> Option<Result<Ending>> {
    let events = match burst {
        Ok(events) => events,
        Err(errors) => {
            for e in errors {
                log(&format!("sluice host: watching files: {e}"));
            }
            return None;
        }
    };
    let w = watch?;
    let seen = w.seen(
        events
            .iter()
            .flat_map(|e| e.paths.iter().map(PathBuf::as_path)),
    );
    act(hosting, take_up, w, registry, seen).await
}

/// What a burst of events touched, or what the watch may have missed,
/// acted on. `Some` where the process ends.
async fn act(
    hosting: &Arc<Hosting>,
    take_up: &mut TakeUp,
    w: &mut watch::Watch,
    registry: &crate::host_count::HostCount,
    seen: watch::Seen,
) -> Option<Result<Ending>> {
    if seen.state {
        // 135: a state folder removed ends it, its socket with it, though
        // another made a folder at its path again since; it reads no
        // repository after, so nothing of it writes there again.
        if hosting.folder_gone() {
            log("sluice host: its state folder is gone; it ends");
            return Some(end(hosting).await);
        }
        scan_with(hosting, Some(w));
    }
    if seen.settings {
        w.settings(take_up.settings.path());
        take_up.look(hosting);
        take_up.again = Some(Instant::now() + Duration::from_secs(1));
    }
    if seen.runs {
        runs_changed(hosting, registry, Some(w));
    }
    for repository in &seen.repositories {
        let _ = w.repository(repository);
        hosting.tasks.event(repository);
    }
    None
}

/// The answer of a change of the settings being bound. `Some` where the
/// process handed over.
#[allow(clippy::too_many_arguments)]
async fn taken(
    hosting: &Arc<Hosting>,
    listener: &mut UnixListener,
    take_up: &mut TakeUp,
    lock: &mut Option<File>,
    successor: &Successor,
    telemetry: Option<&crate::telemetry::Telemetry>,
    bound: Result<(), String>,
) -> Option<Ending> {
    let Taken::HandOver(tables) = take_up.landed(hosting, bound) else {
        return None;
    };
    // D5 of `sluice-runs-as-a-daemon` and D3 of
    // `sluice-host-owns-its-workers`: a host process starts afresh in place
    // as the executable it started as, and keeps its workers.
    log(&format!(
        "sluice: {} changed {}, which the host process takes up by starting afresh in place",
        take_up.file(),
        tables.join(", ")
    ));
    let why = match &successor.program {
        Ok(program) => start_afresh(hosting, listener, lock, successor, telemetry, program).await,
        Err(why) => why.clone(),
    };
    take_up.not_handed(&tables, "it could not start afresh", &why);
    None
}

/// A run ended or began, or the registry changed: the run that ended frees
/// its part for the repository it is owed to (D13), each run the registry
/// counts now is watched, and the room says it changed.
fn runs_changed(
    hosting: &Hosting,
    registry: &crate::host_count::HostCount,
    watch: Option<&mut watch::Watch>,
) {
    let owed = hosting.room.lock().freed(Instant::now()).ok().flatten();
    if let Some(r) = owed {
        hosting.tasks.owed(&r);
    }
    watch_runs(registry, watch);
    hosting.room.changed();
}

/// Watch the registry, and the folder of each run it counts now.
fn watch_runs(registry: &crate::host_count::HostCount, watch: Option<&mut watch::Watch>) {
    let Some(w) = watch else {
        return;
    };
    let dirs = registry.run_dirs().unwrap_or_default();
    w.runs(registry.dir(), &dirs);
}

/// 135 (design D4): [`scan_with`], with no watch to keep.
fn scan(hosting: &Hosting) {
    scan_with(hosting, None);
}

/// 135 (design D4): each repository in the index whose directory is gone
/// leaves it, and is watched no longer; each other is watched, and looked
/// at where nothing holds it and its remote is due to be asked, so work
/// another host wrote to its records is found and an idle repository
/// costs that read alone (129). Each read runs on tokio's pool for
/// blocking work, so the edge never waits on a remote.
fn scan_with(hosting: &Hosting, watch: Option<&mut watch::Watch>) {
    let index = hosting.index();
    match index.prune(crate::profiles::repository_gone) {
        Ok(gone) => {
            for r in gone {
                log(&format!("{}: gone; out of the index", r.display()));
                hosting.tasks.forget(&r);
            }
        }
        Err(e) => log(&format!("sluice host: reading the index: {e:#}")),
    }
    let repositories = index.list().unwrap_or_default();
    if let Some(w) = watch {
        w.repositories(&repositories);
    }
    for repository in repositories {
        hosting.tasks.look(&repository);
    }
}

/// Take every connection queued now, each answered on a task of its own:
/// how many.
fn accept_queued(hosting: &Arc<Hosting>, listener: &UnixListener) -> usize {
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    let mut n = 0;
    while let std::task::Poll::Ready(Ok((stream, _))) = listener.poll_accept(&mut cx) {
        connect(hosting, stream);
        n += 1;
    }
    n
}

/// Answer `stream` on a task of its own, counted while it is open.
fn connect(hosting: &Arc<Hosting>, stream: UnixStream) {
    hosting.connections.fetch_add(1, Ordering::SeqCst);
    let held = hosting.clone();
    hosting.connected.spawn(async move {
        if let Err(e) = answer(&held, stream).await {
            log(&format!("sluice host: a connection: {e:#}"));
        }
        held.connections.fetch_sub(1, Ordering::SeqCst);
        held.nudge.notify_one();
    });
}

/// 154, 71 (design D8 of `each-submission-shows-its-wait-and-its-test`;
/// rule core.66): at each scan the room's waits are drained, whether or
/// not anything is sent, so they never gather; and while `telemetry`
/// sends metrics, the room is measured by repository and branch, each
/// repository named once as its binding names it, `names` keeping it.
fn send_room(
    hosting: &Hosting,
    telemetry: Option<&crate::telemetry::Telemetry>,
    sent: &mut std::collections::BTreeSet<(PathBuf, String)>,
    names: &mut std::collections::BTreeMap<PathBuf, Option<String>>,
) {
    let (running, bound, waits) = {
        let mut room = hosting.room.lock();
        let running = room.running_by_branch().unwrap_or_else(|e| {
            log(&format!("sluice host: counting the room by branch: {e:#}"));
            Default::default()
        });
        (running, room.bound(), room.waits())
    };
    if !telemetry.is_some_and(|t| t.sends_metrics()) {
        return;
    }
    let repositories: Vec<PathBuf> = running
        .keys()
        .chain(sent.iter())
        .map(|(r, _)| r.clone())
        .chain(waits.iter().map(|w| w.repository.clone()))
        .collect();
    for r in repositories {
        names
            .entry(r.clone())
            .or_insert_with(|| name_of(hosting, &r));
    }
    measure_room(
        &running,
        bound,
        &waits,
        &|r| names.get(r).cloned().flatten(),
        sent,
    );
}

/// The name `repository`'s binding gives it in its metrics (rule core.66);
/// `None` where it cannot be opened, said in the host process's log.
fn name_of(hosting: &Hosting, repository: &Path) -> Option<String> {
    let bound = crate::profiles::Bound::open_holding(
        &tasks::open_dir(repository),
        std::env::current_exe().ok()?,
        hosting.settings.instance(),
        std::rc::Rc::new(crate::retry::Policy::round()),
        &hosting.tokens,
    );
    match bound {
        Ok(b) => Some(b.peer().repository),
        Err(e) => {
            log(&format!(
                "sluice host: naming {} for its metrics: {e:#}",
                repository.display()
            ));
            None
        }
    }
}

/// 154, 71 (design D8 of `each-submission-shows-its-wait-and-its-test`;
/// rule core.66): for each repository and branch with runs counted now
/// (`running`) or at the last sending (`sent`), `sluice.host.running`, its
/// runs, and `sluice.host.bound`, the host's bound, a row whose runs ended
/// reading 0 once; and for each of `waits` that names a branch,
/// `sluice.run.waited_for_room` in seconds. Each carries the repository as
/// `name` gives it, and the branch, and nothing else; a repository `name`
/// gives none is passed by. `sent` becomes the rows with runs now.
pub(crate) fn measure_room(
    running: &std::collections::BTreeMap<(PathBuf, String), usize>,
    bound: usize,
    waits: &[room::Wait],
    name: &dyn Fn(&Path) -> Option<String>,
    sent: &mut std::collections::BTreeSet<(PathBuf, String)>,
) {
    let rows: std::collections::BTreeSet<&(PathBuf, String)> =
        running.keys().chain(sent.iter()).collect();
    for row in rows {
        let Some(repository) = name(&row.0) else {
            continue;
        };
        let (repository, branch) = (repository.as_str(), row.1.as_str());
        let runs = running.get(row).copied().unwrap_or(0);
        tracing::info!(
            gauge.sluice.host.running = runs as u64,
            sluice.repository = repository,
            sluice.branch = branch,
        );
        tracing::info!(
            gauge.sluice.host.bound = bound as u64,
            sluice.repository = repository,
            sluice.branch = branch,
        );
    }
    for wait in waits {
        let (Some(branch), Some(repository)) = (&wait.branch, name(&wait.repository)) else {
            continue;
        };
        tracing::info!(
            histogram.sluice.run.waited_for_room = wait.waited.as_secs_f64(),
            sluice.repository = repository.as_str(),
            sluice.branch = branch.as_str(),
        );
    }
    *sent = running
        .iter()
        .filter(|(_, n)| **n > 0)
        .map(|(row, _)| row.clone())
        .collect();
}

/// A connection's lines as it reads them.
type Lines = FramedRead<tokio::net::unix::OwnedReadHalf, LinesCodec>;

/// Send `value` on `to` as one line of JSON.
async fn send(
    to: &mut tokio::net::unix::OwnedWriteHalf,
    value: &impl serde::Serialize,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    to.write_all(&line).await?;
    to.flush().await?;
    Ok(())
}

/// The next line of `lines`, within [`ASK_WAIT`]; `None` where the other
/// end closed the connection.
async fn next_line(lines: &mut Lines) -> Result<Option<String>> {
    use tokio_stream::StreamExt;
    match tokio::time::timeout(ASK_WAIT, lines.next()).await {
        Err(_) => anyhow::bail!("no line within {} seconds", ASK_WAIT.as_secs()),
        Ok(None) => Ok(None),
        Ok(Some(line)) => Ok(Some(line.context("reading a line")?)),
    }
}

/// Once the other end of `lines` closed the connection: a read that finds
/// its end or fails. A line it sends meanwhile is passed by.
async fn closed(lines: &mut Lines) {
    use tokio_stream::StreamExt;
    while let Some(Ok(_)) = lines.next().await {}
}

/// Answer one connection: its hello, then its one request.
async fn answer(hosting: &Arc<Hosting>, stream: UnixStream) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = FramedRead::new(reader, LinesCodec::new_with_max_length(LINE_MAX));
    let Some(line) = next_line(&mut lines).await? else {
        anyhow::bail!("the connection closed before its hello");
    };
    let hello: Hello = serde_json::from_str(line.trim_end())
        .with_context(|| format!("reading {:?}", line.trim_end()))?;
    send(&mut writer, &hosting.named).await?;
    let Some(line) = next_line(&mut lines).await? else {
        return Ok(());
    };
    let request: Request = match serde_json::from_str(line.trim_end()) {
        Ok(r) => r,
        Err(_) => {
            // The change `sluice-host-answers-every-read`, design D9: it
            // answers only the requests of its own release, and names the
            // sluice to run.
            let words = format!(
                "sluice {} asked what this host process, sluice {}, does not know: {}. Run sluice {}.",
                hello.hello,
                hosting.named.version,
                line.trim_end(),
                hosting.named.version
            );
            return send(&mut writer, &Answer::Error(words)).await;
        }
    };
    // The change `the-git-hook-asks-sluice-host`, design D1: the hook's
    // transaction is judged before any read or change, from the state the
    // host keeps, with no wait for a round or a reading.
    if let Request::ReferenceTransaction {
        folder,
        git_dir,
        by,
        lines,
    } = request
    {
        let output = transaction::answer(hosting, folder, git_dir, by, lines).await;
        return send(&mut writer, &Answer::Output(output)).await;
    }
    // Design D4: `sluice host` puts back the hook in each repository it
    // binds for a request, by its own path.
    protection::put(hosting, &request).await;
    if reads::is_read(&request) {
        let output = reads::answer(hosting, request).await;
        return send(&mut writer, &Answer::Output(output)).await;
    }
    // The change `sluice-host-runs-a-sessions-checks`, design D1: a
    // session's command runs here, its output sent as it comes.
    if commands::is_command(&request) {
        return commands::answer(hosting, request, &mut lines, &mut writer).await;
    }
    // The change `sluice-host-makes-each-change`, design D1: `sluice host`
    // makes each change.
    if changes::is_change(&request) {
        let answer = changes::answer(hosting, request).await;
        return send(&mut writer, &answer).await;
    }
    match request {
        // Answered above.
        Request::Status { .. }
        | Request::Overview { .. }
        | Request::Home { .. }
        | Request::Forecast { .. }
        | Request::Inbox { .. }
        | Request::Exec { .. }
        | Request::Preview { .. }
        | Request::Equip { .. }
        | Request::ReleaseWorktree { .. }
        | Request::Previews { .. }
        | Request::Submit { .. }
        | Request::Amend { .. }
        | Request::Withdraw { .. }
        | Request::Confirm { .. }
        | Request::Release { .. }
        | Request::Branch { .. }
        | Request::Protect { .. }
        | Request::ReferenceTransaction { .. } => Ok(()),
        Request::Wait {
            folder,
            caller,
            wait,
        } => {
            let answer = reads::wait(hosting, folder, caller, wait).await;
            send(&mut writer, &answer).await
        }
        Request::Subscribe { folders } => subscribe(hosting, folders, lines, writer).await,
        Request::Serve { repository } => {
            note(hosting, &repository);
            // D16: no reading begun before the change is current.
            hosting.tasks.told(&repository);
            // Where passes are taken only when asked, the repository is
            // held for the next ask; otherwise it is passed at once.
            let task = hosting.tasks.handle(&repository);
            if !asked_only() {
                task.wake();
            }
            send(&mut writer, &Answer::Ok).await
        }
        Request::Pass { folder, at } => {
            // A follower names its folder (the change
            // `sluice-host-makes-each-change`, design D5).
            let repository = match reads::resolved(hosting, &folder).await {
                Ok(r) => r,
                Err(words) => return send(&mut writer, &Answer::Error(words)).await,
            };
            note(hosting, &repository);
            let at = at.and_then(|at| at.parse::<chrono::DateTime<chrono::Utc>>().ok());
            let answer = match hosting.tasks.handle(&repository).pass(at).await {
                Ok(report) => Answer::Pass(report),
                Err(e) => Answer::Error(e),
            };
            send(&mut writer, &answer).await
        }
        Request::Step { folder, branch } => {
            // `sluice step` names its folder (design D5).
            let repository = match reads::resolved(hosting, &folder).await {
                Ok(r) => r,
                Err(words) => return send(&mut writer, &Answer::Error(words)).await,
            };
            note(hosting, &repository);
            let answer = match hosting.tasks.handle(&repository).step(&branch).await {
                Ok(doc) => Answer::Step(doc),
                Err(e) => Answer::Error(e),
            };
            send(&mut writer, &answer).await
        }
        Request::Results {
            repository,
            branch,
            wants,
        } => {
            let held = hosting.clone();
            let results = tokio::task::spawn_blocking(move || {
                results_of(&held, &repository, &branch, &wants)
            })
            .await
            .context("reading the results")?;
            let answer = match results {
                Ok(lines) => Answer::Results(String::from_utf8_lossy(&lines).into_owned()),
                Err(e) => Answer::Error(format!("{e:#}")),
            };
            send(&mut writer, &answer).await
        }
        // D3 of `sluice-runs-as-a-daemon` and of
        // `sluice-host-owns-its-workers`: the host process starts afresh in
        // place, or ends where the asker takes its place.
        Request::HandOver { program, ends } => {
            // Design D3 of `an-older-client-starts-no-older-host`: it hands
            // over to no sluice older than the newest that has run.
            if let Some(why) =
                refused_hand_over(&hosting.folder, &hosting.named.version, &hello.hello)
            {
                log(&format!(
                    "sluice {}: warning: {why}",
                    if hosting.named.daemon {
                        "daemon"
                    } else {
                        "host"
                    }
                ));
                return send(&mut writer, &Answer::Error(why)).await;
            }
            let _ = hosting.control.send(Control::HandOver {
                program,
                by: hello.hello.clone(),
                ends,
            });
            send(&mut writer, &Answer::Ok).await
        }
    }
}

/// sluice's part in a subscription (the change
/// `sluice-host-answers-every-read`, design D6): a folder resolves to its
/// repository's document channel, a follower of the kind its `add` line
/// names, and, for `sluice run`, the follower's reports.
struct Subscribed(Arc<Hosting>);

impl subscription::Serves for Subscribed {
    type Kind = FollowKind;
    type Document = serde_json::Value;
    type Report = pass::Report;
    type Guard = tasks::Follower;

    async fn serve(
        &self,
        folder: PathBuf,
        kind: FollowKind,
    ) -> Result<
        subscription::Served<serde_json::Value, pass::Report, tasks::Follower>,
        serde_json::Value,
    > {
        let hosting = &self.0;
        let resolved = reads::resolved(hosting, &folder).await;
        let gone = |words: &str| {
            let mut doc = crate::told::document(crate::told::Kind::Internal, words, &[]);
            doc["gone"] = true.into();
            doc
        };
        let repository = resolved.map_err(|words| gone(&words))?;
        note(hosting, &repository);
        // The channel first, so the first reading's document has a
        // receiver to go to.
        let documents = hosting
            .tasks
            .documents(&repository)
            .ok_or_else(|| gone("sluice's host process stops"))?;
        // `sluice run` and a wait take each pass's report; a watch takes
        // none.
        let reports_kept = kind != FollowKind::Watch;
        let mut follower = hosting
            .tasks
            .handle(&repository)
            .following(kind, reports_kept);
        let reports = reports_kept.then(|| follower.reports());
        // Where passes are taken only when asked, the follower's first
        // document is of a reading afresh, which its coming asks for: the
        // repository is read before its first document is sent (D14).
        if asked_only() && kind != FollowKind::Run {
            let tasks = hosting.tasks.clone();
            let read = repository.clone();
            hosting.connected.spawn(async move {
                tasks.read(&read, READ_TICK, read_for(), true).await;
            });
        }
        Ok(subscription::Served {
            documents,
            reports,
            guard: follower,
        })
    }

    fn ends(&self, document: &serde_json::Value) -> bool {
        crate::overview::no_sluice(document) || document["gone"] == true
    }
}

/// Serve a subscription on the connection (design D6), until the client
/// closes it or the host process stops.
async fn subscribe(
    hosting: &Arc<Hosting>,
    folders: Vec<protocol::Added>,
    lines: Lines,
    writer: tokio::net::unix::OwnedWriteHalf,
) -> Result<()> {
    let sink =
        tokio_util::codec::FramedWrite::new(writer, LinesCodec::new_with_max_length(LINE_MAX));
    let folders = folders.into_iter().map(|a| (a.folder, a.kind)).collect();
    subscription::serve(
        &Subscribed(hosting.clone()),
        folders,
        lines,
        sink,
        hosting.stop.child_token(),
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))
}

/// The results `ask` names of `repository`, as `branch`'s gate reads them,
/// from the host process's index, on the connection's own thread and with
/// no wait on the repository's task: its binding reads every remote as the
/// last ask fetched it (design D17).
fn results_of(
    hosting: &Hosting,
    repository: &Path,
    branch: &str,
    ask: &sluice_seams::ResultAsk,
) -> Result<Vec<u8>> {
    let bound = crate::profiles::Bound::open_holding(
        &tasks::open_dir(repository),
        std::env::current_exe()?,
        hosting.settings.instance(),
        std::rc::Rc::new(crate::retry::Policy::round().recording_where_set()),
        &hosting.tokens,
    )?;
    let state = bound
        .state
        .reading_results(crate::profiles::records::ResultsFrom::Indexed);
    sluice_seams::State::hold(&state);
    sluice_seams::State::results_for(&state, branch, ask)
}

/// Note `repository` in the index (design D4).
pub(crate) fn note(hosting: &Hosting, repository: &Path) {
    // The edge watches it at once, where it does not yet: a change made
    // before the index's own event would come is read all the same.
    let _ = hosting
        .control
        .send(Control::Noted(repository.to_path_buf()));
    if let Err(e) = hosting.index().note(repository) {
        log(&format!(
            "sluice host: noting {} in the index: {e:#}",
            repository.display()
        ));
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tracing::field::{Field, Visit};
    use tracing_subscriber::layer::SubscriberExt;

    /// Each metric event, as `<metric> <value>` and its other fields after
    /// it as `<field>=<value>`.
    #[derive(Clone, Default)]
    struct Seen(Arc<Mutex<Vec<String>>>);

    struct Line(Vec<String>);

    impl Visit for Line {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            let name = field.name();
            match ["gauge.", "histogram.", "monotonic_counter."]
                .iter()
                .any(|p| name.starts_with(p))
            {
                true => self.0.insert(0, format!("{name} {value:?}")),
                false => self.0.push(format!("{name}={value:?}")),
            }
        }

        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.push(format!("{}={value}", field.name()));
        }
    }

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Seen {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _: tracing_subscriber::layer::Context<'_, S>,
        ) {
            let mut line = Line(Vec::new());
            event.record(&mut line);
            self.0.lock().unwrap().push(line.0.join(" "));
        }
    }

    /// 154, 71, design D8 of `each-submission-shows-its-wait-and-its-test`
    /// (rule core.66): two runs for `main` under a bound of 4, with a wait
    /// of 20 s drained, are sent as such; then none, as 0 once; then nothing
    /// for it; each with the repository and the branch.
    #[test]
    fn the_host_s_room_is_measured_by_branch() {
        let seen = Seen::default();
        let repo = PathBuf::from("/host/repo.git");
        let name = |r: &Path| (r == Path::new("/host/repo.git")).then(|| "acme/app".to_string());
        let main = (repo.clone(), "main".to_string());
        let mut sent = std::collections::BTreeSet::new();
        let sending =
            |running: std::collections::BTreeMap<(PathBuf, String), usize>,
             waits: Vec<room::Wait>,
             sent: &mut std::collections::BTreeSet<(PathBuf, String)>| {
                let at = seen.0.lock().unwrap().len();
                let subscriber = tracing_subscriber::registry().with(seen.clone());
                tracing::subscriber::with_default(subscriber, || {
                    measure_room(&running, 4, &waits, &name, sent)
                });
                seen.0.lock().unwrap()[at..].to_vec()
            };
        let named = "sluice.repository=acme/app sluice.branch=main";
        let first = sending(
            std::collections::BTreeMap::from([(main.clone(), 2)]),
            vec![room::Wait {
                repository: repo.clone(),
                branch: Some("main".into()),
                waited: Duration::from_secs(20),
            }],
            &mut sent,
        );
        assert_eq!(
            first,
            [
                format!("gauge.sluice.host.running 2 {named}"),
                format!("gauge.sluice.host.bound 4 {named}"),
                format!("histogram.sluice.run.waited_for_room 20.0 {named}"),
            ]
        );
        let second = sending(Default::default(), Vec::new(), &mut sent);
        assert_eq!(
            second,
            [
                format!("gauge.sluice.host.running 0 {named}"),
                format!("gauge.sluice.host.bound 4 {named}"),
            ]
        );
        let third = sending(Default::default(), Vec::new(), &mut sent);
        assert!(third.is_empty(), "{third:?}");
        for line in first.iter().chain(&second) {
            for field in line.split(' ').skip(2) {
                let key = field.split_once('=').map_or(field, |(k, _)| k);
                assert!(sluice_core::METRIC_KEYS.contains(&key), "{key} in {line}");
            }
        }
    }
}
