//! A task for each repository the host process holds (26, 129, 135;
//! design D5, D7, D14; the change `sluice-host-runs-on-a-tokio-edge`,
//! design D3).
//!
//! Each repository has a scheduler, a tokio task with an inbox and a
//! `watch` channel of its readings (`tokio-streams-before-rxrust@0b9fc11cda8b`).
//! A trigger marks a reading due. The scheduler starts a reading only where
//! none is in flight. When a reading ends, the scheduler sends it on the
//! channel, and starts one more reading where a trigger came during it.
//! Many triggers during one reading make one reading after it.
//!
//! A reading is a look or a pass. A look reads the repository as an idle
//! tick reads it, on tokio's pool for blocking work. A pass is a round of
//! the synchronous core on the repository's worker: a thread of the pool
//! for blocking work that holds the repository's binding for as long as
//! work is in play or something follows the repository. The binding holds
//! values that stay on one thread, so the worker keeps it, and the
//! scheduler asks the worker for each pass.
//!
//! These start a reading of a repository: a file event, a command, a
//! follower's opening, the end of a part of the host's bound that the
//! repository is owed, and the minute's backstop. While work is in play,
//! the worker passes at its tick, a second while a watch follows the
//! repository and three seconds otherwise. Each wait is a timer on tokio's
//! clock. No wait sleeps and looks again.
//!
//! A pass, as a look, asks the repository's remote at every reading where
//! work is in play, and where nothing is, once a minute at most, a
//! command's change, a watch's opening and a forecast's second ask having
//! the next reading ask at once (129 as amended 2026-11-19). The worker is
//! the repository's one reader on the host, and moves each repository that
//! the host process holds. After each reading that a follower holds the
//! repository for, the scheduler sends the repository's overview document
//! on a `watch` channel (the change `sluice-host-answers-every-read`,
//! design D6). A panic ends only the worker's thread, and the worker starts again at its
//! next tick.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use sluice_core::{Clock, Sluice};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use super::ask::Inside;
use super::clock::TokioClock;
use super::joined::Moment;
use super::log;
use super::pass::{self, Kept, Report};
use super::protocol::{FollowKind, Named, Reading};
use super::room::Shared;
use crate::exec::Host;
use crate::instance::Instance;
use crate::profiles::checks::RoomHandle;
use crate::profiles::{Bound, Tokens};
use crate::{Pause, Skipped};

/// How long a host process that ends gives its tasks to finish the round
/// each is in, and its looks to end (D8).
pub const FINISH_FOR: Duration = Duration::from_secs(60);

/// What the host process's look found of a repository: whether it has
/// work in play, read as an idle tick reads it, and the words a remote
/// refused its ask with (129, 135; design D4, D16).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Looked {
    pub in_play: bool,
    /// 152 (the change `sluice-host-makes-each-change`, design D3): a
    /// branch holds a landing the forge refused, whose rounds come at the
    /// minute.
    pub held: bool,
    pub refused: Option<String>,
    /// The branches it read as last fetched, each with the remote's
    /// refusal (the change `sluice-host-answers-every-read`, design D3).
    pub stale: BTreeMap<String, sluice_seams::ReadRefused>,
}

/// How the host process looks at a repository that no worker holds, and
/// whether the look asks the repository's remote or reads it as last
/// fetched (129 as amended 2026-11-19, 135; design D4). It runs on tokio's
/// pool for blocking work.
pub type InPlay = Arc<dyn Fn(&Path, bool) -> Looked + Send + Sync>;

/// A repository's binding, as its worker holds it on one thread.
pub trait Worker {
    /// Whether work is in play, read as an idle tick reads it.
    fn in_play(&mut self) -> bool;
    /// One pass, which asks the remote where `asks`, at its asker's time
    /// `at` where it names one (D14), its tick `tick`.
    fn pass(
        &mut self,
        asks: bool,
        at: Option<chrono::DateTime<chrono::Utc>>,
        tick: Duration,
    ) -> Result<Report, String>;
    /// One round of `branch`, as `sluice step --json` prints it, and
    /// whether work is in play after it.
    fn step(&mut self, branch: &str) -> (Result<serde_json::Value, String>, bool);
    /// Whether the repository is gone. A gone repository is held no
    /// longer.
    fn gone(&mut self) -> bool;
}

/// How a worker binds its repository, on its own thread: the binding, or
/// the words that say why it could not start.
pub type Bind = Arc<dyn Fn(&Path) -> Result<Box<dyn Worker>, String> + Send + Sync>;

/// A repository's overview document, read after a reading that ended at a
/// moment: `None` where none could be made (the change
/// `sluice-host-answers-every-read`, design D6).
pub type DocumentRead = Arc<
    dyn Fn(
            PathBuf,
            Moment,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<serde_json::Value>> + Send>>
        + Send
        + Sync,
>;

/// A repository's documents, as its subscribers receive them: `None`
/// before the first reading of a holding (design D6).
pub type Documents = watch::Receiver<Option<serde_json::Value>>;

/// How many times the schedulers woke, by what woke them: the measure of
/// the claim that the host process waits on events, not on a poll.
#[derive(Debug, Default)]
pub struct Wakes {
    /// A timer that fell due: a tick or the minute's backstop.
    pub timers: AtomicU64,
    /// A job came to the inbox.
    pub jobs: AtomicU64,
    /// Work in flight ended.
    pub landed: AtomicU64,
}

/// A moment of a reading: when, on tokio's clock, and its place in the
/// order of every moment marked, which tells two moments apart where a
/// paused clock gives both one time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    at: Instant,
    n: Moment,
}

impl Stamp {
    fn now() -> Stamp {
        Stamp {
            at: Instant::now(),
            n: Moment::now(),
        }
    }
}

/// One repository's reading, as the host process marks it: when its last
/// pass or look began and ended, whether the look made it, what a remote
/// refused it, when a command last told of a change to the repository,
/// and when its remote was last asked and whether work was in play then
/// (129; design D7, D15, D16). It also says whether the scheduler is
/// busy, a worker holds the repository, or a look is in flight.
#[derive(Clone, Debug, Default)]
pub struct Marks {
    began: Option<Stamp>,
    ended: Option<Stamp>,
    /// When the reading that last ended began.
    ended_began: Option<Stamp>,
    ended_at: Option<String>,
    /// The words the remote refused the last ask with, kept by the
    /// readings that ask nothing until the next ask.
    refused: Option<String>,
    /// Whether the look made the last reading, which writes no reader's
    /// document.
    looked: bool,
    /// The branches the last ask read as last fetched, with the remote's
    /// refusal, kept as `refused` is.
    stale: BTreeMap<String, sluice_seams::ReadRefused>,
    /// When a command last told of a change, its `serve`.
    told: Option<Stamp>,
    /// 129 as amended 2026-11-19: when a reading last began that asked the
    /// repository's remote.
    asked: Option<Stamp>,
    /// Whether the last reading to end found work in play.
    in_play: bool,
    /// When something last called for the remote to be asked at once: a
    /// watch's opening, a forecast's second ask, a binding made afresh.
    due: Option<Stamp>,
    /// Work is in flight, or a worker holds the repository: what keeps the
    /// host process from ending.
    busy: bool,
    /// A worker holds the repository.
    held: bool,
    /// A look is in flight.
    looking: bool,
}

impl Marks {
    /// 129 as amended 2026-11-19: whether a reading beginning at `now`
    /// asks the repository's remote: where none has asked yet, where the
    /// last reading found work in play, where the last ask began `every`
    /// ago or longer, and where a command told of a change or something
    /// called for an ask since the last ask began. Otherwise it reads the
    /// remote as the last ask fetched it.
    fn asks(&self, now: Stamp, every: Duration) -> bool {
        let Some(at) = self.asked else {
            return true;
        };
        self.in_play
            || now.at.saturating_duration_since(at.at) >= every
            || self.told.is_some_and(|t| t.n > at.n)
            || self.due.is_some_and(|d| d.n > at.n)
    }

    /// Whether the reading is current at `now` with `tick`: one that ended
    /// within its tick, or one under way that began within a tick of the
    /// last one's end (64, 129), and in either case one that began after a
    /// command last told of a change, so a read after a submit, a
    /// withdrawal or a branch's removal reads what it changed (D15, D16).
    fn current(&self, now: Instant, tick: Duration) -> bool {
        if self
            .told
            .is_some_and(|t| self.began.is_none_or(|b| b.n < t.n))
        {
            return false;
        }
        match (self.began, self.ended) {
            (Some(b), Some(e)) if b.n > e.n => b.at.saturating_duration_since(e.at) < tick,
            (_, Some(e)) => now.saturating_duration_since(e.at) < tick,
            _ => false,
        }
    }

    /// Whether the reading that last ended ended at or after `asked` and
    /// began after a command last told of a change, so it reads that
    /// change (D15, D16).
    fn read_since(&self, asked: Stamp) -> bool {
        self.ended.is_some_and(|e| e.n > asked.n)
            && self
                .told
                .is_none_or(|t| self.ended_began.is_some_and(|b| b.n > t.n))
    }

    fn answer(&self, current: bool) -> Reading {
        Reading {
            current,
            ended: self.ended_at.clone(),
            refused: self.refused.clone(),
            looked: self.looked,
            stale: self.stale.clone(),
        }
    }
}

/// What a scheduler is asked.
enum Job {
    /// A pass now, answered once it ends, at its asker's time where it
    /// names one (D14).
    Pass(
        oneshot::Sender<Result<Report, String>>,
        Option<chrono::DateTime<chrono::Utc>>,
    ),
    /// One round of a branch, answered with the step's document (D10).
    Step(String, oneshot::Sender<Result<serde_json::Value, String>>),
    /// A command holds the repository: a worker binds it.
    Hold,
    /// A command told of a change, or a freed part is owed to the
    /// repository: a worker binds it, and passes at once (D13).
    Wake,
    /// A one-shot read: a reading, answered with whether one is made.
    Read(oneshot::Sender<bool>),
    /// A file event under the repository's refs or records.
    Event,
    /// The host process's look at a repository in its index.
    Look,
    /// A follower came.
    Follow(FollowKind),
    /// A follower went.
    Unfollowed,
}

struct Slot {
    id: u64,
    kind: FollowKind,
    /// Where its reports go; `None` for a follower that takes none.
    tx: Option<mpsc::UnboundedSender<Report>>,
    /// Whether a pass began since it came: it is told only of such passes,
    /// so its first report is of the pass it asked for (D9).
    armed: bool,
}

type Followers = Arc<Mutex<Vec<Slot>>>;

/// A repository's scheduler, as the tasks hold it.
#[derive(Clone)]
struct Repo {
    /// The scheduler's own number, so a scheduler that ends removes only
    /// itself.
    id: u64,
    jobs: mpsc::UnboundedSender<Job>,
    marks: Arc<watch::Sender<Marks>>,
    followers: Followers,
    /// The repository's overview document (design D6).
    documents: Arc<watch::Sender<Option<serde_json::Value>>>,
}

impl Repo {
    /// Send `job` to the scheduler, its marks busy until the scheduler
    /// takes it, so the host process never reads itself idle between the
    /// two.
    fn send(&self, job: Job) -> Result<(), ()> {
        self.marks
            .send_if_modified(|m| !std::mem::replace(&mut m.busy, true));
        self.jobs.send(job).map_err(|_| ())
    }
}

/// The token and the tracker of the schedulers that run now.
struct Running {
    token: CancellationToken,
    tracker: TaskTracker,
}

struct Inner {
    repos: Mutex<BTreeMap<PathBuf, Repo>>,
    /// Repositories whose worker could not start, with the words it
    /// could not start with: events and the backstop pass them by until a
    /// command asks for them again.
    refused: Mutex<BTreeMap<PathBuf, String>>,
    /// The words last logged of each repository whose worker could not
    /// start, which a command's ask leaves as they are: a binding refused
    /// again for the same cause logs nothing more, until a worker binds
    /// the repository.
    said: Mutex<BTreeMap<PathBuf, String>>,
    bind: Bind,
    look: InPlay,
    /// Passes are taken only when asked (D14): a freed part wakes nothing,
    /// and every reading asks the remote, as a command's ask.
    asked_only: AtomicBool,
    /// 129 as amended 2026-11-19: how often a repository with nothing in
    /// play has its remote asked, at most: the minute's backstop.
    idle_ask: Duration,
    next: AtomicU64,
    running: Mutex<Running>,
    /// Moved each time a scheduler's busy mark changes, so the host
    /// process looks at whether it is idle.
    activity: watch::Sender<u64>,
    wakes: Arc<Wakes>,
    runtime: tokio::runtime::Handle,
    /// How a repository's document is read (design D6).
    document: std::sync::OnceLock<DocumentRead>,
    /// Each repository's change in flight, which its worker's passes and
    /// steps wait out, and which waits out the one in flight (the change
    /// `sluice-host-makes-each-change`, design D1).
    changing: Mutex<BTreeMap<PathBuf, Changing>>,
}

/// A repository's change in flight, against its worker's passes and
/// steps: a change holds it whole, and a pass or a step shares it.
pub type Changing = Arc<std::sync::RwLock<()>>;

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The host process's tasks, one scheduler for each repository it holds.
#[derive(Clone)]
pub struct Tasks(Arc<Inner>);

impl Tasks {
    /// The tasks, their schedulers on the runtime that this is called in.
    pub fn new(bind: Bind, look: InPlay) -> Tasks {
        Tasks(Arc::new(Inner {
            repos: Mutex::new(BTreeMap::new()),
            refused: Mutex::new(BTreeMap::new()),
            said: Mutex::new(BTreeMap::new()),
            bind,
            look,
            asked_only: AtomicBool::new(false),
            idle_ask: super::IDLE_ASK,
            next: AtomicU64::new(1),
            running: Mutex::new(Running {
                token: CancellationToken::new(),
                tracker: TaskTracker::new(),
            }),
            activity: watch::Sender::new(0),
            wakes: Arc::default(),
            runtime: tokio::runtime::Handle::current(),
            document: std::sync::OnceLock::new(),
            changing: Mutex::default(),
        }))
    }

    /// Read each repository's document with `read` after each reading
    /// that ends while a follower holds the repository (design D6).
    pub fn documenting(&self, read: DocumentRead) {
        let _ = self.0.document.set(read);
    }

    /// `repository`'s documents, its scheduler started where none runs;
    /// `None` while the tasks stop (design D6).
    pub fn documents(&self, repository: &Path) -> Option<Documents> {
        self.repo(repository).map(|r| r.documents.subscribe())
    }

    /// What a change to `repository` holds while it is made, which its
    /// worker's passes and steps share: a change waits out the pass in
    /// flight, and no pass begins until the change is made, so a pass
    /// never takes a lock the change reaches for (the change
    /// `sluice-host-makes-each-change`, design D1).
    pub fn changing(&self, repository: &Path) -> Changing {
        locked(&self.0.changing)
            .entry(repository.to_path_buf())
            .or_default()
            .clone()
    }

    /// The same tasks, passing only when asked (D14).
    pub fn passes_when_asked(self) -> Tasks {
        self.0.asked_only.store(true, Ordering::SeqCst);
        self
    }

    /// The same tasks, a repository with nothing in play having its remote
    /// asked every `every` at most: a test's minute.
    pub fn asking_idle_every(mut self, every: Duration) -> Tasks {
        Arc::get_mut(&mut self.0).expect("just made").idle_ask = every;
        self
    }

    /// How many times the schedulers woke, by what woke them.
    pub fn wakes(&self) -> Arc<Wakes> {
        self.0.wakes.clone()
    }

    /// A receiver that changes each time a scheduler's busy mark changes.
    pub fn activity(&self) -> watch::Receiver<u64> {
        self.0.activity.subscribe()
    }

    fn asked_only(&self) -> bool {
        self.0.asked_only.load(Ordering::SeqCst)
    }

    /// `repository`'s scheduler, started where none runs; `None` while the
    /// tasks stop.
    fn repo(&self, repository: &Path) -> Option<Repo> {
        let mut repos = locked(&self.0.repos);
        if let Some(r) = repos.get(repository) {
            return Some(r.clone());
        }
        let running = locked(&self.0.running);
        if running.token.is_cancelled() || running.tracker.is_closed() {
            return None;
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let repo = Repo {
            id: self.0.next.fetch_add(1, Ordering::SeqCst),
            jobs: tx,
            // Busy until its scheduler first says what it holds.
            marks: Arc::new(watch::Sender::new(Marks {
                busy: true,
                ..Marks::default()
            })),
            followers: Arc::default(),
            documents: Arc::new(watch::Sender::new(None)),
        };
        let scheduler = Scheduler {
            documents: repo.documents.clone(),
            tasks: self.clone(),
            repository: repository.to_path_buf(),
            id: repo.id,
            jobs: rx,
            marks: repo.marks.clone(),
            followers: repo.followers.clone(),
            token: running.token.child_token(),
            worker: None,
            broken: false,
            ended: false,
            flight: None,
            looking: None,
            commands: VecDeque::new(),
            wants_worker: false,
            reading_due: false,
            in_play: false,
            held: false,
            last_pass: None,
            stopping: false,
        };
        running.tracker.spawn_on(scheduler.run(), &self.0.runtime);
        repos.insert(repository.to_path_buf(), repo.clone());
        Some(repo)
    }

    fn send(&self, repository: &Path, job: Job) {
        if let Some(r) = self.repo(repository) {
            let _ = r.send(job);
        }
    }

    /// A part of the host's bound freed that `repository` is owed: a worker
    /// binds it, where none does, and passes at once, as a `serve` has it,
    /// so the repository the part is owed to takes it within a moment;
    /// under passes when asked, nothing passes by itself (D13, D14).
    pub fn owed(&self, repository: &Path) {
        if self.asked_only() {
            return;
        }
        self.send(repository, Job::Wake);
    }

    /// `repository`'s way for a command, a worker binding it where none
    /// does.
    pub fn handle(&self, repository: &Path) -> Handle {
        let repo = self.repo(repository);
        if let Some(r) = &repo {
            let _ = r.send(Job::Hold);
        }
        Handle {
            repo,
            tasks: self.clone(),
            repository: repository.to_path_buf(),
        }
    }

    /// The host process's look at `repository`, a repository in its index
    /// (129 as amended 2026-11-19, 135; design D4): a reading where no
    /// worker holds it and its remote is due to be asked, so an idle
    /// repository costs that one ask a minute alone. A repository whose
    /// worker could not start is passed by until a command asks for it
    /// again. The reading runs on tokio's pool for blocking work, so a
    /// remote that is slow to answer holds neither the loop that accepts
    /// commands' connections nor another repository's reading (rule
    /// core.29).
    pub fn look(&self, repository: &Path) {
        self.send(repository, Job::Look);
    }

    /// A file event under `repository`'s refs or records: a reading where
    /// nothing is in play, so a change another process made is read once
    /// its events stop.
    pub fn event(&self, repository: &Path) {
        self.send(repository, Job::Event);
    }

    /// A command told of a change to `repository`, its `serve`: no reading
    /// begun before now is current, so the next one-shot read waits for
    /// one that reads the change (D15, D16), and that reading asks the
    /// remote at once (129 as amended 2026-11-19).
    pub fn told(&self, repository: &Path) {
        if let Some(r) = self.repo(repository) {
            r.marks.send_modify(|m| m.told = Some(Stamp::now()));
        }
    }

    /// When a command last told of a change to `repository`, in the order
    /// of the moments that a read joins by (the change
    /// `sluice-host-answers-every-read`, design D5).
    pub fn told_at(&self, repository: &Path) -> Option<Moment> {
        self.marks(repository).told.map(|t| t.n)
    }

    /// The next reading of `repository` asks its remote at once, whatever
    /// is in play: a watch opened, a forecast asks a reading afresh, or a
    /// worker bound the repository afresh (129 as amended 2026-11-19).
    pub fn due(&self, repository: &Path) {
        if let Some(r) = self.repo(repository) {
            r.marks.send_modify(|m| m.due = Some(Stamp::now()));
        }
    }

    /// 129 (design D16): `repository` read for a one-shot read: answered
    /// at once where a reading of it is current, one that ended within
    /// `tick`, or one under way that began within a tick of the last one's
    /// end. Where none is, the scheduler makes a reading, by the worker's
    /// pass where one holds the repository and by a look otherwise, and
    /// this answers when a reading that began after a command last told
    /// of a change ends after the ask, waiting `within` at most, then
    /// answering with the reading as it stands. A reading under way when a
    /// command told of a change is followed by one more, so a read after a
    /// submit reads what it changed (D15). Where passes are taken only
    /// when asked, the reading is a look, which runs no round, so a test's
    /// rounds stay those its commands ask for (D14). With `afresh`, no
    /// reading is current but one begun after the ask, and that one asks
    /// the remote: a forecast's second ask, where the first reading lacked
    /// what it forms from (129 as amended 2026-11-19). A reading made for a
    /// read asks the remote only where it is due ([`Marks::asks`]).
    pub async fn read(
        &self,
        repository: &Path,
        tick: Duration,
        within: Duration,
        afresh: bool,
    ) -> Reading {
        let Some(repo) = self.repo(repository) else {
            return Marks::default().answer(false);
        };
        if afresh {
            repo.marks.send_modify(|m| m.due = Some(Stamp::now()));
        }
        let asked = Stamp::now();
        let mut marks = repo.marks.subscribe();
        let now = marks.borrow_and_update().clone();
        if !afresh && now.current(asked.at, tick) {
            return now.answer(true);
        }
        let (tx, made) = oneshot::channel();
        let _ = repo.send(Job::Read(tx));
        if !made.await.unwrap_or(false) {
            let m = marks.borrow().clone();
            return m.answer(m.current(Instant::now(), tick));
        }
        let until = asked.at + within;
        loop {
            {
                let m = marks.borrow_and_update();
                if m.read_since(asked) {
                    return m.answer(true);
                }
            }
            match tokio::time::timeout_at(until, marks.changed()).await {
                Ok(Ok(())) => {}
                _ => return marks.borrow().answer(false),
            }
        }
    }

    fn marks(&self, repository: &Path) -> Marks {
        locked(&self.0.repos)
            .get(repository)
            .map(|r| r.marks.borrow().clone())
            .unwrap_or_default()
    }

    /// `repository`'s last reading, as a read that it answers takes it
    /// (the change `sluice-host-answers-every-read`, design D6).
    pub fn last_reading(&self, repository: &Path) -> Reading {
        self.marks(repository).answer(true)
    }

    /// Whether a look at `repository` is in flight.
    pub fn looking(&self, repository: &Path) -> bool {
        self.marks(repository).looking
    }

    /// Whether a worker holds `repository`.
    pub fn holds(&self, repository: &Path) -> bool {
        self.marks(repository).held
    }

    /// The repositories a worker holds.
    pub fn held(&self) -> Vec<PathBuf> {
        locked(&self.0.repos)
            .iter()
            .filter(|(_, r)| r.marks.borrow().held)
            .map(|(p, _)| p.clone())
            .collect()
    }

    /// Whether any scheduler is busy: a worker holds its repository, or
    /// work is in flight.
    pub fn any(&self) -> bool {
        locked(&self.0.repos)
            .values()
            .any(|r| r.marks.borrow().busy)
    }

    /// `repository` left the host process's index: its scheduler ends
    /// once its work in flight ends.
    pub fn forget(&self, repository: &Path) {
        let repo = locked(&self.0.repos).remove(repository);
        drop(repo);
    }

    /// Every scheduler finishes the reading it is in, lets its worker go
    /// and ends, starting nothing: a newer sluice's hand-over, a signal,
    /// or a state folder gone (D8). It waits [`FINISH_FOR`] at most.
    pub async fn stop_all(&self) {
        let tracker = {
            let running = locked(&self.0.running);
            running.token.cancel();
            running.tracker.close();
            running.tracker.clone()
        };
        let _ = tokio::time::timeout(FINISH_FOR, tracker.wait()).await;
        locked(&self.0.repos).clear();
    }

    /// Start schedulers again after [`Tasks::stop_all`]: a hand-over whose
    /// successor did not start (design D20).
    pub fn resume(&self) {
        *locked(&self.0.running) = Running {
            token: CancellationToken::new(),
            tracker: TaskTracker::new(),
        };
    }
}

/// A command's way to a repository's scheduler.
pub struct Handle {
    repo: Option<Repo>,
    tasks: Tasks,
    repository: PathBuf,
}

impl Handle {
    fn send(&self, job: Job) -> Result<(), String> {
        self.repo
            .as_ref()
            .ok_or(())
            .and_then(|r| r.send(job))
            .map_err(|()| "the repository's task ended".to_string())
    }

    /// A pass now, once it ends, at its asker's time `at` where it names
    /// one (D14).
    pub async fn pass(&self, at: Option<chrono::DateTime<chrono::Utc>>) -> Result<Report, String> {
        let (tx, rx) = oneshot::channel();
        self.send(Job::Pass(tx, at))?;
        rx.await
            .map_err(|_| "the repository's task ended before its pass".to_string())?
    }

    /// One round of `branch`, as `sluice step --json` prints it.
    pub async fn step(&self, branch: &str) -> Result<serde_json::Value, String> {
        let (tx, rx) = oneshot::channel();
        self.send(Job::Step(branch.to_string(), tx))?;
        rx.await
            .map_err(|_| "the repository's task ended before its round".to_string())?
    }

    /// A pass at once.
    pub fn wake(&self) {
        let _ = self.send(Job::Wake);
    }

    /// Follow the repository's passes while the follower lives, told only
    /// of passes begun after it came. A wait's or a watch's follow asks a
    /// pass at once, so its first report is the host process's pass of
    /// the repository as it stands when it began to follow, whatever its
    /// timeout (D9), and a watch's opening has that pass ask the remote at
    /// once, whatever is in play (129 as amended 2026-11-19); `sluice run`
    /// follows the passes as they come.
    pub fn follow(&self, kind: FollowKind) -> Follower {
        self.following(kind, true)
    }

    /// Follow the repository as [`Handle::follow`] does; where not
    /// `reports`, no report is kept for the follower (design D6).
    pub fn following(&self, kind: FollowKind, reports: bool) -> Follower {
        let (tx, rx) = mpsc::unbounded_channel();
        let tx = reports.then_some(tx);
        let id = self.tasks.0.next.fetch_add(1, Ordering::SeqCst);
        let Some(repo) = &self.repo else {
            // The tasks stop: a follower whose task ended at once.
            return Follower {
                rx,
                followers: Arc::default(),
                id,
                jobs: None,
            };
        };
        // A watch's or a wait's coming asks the remote at once (the change
        // `sluice-host-answers-every-read`, design D6).
        if matches!(kind, FollowKind::Watch | FollowKind::Wait) {
            self.tasks.due(&self.repository);
        }
        locked(&repo.followers).push(Slot {
            id,
            kind,
            tx,
            armed: false,
        });
        let _ = repo.send(Job::Follow(kind));
        Follower {
            rx,
            followers: repo.followers.clone(),
            id,
            jobs: Some(repo.jobs.clone()),
        }
    }
}

/// A follower of a repository's passes, counted while it lives.
pub struct Follower {
    rx: mpsc::UnboundedReceiver<Report>,
    followers: Followers,
    id: u64,
    jobs: Option<mpsc::UnboundedSender<Job>>,
}

impl Follower {
    /// The next pass's report; `None` where the repository's task ended
    /// under it.
    pub async fn next(&mut self) -> Option<Report> {
        self.rx.recv().await
    }

    /// Its reports, and the follower that keeps the repository followed
    /// while it lives (design D6).
    pub fn reports(&mut self) -> mpsc::UnboundedReceiver<Report> {
        let (_, empty) = mpsc::unbounded_channel();
        std::mem::replace(&mut self.rx, empty)
    }
}

impl Drop for Follower {
    fn drop(&mut self) {
        locked(&self.followers).retain(|s| s.id != self.id);
        if let Some(jobs) = &self.jobs {
            let _ = jobs.send(Job::Unfollowed);
        }
    }
}

/// What a scheduler asks its worker.
enum Work {
    Pass {
        asks: bool,
        at: Option<chrono::DateTime<chrono::Utc>>,
        tick: Duration,
        reply: oneshot::Sender<Result<Report, String>>,
    },
    Step {
        branch: String,
        reply: oneshot::Sender<(Result<serde_json::Value, String>, bool)>,
    },
}

/// How a worker's thread ended.
#[derive(Debug, PartialEq, Eq)]
enum WorkerEnd {
    /// Its scheduler let it go.
    LetGo,
    /// Its repository is gone.
    Gone,
}

/// A scheduler's worker: the way to its thread, and the thread.
struct WorkerLink {
    work: mpsc::UnboundedSender<Work>,
    /// How its thread ended: its end, or the words it panicked with.
    join: oneshot::Receiver<Result<WorkerEnd, String>>,
}

/// Work in flight.
enum Flight {
    /// A worker binds the repository: whether work is in play, or why it
    /// could not start.
    Bind(oneshot::Receiver<Result<bool, String>>, WorkerLink),
    /// A pass, its reply to each command that asked for it.
    Pass {
        rx: oneshot::Receiver<Result<Report, String>>,
        asks: bool,
        replies: Vec<oneshot::Sender<Result<Report, String>>>,
    },
    Step {
        rx: oneshot::Receiver<(Result<serde_json::Value, String>, bool)>,
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    },
    /// The worker is let go: its thread's end.
    Unbind(oneshot::Receiver<Result<WorkerEnd, String>>),
}

/// How work in flight ended.
enum Landed {
    Bound(Result<bool, String>, WorkerLink),
    Passed(
        Option<Result<Report, String>>,
        bool,
        Vec<oneshot::Sender<Result<Report, String>>>,
    ),
    Stepped(
        Option<(Result<serde_json::Value, String>, bool)>,
        oneshot::Sender<Result<serde_json::Value, String>>,
    ),
    Looked(Looked, bool),
    Unbound(Result<WorkerEnd, String>),
}

/// Wait for the work in `flight` to end, and take it out.
async fn land(flight: &mut Option<Flight>) -> Landed {
    let landed = match flight.as_mut().expect("work in flight") {
        Flight::Bind(rx, _) => {
            let bound = rx
                .await
                .unwrap_or_else(|_| Err("its worker ended as it started".to_string()));
            let Some(Flight::Bind(_, link)) = flight.take() else {
                unreachable!()
            };
            return Landed::Bound(bound, link);
        }
        Flight::Pass { rx, .. } => rx.await.ok(),
        Flight::Step { rx, .. } => {
            let stepped = rx.await.ok();
            let Some(Flight::Step { reply, .. }) = flight.take() else {
                unreachable!()
            };
            return Landed::Stepped(stepped, reply);
        }
        Flight::Unbind(join) => {
            let ended = join
                .await
                .unwrap_or_else(|_| Err("its thread ended".to_string()));
            flight.take();
            return Landed::Unbound(ended);
        }
    };
    let Some(Flight::Pass { asks, replies, .. }) = flight.take() else {
        unreachable!()
    };
    Landed::Passed(landed, asks, replies)
}

/// A command that waits for the worker.
enum Command {
    Pass(
        oneshot::Sender<Result<Report, String>>,
        Option<chrono::DateTime<chrono::Utc>>,
    ),
    Step(String, oneshot::Sender<Result<serde_json::Value, String>>),
}

/// One repository's scheduler (D3).
struct Scheduler {
    /// The repository's overview document (design D6).
    documents: Arc<watch::Sender<Option<serde_json::Value>>>,
    tasks: Tasks,
    repository: PathBuf,
    id: u64,
    jobs: mpsc::UnboundedReceiver<Job>,
    marks: Arc<watch::Sender<Marks>>,
    followers: Followers,
    token: CancellationToken,
    worker: Option<WorkerLink>,
    /// The worker's thread ended under a pass: it is let go, and bound
    /// again at its next tick.
    broken: bool,
    /// The repository is gone.
    ended: bool,
    /// The worker's lane: a binding, a pass, a step or a worker let go.
    flight: Option<Flight>,
    /// The look's lane: a look, and whether it asks the remote. A look runs
    /// beside work of the worker's lane that is no reading, a step among
    /// it, so a one-shot read during a long step is answered at once. A
    /// pass and a look never run at once: one reading is in flight.
    looking: Option<(JoinHandle<Looked>, bool)>,
    commands: VecDeque<Command>,
    wants_worker: bool,
    reading_due: bool,
    in_play: bool,
    /// 152 (the change `sluice-host-makes-each-change`, design D3): the
    /// last reading found a landing the forge refused held, whose rounds
    /// come at the minute's backstop and keep the host process.
    held: bool,
    last_pass: Option<Instant>,
    stopping: bool,
}

impl Scheduler {
    async fn run(mut self) {
        let wakes = self.tasks.0.wakes.clone();
        loop {
            if self.flight.is_none() {
                self.start_next();
            }
            self.look_beside();
            self.publish();
            let idle = self.flight.is_none() && self.looking.is_none();
            if idle && (self.stopping || self.ended) {
                break;
            }
            let ready = idle && !self.stopping;
            let tick = self.tick_at().filter(|_| ready);
            let backstop = self.backstop_at().filter(|_| ready);
            let far = Instant::now() + Duration::from_secs(86_400);
            tokio::select! {
                biased;
                landed = land(&mut self.flight), if self.flight.is_some() => {
                    wakes.landed.fetch_add(1, Ordering::SeqCst);
                    self.landed(landed);
                }
                looked = async {
                    match self.looking.as_mut() {
                        Some((join, _)) => join.await,
                        None => std::future::pending().await,
                    }
                }, if self.looking.is_some() => {
                    wakes.landed.fetch_add(1, Ordering::SeqCst);
                    let (_, asks) = self.looking.take().expect("a look");
                    let looked = looked.unwrap_or(Looked {
                        in_play: true,
                        refused: None,
                        ..Looked::default()
                    });
                    self.landed(Landed::Looked(looked, asks));
                }
                () = self.token.cancelled(), if !self.stopping => {
                    self.stopping = true;
                }
                job = self.jobs.recv(), if !self.stopping => {
                    wakes.jobs.fetch_add(1, Ordering::SeqCst);
                    match job {
                        Some(job) => self.take(job),
                        None => self.stopping = true,
                    }
                }
                () = tokio::time::sleep_until(tick.unwrap_or(far)), if tick.is_some() => {
                    wakes.timers.fetch_add(1, Ordering::SeqCst);
                    self.reading_due = true;
                }
                () = tokio::time::sleep_until(backstop.unwrap_or(far)), if backstop.is_some() => {
                    wakes.timers.fetch_add(1, Ordering::SeqCst);
                    self.reading_due = true;
                }
            }
        }
        for command in self.commands.drain(..) {
            let ended = "the repository's task ended".to_string();
            match command {
                Command::Pass(reply, _) => drop(reply.send(Err(ended))),
                Command::Step(_, reply) => drop(reply.send(Err(ended))),
            }
        }
        // A repository gone, whatever ended its scheduler: its followers
        // get the document that says so (design D6).
        if !self.repository.is_dir() {
            self.gone();
        }
        locked(&self.followers).clear();
        self.marks.send_modify(|m| {
            m.busy = false;
            m.held = false;
            m.looking = false;
        });
        {
            let mut repos = locked(&self.tasks.0.repos);
            if repos.get(&self.repository).is_some_and(|r| r.id == self.id) {
                repos.remove(&self.repository);
            }
        }
        self.tasks.0.activity.send_modify(|n| *n += 1);
    }

    fn asked_only(&self) -> bool {
        self.tasks.asked_only()
    }

    fn refused(&self) -> bool {
        locked(&self.tasks.0.refused).contains_key(&self.repository)
    }

    fn watched(&self) -> bool {
        locked(&self.followers)
            .iter()
            .any(|s| s.kind == FollowKind::Watch)
    }

    fn followed(&self) -> bool {
        !locked(&self.followers).is_empty()
    }

    /// When the next pass falls due while work is in play: at once where
    /// none has passed, else a tick after the last.
    fn tick_at(&self) -> Option<Instant> {
        if self.asked_only() || !self.in_play || (self.worker.is_none() && self.refused()) {
            return None;
        }
        let every = tick(self.watched());
        Some(self.last_pass.map_or_else(Instant::now, |at| at + every))
    }

    /// The minute's backstop, where nothing is in play: a reading a minute
    /// after the last that asked the remote, for a change no event told
    /// of; at once where none has asked yet.
    fn backstop_at(&self) -> Option<Instant> {
        if self.in_play
            || (self.worker.is_none() && self.refused())
            || (self.worker.is_some() && self.asked_only())
        {
            return None;
        }
        let asked = self.marks.borrow().asked;
        Some(asked.map_or_else(Instant::now, |at| at.at + self.tasks.0.idle_ask))
    }

    /// Show what the scheduler holds and does in its marks, and move the
    /// tasks' activity where whether it is busy changed.
    fn publish(&mut self) {
        let busy = self.worker.is_some()
            || self.flight.is_some()
            || self.looking.is_some()
            || self.held
            || !self.jobs.is_empty();
        let held = self.worker.is_some()
            && !matches!(
                self.flight,
                Some(Flight::Unbind(_)) | Some(Flight::Bind(..))
            );
        let looking = self.looking.is_some();
        let mut moved = false;
        self.marks.send_if_modified(|m| {
            let changed = m.busy != busy || m.held != held || m.looking != looking;
            moved = m.busy != busy;
            m.busy = busy;
            m.held = held;
            m.looking = looking;
            changed
        });
        if moved {
            self.tasks.0.activity.send_modify(|n| *n += 1);
        }
    }

    /// Take a job from the inbox.
    fn take(&mut self, job: Job) {
        let started = |s: &Self| {
            locked(&s.tasks.0.refused).remove(&s.repository);
        };
        match job {
            Job::Pass(reply, at) => {
                started(self);
                self.commands.push_back(Command::Pass(reply, at));
            }
            Job::Step(branch, reply) => {
                started(self);
                self.commands.push_back(Command::Step(branch, reply));
            }
            Job::Hold => {
                started(self);
                self.wants_worker = true;
            }
            Job::Wake => {
                started(self);
                self.wants_worker = true;
                if !self.asked_only() {
                    self.reading_due = true;
                }
            }
            Job::Read(reply) => {
                // A reading in flight that began after a command last told
                // of a change answers the read as it ends (D15, D16).
                let m = self.marks.borrow().clone();
                let in_flight =
                    self.looking.is_some() || matches!(self.flight, Some(Flight::Pass { .. }));
                let answers =
                    in_flight && m.told.is_none_or(|t| m.began.is_some_and(|b| b.n > t.n));
                if !answers {
                    self.reading_due = true;
                }
                let _ = reply.send(true);
            }
            Job::Event => {
                // While work is in play the worker passes at its tick, and
                // where passes are taken only when asked, a worker's
                // repository is read only at an ask (D14).
                let held = self.worker.is_some() && (self.in_play || self.asked_only());
                if !held && !(self.worker.is_none() && self.refused()) {
                    self.reading_due = true;
                }
            }
            Job::Look => {
                let due = self.asked_only()
                    || self
                        .marks
                        .borrow()
                        .asks(Stamp::now(), self.tasks.0.idle_ask);
                if self.worker.is_none() && !self.refused() && due {
                    self.reading_due = true;
                }
            }
            Job::Follow(kind) => {
                self.wants_worker = true;
                if !self.asked_only() && matches!(kind, FollowKind::Wait | FollowKind::Watch) {
                    self.reading_due = true;
                }
            }
            Job::Unfollowed => {
                // A repository held again is read afresh: no document of
                // this holding is sent to the next follower (design D6).
                if !self.followed() {
                    self.documents.send_replace(None);
                }
            }
        }
    }

    /// Read the repository's document after a reading that ended now,
    /// where a follower holds the repository, and send it where it changed
    /// in a part other than its time (design D6). It names its tick (64,
    /// 129).
    fn document(&self) {
        if !self.followed() {
            return;
        }
        let Some(read) = self.tasks.0.document.get().cloned() else {
            return;
        };
        let documents = self.documents.clone();
        let repository = self.repository.clone();
        let every = tick(self.watched());
        let after = Moment::now();
        let tracker = locked(&self.tasks.0.running).tracker.clone();
        // A stop ends the read: no follower reads its document after it.
        let token = self.token.clone();
        tracker.spawn_on(
            async move {
                let doc = tokio::select! {
                    doc = read(repository, after) => doc,
                    () = token.cancelled() => None,
                };
                let Some(doc) = doc else {
                    return;
                };
                send_document(&documents, crate::overview::ticked(doc, every));
            },
            &self.tasks.0.runtime,
        );
    }

    /// The repository is gone: its followers get the document that says
    /// so, in the words its last read failed with where it failed, and it
    /// is held no longer (design D6).
    fn gone(&self) {
        let failed = self
            .documents
            .borrow()
            .clone()
            .filter(|d| d.get("error").is_some());
        let mut doc = failed.unwrap_or_else(|| {
            let words = format!("the repository at {} is gone", self.repository.display());
            crate::told::document(crate::told::Kind::Internal, &words, &[])
        });
        doc["gone"] = true.into();
        send_document(&self.documents, doc);
    }

    /// Start the next work, where none is in flight.
    fn start_next(&mut self) {
        if self.stopping || self.ended || self.broken {
            if let Some(link) = self.worker.take() {
                drop(link.work);
                self.flight = Some(Flight::Unbind(link.join));
            }
            self.broken = false;
            return;
        }
        if let Some(command) = self.commands.pop_front() {
            // A pass is a reading: it waits for the look in flight.
            if matches!(command, Command::Pass(..)) && self.looking.is_some() {
                self.commands.push_front(command);
                return;
            }
            if self.worker.is_none() {
                self.commands.push_front(command);
                self.bind();
                return;
            }
            match command {
                Command::Pass(reply, at) => self.pass(at, vec![reply]),
                Command::Step(branch, reply) => {
                    let (tx, rx) = oneshot::channel();
                    let sent = self
                        .worker
                        .as_ref()
                        .map(|w| w.work.send(Work::Step { branch, reply: tx }));
                    if !matches!(sent, Some(Ok(()))) {
                        self.broken = true;
                    }
                    self.flight = Some(Flight::Step { rx, reply });
                }
            }
            return;
        }
        if self.wants_worker && self.worker.is_none() {
            self.bind();
            return;
        }
        self.wants_worker = false;
        if self.reading_due {
            let asked_only = self.asked_only();
            if self.worker.is_some() && !asked_only {
                if self.looking.is_some() {
                    return;
                }
                self.reading_due = false;
                self.pass(None, Vec::new());
            } else if self.worker.is_none() && self.in_play && !asked_only && !self.refused() {
                self.bind();
            } else if self.looking.is_none() {
                self.reading_due = false;
                self.look();
            }
            return;
        }
        if self.worker.is_some() && !self.in_play && !self.followed() {
            let link = self.worker.take().expect("a worker");
            drop(link.work);
            self.flight = Some(Flight::Unbind(link.join));
        }
    }

    /// A worker binds the repository, on a thread of its own, entered
    /// into the runtime so it reads tokio's clock. The thread is no task of
    /// tokio's pool for blocking work: it lives as long as work is in play,
    /// and a paused clock moves on while it waits.
    fn bind(&mut self) {
        let (work, inbox) = mpsc::unbounded_channel();
        let (tx, rx) = oneshot::channel();
        let (ended, join) = oneshot::channel();
        let bind = self.tasks.0.bind.clone();
        let repository = self.repository.clone();
        let asked_only = self.asked_only();
        let runtime = self.tasks.0.runtime.clone();
        let changing = self.tasks.changing(&self.repository);
        let started = std::thread::Builder::new()
            .name("sluice-worker".to_string())
            .spawn(move || {
                let _entered = runtime.enter();
                let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker(bind, repository, asked_only, inbox, tx, changing)
                }));
                let _ = ended.send(ran.map_err(|panic| {
                    panic
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_default()
                }));
            });
        if let Err(e) = started {
            log(&format!(
                "sluice host: {}: its worker could not start: {e}",
                self.repository.display()
            ));
        }
        self.flight = Some(Flight::Bind(rx, WorkerLink { work, join }));
    }

    /// Its reading begins now (D7, D16): whether it asks the repository's
    /// remote, decided and marked as it begins (129 as amended
    /// 2026-11-19).
    fn begin_reading(&self) -> bool {
        let asked_only = self.asked_only();
        let idle_ask = self.tasks.0.idle_ask;
        let mut asks = false;
        self.marks.send_modify(|m| {
            let now = Stamp::now();
            m.began = Some(now);
            asks = asked_only || m.asks(now, idle_ask);
            if asks {
                m.asked = Some(now);
            }
        });
        asks
    }

    /// Its reading ends now, having found work in play where `in_play`,
    /// a look having made it where `looked` (D16). Where it `asked` the
    /// remote, its words are the remote's refusal, `refused`, or none; a
    /// reading that asked nothing keeps the last ask's.
    fn end_reading(
        &self,
        asked: bool,
        refused: Option<String>,
        stale: BTreeMap<String, sluice_seams::ReadRefused>,
        in_play: bool,
        looked: bool,
    ) {
        self.marks.send_modify(|m| {
            m.ended = Some(Stamp::now());
            m.ended_began = m.began;
            m.ended_at =
                Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
            if asked {
                m.refused = refused;
                m.stale = stale;
            }
            m.in_play = in_play;
            m.looked = looked;
        });
    }

    /// A pass by the worker, its followers armed (D9).
    fn pass(
        &mut self,
        at: Option<chrono::DateTime<chrono::Utc>>,
        replies: Vec<oneshot::Sender<Result<Report, String>>>,
    ) {
        for s in locked(&self.followers).iter_mut() {
            s.armed = true;
        }
        let asks = self.begin_reading();
        let (tx, rx) = oneshot::channel();
        let sent = self.worker.as_ref().map(|w| {
            w.work.send(Work::Pass {
                asks,
                at,
                tick: tick(self.watched()),
                reply: tx,
            })
        });
        if !matches!(sent, Some(Ok(()))) {
            self.broken = true;
        }
        self.flight = Some(Flight::Pass { rx, asks, replies });
    }

    /// A reading due during a step is a look made beside the step, so a
    /// read during a long step is answered once the look ends (the change
    /// `sluice-host-answers-every-read`, design D3); where passes are taken
    /// only when asked, so is one due during any work of the worker's lane
    /// that is no reading (D14).
    fn look_beside(&mut self) {
        let beside = match &self.flight {
            Some(Flight::Step { .. }) => true,
            Some(Flight::Pass { .. }) | None => false,
            Some(_) => self.asked_only(),
        };
        if self.reading_due && beside && self.looking.is_none() && !self.stopping && !self.ended {
            self.reading_due = false;
            self.look();
        }
    }

    /// A look, on tokio's pool for blocking work.
    fn look(&mut self) {
        let asks = self.begin_reading();
        let look = self.tasks.0.look.clone();
        let repository = self.repository.clone();
        let join = tokio::task::spawn_blocking(move || look(&repository, asks));
        self.looking = Some((join, asks));
    }

    /// Take in work that ended.
    fn landed(&mut self, landed: Landed) {
        match landed {
            Landed::Bound(Ok(in_play), link) => {
                self.worker = Some(link);
                locked(&self.tasks.0.refused).remove(&self.repository);
                locked(&self.tasks.0.said).remove(&self.repository);
                // 129 as amended 2026-11-19: a binding that has asked
                // nothing lists the branches on a remote by asking it, so
                // its first pass asks.
                self.marks.send_modify(|m| m.due = Some(Stamp::now()));
                if self.asked_only() {
                    self.in_play = in_play;
                } else {
                    self.in_play = true;
                    self.reading_due = true;
                }
            }
            Landed::Bound(Err(_), _) if self.followed() && !self.repository.is_dir() => {
                // The repository is gone: held no longer (design D6).
                self.wants_worker = false;
                self.ended = true;
                self.gone();
            }
            Landed::Bound(Err(words), _) => {
                self.wants_worker = false;
                {
                    let mut said = locked(&self.tasks.0.said);
                    if said.get(&self.repository) != Some(&words) {
                        log(&words);
                        said.insert(self.repository.clone(), words.clone());
                    }
                }
                locked(&self.tasks.0.refused).insert(self.repository.clone(), words);
                for command in self.commands.drain(..) {
                    let ended = "the repository's task ended".to_string();
                    match command {
                        Command::Pass(reply, _) => drop(reply.send(Err(ended))),
                        Command::Step(_, reply) => drop(reply.send(Err(ended))),
                    }
                }
            }
            Landed::Passed(report, asks, replies) => {
                let report = match report {
                    Some(r) => r,
                    None => {
                        self.broken = true;
                        Err("the repository's task ended before its pass".to_string())
                    }
                };
                self.in_play = report.as_ref().map_or(true, |r| r.in_play && !r.no_sluice);
                self.held = report.as_ref().is_ok_and(|r| r.held);
                self.end_reading(
                    asks,
                    report.as_ref().ok().and_then(|r| r.refused.clone()),
                    report.as_ref().map(|r| r.stale.clone()).unwrap_or_default(),
                    self.in_play,
                    false,
                );
                self.last_pass = Some(Instant::now());
                if let Ok(r) = &report {
                    locked(&self.followers).retain(|s| {
                        !s.armed || s.tx.as_ref().is_none_or(|tx| tx.send(r.clone()).is_ok())
                    });
                }
                for reply in replies {
                    let _ = reply.send(report.clone());
                }
                self.document();
            }
            Landed::Stepped(stepped, reply) => match stepped {
                Some((answer, in_play)) => {
                    self.in_play = in_play;
                    let _ = reply.send(answer);
                }
                None => {
                    self.broken = true;
                    let _ = reply.send(Err(
                        "the repository's task ended before its round".to_string()
                    ));
                }
            },
            Landed::Looked(looked, asks) => {
                self.end_reading(
                    asks,
                    looked.refused.clone(),
                    looked.stale.clone(),
                    looked.in_play,
                    true,
                );
                self.document();
                self.in_play = looked.in_play;
                self.held = looked.held;
                if looked.in_play && !self.refused() && !self.stopping {
                    self.wants_worker = true;
                }
            }
            Landed::Unbound(ended) => match ended {
                Ok(WorkerEnd::LetGo) => {}
                Ok(WorkerEnd::Gone) => {
                    self.ended = true;
                    self.gone();
                }
                Err(words) => {
                    log(&format!(
                        "sluice host: {}: its task panicked, and starts again at its next tick: {words}",
                        self.repository.display()
                    ));
                    // Its next tick binds a worker again.
                    self.in_play = true;
                    self.last_pass = Some(Instant::now());
                }
            },
        }
    }
}

/// Send `doc` to a repository's followers, where any follows and it
/// changed in a part other than its time (design D6).
fn send_document(documents: &watch::Sender<Option<serde_json::Value>>, doc: serde_json::Value) {
    if documents.receiver_count() == 0 {
        return;
    }
    let now = crate::overview::untimed(&doc);
    documents.send_if_modified(|d| {
        // A repository gone stays gone: a read in flight as it went sends
        // nothing after it.
        if d.as_ref().is_some_and(|d| d["gone"] == true) {
            return false;
        }
        let changed = d.as_ref().map(crate::overview::untimed).as_ref() != Some(&now);
        if changed {
            *d = Some(doc);
        }
        changed
    });
}

/// A worker's thread: its binding of `repository`, then each work its
/// scheduler asks, until the scheduler lets it go or the repository is
/// gone.
fn worker(
    bind: Bind,
    repository: PathBuf,
    asked_only: bool,
    mut inbox: mpsc::UnboundedReceiver<Work>,
    bound: oneshot::Sender<Result<bool, String>>,
    changing: Changing,
) -> WorkerEnd {
    let mut held = match bind(&repository) {
        Ok(w) => w,
        Err(words) => {
            let _ = bound.send(Err(words));
            return WorkerEnd::LetGo;
        }
    };
    let in_play = asked_only && held.in_play();
    let _ = bound.send(Ok(in_play));
    while let Some(work) = inbox.blocking_recv() {
        if held.gone() {
            return WorkerEnd::Gone;
        }
        match work {
            Work::Pass {
                asks,
                at,
                tick,
                reply,
            } => {
                let _sharing = changing.read().unwrap_or_else(|e| e.into_inner());
                let _ = reply.send(held.pass(asks, at, tick));
            }
            Work::Step { branch, reply } => {
                let _sharing = changing.read().unwrap_or_else(|e| e.into_inner());
                let _ = reply.send(held.step(&branch));
            }
        }
    }
    WorkerEnd::LetGo
}

/// The host's settings as the host process holds them, and how many
/// changes of them it took up: each repository's worker binds afresh at
/// the start of its next pass once the count moves, so no pass runs under
/// two settings (135; record 0107; rules core.14, core.54; design D19).
pub struct Settings {
    taken: AtomicU64,
    instance: Mutex<Instance>,
}

impl Settings {
    pub fn new(instance: Instance) -> Arc<Settings> {
        Arc::new(Settings {
            taken: AtomicU64::new(0),
            instance: Mutex::new(instance),
        })
    }

    /// How many changes were taken up, and the settings held now.
    pub fn current(&self) -> (u64, Instance) {
        let instance = locked(&self.instance);
        (self.taken.load(Ordering::SeqCst), instance.clone())
    }

    /// The settings held now.
    pub fn instance(&self) -> Instance {
        locked(&self.instance).clone()
    }

    /// Hold `instance`, a change taken up, which every worker binds at the
    /// start of its next pass.
    pub fn take_up(&self, instance: Instance) {
        let mut held = locked(&self.instance);
        *held = instance;
        self.taken.fetch_add(1, Ordering::SeqCst);
    }
}

/// How a repository's worker binds it in the host process: its own
/// binding, of the host's settings as `settings` holds them, its
/// identity's tokens held in the host process's `tokens`, holding its
/// parts of `room`, passing at each ask of its scheduler (design D5, D14,
/// D19; record 0108 as amended 2026-11-19).
pub fn repository_bind(
    room: Shared,
    asked_only: bool,
    named: Named,
    settings: Arc<Settings>,
    tokens: Tokens,
) -> Bind {
    Arc::new(move |common: &Path| -> Result<Box<dyn Worker>, String> {
        RepositoryWorker::open(
            common,
            room.clone(),
            asked_only,
            named.clone(),
            settings.clone(),
            tokens.clone(),
        )
        .map(|w| Box::new(w) as Box<dyn Worker>)
    })
}

/// The folder a repository is bound from: its main worktree where its
/// common git directory is one's `.git`, else the git directory itself.
pub fn open_dir(common: &Path) -> PathBuf {
    match common.parent() {
        Some(top) if common.file_name() == Some(".git".as_ref()) && top.join(".git") == common => {
            top.to_path_buf()
        }
        _ => common.to_path_buf(),
    }
}

/// How long a worker waits between its passes while work is in play (64,
/// 129).
fn tick(watched: bool) -> Duration {
    Duration::from_secs(if watched { 1 } else { 3 })
}

/// A repository's worker in the host process.
struct RepositoryWorker {
    common: PathBuf,
    room: Shared,
    named: Named,
    tokens: Tokens,
    settings: Arc<Settings>,
    /// The changes of the settings taken up when this binding was made.
    taken: u64,
    asked_only: bool,
    bound: Bound,
    /// 129: each policy read, by revision, kept across the worker's passes,
    /// and read once more under a binding made afresh (record 0107).
    policies: sluice_core::Policies,
    /// In a test's host process, a pass's time moves on by its tick as a
    /// waiting command's does (design D14).
    skipped: Skipped,
    clock: TokioClock,
    task: Task,
}

impl RepositoryWorker {
    fn open(
        common: &Path,
        room: Shared,
        asked_only: bool,
        named: Named,
        settings: Arc<Settings>,
        tokens: Tokens,
    ) -> Result<RepositoryWorker, String> {
        let (taken, instance) = settings.current();
        let bound = bind(common, &room, &named, &tokens, instance).map_err(|e| {
            format!(
                "sluice host: {}: could not be opened: {e:#}",
                common.display()
            )
        })?;
        let worker = RepositoryWorker {
            common: common.to_path_buf(),
            room,
            named,
            tokens,
            settings,
            taken,
            asked_only,
            task: Task {
                kept: Kept::default(),
                repository: common.to_string_lossy().into_owned(),
                in_play: true,
            },
            bound,
            policies: sluice_core::Policies::default(),
            skipped: Skipped::new(tick(false)),
            clock: TokioClock::new(),
        };
        // 87: a host whose config names a system it is not claims no run
        // here.
        if let Err(e) =
            sluice_of(&worker.bound, &worker.policies, &worker.clock).check_host_systems()
        {
            return Err(format!(
                "sluice host: {}: {e:#}; this host claims no run of it",
                common.display()
            ));
        }
        Ok(worker)
    }

    /// D19: a pass, or a step, under settings the host process took up
    /// since this binding was made binds afresh first, its reader's lock,
    /// followers and parts kept, as `Task` and the room hold them.
    /// Settings that do not bind here leave it on its binding.
    fn bind_afresh(&mut self) {
        let (now, instance) = self.settings.current();
        if now == self.taken {
            return;
        }
        self.taken = now;
        let fresh = bind(
            &self.common,
            &self.room,
            &self.named,
            &self.tokens,
            instance,
        )
        .and_then(|b| {
            sluice_of(&b, &sluice_core::Policies::default(), &self.clock).check_host_systems()?;
            Ok(b)
        });
        match fresh {
            Ok(b) => {
                self.bound = b;
                self.policies = sluice_core::Policies::default();
            }
            Err(e) => log(&format!(
                "sluice host: {}: the host's settings taken up do not bind here, so it keeps \
                 the settings it held: {e:#}",
                self.common.display()
            )),
        }
    }
}

impl Worker for RepositoryWorker {
    fn in_play(&mut self) -> bool {
        let clock: &dyn Clock = match self.asked_only {
            true => &self.skipped,
            false => &self.clock,
        };
        pass::in_play(&sluice_of(&self.bound, &self.policies, clock))
    }

    fn pass(
        &mut self,
        asks: bool,
        at: Option<chrono::DateTime<chrono::Utc>>,
        tick: Duration,
    ) -> Result<Report, String> {
        self.bind_afresh();
        // D14: the pass's time is its asker's where it names one, so two
        // waits asking passes move it no faster than one; else it moves on
        // by its tick after the pass.
        if let (true, Some(at)) = (self.asked_only, at) {
            self.skipped.set(at);
        }
        let clock: &dyn Clock = match self.asked_only {
            true => &self.skipped,
            false => &self.clock,
        };
        let s = sluice_of(&self.bound, &self.policies, clock);
        let _ = tick;
        let report = self.task.pass(&s, &self.bound, clock, asks);
        drop(s);
        let report = report.map(|r| crate::host_process::pass::Report {
            stale: self.bound.state.stale_all(),
            ..r
        });
        if self.asked_only && at.is_none() {
            self.skipped.pause();
        }
        report
    }

    fn step(&mut self, branch: &str) -> (Result<serde_json::Value, String>, bool) {
        self.bind_afresh();
        let clock: &dyn Clock = match self.asked_only {
            true => &self.skipped,
            false => &self.clock,
        };
        let s = sluice_of(&self.bound, &self.policies, clock);
        let answer =
            pass::step(&s, &*self.bound.pulls, clock, branch).map_err(|e| format!("{e:#}"));
        let in_play = pass::in_play(&s);
        (answer, in_play)
    }

    fn gone(&mut self) -> bool {
        let gone = self.bound.repository_gone(&self.task.repository);
        if gone {
            log(&format!(
                "sluice host: {}: gone; held no longer",
                self.common.display()
            ));
        }
        gone
    }
}

/// `common`'s binding, of the settings `instance`: its parts held of the
/// host's room, its results from the host process's index, its tokens the
/// host process's (design D12, D17; record 0108 as amended 2026-11-19).
fn bind(
    common: &Path,
    room: &Shared,
    named: &Named,
    tokens: &Tokens,
    instance: Instance,
) -> anyhow::Result<Bound> {
    let bound = Bound::open_holding(
        &open_dir(common),
        std::env::current_exe()?,
        instance,
        Rc::new(crate::retry::Policy::round().recording_where_set()),
        tokens,
    )?;
    Ok(Bound {
        tests: bound.tests.with_room(RoomHandle::Here(room.clone())),
        state: bound
            .state
            .reading_results(crate::profiles::records::ResultsFrom::Indexed),
        host_process: Box::new(Inside {
            room: room.clone(),
            named: named.clone(),
        }),
        ..bound
    })
}

/// A sluice over `bound`, as this host's peer, keeping `policies`.
fn sluice_of<'a>(
    bound: &'a Bound,
    policies: &'a sluice_core::Policies,
    clock: &'a dyn Clock,
) -> Sluice<'a> {
    Sluice::new(bound.seams(), clock)
        .as_peer(bound.peer())
        .keeping(policies)
}

/// What a repository's worker holds between its passes.
struct Task {
    kept: Kept,
    in_play: bool,
    repository: String,
}

impl Task {
    /// One pass, as the repository's driver on the host.
    fn pass(
        &mut self,
        s: &Sluice<'_>,
        bound: &Bound,
        clock: &dyn Clock,
        asks: bool,
    ) -> Result<Report, String> {
        let report = pass::pass(s, bound, &*bound.pulls, clock, asks, &mut self.kept)
            .map_err(|e| format!("{e:#}"));
        match &report {
            Ok(r) => {
                for line in &r.lines {
                    log(&format!("{}: {line}", self.repository));
                }
                for (branch, words) in &r.failed {
                    log(&format!("{}: {branch}: {words}", self.repository));
                }
                self.in_play = r.in_play && !r.no_sluice;
            }
            Err(e) => {
                log(&format!("{}: {e}", self.repository));
                self.in_play = true;
            }
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// What the fakes saw: each binding, each pass with whether it asked
    /// the remote, and each look with whether it asked.
    #[derive(Clone, Default)]
    struct Seen {
        binds: Arc<Mutex<Vec<PathBuf>>>,
        passes: Arc<Mutex<Vec<(PathBuf, bool)>>>,
        looks: Arc<Mutex<Vec<(PathBuf, bool)>>>,
    }

    impl Seen {
        fn binds(&self, r: &Path) -> usize {
            locked(&self.binds).iter().filter(|b| *b == r).count()
        }

        fn passes(&self, r: &Path) -> Vec<bool> {
            locked(&self.passes)
                .iter()
                .filter(|(p, _)| p == r)
                .map(|(_, asks)| *asks)
                .collect()
        }

        fn looks(&self, r: &Path) -> Vec<bool> {
            locked(&self.looks)
                .iter()
                .filter(|(p, _)| p == r)
                .map(|(_, asks)| *asks)
                .collect()
        }
    }

    /// A worker that records its passes, finding work in play where
    /// `work` says, each pass taking `takes`; the first binding of
    /// `panics` panics in its first pass.
    struct Fake {
        repository: PathBuf,
        seen: Seen,
        work: Arc<AtomicBool>,
        takes: Duration,
        panics: bool,
        refused: Option<String>,
    }

    impl Worker for Fake {
        fn in_play(&mut self) -> bool {
            self.work.load(Ordering::SeqCst)
        }

        fn pass(
            &mut self,
            asks: bool,
            _: Option<chrono::DateTime<chrono::Utc>>,
            _: Duration,
        ) -> Result<Report, String> {
            if self.panics {
                panic!("a fault in one repository's pass");
            }
            std::thread::sleep(self.takes);
            locked(&self.seen.passes).push((self.repository.clone(), asks));
            Ok(Report {
                in_play: self.work.load(Ordering::SeqCst),
                refused: self.refused.clone().filter(|_| asks),
                ..Report::default()
            })
        }

        fn step(&mut self, _: &str) -> (Result<serde_json::Value, String>, bool) {
            (
                Ok(serde_json::Value::Null),
                self.work.load(Ordering::SeqCst),
            )
        }

        fn gone(&mut self) -> bool {
            false
        }
    }

    /// The fakes' binding: work in play where `work` says; a repository
    /// named in `refused` refuses to start; `panics` panics in its first
    /// binding's pass.
    struct Fakes {
        seen: Seen,
        work: Arc<AtomicBool>,
        takes: Duration,
        refused: Option<PathBuf>,
        panics: Option<PathBuf>,
        /// The words the remote refuses each ask with.
        remote: Option<String>,
    }

    impl Default for Fakes {
        fn default() -> Fakes {
            Fakes {
                seen: Seen::default(),
                work: Arc::default(),
                takes: Duration::ZERO,
                refused: None,
                panics: None,
                remote: None,
            }
        }
    }

    impl Fakes {
        fn bind(&self) -> Bind {
            let (seen, work, takes) = (self.seen.clone(), self.work.clone(), self.takes);
            let (refused, panics) = (self.refused.clone(), self.panics.clone());
            let remote = self.remote.clone();
            Arc::new(move |r: &Path| {
                locked(&seen.binds).push(r.to_path_buf());
                if refused.as_deref() == Some(r) {
                    return Err("its host's systems are not this host's".to_string());
                }
                let first = locked(&seen.binds).iter().filter(|b| *b == r).count() == 1;
                Ok(Box::new(Fake {
                    repository: r.to_path_buf(),
                    seen: seen.clone(),
                    work: work.clone(),
                    takes,
                    panics: first && panics.as_deref() == Some(r),
                    refused: remote.clone(),
                }) as Box<dyn Worker>)
            })
        }

        /// A look that finds work in play where `work` says, its remote's
        /// refusal `refused` where it asks, taking `takes`.
        fn look(&self, refused: Option<&str>) -> InPlay {
            let (seen, work, takes) = (self.seen.clone(), self.work.clone(), self.takes);
            let refused = refused.map(str::to_string);
            Arc::new(move |r: &Path, asks: bool| {
                std::thread::sleep(takes);
                locked(&seen.looks).push((r.to_path_buf(), asks));
                Looked {
                    in_play: work.load(Ordering::SeqCst),
                    refused: refused.clone().filter(|_| asks),
                    ..Looked::default()
                }
            })
        }

        fn tasks(&self) -> Tasks {
            Tasks::new(self.bind(), self.look(None))
        }
    }

    /// `check` holds within ten seconds of the system's clock: a paused
    /// clock moves on while a worker's thread runs on its own, so tokio's
    /// clock bounds no wait for a worker.
    async fn soon(what: &str, check: impl Fn() -> bool) {
        let until = std::time::Instant::now() + Duration::from_secs(30);
        while !check() {
            assert!(std::time::Instant::now() < until, "{what}: not within 10s");
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }

    /// Whether a reading of `repository` is under way: one began after the
    /// last ended. A fake worker counts its pass before its scheduler marks
    /// the pass's end.
    fn reading(tasks: &Tasks, repository: &Path) -> bool {
        let m = tasks.marks(repository);
        m.began.map(|b| b.n) > m.ended.map(|e| e.n)
    }

    /// Let every task run that can, and every blocking call end, on a
    /// paused clock, the clock not moved.
    async fn settle() {
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        let blocking = tokio::task::spawn_blocking(|| ());
        let _ = blocking.await;
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
    }

    /// 26, 135 (design D5): a worker that panics ends its own thread alone,
    /// and is bound again at its next tick; another repository's worker
    /// passes meanwhile.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_worker_that_panics_is_bound_again_at_its_next_tick() {
        let repo = PathBuf::from("/host/a.git");
        let fakes = Fakes {
            panics: Some(repo.clone()),
            ..Fakes::default()
        };
        fakes.work.store(true, Ordering::SeqCst);
        let tasks = fakes.tasks();
        tasks.handle(&repo).wake();
        soon("the worker panicked", || {
            fakes.seen.binds(&repo) == 1 && !tasks.holds(&repo)
        })
        .await;
        assert!(fakes.seen.passes(&repo).is_empty());
        // Another repository's worker passes meanwhile.
        let other = PathBuf::from("/host/b.git");
        assert!(tasks.handle(&other).pass(None).await.is_ok());
        // Its next tick, three seconds on, binds it again, and it passes.
        soon("bound again", || !fakes.seen.passes(&repo).is_empty()).await;
        assert_eq!(fakes.seen.binds(&repo), 2);
    }

    /// D13, D14: a part of the host's bound that frees has the repository
    /// it is owed to, one waiting with none under test, pass at once; under
    /// passes when asked, nothing passes by itself.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_freed_part_wakes_the_repository_it_is_owed_to() {
        use super::super::room::{Room, Shared};
        use sluice_seams::RunKind;
        let (busy, idle) = (
            PathBuf::from("/host/busy.git"),
            PathBuf::from("/host/idle.git"),
        );
        for asked_only in [false, true] {
            let fakes = Fakes::default();
            let tasks = match asked_only {
                true => fakes.tasks().passes_when_asked(),
                false => fakes.tasks(),
            };
            let room = Shared::new(Room::new(1, 0));
            let told = tasks.clone();
            room.on_free(move |r| told.owed(r));
            let part = room
                .hold(RunKind::Queue, &busy, "main")
                .unwrap()
                .expect("room");
            assert!(
                room.hold(RunKind::Queue, &idle, "main").unwrap().is_none(),
                "the idle repository waits"
            );
            drop(part);
            tokio::time::sleep(Duration::from_millis(300)).await;
            if asked_only {
                assert!(fakes.seen.passes(&idle).is_empty());
            } else {
                soon("the idle repository passes", || {
                    !fakes.seen.passes(&idle).is_empty()
                })
                .await;
            }
            assert!(fakes.seen.passes(&busy).is_empty());
        }
    }

    /// 129 (design D16): a read is answered once a reading of its
    /// repository is current: one ended within its tick at once; with none,
    /// a look's or the worker's next pass's, its refusal named; under passes
    /// when asked, from a look, though a worker holds the repository.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_read_is_answered_once_a_reading_is_current() {
        let repo = PathBuf::from("/host/read.git");
        let fakes = Fakes {
            remote: Some("origin: 503 Service Unavailable".to_string()),
            ..Fakes::default()
        };
        let tasks = Tasks::new(
            fakes.bind(),
            fakes.look(Some("origin: 503 Service Unavailable")),
        );
        let (tick, within) = (Duration::from_millis(500), Duration::from_secs(5));

        // No reading: a look reads now, and its refusal is named.
        let read = tasks.read(&repo, tick, within, false).await;
        assert!(read.current && read.ended.is_some(), "{read:?}");
        assert_eq!(
            read.refused.as_deref(),
            Some("origin: 503 Service Unavailable")
        );
        assert_eq!(fakes.seen.looks(&repo).len(), 1);
        // One ended within its tick: at once, nothing read again.
        let began = Instant::now();
        assert!(tasks.read(&repo, tick, within, false).await.current);
        assert!(began.elapsed() < Duration::from_millis(50));
        assert_eq!(fakes.seen.looks(&repo).len(), 1);
        // Past its tick: read again.
        tokio::time::sleep(tick).await;
        assert!(tasks.read(&repo, tick, within, false).await.current);
        assert_eq!(fakes.seen.looks(&repo).len(), 2);

        // With a worker holding it, its next pass is the reading: a wait's
        // follow asks a pass of its own first (D9), which asks the remote,
        // as a binding made afresh has asked nothing yet.
        let _held = tasks.handle(&repo).follow(FollowKind::Wait);
        // The bind's own pass and the follow's are one pass, or two where
        // the follow came while the first was under way.
        soon("the follow's own pass", || {
            !fakes.seen.passes(&repo).is_empty() && !reading(&tasks, &repo)
        })
        .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!reading(&tasks, &repo));
        let passed = fakes.seen.passes(&repo).len();
        assert!(fakes.seen.passes(&repo)[0], "the first pass asks");
        tokio::time::sleep(tick).await;
        let read = tasks.read(&repo, tick, within, false).await;
        assert!(read.current, "{read:?}");
        // Nothing in play and the remote asked within the minute: the pass
        // asked nothing, and the last ask's refusal stands (129 as amended
        // 2026-11-19).
        assert_eq!(
            read.refused.as_deref(),
            Some("origin: 503 Service Unavailable")
        );
        assert_eq!(fakes.seen.passes(&repo).len(), passed + 1);
        assert_eq!(fakes.seen.passes(&repo).last(), Some(&false));
        assert_eq!(fakes.seen.looks(&repo).len(), 2);

        // Under passes when asked, a look reads, running no pass, though a
        // worker holds the repository.
        let asked = Tasks::new(fakes.bind(), fakes.look(None)).passes_when_asked();
        let _task = asked.handle(&repo).follow(FollowKind::Wait);
        let read = asked.read(&repo, tick, within, false).await;
        assert!(read.current && read.ended.is_some(), "{read:?}");
        assert_eq!(
            fakes.seen.passes(&repo).len(),
            passed + 1,
            "no pass was run"
        );
        assert_eq!(fakes.seen.looks(&repo).len(), 3);
        // Asked afresh, it reads again though its reading is current.
        assert!(asked.read(&repo, tick, within, false).await.current);
        assert_eq!(fakes.seen.looks(&repo).len(), 3);
        assert!(asked.read(&repo, tick, within, true).await.current);
        assert_eq!(fakes.seen.looks(&repo).len(), 4);
    }

    /// 129 (design D15, D16): a reading begun before a command told of a
    /// change, its `serve`, is not current, so the next read reads the
    /// change, by a look under passes when asked; the look's answer says
    /// the look made it, which wrote no reader's document.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_read_after_a_change_reads_it() {
        let repo = PathBuf::from("/host/told.git");
        let fakes = Fakes::default();
        let tasks = fakes.tasks().passes_when_asked();
        let (tick, within) = (Duration::from_secs(60), Duration::from_secs(5));
        let _task = tasks.handle(&repo).follow(FollowKind::Wait);
        tokio::time::sleep(Duration::from_millis(300)).await;
        let looked = fakes.seen.looks(&repo).len();
        // A pass's reading: current, and no look's.
        tasks.handle(&repo).pass(None).await.unwrap();
        let read = tasks.read(&repo, tick, within, false).await;
        assert!(read.current && !read.looked, "{read:?}");
        assert_eq!(fakes.seen.looks(&repo).len(), looked);
        // Told of a change: read again, by a look, though within its tick.
        tasks.told(&repo);
        let read = tasks.read(&repo, tick, within, false).await;
        assert!(read.current && read.looked, "{read:?}");
        assert_eq!(fakes.seen.looks(&repo).len(), looked + 1);
        // The look's reading began after the change: current, read once.
        assert!(tasks.read(&repo, tick, within, false).await.current);
        assert_eq!(fakes.seen.looks(&repo).len(), looked + 1);
    }

    /// 129 (design D15, D16): a look under way when a command told of a
    /// change, the look a host process makes of its index as it starts,
    /// that ends after a read was asked, is not that read's answer: the
    /// read's trigger during it makes one more look, and the read answers
    /// from the look begun after the change, so the next read is answered
    /// at once.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_reading_under_way_when_told_is_read_again() {
        let repo = PathBuf::from("/host/under-way.git");
        let fakes = Fakes {
            takes: Duration::from_millis(200),
            ..Fakes::default()
        };
        let tasks = fakes.tasks().passes_when_asked();
        let (tick, within) = (Duration::from_secs(60), Duration::from_secs(5));
        tasks.look(&repo);
        soon("the look under way", || tasks.looking(&repo)).await;
        tasks.told(&repo);
        let read = tasks.read(&repo, tick, within, false).await;
        assert!(read.current && read.looked, "{read:?}");
        assert_eq!(fakes.seen.looks(&repo).len(), 2, "looked again once told");
        let began = Instant::now();
        assert!(tasks.read(&repo, tick, within, false).await.current);
        assert!(began.elapsed() < Duration::from_millis(50));
        assert_eq!(fakes.seen.looks(&repo).len(), 2);
    }

    /// reader-tick, "An idle tick starts no process" (129, 135; design
    /// D4): the host process's look binds no worker for a repository with
    /// nothing in play, binds one for a repository with work, and passes by
    /// one whose worker could not start, saying why once for each cause,
    /// until a command asks for it, which binds it again and says nothing
    /// more where the cause is the same.
    #[tokio::test(start_paused = true)]
    async fn a_look_binds_a_worker_only_where_work_is_in_play() {
        let (idle, busy, refused) = (
            PathBuf::from("/host/idle.git"),
            PathBuf::from("/host/busy.git"),
            PathBuf::from("/host/refused.git"),
        );
        let fakes = Fakes {
            refused: Some(refused.clone()),
            ..Fakes::default()
        };
        let busy_work = Arc::new(AtomicBool::new(true));
        let work = busy_work.clone();
        let busy_too = busy.clone();
        let idle_look = fakes.look(None);
        let looks: InPlay = Arc::new(move |r: &Path, asks: bool| {
            let looked = idle_look(r, asks);
            Looked {
                in_play: r != Path::new("/host/idle.git")
                    && (r != busy_too.as_path() || work.load(Ordering::SeqCst)),
                ..looked
            }
        });
        let tasks = Tasks::new(fakes.bind(), looks);

        tasks.look(&idle);
        soon("the idle repository's look", || {
            fakes.seen.looks(&idle).len() == 1
        })
        .await;
        settle().await;
        assert_eq!(
            fakes.seen.binds(&idle),
            0,
            "no worker for an idle repository"
        );

        tasks.look(&busy);
        soon("the busy repository's worker binds", || {
            fakes.seen.binds(&busy) == 1
        })
        .await;

        tasks.look(&refused);
        soon("the refused worker", || fakes.seen.binds(&refused) == 1).await;
        settle().await;
        assert!(!tasks.holds(&refused));
        tasks.look(&refused);
        tasks.event(&refused);
        tokio::time::advance(IDLE_ASK_TEST).await;
        settle().await;
        assert_eq!(fakes.seen.binds(&refused), 1, "the look passes it by");
        assert_eq!(
            locked(&tasks.0.refused).get(&refused).map(String::as_str),
            Some("its host's systems are not this host's")
        );
        // A command's ask binds it again; the words said stay said.
        tasks.handle(&refused);
        soon("a command's ask binds it", || {
            fakes.seen.binds(&refused) == 2
        })
        .await;
        settle().await;
        assert_eq!(
            locked(&tasks.0.said).get(&refused).map(String::as_str),
            Some("its host's systems are not this host's")
        );
        assert_eq!(fakes.seen.binds(&idle), 0);
        busy_work.store(false, Ordering::SeqCst);
    }

    const IDLE_ASK_TEST: Duration = Duration::from_secs(61);

    /// Tasks that pass only when asked, each document read giving the
    /// next of `docs` (the change `sluice-host-answers-every-read`, design
    /// D6), and how many documents were read.
    fn documented(docs: Vec<serde_json::Value>) -> (Tasks, Arc<AtomicUsize>) {
        let tasks = Fakes::default().tasks().passes_when_asked();
        let read = Arc::new(AtomicUsize::new(0));
        let n = read.clone();
        let docs = Arc::new(docs);
        tasks.documenting(Arc::new(move |_, _| {
            let i = n.fetch_add(1, Ordering::SeqCst);
            let doc = docs.get(i).or(docs.last()).cloned();
            Box::pin(async move { doc })
        }));
        (tasks, read)
    }

    /// design D6: a document that changed only in its time, `at`, is not
    /// sent: a subscriber's receiver does not wake for it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_document_that_changed_only_its_time_wakes_no_receiver() {
        let repo = PathBuf::from("/host/documented.git");
        let (tasks, read) = documented(vec![
            serde_json::json!({"at": "1", "branches": ["main"]}),
            serde_json::json!({"at": "2", "branches": ["main"]}),
            serde_json::json!({"at": "3", "branches": ["main", "bolt/x"]}),
        ]);
        let mut documents = tasks.documents(&repo).expect("its channel");
        let handle = tasks.handle(&repo);
        let _follower = handle.following(FollowKind::Watch, false);
        handle.pass(None).await.expect("a pass");
        documents.changed().await.expect("the first document");
        assert_eq!(documents.borrow_and_update().as_ref().unwrap()["at"], "1");
        handle.pass(None).await.expect("a pass");
        soon("the second document is read", || {
            read.load(Ordering::SeqCst) == 2
        })
        .await;
        settle().await;
        assert!(
            !documents.has_changed().unwrap(),
            "woken for its time alone"
        );
        handle.pass(None).await.expect("a pass");
        documents.changed().await.expect("the third document");
        let doc = documents.borrow_and_update().clone().unwrap();
        assert_eq!(doc["branches"], serde_json::json!(["main", "bolt/x"]));
        assert_eq!(doc["tick"], 1, "a watch follows: its tick is a second");
    }

    /// resident-subscriptions, "A client that reads nothing for ten
    /// seconds" (design D6): three passes each change the document while a
    /// subscriber reads nothing; the next document it reads is the newest,
    /// and the two older ones never reach it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_slow_subscriber_gets_the_newest_overview() {
        let repo = PathBuf::from("/host/slow-subscriber.git");
        let (tasks, read) = documented(
            (1..=3)
                .map(|n| serde_json::json!({"at": n.to_string(), "n": n}))
                .collect(),
        );
        let documents = tasks.documents(&repo).expect("its channel");
        let handle = tasks.handle(&repo);
        let _follower = handle.following(FollowKind::Wait, false);
        for _ in 0..3 {
            handle.pass(None).await.expect("a pass");
        }
        soon("three documents are read", || {
            read.load(Ordering::SeqCst) == 3
        })
        .await;
        soon("the newest is sent", || {
            documents.borrow().as_ref().is_some_and(|d| d["n"] == 3)
        })
        .await;
        let mut stream = tokio_stream::wrappers::WatchStream::new(documents.clone());
        use tokio_stream::StreamExt;
        let first = stream.next().await.flatten().expect("a document");
        assert_eq!(first["n"], 3, "the newest, alone");
        assert!(documents.has_changed().is_ok());
    }

    /// resident-reads, "A read past its bound is answered from the last
    /// state" (the change `sluice-host-answers-every-read`, design D4): a
    /// read whose reading does not end within `read_for()` is answered as
    /// not current, with when the last reading ended, so `sluice host`
    /// answers from the state it last kept. The look holds the paused clock
    /// while it runs; the test moves the clock past the bound.
    #[tokio::test(start_paused = true)]
    async fn a_read_past_its_bound_answers_from_the_held_state() {
        let repo = PathBuf::from("/host/slow-reading.git");
        let held = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let gate = held.clone();
        let look: InPlay = Arc::new(move |_: &Path, _: bool| {
            let (blocked, ended) = &*gate;
            let mut b = locked(blocked);
            while *b {
                b = ended.wait(b).unwrap_or_else(|e| e.into_inner());
            }
            Looked::default()
        });
        let tasks = Tasks::new(Fakes::default().bind(), look).passes_when_asked();
        let tick = super::super::READ_TICK;
        let first = tasks
            .read(&repo, tick, super::super::read_for(), false)
            .await;
        assert!(first.current, "{first:?}");
        assert!(first.ended.is_some(), "{first:?}");

        tokio::time::advance(tick + Duration::from_secs(1)).await;
        *locked(&held.0) = true;
        let asked = {
            let tasks = tasks.clone();
            let repo = repo.clone();
            tokio::spawn(async move {
                tasks
                    .read(&repo, tick, super::super::read_for(), false)
                    .await
            })
        };
        settle().await;
        assert!(reading(&tasks, &repo), "a reading is in flight");
        tokio::time::advance(super::super::read_for() + Duration::from_secs(1)).await;
        let answered = asked.await.unwrap();
        assert!(!answered.current, "{answered:?}");
        assert_eq!(answered.ended, first.ended, "the state last kept");
        *locked(&held.0) = false;
        held.1.notify_all();
    }

    /// reader-tick, "A remote's records are asked for once a minute where
    /// nothing is in play" (129 as amended 2026-11-19): the look at an
    /// idle repository asks its remote, and is not taken again until a
    /// minute after that ask; a one-shot read within the minute reads it by
    /// a look that asks nothing, keeping the last ask's refusal; a
    /// command's change, a forecast's second ask and the minute's end each
    /// have the next reading ask. Under passes when asked, every reading
    /// asks.
    #[tokio::test(start_paused = true)]
    async fn an_idle_repositorys_remote_is_asked_once_a_minute() {
        let repo = PathBuf::from("/host/idle-asks.git");
        let fakes = Fakes::default();
        let tasks = Tasks::new(
            fakes.bind(),
            fakes.look(Some("origin: 503 Service Unavailable")),
        );
        let (tick, within) = (Duration::from_millis(50), Duration::from_secs(5));
        let looks = || fakes.seen.looks(&repo);

        tasks.look(&repo);
        soon("the first look", || looks().len() == 1).await;
        assert_eq!(looks(), [true], "the first look asks");
        // Within the minute, the host process's look passes it by.
        tasks.look(&repo);
        settle().await;
        assert_eq!(looks(), [true], "no look within the minute");
        // A one-shot read past its tick reads by a look that asks nothing,
        // and keeps the last ask's refusal.
        tokio::time::advance(tick).await;
        let read = tasks.read(&repo, tick, within, false).await;
        assert!(read.current, "{read:?}");
        assert_eq!(
            read.refused.as_deref(),
            Some("origin: 503 Service Unavailable")
        );
        assert_eq!(looks(), [true, false]);
        // A command's change has the next reading ask.
        tasks.told(&repo);
        tasks.read(&repo, tick, within, false).await;
        assert_eq!(looks(), [true, false, true]);
        // So does a forecast's second ask.
        tasks.read(&repo, tick, within, true).await;
        assert_eq!(looks(), [true, false, true, true]);
        // A minute after the last ask, the backstop asks again, with no
        // look of the host process's.
        tokio::time::advance(Duration::from_secs(59)).await;
        settle().await;
        assert_eq!(looks().len(), 4, "not yet due");
        tokio::time::advance(Duration::from_secs(1)).await;
        soon("the backstop", || looks().len() == 5).await;
        assert!(looks()[4], "the backstop at the minute's end asks");

        // Under passes when asked, every reading asks.
        let repo = PathBuf::from("/host/idle-asks-asked.git");
        let asked_only = Tasks::new(fakes.bind(), fakes.look(None)).passes_when_asked();
        asked_only.look(&repo);
        soon("the first look", || fakes.seen.looks(&repo).len() == 1).await;
        asked_only.look(&repo);
        soon("the second look", || fakes.seen.looks(&repo).len() == 2).await;
        assert_eq!(fakes.seen.looks(&repo), [true, true]);
    }

    /// reader-tick, "A remote's records are asked for once a minute where
    /// nothing is in play" (129 as amended 2026-11-19): a worker's pass
    /// asks the remote where the last reading found work in play, at a
    /// watch's opening and after a command's change, and otherwise not
    /// within the minute.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_pass_asks_where_work_is_in_play_or_a_watch_opens() {
        let repo = PathBuf::from("/host/pass-asks.git");
        let fakes = Fakes::default();
        let tasks = fakes.tasks();
        let passes = || fakes.seen.passes(&repo);
        let pass = |n: usize| {
            tasks.handle(&repo).wake();
            let (seen, repo, tasks) = (fakes.seen.clone(), repo.clone(), tasks.clone());
            async move {
                soon("the pass", || {
                    seen.passes(&repo).len() == n && !reading(&tasks, &repo)
                })
                .await
            }
        };

        tasks.look(&repo);
        soon("the look", || fakes.seen.looks(&repo).len() == 1).await;
        assert_eq!(fakes.seen.looks(&repo), [true]);
        // A watch's opening asks at once, though the look just asked.
        let watch = tasks.handle(&repo).follow(FollowKind::Watch);
        soon("the watch's first pass", || passes().len() == 1).await;
        assert_eq!(passes(), [true]);
        // Its passes after ask nothing while nothing is in play.
        pass(2).await;
        pass(3).await;
        assert_eq!(passes(), [true, false, false]);
        // A pass that finds work in play has the next one ask, and each
        // after while work stays in play.
        fakes.work.store(true, Ordering::SeqCst);
        pass(4).await;
        pass(5).await;
        pass(6).await;
        assert_eq!(&passes()[3..], [false, true, true]);
        // Once nothing is in play, the pass after the last that found work
        // asks, and none after it within the minute.
        fakes.work.store(false, Ordering::SeqCst);
        pass(7).await;
        pass(8).await;
        assert_eq!(&passes()[6..], [true, false]);
        // A command's change has the next pass ask.
        tasks.told(&repo);
        pass(9).await;
        pass(10).await;
        assert_eq!(&passes()[8..], [true, false]);
        drop(watch);
    }

    /// host-process, "Each repository's queues are moved apart" (135;
    /// rule core.29): a look whose read of one repository waits on its
    /// remote holds no other repository's look or worker and no command,
    /// and counts as the process busy until it ends, when it binds that
    /// repository's worker where work is in play. A second look while the
    /// first reads joins one reading after it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_slow_look_holds_no_command_and_no_other_look() {
        let (slow, quick, other) = (
            PathBuf::from("/host/slow.git"),
            PathBuf::from("/host/quick.git"),
            PathBuf::from("/host/other.git"),
        );
        let fakes = Fakes::default();
        fakes.work.store(true, Ordering::SeqCst);
        let answer = Arc::new(AtomicBool::new(false));
        let reads = Arc::new(AtomicUsize::new(0));
        let (waiting, counted) = (answer.clone(), reads.clone());
        let tasks = Tasks::new(
            fakes.bind(),
            Arc::new(move |r: &Path, _: bool| {
                if r == Path::new("/host/slow.git") {
                    counted.fetch_add(1, Ordering::SeqCst);
                    // A remote that does not answer, as long as it takes.
                    while !waiting.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
                Looked {
                    in_play: true,
                    refused: None,
                    ..Looked::default()
                }
            }),
        );
        tasks.look(&slow);
        tasks.look(&quick);
        soon("the quick repository's worker binds", || {
            fakes.seen.binds(&quick) == 1
        })
        .await;
        // A command's ask is answered meanwhile.
        assert!(tasks.handle(&other).pass(None).await.is_ok());
        // A second look while the first still reads reads nothing yet.
        tasks.event(&slow);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert!(tasks.looking(&slow) && fakes.seen.binds(&slow) == 0);
        assert!(tasks.any(), "a look under way keeps the process");
        answer.store(true, Ordering::SeqCst);
        soon("the slow repository's worker binds", || {
            fakes.seen.binds(&slow) == 1
        })
        .await;
        assert!(reads.load(Ordering::SeqCst) <= 2);
        fakes.work.store(false, Ordering::SeqCst);
    }

    /// D3: many triggers while a reading is in flight make one more reading
    /// after it, and no third.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn triggers_during_a_reading_make_one_more() {
        let repo = PathBuf::from("/host/joined.git");
        let fakes = Fakes {
            takes: Duration::from_millis(100),
            ..Fakes::default()
        };
        let tasks = fakes.tasks();
        let _watch = tasks.handle(&repo).follow(FollowKind::Run);
        soon("the worker binds", || tasks.holds(&repo)).await;
        tasks.handle(&repo).wake();
        soon("a pass in flight", || {
            locked(&tasks.0.repos).get(&repo).is_some_and(|r| {
                let m = r.marks.borrow();
                m.began.map(|b| b.n) > m.ended.map(|e| e.n)
            })
        })
        .await;
        let before = fakes.seen.passes(&repo).len();
        for _ in 0..5 {
            tasks.handle(&repo).wake();
            tasks.event(&repo);
        }
        soon("one more pass", || {
            fakes.seen.passes(&repo).len() == before + 2
        })
        .await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(fakes.seen.passes(&repo).len(), before + 2, "no third");
    }
}
