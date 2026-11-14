//! The host's room (71, 150, S75; rule core.49; records 0087 and 0123 as
//! record 0093 amends them; design D12, D13): the one value in sluice's
//! host process that decides whether a run may start, held behind a mutex
//! by every repository's task and every session's or preview's ask.
//!
//! It counts the runs going on the host and the parts held for runs about
//! to start, through the host's registry where it is given one, so an
//! older sluice's `sluice exec` counts against it and is counted, and a
//! room made afresh counts the runs that outlived the process before it.
//! It keeps the runs waiting for room, by kind and repository, and whose
//! turn the next contested run is.
//!
//! - A combination's part is held where a run of the bound is free, unless
//!   that run is one a session may take, a session's or a preview's run
//!   waits, and the turn is the sessions'. Held in room a waiting session
//!   could have taken, the turn becomes the sessions'.
//! - A combination's part is held only for the repository the share puts
//!   first (record 0123; D13): among the repositories with a combination
//!   waiting and the one asking, the one with the fewest runs under test,
//!   its runs holding a part of any kind, and among equals the one whose
//!   combination has waited longest. Where more runs are free than one,
//!   as many repositories are first as runs are free. Nothing under test
//!   is stopped for the share.
//! - A session's or a preview's part is held where a run of the bound less
//!   the reserve is free and owed to no one: no session's or preview's run
//!   waits ahead of it, and either the turn is the sessions' or the
//!   combinations waiting are fewer than the free runs. Held, the turn
//!   becomes the queue's.
//! - A refusal records or renews the run's wait, and a hold removes it. A
//!   combination's wait counts within thirty seconds of its round's last
//!   ask; a session's or a preview's while its asker waits.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sluice_seams::{
    AtBound, Equip, Equipped, HostRun, KeptOutTask, Launch, Limits, Part, Platform, Progress,
    Proof, Reading, Reads, Refusals, RunHandle, RunKind, RunReport, RunRequest, Running, Setting,
    Settings, TaskLog, TaskProgress, Tests,
};

use crate::host_count::{self, Admission, HostCount};

/// How long a combination's wait counts after its round last asked for
/// room (record 0087): ten of a pass's ticks.
pub const QUEUE_WAIT: Duration = Duration::from_secs(30);

/// Whose the next run a combination and a session both want is (150).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Turn {
    /// A combination's run starts first.
    #[default]
    Queue,
    /// A waiting session's or preview's run starts first.
    Sessions,
}

/// Why a run waits; a session's or a preview's says it in the words it
/// says once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// No run of the bound less the reserve is free, or one that waited
    /// longer is owed it.
    AtBound,
    /// A free run is owed to a waiting combination.
    CombinationFirst,
}

impl Why {
    /// What a waiting `sluice exec` or `sluice preview` says, once.
    pub fn words(self) -> &'static str {
        match self {
            Why::AtBound => "waiting: the host is at its bound",
            Why::CombinationFirst => "waiting: a combination's run starts first",
        }
    }
}

/// The reasons a waiting `sluice exec` or `sluice preview` has said: each
/// once, whatever order the host process answers them in (150).
#[derive(Debug, Default)]
pub struct Told(Vec<Why>);

impl Told {
    /// `why`'s words, where they were not said yet.
    pub fn first(&mut self, why: Why) -> Option<&'static str> {
        if self.0.contains(&why) {
            return None;
        }
        self.0.push(why);
        Some(why.words())
    }
}

/// The runs waiting for room, by kind: what the overview's `room` shows
/// (150).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Waiting {
    pub queue: usize,
    pub sessions: usize,
}

/// One repository's share of the host's bound (record 0123): its runs
/// under test, a part of any kind held for each, and its runs waiting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Share {
    pub under_test: usize,
    pub waiting: usize,
}

/// Who asks for room: a combination's round, by the repository its runs
/// are for, or a session's or a preview's asker, by the wait it took.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asker {
    Queue(PathBuf),
    Session(u64),
}

impl Asker {
    fn kind(&self) -> RunKind {
        match self {
            Asker::Queue(_) => RunKind::Queue,
            Asker::Session(_) => RunKind::Session,
        }
    }
}

/// What an ask came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Asked {
    /// The part, by its id in the room.
    Held(u64),
    /// No room; a session's or a preview's run says why.
    Refused(Why),
}

/// A part held and not yet handed to its run's supervisor.
struct Held {
    kind: RunKind,
    repository: PathBuf,
    /// Its entry in the host's registry, where the room keeps one.
    admission: Option<Admission>,
}

/// A combination's wait for room, for one repository.
#[derive(Clone, Copy, Debug)]
struct QueueWait {
    /// When it began: its first ask refused since it last held a part.
    since: Instant,
    /// When its round last asked.
    last: Instant,
}

/// A session's or a preview's wait.
#[derive(Clone, Debug)]
struct SessionWait {
    id: u64,
    repository: PathBuf,
}

/// How a room counts the runs going on the host.
enum Count {
    /// Its own: each run started from a part, until it ends.
    Own(BTreeMap<u64, (RunKind, PathBuf)>),
    /// The host's registry of running checks: every live entry, an older
    /// sluice's and the parts it holds among them.
    Registry(HostCount),
    /// As last seen of a check seam that counts them itself, as the tests'
    /// fakes do, with the parts it holds beside them.
    Seen(Running),
}

/// The host's room: see the module's documentation.
pub struct Room {
    bound: usize,
    reserved: usize,
    count: Count,
    held: BTreeMap<u64, Held>,
    /// Combinations' waits, by repository.
    queue_waits: BTreeMap<PathBuf, QueueWait>,
    /// Sessions' and previews' waits in the order they came to wait.
    session_waits: Vec<SessionWait>,
    turn: Turn,
    next: u64,
    /// The runs counted at the last look, so a run that ended is found.
    last: Option<usize>,
}

impl Room {
    /// A room of `bound` runs, `reserved` of them kept for combinations,
    /// counting only what it holds itself.
    pub fn new(bound: usize, reserved: usize) -> Room {
        Room {
            bound,
            reserved,
            count: Count::Own(BTreeMap::new()),
            held: BTreeMap::new(),
            queue_waits: BTreeMap::new(),
            session_waits: Vec::new(),
            turn: Turn::Queue,
            next: 1,
            last: None,
        }
    }

    /// A room of `bound` runs, `reserved` of them kept for combinations,
    /// counting through the host's registry `registry`: every live entry,
    /// an older sluice's and the runs that outlived an earlier room among
    /// them, counts against it.
    pub fn over(registry: HostCount, bound: usize, reserved: usize) -> Room {
        Room {
            count: Count::Registry(registry),
            ..Room::new(bound, reserved)
        }
    }

    /// A room of `bound` runs, `reserved` of them kept for combinations,
    /// counting the runs going as [`Room::see`] last saw them.
    pub fn seeing(bound: usize, reserved: usize) -> Room {
        Room {
            count: Count::Seen(Running::default()),
            ..Room::new(bound, reserved)
        }
    }

    /// The runs going on the host as a check seam that counts them itself
    /// sees them now, the parts this room holds apart.
    pub fn see(&mut self, running: Running) {
        if let Count::Seen(r) = &mut self.count {
            *r = running;
        }
    }

    pub fn bound(&self) -> usize {
        self.bound
    }

    /// The bound and the reserve as the host's settings say them now,
    /// from the next admission (record 0107; D19).
    pub fn bounds(&mut self, bound: usize, reserved: usize) {
        self.bound = bound;
        self.reserved = reserved;
    }

    pub fn reserved(&self) -> usize {
        self.reserved
    }

    pub fn turn(&self) -> Turn {
        self.turn
    }

    /// The runs going on the host and the parts held, by kind.
    pub fn running(&self) -> Result<Running> {
        let (mut out, going): (Running, Vec<RunKind>) = match &self.count {
            Count::Registry(r) => return r.running(),
            Count::Own(going) => (Running::default(), going.values().map(|g| g.0).collect()),
            Count::Seen(r) => (*r, Vec::new()),
        };
        for kind in self.held.values().map(|h| h.kind).chain(going) {
            match kind {
                RunKind::Queue => out.queue += 1,
                RunKind::Session => out.sessions += 1,
            }
        }
        Ok(out)
    }

    /// Each repository's runs under test: its runs going and its parts
    /// held, of any kind (record 0123; D13).
    fn under_test(&self) -> Result<BTreeMap<PathBuf, usize>> {
        let mut out: BTreeMap<PathBuf, usize> = match &self.count {
            // The registry holds the room's own parts among its entries.
            Count::Registry(r) => return r.under_test(),
            Count::Own(going) => {
                let mut out = BTreeMap::new();
                for (_, repository) in going.values() {
                    *out.entry(repository.clone()).or_default() += 1;
                }
                out
            }
            Count::Seen(_) => BTreeMap::new(),
        };
        for h in self.held.values() {
            *out.entry(h.repository.clone()).or_default() += 1;
        }
        Ok(out)
    }

    /// The runs waiting at `now`, by kind.
    pub fn waiting(&mut self, now: Instant) -> Waiting {
        self.lapse(now);
        Waiting {
            queue: self.queue_waits.len(),
            sessions: self.session_waits.len(),
        }
    }

    /// Each repository with a run under test or waiting, and its share, at
    /// `now` (record 0123; D13).
    pub fn repositories(&mut self, now: Instant) -> Result<BTreeMap<PathBuf, Share>> {
        self.lapse(now);
        let mut out: BTreeMap<PathBuf, Share> = BTreeMap::new();
        for (repository, n) in self.under_test()? {
            out.entry(repository).or_default().under_test = n;
        }
        for repository in self.queue_waits.keys() {
            out.entry(repository.clone()).or_default().waiting += 1;
        }
        for w in &self.session_waits {
            out.entry(w.repository.clone()).or_default().waiting += 1;
        }
        Ok(out)
    }

    /// A session's or a preview's wait for a run of `repository`, from
    /// now, behind every one before it: its asker's id, which it asks with
    /// and lets go of.
    pub fn wait(&mut self, repository: &Path) -> u64 {
        let id = self.id();
        self.session_waits.push(SessionWait {
            id,
            repository: repository.to_path_buf(),
        });
        id
    }

    /// The session's or preview's wait `id` is let go: its asker ended,
    /// however it ended, or its run holds its part.
    pub fn let_go(&mut self, id: u64) {
        self.session_waits.retain(|w| w.id != id);
    }

    /// The repositories whose combinations come before `repository`'s for
    /// the next free run at `now`: those waiting with fewer runs under
    /// test, and among equals those that have waited longer (record 0123;
    /// D13). One that asks with no wait recorded waits from its ask.
    fn ahead_of(&self, repository: &Path, now: Instant) -> Result<usize> {
        let under = self.under_test()?;
        let rank = |r: &Path| {
            let since = self.queue_waits.get(r).map_or(now, |w| w.since);
            (under.get(r).copied().unwrap_or(0), since)
        };
        let mine = rank(repository);
        Ok(self
            .queue_waits
            .keys()
            .filter(|r| r.as_path() != repository)
            .filter(|r| {
                let theirs = rank(r);
                theirs < mine || (theirs == mine && r.as_path() < repository)
            })
            .count())
    }

    /// The repository whose combination the next free run is owed to at
    /// `now`, where one waits: the one the share puts first (D13).
    pub fn owed(&mut self, now: Instant) -> Result<Option<PathBuf>> {
        self.lapse(now);
        for r in self.queue_waits.keys() {
            if self.ahead_of(r, now)? == 0 {
                return Ok(Some(r.clone()));
            }
        }
        Ok(None)
    }

    /// Ask for a part for `asker`'s run at `now`, the run's repository
    /// named for the share and the registry's entry: held, or refused with
    /// its wait recorded or renewed (records 0087, 0123).
    pub fn ask(&mut self, asker: &Asker, repository: &Path, now: Instant) -> Result<Asked> {
        self.lapse(now);
        let count = self.running()?.total();
        let free = self.bound.saturating_sub(count);
        let for_sessions = self
            .bound
            .saturating_sub(self.reserved)
            .saturating_sub(count);
        let kind = asker.kind();
        let refused = match asker {
            Asker::Queue(repo) => {
                let sessions = self.session_waits.len();
                let owed = match self.turn {
                    Turn::Sessions => sessions.min(for_sessions),
                    Turn::Queue => 0,
                };
                let room = free.saturating_sub(owed);
                if room > self.ahead_of(repo, now)? {
                    // A run a waiting session could have taken: the next
                    // contested run is the sessions'.
                    if sessions > 0 && room <= for_sessions {
                        self.turn = Turn::Sessions;
                    }
                    self.queue_waits.remove(repo);
                    None
                } else {
                    self.queue_wait(repo, now);
                    Some(Why::AtBound)
                }
            }
            Asker::Session(id) => {
                if !self.session_waits.iter().any(|w| w.id == *id) {
                    self.session_waits.push(SessionWait {
                        id: *id,
                        repository: repository.to_path_buf(),
                    });
                }
                let first = self.session_waits.first().map(|w| w.id) == Some(*id);
                let owed = match self.turn {
                    Turn::Queue => self.queue_waits.len(),
                    Turn::Sessions => 0,
                };
                if for_sessions == 0 || !first {
                    Some(Why::AtBound)
                } else if free <= owed {
                    Some(Why::CombinationFirst)
                } else {
                    self.let_go(*id);
                    self.turn = Turn::Queue;
                    None
                }
            }
        };
        if let Some(why) = refused {
            return Ok(Asked::Refused(why));
        }
        let admission = match &self.count {
            Count::Registry(r) => {
                let (entry_kind, limit) = match kind {
                    RunKind::Queue => (host_count::Kind::Queue, self.bound),
                    RunKind::Session => (
                        host_count::Kind::Session,
                        self.bound.saturating_sub(self.reserved),
                    ),
                };
                let entry = host_count::Entry {
                    kind: entry_kind,
                    repository: repository.to_path_buf(),
                    place: String::new(),
                    run_dir: PathBuf::new(),
                    pid: std::process::id(),
                    admitted: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                };
                match r.admit(&entry, limit)? {
                    Some(a) => Some(a),
                    // An older sluice took the run between the count and
                    // the admission.
                    None => {
                        if let Asker::Queue(repo) = asker {
                            self.queue_wait(repo, now);
                        }
                        return Ok(Asked::Refused(Why::AtBound));
                    }
                }
            }
            Count::Own(_) | Count::Seen(_) => None,
        };
        let id = self.id();
        self.held.insert(
            id,
            Held {
                kind,
                repository: repository.to_path_buf(),
                admission,
            },
        );
        Ok(Asked::Held(id))
    }

    /// `repository`'s combination waits from `now`, or still waits.
    fn queue_wait(&mut self, repository: &Path, now: Instant) {
        self.queue_waits
            .entry(repository.to_path_buf())
            .and_modify(|w| w.last = now)
            .or_insert(QueueWait {
                since: now,
                last: now,
            });
    }

    /// The part `id` begins its run in `run_dir`, in the place `place`:
    /// its registry entry names them from now.
    pub fn begin(&mut self, id: u64, place: &str, run_dir: &Path) -> Result<()> {
        if let Some(Held {
            admission: Some(a), ..
        }) = self.held.get_mut(&id)
        {
            a.begin(place, run_dir)?;
        }
        Ok(())
    }

    /// The part `id`'s run's supervisor holds its run directory, which
    /// keeps it counted from now: the part is the room's no longer.
    pub fn started(&mut self, id: u64) {
        if let Some(held) = self.held.remove(&id) {
            if let Some(a) = held.admission {
                a.started();
            }
            if let Count::Own(going) = &mut self.count {
                going.insert(id, (held.kind, held.repository));
            }
        }
    }

    /// The part `id` dropped unstarted, by a start that failed, a refused
    /// write or a connection that closed: its room is free at once, and
    /// the repository it is owed to is answered, where one waits (D13).
    pub fn give_back(&mut self, id: u64) -> Option<PathBuf> {
        self.held.remove(&id)?;
        self.last = self.running().ok().map(|r| r.total());
        self.owed(Instant::now()).ok().flatten()
    }

    /// The repository a run that ended since the last look is owed to, at
    /// `now`, where a run ended and one waits (D13): the runs counted now
    /// are fewer than at the last look.
    pub fn freed(&mut self, now: Instant) -> Result<Option<PathBuf>> {
        let total = self.running()?.total();
        let was = self.last.replace(total);
        match was {
            Some(was) if total < was => self.owed(now),
            _ => Ok(None),
        }
    }

    /// The run started from part `id` ended, where no registry counts it.
    pub fn ended(&mut self, id: u64) {
        if let Count::Own(going) = &mut self.count {
            going.remove(&id);
        }
    }

    /// Whether any part is held or any run waits: what keeps the host
    /// process from ending (D6).
    pub fn busy(&mut self, now: Instant) -> bool {
        self.lapse(now);
        !self.held.is_empty() || !self.queue_waits.is_empty() || !self.session_waits.is_empty()
    }

    /// Combinations' waits whose rounds stopped asking go.
    fn lapse(&mut self, now: Instant) {
        self.queue_waits
            .retain(|_, w| now.saturating_duration_since(w.last) < QUEUE_WAIT);
    }

    fn id(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }
}

/// What a freed part's repository is told: the host process's task of it
/// passes at once (D13).
pub type Owed = Arc<dyn Fn(&Path) + Send + Sync>;

/// The room as every holder in one process shares it, with whom it tells
/// of a freed part's repository.
#[derive(Clone)]
pub struct Shared {
    room: Arc<Mutex<Room>>,
    owed: Arc<Mutex<Option<Owed>>>,
}

impl Shared {
    pub fn new(room: Room) -> Shared {
        Shared {
            room: Arc::new(Mutex::new(room)),
            owed: Arc::default(),
        }
    }

    /// The room, for one step; a holder that panicked leaves it as it was.
    pub fn lock(&self) -> MutexGuard<'_, Room> {
        self.room.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Tell `owed` of each repository a part that frees is owed to (D13).
    pub fn on_free(&self, owed: impl Fn(&Path) + Send + Sync + 'static) {
        *self.owed.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(owed));
    }

    /// Tell whom the room tells that a part `repository` is owed to frees.
    pub fn tell(&self, repository: &Path) {
        let owed = self.owed.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(owed) = owed {
            owed(repository);
        }
    }

    /// A part of `kind` for a run of `repository`, as the check seam's
    /// `hold` gives it; `None` where the room refuses it, a combination's
    /// wait recorded (71; rule core.49). A session's or a preview's ask
    /// here waits for nothing: its wait is let go as it is refused.
    pub fn hold(&self, kind: RunKind, repository: &Path) -> Result<Option<Part>> {
        let mut room = self.lock();
        let asked = match kind {
            RunKind::Queue => room.ask(
                &Asker::Queue(repository.to_path_buf()),
                repository,
                Instant::now(),
            )?,
            RunKind::Session => {
                let waiter = room.wait(repository);
                let asked = room.ask(&Asker::Session(waiter), repository, Instant::now());
                room.let_go(waiter);
                asked?
            }
        };
        drop(room);
        Ok(match asked {
            Asked::Held(id) => Some(self.part(kind, id)),
            Asked::Refused(_) => None,
        })
    }

    /// The seam's part for the room's part `id`, which gives it back when
    /// dropped unstarted.
    pub fn part(&self, kind: RunKind, id: u64) -> Part {
        Part::new(
            kind,
            Holding {
                room: self.clone(),
                id,
                done: false,
            },
        )
    }
}

/// A part of the room as the seam carries it: given back when dropped
/// unless its run's supervisor took it.
pub struct Holding {
    room: Shared,
    id: u64,
    done: bool,
}

impl Holding {
    /// Its run begins in `run_dir`, in the place `place`.
    pub fn begin(&self, place: &str, run_dir: &Path) -> Result<()> {
        self.room.lock().begin(self.id, place, run_dir)
    }

    /// Its run's supervisor holds the run directory.
    pub fn started(mut self) {
        self.room.lock().started(self.id);
        self.done = true;
    }
}

impl Drop for Holding {
    fn drop(&mut self) {
        if !self.done {
            let owed = self.room.lock().give_back(self.id);
            if let Some(repository) = owed {
                self.room.tell(&repository);
            }
        }
    }
}

/// A check seam whose parts are held of a room, every other ask its own:
/// the tests' fakes as sluice's host process holds them (design D14). The
/// room counts the runs going as the seam sees them.
pub struct Roomed<'a> {
    pub inner: &'a dyn Tests,
    pub room: Shared,
    /// The repository its runs are for.
    pub repository: PathBuf,
}

impl Roomed<'_> {
    /// Ask for a part for `asker` now: held, or refused with why.
    pub fn ask(&self, asker: &Asker) -> Result<Asked> {
        let running = self.inner.running()?;
        let mut room = self.room.lock();
        room.see(running);
        room.ask(asker, &self.repository, Instant::now())
    }
}

impl Tests for Roomed<'_> {
    fn start(&self, request: &RunRequest, part: Part) -> Result<RunHandle> {
        let kind = part.kind();
        let Ok(held) = part.take::<Holding>() else {
            return Err(AtBound.into());
        };
        // The seam counts the run from its start; the part is the room's
        // until then, and given back where the start fails.
        let run = self.inner.start(request, Part::new(kind, ()))?;
        held.started();
        Ok(run)
    }

    fn hold(&self, kind: RunKind) -> Result<Option<Part>> {
        let asked = match kind {
            RunKind::Queue => self.ask(&Asker::Queue(self.repository.clone()))?,
            RunKind::Session => {
                let waiter = self.room.lock().wait(&self.repository);
                let asked = self.ask(&Asker::Session(waiter));
                self.room.lock().let_go(waiter);
                asked?
            }
        };
        Ok(match asked {
            Asked::Held(id) => Some(self.room.part(kind, id)),
            Asked::Refused(_) => None,
        })
    }

    fn progress(&self, run: &RunHandle) -> Result<Progress> {
        self.inner.progress(run)
    }

    fn unmet(&self, limits: &Limits) -> Result<Option<String>> {
        self.inner.unmet(limits)
    }

    fn switched(&self, run: &RunHandle) -> Result<Option<String>> {
        self.inner.switched(run)
    }

    fn refused(&self, run: &RunHandle) -> Result<Refusals> {
        self.inner.refused(run)
    }

    fn bound(&self) -> Result<usize> {
        self.inner.bound()
    }

    fn platform(&self) -> Result<Platform> {
        self.inner.platform()
    }

    fn launch(&self, launch: &Launch) -> Result<()> {
        self.inner.launch(launch)
    }

    fn job_alive(
        &self,
        kind: &(String, Setting),
        claim: &str,
        remote: Option<&str>,
    ) -> Result<bool> {
        self.inner.job_alive(kind, claim, remote)
    }

    fn wake(&self, kind: &(String, Setting), branch: &str, remote: Option<&str>) -> Result<()> {
        self.inner.wake(kind, branch, remote)
    }

    fn running(&self) -> Result<Running> {
        let running = self.inner.running()?;
        let mut room = self.room.lock();
        room.see(running);
        room.running()
    }

    fn proven(&self, run: &RunHandle, report: &RunReport) -> Result<Proof> {
        self.inner.proven(run, report)
    }

    fn session_runs(&self) -> Result<Vec<HostRun>> {
        self.inner.session_runs()
    }

    fn logs(&self, run: &RunHandle, tasks: &[String]) -> Result<Vec<TaskLog>> {
        self.inner.logs(run, tasks)
    }

    fn tasks(&self, run: &RunHandle) -> Result<Vec<TaskProgress>> {
        self.inner.tasks(run)
    }

    fn kept_out(
        &self,
        worktree: &Path,
        tests: &Settings,
        required: &[String],
    ) -> Result<Vec<KeptOutTask>> {
        self.inner.kept_out(worktree, tests, required)
    }

    fn reads(&self, reading: &Reading) -> Result<Vec<Reads>> {
        self.inner.reads(reading)
    }

    fn equip(&self, equip: &Equip) -> Result<Equipped> {
        self.inner.equip(equip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        PathBuf::from("/host/repo.git")
    }

    fn queue() -> Asker {
        Asker::Queue(repo())
    }

    /// Hold a part of `asker` at `now`, for `repository`, its id.
    fn held_for(room: &mut Room, asker: &Asker, repository: &Path, now: Instant) -> u64 {
        match room.ask(asker, repository, now).unwrap() {
            Asked::Held(id) => id,
            Asked::Refused(why) => panic!("{asker:?} refused: {why:?}"),
        }
    }

    fn held(room: &mut Room, asker: &Asker, now: Instant) -> u64 {
        held_for(room, asker, &repo(), now)
    }

    fn refused_for(room: &mut Room, asker: &Asker, repository: &Path, now: Instant) -> Why {
        match room.ask(asker, repository, now).unwrap() {
            Asked::Held(id) => panic!("{asker:?} held {id}"),
            Asked::Refused(why) => why,
        }
    }

    fn refused(room: &mut Room, asker: &Asker, now: Instant) -> Why {
        refused_for(room, asker, &repo(), now)
    }

    /// A run of `asker`'s held and started at `now`, going until it ends.
    fn go(room: &mut Room, asker: &Asker, now: Instant) -> u64 {
        let id = held(room, asker, now);
        room.started(id);
        id
    }

    /// 150, S75: where a combination's run and a session's both wait for
    /// one free run, the combination's starts, whichever asks first.
    #[test]
    fn a_combination_takes_a_free_run_before_a_waiting_session() {
        let now = Instant::now();
        let mut room = Room::new(1, 0);
        let going = go(&mut room, &queue(), now);
        let agent = room.wait(&repo());
        assert_eq!(
            refused(&mut room, &Asker::Session(agent), now),
            Why::AtBound
        );
        assert_eq!(refused(&mut room, &queue(), now), Why::AtBound);
        room.ended(going);
        assert_eq!(
            refused(&mut room, &Asker::Session(agent), now),
            Why::CombinationFirst,
            "the free run is owed to the waiting combination"
        );
        held(&mut room, &queue(), now);
        assert_eq!(room.turn(), Turn::Sessions);
    }

    /// 150: once a combination's run started in room a waiting session
    /// could have taken, the next such room is the session's.
    #[test]
    fn the_next_contested_run_is_a_waiting_sessions_after_a_combination_took_one() {
        let now = Instant::now();
        let mut room = Room::new(1, 0);
        let going = go(&mut room, &queue(), now);
        let agent = room.wait(&repo());
        assert_eq!(
            refused(&mut room, &Asker::Session(agent), now),
            Why::AtBound
        );
        room.ended(going);
        let first = go(&mut room, &queue(), now);
        assert_eq!(room.turn(), Turn::Sessions);
        room.ended(first);
        // The next combination's ask comes first, and is refused.
        assert_eq!(refused(&mut room, &queue(), now), Why::AtBound);
        held(&mut room, &Asker::Session(agent), now);
        assert_eq!(room.turn(), Turn::Queue);
        assert_eq!(room.waiting(now).sessions, 0);
    }

    /// S75: two agents' checks hold both runs of a bound of two, a third
    /// waits, and a combination falls due; the combination takes the
    /// first run to end, and the third agent the second, though the
    /// branch's next combination waits.
    #[test]
    fn s75_turns_on_a_bound_of_two() {
        let now = Instant::now();
        let mut room = Room::new(2, 0);
        let (one, two) = (room.wait(&repo()), room.wait(&repo()));
        let first = go(&mut room, &Asker::Session(one), now);
        let second = go(&mut room, &Asker::Session(two), now);
        let third = room.wait(&repo());
        assert_eq!(
            refused(&mut room, &Asker::Session(third), now),
            Why::AtBound
        );
        assert_eq!(refused(&mut room, &queue(), now), Why::AtBound);
        assert_eq!(
            room.waiting(now),
            Waiting {
                queue: 1,
                sessions: 1
            }
        );

        room.ended(first);
        assert_eq!(
            refused(&mut room, &Asker::Session(third), now),
            Why::CombinationFirst
        );
        go(&mut room, &queue(), now);
        assert_eq!(
            refused(&mut room, &Asker::Session(third), now),
            Why::AtBound,
            "the third agent's check still waits"
        );

        room.ended(second);
        assert_eq!(refused(&mut room, &queue(), now), Why::AtBound);
        held(&mut room, &Asker::Session(third), now);
        assert_eq!(room.turn(), Turn::Queue);
    }

    /// 150: sessions' and previews' runs start in the order they came to
    /// wait, whatever repository they are for.
    #[test]
    fn sessions_start_in_the_order_they_came_to_wait() {
        let now = Instant::now();
        let mut room = Room::new(1, 0);
        let going = go(&mut room, &queue(), now);
        let other = PathBuf::from("/host/other.git");
        let (a, b) = (room.wait(&repo()), room.wait(&other));
        room.ended(going);
        assert_eq!(
            refused_for(&mut room, &Asker::Session(b), &other, now),
            Why::AtBound
        );
        held(&mut room, &Asker::Session(a), now);
        assert_eq!(
            refused_for(&mut room, &Asker::Session(b), &other, now),
            Why::AtBound
        );
        assert_eq!(room.waiting(now).sessions, 1);
    }

    /// run-bound, "A check waiting for room says why" (150): a check whose
    /// wait is at the bound, then owed to a combination, then at the bound
    /// again, as S75's third agent's is, says each reason once.
    #[test]
    fn a_waiting_check_says_each_reason_once() {
        let mut told = Told::default();
        let said: Vec<&str> = [
            Why::AtBound,
            Why::CombinationFirst,
            Why::AtBound,
            Why::CombinationFirst,
        ]
        .into_iter()
        .filter_map(|why| told.first(why))
        .collect();
        assert_eq!(
            said,
            [
                "waiting: the host is at its bound",
                "waiting: a combination's run starts first"
            ]
        );
    }

    /// 91, 102: the part of the bound kept for combinations is never a
    /// session's or a preview's; the free run is left for a combination.
    #[test]
    fn the_reserve_is_never_a_sessions() {
        let now = Instant::now();
        let mut room = Room::new(2, 1);
        let first = room.wait(&repo());
        go(&mut room, &Asker::Session(first), now);
        let second = room.wait(&repo());
        assert_eq!(
            refused(&mut room, &Asker::Session(second), now),
            Why::AtBound
        );
        held(&mut room, &queue(), now);
    }

    /// Record 0087: a combination's wait counts thirty seconds after its
    /// round last asked, and no longer.
    #[test]
    fn a_combinations_wait_lapses_thirty_seconds_after_its_last_ask() {
        let now = Instant::now();
        let mut room = Room::new(1, 0);
        let going = go(&mut room, &queue(), now);
        refused(&mut room, &queue(), now);
        room.ended(going);
        let agent = room.wait(&repo());
        let later = now + Duration::from_secs(29);
        assert_eq!(
            refused(&mut room, &Asker::Session(agent), later),
            Why::CombinationFirst
        );
        let lapsed = now + Duration::from_secs(31);
        assert_eq!(room.waiting(lapsed).queue, 0);
        held(&mut room, &Asker::Session(agent), lapsed);
    }

    /// Rule core.49: a part dropped unstarted frees its run at once.
    #[test]
    fn a_part_dropped_unstarted_frees_its_run_at_once() {
        let shared = Shared::new(Room::new(1, 0));
        let part = shared.hold(RunKind::Queue, &repo()).unwrap().expect("room");
        assert_eq!(shared.lock().running().unwrap().queue, 1);
        assert!(shared.hold(RunKind::Queue, &repo()).unwrap().is_none());
        drop(part);
        assert_eq!(shared.lock().running().unwrap(), Running::default());
        assert!(shared.hold(RunKind::Queue, &repo()).unwrap().is_some());
    }

    /// Two repositories beside this one's.
    fn others() -> (PathBuf, PathBuf) {
        (
            PathBuf::from("/host/busy.git"),
            PathBuf::from("/host/idle.git"),
        )
    }

    /// 71, record 0123 (D13): a run that frees goes to the repository with
    /// a combination waiting and the fewest runs under test, whichever asks
    /// first: the busy repository holding the host's other runs, a
    /// session's among them, asks first and is refused, its wait recorded,
    /// and the idle one takes the run.
    #[test]
    fn a_freed_part_goes_to_the_repository_with_the_fewest_under_test() {
        let now = Instant::now();
        let (busy, idle) = others();
        let mut room = Room::new(3, 0);
        let mut ids = Vec::new();
        for _ in 0..2 {
            ids.push(held_for(&mut room, &Asker::Queue(busy.clone()), &busy, now));
        }
        let agent = room.wait(&busy);
        ids.push(held_for(&mut room, &Asker::Session(agent), &busy, now));
        for id in &ids {
            room.started(*id);
        }
        // Both wait for the full host.
        let later = now + Duration::from_secs(1);
        refused_for(&mut room, &Asker::Queue(idle.clone()), &idle, later);
        refused_for(&mut room, &Asker::Queue(busy.clone()), &busy, later);
        assert_eq!(
            room.repositories(later).unwrap()[&busy],
            Share {
                under_test: 3,
                waiting: 1
            }
        );
        // One of the busy repository's runs ends, and it asks first.
        room.ended(ids[0]);
        let after = later + Duration::from_secs(1);
        assert_eq!(room.owed(after).unwrap(), Some(idle.clone()));
        assert_eq!(
            refused_for(&mut room, &Asker::Queue(busy.clone()), &busy, after),
            Why::AtBound
        );
        held_for(&mut room, &Asker::Queue(idle.clone()), &idle, after);
        assert_eq!(room.repositories(after).unwrap()[&idle].under_test, 1);
    }

    /// Record 0123 (D13): among repositories with as many runs under test,
    /// the one whose combination has waited longest takes the run that
    /// frees, though the other asks first.
    #[test]
    fn among_equals_the_longest_waiting_goes_first() {
        let now = Instant::now();
        let (first, second) = others();
        let mut room = Room::new(1, 0);
        let going = go(&mut room, &queue(), now);
        refused_for(&mut room, &Asker::Queue(first.clone()), &first, now);
        let later = now + Duration::from_secs(5);
        refused_for(&mut room, &Asker::Queue(second.clone()), &second, later);
        room.ended(going);
        let after = later + Duration::from_secs(1);
        assert_eq!(
            refused_for(&mut room, &Asker::Queue(second.clone()), &second, after),
            Why::AtBound,
            "the later wait asks first and is refused"
        );
        held_for(&mut room, &Asker::Queue(first.clone()), &first, after);
        // Its wait renewed, not begun again: the second goes next.
        assert_eq!(room.owed(after).unwrap(), Some(second));
    }

    /// Record 0123 (D13): the share never stops a run under test: a
    /// repository holding every run keeps them, and the idle one waits for
    /// the first to end.
    #[test]
    fn nothing_under_test_is_stopped_for_the_share() {
        let now = Instant::now();
        let (busy, idle) = others();
        let mut room = Room::new(2, 0);
        let a = held_for(&mut room, &Asker::Queue(busy.clone()), &busy, now);
        room.started(a);
        let b = held_for(&mut room, &Asker::Queue(busy.clone()), &busy, now);
        room.started(b);
        refused_for(&mut room, &Asker::Queue(idle.clone()), &idle, now);
        assert_eq!(room.running().unwrap().queue, 2, "both runs go on");
        assert_eq!(room.repositories(now).unwrap()[&busy].under_test, 2);
        assert_eq!(room.repositories(now).unwrap()[&idle].waiting, 1);
        room.ended(a);
        held_for(&mut room, &Asker::Queue(idle.clone()), &idle, now);
    }
}
