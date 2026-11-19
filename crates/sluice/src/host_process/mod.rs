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
//!
//! `sluice daemon` is the same process with its idle end off, on a machine
//! that chooses to keep it running (137; record 0093 as amended 2026-11-08
//! and 2026-11-18). It takes the host's place from a host process that ends
//! when idle, waits behind another daemon, hands over by starting afresh in
//! its own process, keeping its pid, so whatever keeps it running keeps the
//! new image, and stops on `SIGTERM` or `SIGINT` (design D1 to D7 of
//! `sluice-runs-as-a-daemon`).

pub mod ask;
pub mod client;
pub mod environment;
pub mod folder;
pub mod index;
pub mod pass;
pub mod protocol;
pub mod results;
pub mod room;
pub mod tasks;

use std::ffi::OsString;
use std::fs::File;
use std::io::{BufReader, ErrorKind, Read};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::exec::Host;
use crate::host_settings::{HostSettings, Look};
use crate::instance::Instance;
use crate::profiles::places::{repository_key, user_scratch, USER_SCRATCH_VAR};
pub use ask::{Following, Here, Holding, Holds, HostProcess, Socketed};
use folder::{Folder, How};
use protocol::{receive, send, Answer, Hello, Named, Request, RoomView, Started};
use room::{Asked, Asker, Room, Shared};

/// Set to `record` in tests: the host process takes no pass of its own,
/// and passes only when a command asks (design D14).
pub const ROUND_SLEEP_VAR: &str = "SLUICE_ROUND_SLEEP";

/// The variable a daemon starting afresh in place names its start lock's
/// descriptor in, so the image started afresh holds it until its socket
/// answers (design D6 of `sluice-runs-as-a-daemon`).
pub const START_LOCK_FD_VAR: &str = "SLUICE_START_LOCK_FD";

/// How often the host process looks at whether it should end, and at
/// whether its state folder is still there (design D6).
const LOOK: Duration = Duration::from_millis(100);

/// How often it looks at each repository in its index that it holds no
/// task for, as an idle tick reads it (129; design D4).
const SCAN: Duration = Duration::from_secs(3);

/// The tick a reading is current within for a one-shot read (64, 129;
/// design D16).
pub const READ_TICK: Duration = Duration::from_secs(3);

/// How long a one-shot read waits for a current reading: the round's retry
/// bound and a tick (design D16).
pub fn read_for() -> Duration {
    crate::retry::COMMAND_FOR + READ_TICK
}

/// The files of `host/`, by their names in it.
const LOCK: &str = "lock";
const START_LOCK: &str = "start.lock";
const VERSION: &str = "version";
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
    /// Its folder, `host/`, held open since it started: every file it
    /// opens or moves there is named through it, and its device and inode
    /// tell a folder made again at that path, which is not its own
    /// (design D6).
    pub folder: Arc<Folder>,
    /// The host's settings it holds, which every binding it makes is of
    /// (record 0107; design D19).
    pub settings: Arc<tasks::Settings>,
    /// The forge's tokens it holds, one for each repository, which every
    /// binding it makes holds its tokens in, so a repository's token is
    /// got once for its hour, never for each pass, look or ask that binds
    /// afresh (record 0108 as amended 2026-11-19).
    pub tokens: crate::profiles::Tokens,
    /// A daemon asked to hand over: the executable the asker named, or
    /// `None` where it named none (design D3 of `sluice-runs-as-a-daemon`).
    pub afresh: Mutex<Option<Afresh>>,
}

/// A daemon's ask to start afresh in place, as a newer command made it
/// (design D3 of `sluice-runs-as-a-daemon`).
pub(crate) struct Afresh {
    /// The executable the asker named, resolved through every link.
    program: Option<PathBuf>,
    /// The asker's version, as its `hello` said it.
    by: String,
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

/// How the host process's loop ended.
enum Ending {
    /// It ends: no work in play, a newer sluice's ask or its state folder
    /// gone.
    Ended,
    /// It handed its repositories to a successor, whose socket is now the
    /// host's (design D20).
    HandedOver,
}

/// `sluice host`: run as the host process of this user's state folder,
/// or, where another holds its lock, end at once and move nothing (135;
/// design D1). `choices` are the telemetry its own command line and
/// sluice's variables choose (151; rule core.53). Its exit.
pub fn run(choices: crate::telemetry::Choices) -> Result<i32> {
    let successor = Successor::now();
    let paths = Paths::host()?;
    let folder =
        Folder::open(&paths.dir).with_context(|| format!("opening {}", paths.dir.display()))?;
    let Some(lock) = folder.try_lock(LOCK)? else {
        log("sluice host: another host process holds this host's lock; this one ends");
        return Ok(0);
    };
    run_holding(
        paths,
        Arc::new(folder),
        lock,
        choices,
        successor,
        false,
        None,
        false,
    )
}

/// The start lock a daemon started afresh in place was handed (design D6
/// of `sluice-runs-as-a-daemon`): [`START_LOCK_FD_VAR`] taken out of the
/// environment, so no child inherits its name, and its descriptor set to
/// close on exec again, so no child inherits it either. `None` where none
/// was handed, or the variable names no open descriptor. Called first in
/// `main`, before any thread or child starts.
pub fn take_start_lock() -> Option<File> {
    let named = std::env::var_os(START_LOCK_FD_VAR)?;
    std::env::remove_var(START_LOCK_FD_VAR);
    let fd: RawFd = named.to_str()?.parse().ok()?;
    // SAFETY: the descriptor is this process's from its last image, which
    // named it for this image alone; only one that is open is taken.
    let held = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
    nix::fcntl::fcntl(held, nix::fcntl::FcntlArg::F_GETFD).ok()?;
    nix::fcntl::fcntl(
        held,
        nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::FD_CLOEXEC),
    )
    .ok()?;
    // SAFETY: as above; nothing else of this process owns it.
    Some(unsafe { File::from_raw_fd(fd) })
}

/// `sluice daemon`: run as the host process of this user's state folder,
/// in the foreground, never ending for being idle (135, 137; record 0093
/// as amended 2026-11-08; design D1 of `sluice-runs-as-a-daemon`). It
/// notes the repository it is started in where that has sluice (D8),
/// takes the host's place from a host process that ends when idle, or
/// waits behind another daemon (D2), and stops on `SIGTERM` or `SIGINT`
/// (D7). `start` is the start lock its last image handed it, held until
/// its socket answers (D6). Its exit.
pub fn daemon(choices: crate::telemetry::Choices, start: Option<File>) -> Result<i32> {
    crate::stop_on_interrupt()?;
    crate::stop_on_terminate()?;
    let successor = Successor::now();
    let paths = Paths::host()?;
    // Its last image noted it: one started afresh notes nothing, so it
    // takes no lock on the start lock it holds (D6).
    let afresh = start.is_some();
    if !afresh {
        note_here(&paths);
    }
    let folder = Arc::new(
        Folder::open(&paths.dir).with_context(|| format!("opening {}", paths.dir.display()))?,
    );
    let (lock, start) = match folder.try_lock(LOCK)? {
        Some(lock) => (lock, start),
        None => {
            // D6: a start lock it was handed is let go before it waits, so
            // the host process that holds the host can start afresh itself.
            drop(start);
            match take_place(&paths, &folder, &successor)? {
                Some(placed) => placed,
                None => return Ok(0),
            }
        }
    };
    run_holding(paths, folder, lock, choices, successor, true, start, afresh)
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
/// starting afresh in place keeps its place (D6).
fn take_place(
    paths: &Paths,
    folder: &Folder,
    successor: &Successor,
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
        if crate::stopped() {
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
                    if crate::stopped() {
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
            _ => match handed_over(paths, folder, &client, successor)? {
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
) -> Result<Handed> {
    let start = folder.lock(START_LOCK)?;
    let until = Instant::now() + client::HAND_OVER_WAIT;
    let mut asked = std::collections::BTreeSet::new();
    let mut last = None;
    loop {
        if crate::stopped() {
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
                    let _ = c.ask(&Request::HandOver { program });
                }
                last = Some(c.named);
            }
        }
        if Instant::now() >= until {
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

fn run_holding(
    paths: Paths,
    folder: Arc<Folder>,
    lock: File,
    choices: crate::telemetry::Choices,
    successor: Successor,
    daemon: bool,
    start: Option<File>,
    afresh: bool,
) -> Result<i32> {
    let mut lock = Some(lock);
    take_log(&folder)?;
    let named = Named {
        version: crate::VERSION.to_string(),
        pid: std::process::id(),
        since: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        asked_only: asked_only(),
        daemon,
    };
    folder.write(VERSION, &serde_json::to_vec(&named)?)?;
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
    // D6 of `sluice-runs-as-a-daemon`: its socket answers, so a command
    // that waited on the start lock its last image or its taking of the
    // host's place held reaches it.
    drop(start);
    // Record 0107 (design D19): the host's settings, held with the sign of
    // their file, which it looks at between its ticks.
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
    // held for every binding it makes.
    let tokens = crate::profiles::Tokens::default();
    let looked_with = settings.clone();
    let looked_holding = tokens.clone();
    let tasks = tasks::Tasks::new(
        tasks::repository_body(
            room.clone(),
            asked_only(),
            named.clone(),
            settings.clone(),
            tokens.clone(),
        ),
        Arc::new(move |r: &Path| {
            index::in_play(r, looked_with.instance(), &looked_holding).unwrap_or(tasks::Looked {
                in_play: true,
                refused: None,
            })
        }),
    );
    let tasks = match asked_only() {
        true => tasks.passes_when_asked(),
        false => tasks,
    };
    // D13: a freed part has the repository it is owed to pass at once.
    let told = tasks.clone();
    room.on_free(move |r| told.owed(r));
    let hosting = Arc::new(Hosting {
        tasks,
        paths,
        named,
        room,
        connections: AtomicUsize::new(0),
        stopping: AtomicBool::new(false),
        folder,
        settings,
        tokens,
        afresh: Mutex::new(None),
    });
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
    // 135 (design D4): the repositories it holds, found again; a gone one,
    // or one with no sluice, leaves the index.
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
    for repository in index.list().unwrap_or_default() {
        hosting.tasks.look(&repository);
    }
    let ending = serve(
        &hosting,
        listener,
        TakeUp::new(host_settings),
        &mut lock,
        &successor,
        telemetry.as_ref(),
    );
    // A successor's socket is the host's now: only this process's own is
    // removed.
    if !matches!(ending, Ok(Ending::HandedOver)) {
        let _ = std::fs::remove_file(&hosting.paths.socket);
    }
    log(&format!(
        "sluice {} {} ends",
        if hosting.named.daemon {
            "daemon"
        } else {
            "host"
        },
        hosting.named.pid
    ));
    if let Some(t) = telemetry {
        t.finish();
    }
    ending.map(|_| 0)
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

/// How often it looks at the host's settings file's sign (record 0107;
/// design D19): its entries alone, no process started and no git read
/// (rule core.31).
const SETTINGS_LOOK: Duration = Duration::from_secs(1);

/// The host's settings as its main loop looks at them, and a change being
/// bound on a thread of its own (record 0107; rule core.54; design D19).
struct TakeUp {
    settings: HostSettings,
    looked: Instant,
    /// A change being bound: its settings, the tables it moves, and the
    /// answer of the thread binding it.
    binding: Option<(
        Instance,
        Vec<&'static str>,
        mpsc::Receiver<Result<(), String>>,
    )>,
}

/// What a look at the host's settings asks of the loop.
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
            looked: Instant::now(),
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

    /// Between ticks, never within a pass: a change found bound on a
    /// thread of its own against the repositories it holds, the host's
    /// systems checked, then held for every task's next pass and the
    /// room's next admission, said once in its log. A file gone,
    /// unreadable or refused, or settings that do not bind, leave it on
    /// what it holds, said once by the sign kept (design D19).
    fn look(&mut self, hosting: &Hosting) -> Taken {
        if let Some((_, _, answer)) = &self.binding {
            let bound = match answer.try_recv() {
                Ok(bound) => bound,
                Err(mpsc::TryRecvError::Empty) => return Taken::Done,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err("binding them stopped short".to_string())
                }
            };
            let (instance, tables, _) = self.binding.take().expect("a change being bound");
            let afresh = tables.iter().any(|t| crate::instance::AFRESH.contains(t));
            match bound {
                // D20: the successor reads the file itself; this process
                // keeps what it holds until it ends.
                Ok(()) if afresh => return Taken::HandOver(tables),
                Err(why) if afresh => self.not_handed(&tables, "its settings do not bind", &why),
                Ok(()) => {
                    hosting
                        .room
                        .lock()
                        .bounds(instance.bound, instance.reserved);
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
            return Taken::Done;
        }
        if self.looked.elapsed() < SETTINGS_LOOK {
            return Taken::Done;
        }
        self.looked = Instant::now();
        match self.settings.look() {
            Look::Same => {}
            Look::Refused(why) => self.refused(&why),
            Look::Changed(instance, _text) => {
                let tables = self.settings.held().changed_tables(&instance);
                let mut repositories = hosting.tasks.held();
                if repositories.is_empty() {
                    repositories = hosting.index().list().unwrap_or_default();
                }
                let (tx, rx) = mpsc::channel();
                let checked = instance.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(binds(&checked, &repositories).map_err(|e| format!("{e:#}")));
                });
                self.binding = Some((instance, tables, rx));
            }
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
/// one, and each of `repositories` bound with it, the host's systems
/// checked against its policies (87; design D19).
fn binds(instance: &Instance, repositories: &[PathBuf]) -> Result<()> {
    let retry = std::rc::Rc::new(crate::retry::Policy::round().recording_where_set());
    if let Some(forge) = &instance.forge {
        crate::profiles::github::App::open(forge, retry.clone())?;
    }
    for r in repositories {
        let bound = crate::profiles::Bound::open_with(
            &tasks::open_dir(r),
            std::env::current_exe()?,
            instance.clone(),
            retry.clone(),
        )?;
        sluice_core::Sluice::new(bound.seams(), &sluice_core::SystemClock)
            .as_peer(bound.peer())
            .check_host_systems()?;
    }
    Ok(())
}

/// How long it waits for its successor to answer: a start's (design D2,
/// D20).
const SUCCESSOR_WAIT: Duration = Duration::from_secs(10);

/// D20: hand the repositories to a successor it starts itself, for a
/// change of `tables` no binding takes up in place: every task finishes
/// its pass and lets its locks go, its telemetry sends what it holds, and
/// under the start lock, held until a host process of another pid
/// answers, it lets its lock and its socket go and starts `<the executable
/// it started as> host` with the environment it started with, detached.
/// Nothing passes to the successor but what the records and the host's
/// state hold: it reads the settings file itself. Where none answers, it
/// takes its lock and socket back and goes on as it was, said once.
/// Whether it handed over.
fn hand_over(
    hosting: &Hosting,
    listener: &mut UnixListener,
    lock: &mut Option<File>,
    successor: &Successor,
    telemetry: Option<&crate::telemetry::Telemetry>,
    take_up: &TakeUp,
    tables: &[&'static str],
) -> bool {
    let program = match &successor.program {
        Ok(p) => p.clone(),
        Err(why) => {
            take_up.not_handed(tables, "its successor could not start", why);
            return false;
        }
    };
    let start = match hosting.folder.lock(START_LOCK) {
        Ok(f) => f,
        Err(e) => {
            take_up.not_handed(tables, "its successor could not start", &format!("{e:#}"));
            return false;
        }
    };
    log(&format!(
        "sluice: {} changed {}, which the host process takes up by handing its repositories to \
         a fresh one",
        take_up.file(),
        tables.join(", ")
    ));
    hosting.tasks.stop_all();
    if let Some(t) = telemetry {
        t.flush();
    }
    drop(lock.take());
    let _ = std::fs::remove_file(&hosting.paths.socket);
    let log_path = hosting.paths.log();
    let spawned = hosting.folder.file(LOG, How::Append).and_then(|out| {
        let mut command = std::process::Command::new(&program);
        command
            .arg("host")
            .current_dir("/")
            .stdin(std::process::Stdio::null())
            .stdout(out.try_clone()?)
            .stderr(out)
            .env_clear()
            .envs(successor.env.iter().cloned());
        // SAFETY: setsid is async-signal-safe; nothing else runs here.
        unsafe {
            use std::os::unix::process::CommandExt;
            command.pre_exec(|| {
                nix::unistd::setsid().map_err(std::io::Error::from)?;
                Ok(())
            });
        }
        command.spawn()
    });
    let own = std::process::id();
    let answered = match spawned {
        Ok(child) => {
            std::thread::spawn(move || {
                let mut child = child;
                child.wait()
            });
            let client = client::Client::new(&program, successor.env.clone());
            let until = Instant::now() + SUCCESSOR_WAIT;
            loop {
                let pid = client
                    .as_ref()
                    .ok()
                    .and_then(|c| c.answering().ok().flatten())
                    .map(|c| c.named.pid);
                if pid.is_some_and(|p| p != own) {
                    break true;
                }
                if Instant::now() >= until {
                    break false;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        Err(_) => false,
    };
    if answered {
        drop(start);
        return true;
    }
    // A successor that took the lock late holds the host now.
    match hosting.folder.try_lock(LOCK) {
        Ok(Some(f)) => *lock = Some(f),
        _ => {
            drop(start);
            return true;
        }
    }
    let _ = std::fs::remove_file(&hosting.paths.socket);
    match UnixListener::bind(&hosting.paths.socket) {
        Ok(l) => {
            let _ = l.set_nonblocking(true);
            *listener = l;
        }
        Err(e) => log(&format!(
            "sluice host: binding {} again: {e}",
            hosting.paths.socket.display()
        )),
    }
    hosting.tasks.resume();
    // Its repositories with work in play are held again at once, as its
    // start finds them (design D4).
    scan(hosting);
    drop(start);
    take_up.not_handed(
        tables,
        "its successor did not start",
        &format!("see {}", log_path.display()),
    );
    false
}

/// D3, D5 and D6 of `sluice-runs-as-a-daemon`: a daemon starts afresh in
/// its own process as `program`, running `daemon` with the environment it
/// started with, keeping its pid, so whatever keeps it running keeps the
/// image started afresh: every task finishes its round and lets its locks
/// go, its telemetry sends what it holds, and under the start lock, handed
/// to the image started afresh by its descriptor, it lets its lock and
/// socket go and replaces its image. It returns only where the start
/// failed: its lock and socket taken back, its tasks going on, and why.
fn start_afresh(
    hosting: &Hosting,
    listener: &mut UnixListener,
    lock: &mut Option<File>,
    successor: &Successor,
    telemetry: Option<&crate::telemetry::Telemetry>,
    program: &Path,
) -> String {
    let start = match hosting.folder.lock(START_LOCK) {
        Ok(f) => f,
        Err(e) => return format!("{e:#}"),
    };
    log("sluice daemon: it starts afresh in its own process, keeping its pid");
    hosting.tasks.stop_all();
    if let Some(t) = telemetry {
        t.flush();
    }
    let handed = || -> Result<std::process::Command> {
        nix::fcntl::fcntl(
            &start,
            nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::empty()),
        )?;
        let mut command = std::process::Command::new(program);
        command
            .arg("daemon")
            .env_clear()
            .envs(
                successor
                    .env
                    .iter()
                    .filter(|(name, _)| name != START_LOCK_FD_VAR)
                    .cloned(),
            )
            .env(START_LOCK_FD_VAR, start.as_raw_fd().to_string());
        Ok(command)
    };
    let why = match handed() {
        Ok(command) => {
            drop(lock.take());
            let _ = std::fs::remove_file(&hosting.paths.socket);
            format!("{:#}", crate::hand_over::replace(&command))
        }
        Err(e) => format!("{e:#}"),
    };
    // The start failed: the process as it was.
    let _ = nix::fcntl::fcntl(
        &start,
        nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::FD_CLOEXEC),
    );
    if lock.is_none() {
        match hosting.folder.try_lock(LOCK) {
            Ok(Some(f)) => *lock = Some(f),
            Ok(None) => log("sluice daemon: another holds the host's lock now"),
            Err(e) => log(&format!(
                "sluice daemon: taking the host's lock again: {e:#}"
            )),
        }
        let _ = std::fs::remove_file(&hosting.paths.socket);
        match UnixListener::bind(&hosting.paths.socket) {
            Ok(l) => {
                let _ = l.set_nonblocking(true);
                *listener = l;
            }
            Err(e) => log(&format!(
                "sluice daemon: binding {} again: {e}",
                hosting.paths.socket.display()
            )),
        }
    }
    hosting.tasks.resume();
    scan(hosting);
    drop(start);
    why
}

/// Accept connections, each answered on a thread of its own, until the
/// process should end or hands over (design D6, D8, D19, D20).
fn serve(
    hosting: &Arc<Hosting>,
    mut listener: UnixListener,
    mut take_up: TakeUp,
    lock: &mut Option<File>,
    successor: &Successor,
    telemetry: Option<&crate::telemetry::Telemetry>,
) -> Result<Ending> {
    let listener = &mut listener;
    let mut looked = Instant::now();
    let mut scanned = Instant::now();
    // 154 (design D8 of `each-submission-shows-its-wait-and-its-test`): the
    // rows of the room last sent, and each repository's name as its
    // binding names it, read once.
    let mut sent = std::collections::BTreeSet::new();
    let mut names = std::collections::BTreeMap::new();
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
        // D7 of `sluice-runs-as-a-daemon`: a daemon signalled to stop ends
        // as a hand-over ends it.
        if hosting.named.daemon && !hosting.stopping.load(Ordering::SeqCst) && crate::stopped() {
            log("sluice daemon: signalled to stop; it ends");
            hosting.stopping.store(true, Ordering::SeqCst);
        }
        if !hosting.stopping.load(Ordering::SeqCst) {
            match take_up.look(hosting) {
                // D5 of `sluice-runs-as-a-daemon`: a daemon starts afresh in
                // place as the executable it started as.
                Taken::HandOver(tables) if hosting.named.daemon => {
                    let why = match &successor.program {
                        Ok(program) => {
                            start_afresh(hosting, listener, lock, successor, telemetry, program)
                        }
                        Err(why) => why.clone(),
                    };
                    take_up.not_handed(&tables, "it could not start afresh", &why);
                }
                Taken::HandOver(tables) => {
                    if hand_over(
                        hosting, listener, lock, successor, telemetry, &take_up, &tables,
                    ) {
                        return Ok(Ending::HandedOver);
                    }
                }
                Taken::Done => {}
            }
            hosting.tasks.tick();
            // D13: a run that ended since the last look frees its part for
            // the repository it is owed to.
            let owed = hosting.room.lock().freed(Instant::now()).ok().flatten();
            if let Some(r) = owed {
                hosting.tasks.owed(&r);
            }
            if scanned.elapsed() >= SCAN {
                scanned = Instant::now();
                scan(hosting);
                send_room(hosting, telemetry, &mut sent, &mut names);
            }
        }
        if hosting.stopping.load(Ordering::SeqCst) {
            // D3 of `sluice-runs-as-a-daemon`: a daemon a newer command asked
            // to hand over starts afresh as the executable the asker named,
            // or as its own; where it cannot, it goes on as it was, said
            // once.
            let asked = hosting
                .afresh
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(asked) = asked {
                let program = match (asked.program, &successor.program) {
                    (Some(p), _) => Ok(p),
                    (None, Ok(p)) => Ok(p.clone()),
                    (None, Err(why)) => Err(why.clone()),
                };
                let (shown, why) = match &program {
                    Ok(p) => (
                        p.display().to_string(),
                        start_afresh(hosting, listener, lock, successor, telemetry, p),
                    ),
                    Err(why) => ("its own executable".to_string(), why.clone()),
                };
                log(&format!(
                    "sluice daemon: warning: sluice {} asked it to hand over, and it could not \
                     start afresh as {shown}, so it goes on as it was: {why}",
                    asked.by
                ));
                hosting.stopping.store(false, Ordering::SeqCst);
                continue;
            }
            hosting.tasks.stop_all();
            return Ok(Ending::Ended);
        }
        // D1 of `sluice-runs-as-a-daemon`: a daemon never ends for being
        // idle.
        if hosting.named.daemon {
            continue;
        }
        if hosting.tasks.any()
            || hosting.connections.load(Ordering::SeqCst) > 0
            || hosting.room.lock().busy(Instant::now())
        {
            continue;
        }
        // Idle: it decides under the start lock, which it keeps until it
        // ends, after taking every connection already queued (design D6).
        let Ok(Some(start)) = hosting.folder.try_lock(START_LOCK) else {
            continue;
        };
        if accept_all(hosting, listener) > 0 {
            drop(start);
            continue;
        }
        let _ = std::fs::remove_file(&hosting.paths.socket);
        // The start lock goes with the process.
        std::mem::forget(start);
        return Ok(Ending::Ended);
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
    let index = hosting.index();
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
            // D16: no reading begun before the change is current.
            hosting.tasks.told(&repository);
            // Where passes are taken only when asked, the repository is
            // held for the next ask; otherwise it is passed at once.
            let task = hosting.tasks.handle(&repository);
            if !asked_only() {
                task.wake();
            }
            send(&mut writer, &Answer::Ok)
        }
        Request::Pass { repository, at } => {
            note(hosting, &repository);
            let at = at.and_then(|at| at.parse::<chrono::DateTime<chrono::Utc>>().ok());
            let answer = match hosting.tasks.handle(&repository).pass(at) {
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
            branch,
        } => hold(
            hosting,
            kind,
            &repository,
            branch.as_deref(),
            &stream,
            &mut writer,
            &mut reader,
        ),
        Request::Read { repository, afresh } => {
            note(hosting, &repository);
            let reading = hosting
                .tasks
                .read(&repository, READ_TICK, read_for(), afresh);
            send(&mut writer, &Answer::Read(reading))
        }
        Request::Results {
            repository,
            branch,
            wants,
        } => {
            let answer = match results_of(hosting, &repository, &branch, &wants) {
                Ok(lines) => Answer::Results(String::from_utf8_lossy(&lines).into_owned()),
                Err(e) => Answer::Error(format!("{e:#}")),
            };
            send(&mut writer, &answer)
        }
        Request::Room => send(&mut writer, &Answer::Room(view(hosting)?)),
        // D3 of `sluice-runs-as-a-daemon`: a daemon starts afresh in place;
        // a host process that is no daemon ends, whatever the asker named.
        Request::HandOver { program } if hosting.named.daemon => {
            log(&format!(
                "sluice daemon: sluice {} asked this one to hand over; it starts afresh in place",
                hello.hello
            ));
            *hosting
                .afresh
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Afresh {
                program,
                by: hello.hello.clone(),
            });
            hosting.stopping.store(true, Ordering::SeqCst);
            send(&mut writer, &Answer::Ok)
        }
        Request::HandOver { .. } => {
            log(&format!(
                "sluice host: sluice {} asked this one to hand over; it ends",
                hello.hello
            ));
            hosting.stopping.store(true, Ordering::SeqCst);
            send(&mut writer, &Answer::Ok)
        }
    }
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
fn note(hosting: &Hosting, repository: &Path) {
    if let Err(e) = hosting.index().note(repository) {
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
    branch: Option<&str>,
    stream: &UnixStream,
    writer: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
) -> Result<()> {
    // A session's or a preview's ask waits its turn; a combination's, from
    // a profile beside the host process (task 3.4), is answered once, held
    // or refused, as a round's own ask is, and counts as no session's wait.
    let waiter = match kind {
        sluice_seams::RunKind::Session => Some(hosting.room.lock().wait(repository, branch)),
        sluice_seams::RunKind::Queue => None,
    };
    let let_go = || {
        if let Some(w) = waiter {
            hosting.room.lock().let_go(w);
        }
    };
    let asker = match waiter {
        Some(w) => Asker::Session(w),
        None => Asker::Queue(repository.to_path_buf()),
    };
    let mut said = None;
    let id = loop {
        if hosting.stopping.load(Ordering::SeqCst) || closed(stream) {
            let_go();
            return Ok(());
        }
        let asked = hosting
            .room
            .lock()
            .ask(&asker, repository, branch, Instant::now());
        match asked {
            Ok(Asked::Held(id)) => break id,
            Ok(Asked::Refused(why)) => {
                if said != Some(why) {
                    said = Some(why);
                    if send(writer, &Answer::Waiting(why)).is_err() {
                        let_go();
                        return Ok(());
                    }
                }
                if waiter.is_none() {
                    return Ok(());
                }
            }
            Err(e) => {
                let_go();
                return send(writer, &Answer::Error(format!("{e:#}")));
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let_go();
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
