//! A task for each repository the host process holds (26, 129, 135;
//! design D5, D7, D14): a thread of its own with its own binding of the
//! repository, so a slow fetch or a refused policy in one holds no other's
//! round. It passes at its tick, a second while a watch follows it and
//! three otherwise, or, where passes are taken only when asked, at each
//! ask. It is the repository's reader on the host, and holds its driver's
//! lock, so an older sluice's waits and watches follow it; where an older
//! `sluice run` holds that lock, it runs no round and follows that run's
//! document. It ends once its repository has nothing in play and nothing
//! follows it. A panic ends only its own thread, which is started again
//! at the host process's next tick.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use sluice_core::{Clock, Sluice, SystemClock};

use super::ask::Inside;
use super::log;
use super::pass::{self, Kept, Report};
use super::protocol::{FollowKind, Named, Reading};
use super::room::Shared;
use crate::exec::Host;
use crate::instance::Instance;
use crate::profiles::checks::RoomHandle;
use crate::profiles::Bound;
use crate::reader::{Driver, Drives, Reader};
use crate::{Pause, Skipped};

/// How long a host process that ends gives its tasks to finish the round
/// each is in, and its looks to end (D8).
pub const FINISH_FOR: Duration = Duration::from_secs(60);

/// What a task is asked.
pub enum Job {
    /// A pass now, answered once it ends, at its asker's time where it
    /// names one (D14).
    Pass(
        Sender<Result<Report, String>>,
        Option<chrono::DateTime<chrono::Utc>>,
    ),
    /// One round of a branch, answered with the step's document (D10).
    Step(String, Sender<Result<serde_json::Value, String>>),
    /// A pass at once rather than at the next tick.
    Wake,
}

/// What a task's thread runs, given its inbox.
pub type Body = Arc<dyn Fn(Inbox) + Send + Sync>;

/// What the host process's look found of a repository: whether it has
/// work in play, read as an idle tick reads it, and the words a remote
/// refused its ask with (129, 135; design D4, D16).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Looked {
    pub in_play: bool,
    pub refused: Option<String>,
}

/// How the host process looks at a repository it holds no task for (129,
/// 135; design D4).
pub type InPlay = Arc<dyn Fn(&Path) -> Looked + Send + Sync>;

/// One repository's reading, as the host process marks it: when its last
/// pass or look began and ended, and what a remote refused it (129;
/// design D7, D16).
#[derive(Clone, Debug, Default)]
struct Marks {
    began: Option<Instant>,
    ended: Option<Instant>,
    ended_at: Option<String>,
    refused: Option<String>,
}

impl Marks {
    /// Whether the reading is current at `now` with `tick`: one that ended
    /// within its tick, or one under way that began within a tick of the
    /// last one's end (64, 129).
    fn current(&self, now: Instant, tick: Duration) -> bool {
        match (self.began, self.ended) {
            (Some(b), Some(e)) if b > e => b.saturating_duration_since(e) < tick,
            (_, Some(e)) => now.saturating_duration_since(e) < tick,
            _ => false,
        }
    }

    fn answer(&self, current: bool) -> Reading {
        Reading {
            current,
            ended: self.ended_at.clone(),
            refused: self.refused.clone(),
        }
    }
}

struct Slot {
    id: u64,
    kind: FollowKind,
    tx: Sender<Report>,
    /// Whether a pass began since it came: it is told only of such passes,
    /// so its first report is of the pass it asked for (D9).
    armed: bool,
}

type Followers = Arc<Mutex<Vec<Slot>>>;

struct Entry {
    jobs: Sender<Job>,
    followers: Followers,
}

struct Inner {
    running: Mutex<BTreeMap<PathBuf, Entry>>,
    /// Repositories whose task panicked, to start again at the next tick.
    panicked: Mutex<BTreeSet<PathBuf>>,
    /// Repositories whose task could not start, with the words it logged
    /// once: the host process's look passes them by until a command asks
    /// for them again.
    refused: Mutex<BTreeMap<PathBuf, String>>,
    /// Repositories the host process's look is reading now, each on a
    /// thread of its own.
    looking: Mutex<BTreeSet<PathBuf>>,
    /// Each repository's reading, as its passes and looks mark it (D16).
    readings: Mutex<BTreeMap<PathBuf, Marks>>,
    body: Body,
    in_play: InPlay,
    stopping: AtomicBool,
    /// Passes are taken only when asked (D14): a freed part wakes nothing.
    asked_only: AtomicBool,
    next: AtomicU64,
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The host process's tasks, one for each repository it holds.
#[derive(Clone)]
pub struct Tasks(Arc<Inner>);

impl Tasks {
    pub fn new(body: Body, in_play: InPlay) -> Tasks {
        Tasks(Arc::new(Inner {
            running: Mutex::new(BTreeMap::new()),
            panicked: Mutex::new(BTreeSet::new()),
            refused: Mutex::new(BTreeMap::new()),
            looking: Mutex::new(BTreeSet::new()),
            readings: Mutex::new(BTreeMap::new()),
            body,
            in_play,
            stopping: AtomicBool::new(false),
            asked_only: AtomicBool::new(false),
            next: AtomicU64::new(1),
        }))
    }

    /// The same tasks, passing only when asked (D14).
    pub fn passes_when_asked(self) -> Tasks {
        self.0.asked_only.store(true, Ordering::SeqCst);
        self
    }

    /// A part of the host's bound freed that `repository` is owed: its
    /// task, started where none runs, passes at once, as a `serve` has it,
    /// so the repository the part is owed to takes it within a moment;
    /// under passes when asked, nothing passes by itself (D13, D14).
    pub fn owed(&self, repository: &Path) {
        if self.0.stopping.load(Ordering::SeqCst) || self.0.asked_only.load(Ordering::SeqCst) {
            return;
        }
        self.handle(repository).wake();
    }

    /// `repository`'s task, started where none runs.
    pub fn handle(&self, repository: &Path) -> Handle {
        let mut running = locked(&self.0.running);
        let entry = match running.get(repository) {
            Some(e) => e,
            None => {
                let (tx, rx) = mpsc::channel();
                let followers: Followers = Arc::default();
                let inbox = Inbox {
                    repository: repository.to_path_buf(),
                    jobs: rx,
                    followers: followers.clone(),
                    tasks: self.clone(),
                };
                let tasks = self.clone();
                let repo = repository.to_path_buf();
                std::thread::spawn(move || {
                    let body = tasks.0.body.clone();
                    let ran =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(inbox)));
                    if let Err(panic) = ran {
                        let words = panic
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                            .unwrap_or_default();
                        log(&format!(
                            "sluice host: {}: its task panicked, and starts again at the next tick: {words}",
                            repo.display()
                        ));
                        locked(&tasks.0.running).remove(&repo);
                        locked(&tasks.0.panicked).insert(repo);
                    }
                });
                running.insert(
                    repository.to_path_buf(),
                    Entry {
                        jobs: tx,
                        followers,
                    },
                );
                running.get(repository).expect("just inserted")
            }
        };
        Handle {
            jobs: entry.jobs.clone(),
            followers: entry.followers.clone(),
            tasks: self.clone(),
        }
    }

    /// Start `repository`'s task where none runs and work is in play there,
    /// read as an idle tick reads it (129, 135; design D4): the host
    /// process's look at its index at its start and every few seconds
    /// after, so an idle repository costs it that read alone. The read is
    /// made on a thread of its own, one for each repository, and this
    /// returns at once: a remote that is slow to answer, or a pause the
    /// read waits out, holds neither the thread that accepts commands'
    /// connections nor another repository's look (rule core.29). A
    /// repository still being read, or whose task could not start, is
    /// passed by, the latter until a command asks for it again.
    pub fn look(&self, repository: &Path) {
        if locked(&self.0.refused).contains_key(repository) {
            return;
        }
        self.look_at(repository, false);
    }

    /// The look at `repository`, where none is under way and, unless
    /// `beside` a task, no task runs for it, its reading marked as it
    /// begins and ends (D16).
    fn look_at(&self, repository: &Path, beside: bool) {
        if self.0.stopping.load(Ordering::SeqCst)
            || (!beside && self.holds(repository))
            || !locked(&self.0.looking).insert(repository.to_path_buf())
        {
            return;
        }
        let tasks = self.clone();
        let repository = repository.to_path_buf();
        std::thread::spawn(move || {
            tasks.begin_reading(&repository);
            let in_play = tasks.0.in_play.clone();
            let looked =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| in_play(&repository)))
                    .unwrap_or(Looked {
                        in_play: true,
                        refused: None,
                    });
            tasks.end_reading(&repository, looked.refused.clone());
            // Started before the look is let go, so the process never reads
            // itself idle between the two; a repository whose task could not
            // start is a command's to ask for again.
            if looked.in_play
                && !tasks.0.stopping.load(Ordering::SeqCst)
                && !locked(&tasks.0.refused).contains_key(&repository)
            {
                tasks.handle(&repository);
            }
            locked(&tasks.0.looking).remove(&repository);
        });
    }

    /// `repository`'s reading begins now (D16).
    fn begin_reading(&self, repository: &Path) {
        locked(&self.0.readings)
            .entry(repository.to_path_buf())
            .or_default()
            .began = Some(Instant::now());
    }

    /// `repository`'s reading ends now, a remote having refused it with
    /// `refused`, where one did (D16).
    fn end_reading(&self, repository: &Path, refused: Option<String>) {
        let mut readings = locked(&self.0.readings);
        let marks = readings.entry(repository.to_path_buf()).or_default();
        marks.ended = Some(Instant::now());
        marks.ended_at =
            Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        marks.refused = refused;
    }

    /// 129 (design D16): `repository` read for a one-shot read: answered
    /// once a reading of it is current, one that ended within `tick`, or one
    /// under way that began within a tick of the last one's end. Where none
    /// is, it reads now, by its task's next pass where one runs and by the
    /// look otherwise, and answers when that reading ends, waiting `within`
    /// at most, then answering with the reading as it stands. Where passes
    /// are taken only when asked, a reading that is not current is read by
    /// the look, which runs no round, so a test's rounds stay those its
    /// commands ask for (D14). With `afresh`, no reading is current but one
    /// begun after the ask: a forecast's second ask, where the first
    /// reading lacked what it forms from.
    pub fn read(
        &self,
        repository: &Path,
        tick: Duration,
        within: Duration,
        afresh: bool,
    ) -> Reading {
        let asked = Instant::now();
        let marks = || {
            locked(&self.0.readings)
                .get(repository)
                .cloned()
                .unwrap_or_default()
        };
        let now = marks();
        if !afresh && now.current(asked, tick) {
            return now.answer(true);
        }
        if self.0.asked_only.load(Ordering::SeqCst) {
            self.look_at(repository, true);
        } else if self.holds(repository) {
            self.handle(repository).wake();
        } else {
            self.look_at(repository, false);
        }
        let until = asked + within;
        loop {
            let m = marks();
            if m.ended.is_some_and(|e| e >= asked) {
                return m.answer(true);
            }
            if Instant::now() >= until {
                return m.answer(false);
            }
            // A look that could not start, the task's start refused, is no
            // reading under way: it reads as it stands.
            if !self.holds(repository) && !self.looking(repository) {
                std::thread::sleep(Duration::from_millis(20));
                let m = marks();
                if !m.ended.is_some_and(|e| e >= asked) && !self.looking(repository) {
                    return m.answer(m.current(Instant::now(), tick));
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Whether the look is reading `repository` now.
    pub fn looking(&self, repository: &Path) -> bool {
        locked(&self.0.looking).contains(repository)
    }

    /// Whether a task runs for `repository`.
    pub fn holds(&self, repository: &Path) -> bool {
        locked(&self.0.running).contains_key(repository)
    }

    /// The host process's tick: each task that panicked starts again.
    pub fn tick(&self) {
        let panicked = std::mem::take(&mut *locked(&self.0.panicked));
        for repository in panicked {
            self.handle(&repository);
        }
    }

    /// The repositories a task runs for.
    pub fn held(&self) -> Vec<PathBuf> {
        locked(&self.0.running).keys().cloned().collect()
    }

    /// Whether any task runs, one that panicked waits to start again, or
    /// the look is reading a repository.
    pub fn any(&self) -> bool {
        !locked(&self.0.running).is_empty()
            || !locked(&self.0.panicked).is_empty()
            || !locked(&self.0.looking).is_empty()
    }

    /// Every task finishes the round it is in and ends, and every look
    /// under way ends, starting nothing: a newer sluice's hand-over, or a
    /// state folder gone (D8). It waits [`FINISH_FOR`] at most.
    pub fn stop_all(&self) {
        self.0.stopping.store(true, Ordering::SeqCst);
        locked(&self.0.panicked).clear();
        let until = Instant::now() + FINISH_FOR;
        while (!locked(&self.0.running).is_empty() || !locked(&self.0.looking).is_empty())
            && Instant::now() < until
        {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Start tasks and looks again after [`Tasks::stop_all`]: a hand-over
    /// whose successor did not start (design D20).
    pub fn resume(&self) {
        self.0.stopping.store(false, Ordering::SeqCst);
    }
}

/// A command's way to a repository's task.
pub struct Handle {
    jobs: Sender<Job>,
    followers: Followers,
    tasks: Tasks,
}

impl Handle {
    /// A pass now, once it ends, at its asker's time `at` where it names
    /// one (D14).
    pub fn pass(&self, at: Option<chrono::DateTime<chrono::Utc>>) -> Result<Report, String> {
        let (tx, rx) = mpsc::channel();
        self.jobs
            .send(Job::Pass(tx, at))
            .map_err(|_| "the repository's task ended".to_string())?;
        rx.recv()
            .map_err(|_| "the repository's task ended before its pass".to_string())?
    }

    /// One round of `branch`, as `sluice step --json` prints it.
    pub fn step(&self, branch: &str) -> Result<serde_json::Value, String> {
        let (tx, rx) = mpsc::channel();
        self.jobs
            .send(Job::Step(branch.to_string(), tx))
            .map_err(|_| "the repository's task ended".to_string())?;
        rx.recv()
            .map_err(|_| "the repository's task ended before its round".to_string())?
    }

    /// A pass at once.
    pub fn wake(&self) {
        let _ = self.jobs.send(Job::Wake);
    }

    /// Follow the repository's passes while the follower lives, told only
    /// of passes begun after it came. A wait's or a watch's follow asks a
    /// pass at once, so its first report is the host process's pass of
    /// the repository as it stands when it began to follow, whatever its
    /// timeout (D9); `sluice run` follows the passes as they come.
    pub fn follow(&self, kind: FollowKind) -> Follower {
        let (tx, rx) = mpsc::channel();
        let id = self.tasks.0.next.fetch_add(1, Ordering::SeqCst);
        locked(&self.followers).push(Slot {
            id,
            kind,
            tx,
            armed: false,
        });
        // Under passes when asked, its follower asks its own (D14).
        let asked_only = self.tasks.0.asked_only.load(Ordering::SeqCst);
        if !asked_only && matches!(kind, FollowKind::Wait | FollowKind::Watch) {
            self.wake();
        }
        Follower {
            rx,
            followers: self.followers.clone(),
            id,
        }
    }
}

/// What a follower is told next.
pub enum Next {
    Report(Report),
    Nothing,
    /// Its task ended under it.
    Ended,
}

/// A follower of a repository's passes, counted while it lives.
pub struct Follower {
    rx: Receiver<Report>,
    followers: Followers,
    id: u64,
}

impl Follower {
    pub fn next(&self, wait: Duration) -> Next {
        match self.rx.recv_timeout(wait) {
            Ok(r) => Next::Report(r),
            Err(RecvTimeoutError::Timeout) => Next::Nothing,
            Err(RecvTimeoutError::Disconnected) => Next::Ended,
        }
    }
}

impl Drop for Follower {
    fn drop(&mut self) {
        locked(&self.followers).retain(|s| s.id != self.id);
    }
}

/// What a task's thread is given.
pub struct Inbox {
    pub repository: PathBuf,
    jobs: Receiver<Job>,
    followers: Followers,
    tasks: Tasks,
}

impl Inbox {
    /// The next job, waiting up to `wait`.
    pub fn next(&self, wait: Duration) -> Option<Job> {
        self.jobs.recv_timeout(wait).ok()
    }

    /// Whether a watch follows the repository: its tick is a second.
    pub fn watched(&self) -> bool {
        locked(&self.followers)
            .iter()
            .any(|s| s.kind == FollowKind::Watch)
    }

    /// Whether anything follows the repository.
    pub fn followed(&self) -> bool {
        !locked(&self.followers).is_empty()
    }

    /// Tell every follower that came before the pass began of `report`;
    /// one that came since waits for the next (D9).
    pub fn tell(&self, report: &Report) {
        locked(&self.followers).retain(|s| !s.armed || s.tx.send(report.clone()).is_ok());
    }

    /// Whether the host process is ending.
    pub fn stopping(&self) -> bool {
        self.tasks.0.stopping.load(Ordering::SeqCst)
    }

    /// End the task where nothing is asked of it and nothing follows it,
    /// decided under the tasks' lock so no ask is lost: `Ok` once it is
    /// ended, or the job that came meanwhile.
    pub fn end_if_idle(&self) -> Result<(), Option<Job>> {
        let mut running = locked(&self.tasks.0.running);
        if let Ok(job) = self.jobs.try_recv() {
            return Err(Some(job));
        }
        if !locked(&self.followers).is_empty() && !self.stopping() {
            return Err(None);
        }
        running.remove(&self.repository);
        Ok(())
    }

    /// End the task whatever is asked of it: its asks answered as ended.
    pub fn end(&self) {
        locked(&self.tasks.0.running).remove(&self.repository);
    }

    /// The task could not start, in `words`: logged where they are not
    /// the words last logged for its repository, and the task ended, so
    /// the host process's look passes the repository by until a command
    /// asks for it again.
    pub fn refuse(&self, words: String) {
        let mut refused = locked(&self.tasks.0.refused);
        if refused.get(&self.repository) != Some(&words) {
            log(&words);
            refused.insert(self.repository.clone(), words);
        }
        drop(refused);
        self.end();
    }

    /// The task started: what refused it before is over.
    pub fn started(&self) {
        locked(&self.tasks.0.refused).remove(&self.repository);
    }

    /// Its repository's reading begins now: a pass begins (D7, D16).
    pub fn begin_reading(&self) {
        for s in locked(&self.followers).iter_mut() {
            s.armed = true;
        }
        self.tasks.begin_reading(&self.repository);
    }

    /// Its repository's reading ends now, a remote having refused it with
    /// `refused`, where one did (D7, D16).
    pub fn end_reading(&self, refused: Option<String>) {
        self.tasks.end_reading(&self.repository, refused);
    }
}

/// The host's settings as the host process holds them, and how many
/// changes of them it took up: each repository's task binds afresh at the
/// start of its next pass once the count moves, so no pass runs under two
/// settings (135; record 0107; rules core.14, core.54; design D19).
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

    /// Hold `instance`, a change taken up, which every task binds at the
    /// start of its next pass.
    pub fn take_up(&self, instance: Instance) {
        let mut held = locked(&self.instance);
        *held = instance;
        self.taken.fetch_add(1, Ordering::SeqCst);
    }
}

/// The body of a repository's task in the host process: its own binding,
/// of the host's settings as `settings` holds them, holding its parts of
/// `room`, passing at its tick or, when `asked_only`, at each ask (design
/// D5, D14, D19).
pub fn repository_body(
    room: Shared,
    asked_only: bool,
    named: Named,
    settings: Arc<Settings>,
) -> Body {
    Arc::new(move |inbox: Inbox| {
        repository(
            inbox,
            room.clone(),
            asked_only,
            named.clone(),
            settings.clone(),
        )
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

/// How long a task waits between its passes (64, 129).
fn tick(watched: bool) -> Duration {
    Duration::from_secs(if watched { 1 } else { 3 })
}

fn repository(inbox: Inbox, room: Shared, asked_only: bool, named: Named, settings: Arc<Settings>) {
    let common = inbox.repository.clone();
    // Its binding, of the settings `instance`: its parts held of the
    // host's room, its results from the host process's index (design D12,
    // D17).
    let bind = |instance: Instance| -> anyhow::Result<Bound> {
        let bound = Bound::open_with(
            &open_dir(&common),
            std::env::current_exe()?,
            instance,
            Rc::new(crate::retry::Policy::round().recording_where_set()),
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
    };
    let (mut taken, instance) = settings.current();
    let mut bound = match bind(instance) {
        Ok(b) => b,
        Err(e) => {
            inbox.refuse(format!(
                "sluice host: {}: could not be opened: {e:#}",
                common.display()
            ));
            return;
        }
    };
    // In a test's host process, a pass's time moves on by its tick as a
    // waiting command's does (design D14).
    let skipped = Skipped::new(tick(false));
    let clock: &dyn Clock = if asked_only { &skipped } else { &SystemClock };
    // 129: each policy read, by revision, kept across the task's passes,
    // and read once more under a binding made afresh (record 0107).
    let mut policies = sluice_core::Policies::default();
    // 87: a host whose config names a system it is not claims no run here.
    if let Err(e) = sluice_of(&bound, &policies, clock).check_host_systems() {
        inbox.refuse(format!(
            "sluice host: {}: {e:#}; this host claims no run of it",
            common.display()
        ));
        return;
    }
    inbox.started();
    let (Ok(readers), repository) = (bound.readers(), common.to_string_lossy().into_owned()) else {
        inbox.end();
        return;
    };
    let mut task = Task {
        driver: None,
        reader: None,
        kept: Kept::default(),
        in_play: pass::in_play(&sluice_of(&bound, &policies, clock)),
        said_driven: false,
        readers,
        repository,
    };
    let mut last: Option<Instant> = None;
    let mut pending: Option<Job> = None;
    loop {
        if inbox.stopping() {
            inbox.end();
            return;
        }
        if bound.repository_gone(&task.repository) {
            log(&format!(
                "sluice host: {}: gone; held no longer",
                common.display()
            ));
            inbox.end();
            return;
        }
        let every = tick(inbox.watched());
        let wait = match (asked_only, last) {
            (true, _) => Duration::from_secs(1),
            (false, None) => Duration::ZERO,
            (false, Some(at)) => every.saturating_sub(at.elapsed()),
        };
        let job = pending.take().or_else(|| inbox.next(wait));
        let due = !asked_only && last.is_none_or(|at| at.elapsed() >= every);
        // D19: a pass, or a step, under settings the host process took up
        // since this binding was made binds afresh first, its reader's
        // lock, followers and parts kept, as `Task` and the room hold
        // them. Settings that do not bind here leave it on its binding.
        let passes = match &job {
            Some(Job::Pass(..)) | Some(Job::Step(..)) => true,
            Some(Job::Wake) => !asked_only,
            None => due,
        };
        if passes {
            let (now, instance) = settings.current();
            if now != taken {
                taken = now;
                let fresh = bind(instance).and_then(|b| {
                    sluice_of(&b, &sluice_core::Policies::default(), clock).check_host_systems()?;
                    Ok(b)
                });
                match fresh {
                    Ok(b) => {
                        bound = b;
                        policies = sluice_core::Policies::default();
                    }
                    Err(e) => log(&format!(
                        "sluice host: {}: the host's settings taken up do not bind here, so it \
                         keeps the settings it held: {e:#}",
                        common.display()
                    )),
                }
            }
        }
        let s = sluice_of(&bound, &policies, clock);
        match job {
            Some(Job::Pass(reply, at)) => {
                // D14: the pass's time is its asker's where it names one,
                // so two waits asking passes move it no faster than one;
                // else it moves on by its tick after the pass.
                if let (true, Some(at)) = (asked_only, at) {
                    skipped.reach(at);
                }
                let report = task.read(&inbox, &s, &bound, clock, every);
                if asked_only && at.is_none() {
                    skipped.pause();
                }
                last = Some(Instant::now());
                if let Ok(r) = &report {
                    inbox.tell(r);
                }
                let _ = reply.send(report);
            }
            Some(Job::Step(branch, reply)) => {
                // D7: an older `sluice run` that drives the repository keeps
                // its rounds while it lives, a step's among them.
                let answer = match task.drives() {
                    true => {
                        pass::step(&s, &*bound.pulls, clock, &branch).map_err(|e| format!("{e:#}"))
                    }
                    false => Err(format!(
                        "an older sluice run drives {}, so sluice's host process runs no round of \
                         it while that run lives",
                        task.repository
                    )),
                };
                let _ = reply.send(answer);
                task.in_play = pass::in_play(&s);
            }
            Some(Job::Wake) if !asked_only => {
                if let Ok(r) = task.read(&inbox, &s, &bound, clock, every) {
                    inbox.tell(&r);
                }
                last = Some(Instant::now());
            }
            Some(Job::Wake) => {}
            None if due => {
                if let Ok(r) = task.read(&inbox, &s, &bound, clock, every) {
                    inbox.tell(&r);
                }
                last = Some(Instant::now());
            }
            None => {}
        }
        drop(s);
        if !task.in_play && !inbox.followed() {
            match inbox.end_if_idle() {
                Ok(()) => return,
                Err(job) => pending = job,
            }
        }
    }
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

/// What a repository's task holds between its passes.
struct Task {
    driver: Option<Driver>,
    reader: Option<Reader>,
    kept: Kept,
    in_play: bool,
    /// Whether it said that an older `sluice run` drives the repository.
    said_driven: bool,
    readers: PathBuf,
    repository: String,
}

impl Task {
    /// One pass, its reading marked as it begins and ends, so a one-shot
    /// read beside it is answered from it (129; design D7, D16).
    fn read(
        &mut self,
        inbox: &Inbox,
        s: &Sluice<'_>,
        bound: &Bound,
        clock: &dyn Clock,
        tick: Duration,
    ) -> Result<Report, String> {
        inbox.begin_reading();
        let report = self.pass(s, bound, clock, tick);
        inbox.end_reading(report.as_ref().ok().and_then(|r| r.refused.clone()));
        report
    }

    /// Whether it drives the repository, its driver's lock taken where no
    /// older `sluice run` holds it (D7).
    fn drives(&mut self) -> bool {
        if self.driver.is_none() {
            self.driver = Driver::try_take(&self.readers, &self.repository).unwrap_or(None);
        }
        self.driver.is_some()
    }

    /// One pass, as the repository's driver and reader on the host; none
    /// while an older `sluice run` drives it or an older reader reads it,
    /// which it follows (D7).
    fn pass(
        &mut self,
        s: &Sluice<'_>,
        bound: &Bound,
        clock: &dyn Clock,
        tick: Duration,
    ) -> Result<Report, String> {
        if !self.drives() {
            if !self.said_driven {
                log(&format!(
                    "sluice host: {}: an older sluice run drives it; this process follows it",
                    self.repository
                ));
                self.said_driven = true;
            }
            self.in_play = pass::in_play(s);
            return Ok(Report {
                in_play: self.in_play,
                ..Report::default()
            });
        }
        self.said_driven = false;
        // 129: it takes an older reader's lease as a newer watch does, and
        // follows until that reader lets go.
        match &mut self.reader {
            Some(r) => {
                let _ = r.step(true, Drives::Nobody);
            }
            None => match Reader::take_given(
                &self.readers,
                &self.repository,
                bound.version(),
                Drives::Nobody,
            ) {
                Ok(mut r) => {
                    // An older reader's lease, taken at once.
                    let _ = r.step(true, Drives::Nobody);
                    self.reader = Some(r);
                }
                Err(e) => return Err(format!("{e:#}")),
            },
        }
        let Some(Reader::Lead(lead)) = &mut self.reader else {
            self.in_play = pass::in_play(s);
            return Ok(Report {
                in_play: self.in_play,
                ..Report::default()
            });
        };
        let report = pass::pass(
            s,
            bound,
            &*bound.pulls,
            clock,
            Some(lead),
            tick,
            &mut self.kept,
        )
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

    /// 26, 135 (design D5): a task that panics ends its own thread alone,
    /// and is started again at the host process's next tick.
    #[test]
    fn a_task_that_panics_is_started_again_at_the_next_tick() {
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = starts.clone();
        let tasks = Tasks::new(
            Arc::new(move |inbox: Inbox| {
                let n = counted.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    panic!("a fault in one repository's pass");
                }
                // The second start answers its asks, then ends when idle.
                answer_until_idle(&inbox);
            }),
            Arc::new(|_: &Path| Looked {
                in_play: true,
                refused: None,
            }),
        );
        let other = Tasks::clone(&tasks);
        let repo = PathBuf::from("/host/a.git");
        tasks.look(&repo);
        let until = Instant::now() + Duration::from_secs(10);
        while starts.load(Ordering::SeqCst) == 0 || locked(&tasks.0.running).contains_key(&repo) {
            assert!(Instant::now() < until, "the task never panicked");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(tasks.any(), "it waits to start again");
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        // Another repository's task runs meanwhile.
        let b = other.handle(Path::new("/host/b.git"));
        assert_eq!(b.pass(None), Ok(Report::default()));
        // The next tick starts it again.
        tasks.tick();
        let a = tasks.handle(&repo);
        assert_eq!(a.pass(None), Ok(Report::default()));
        assert!(starts.load(Ordering::SeqCst) >= 3);
    }

    /// A task's body that answers its asks, then ends when idle.
    fn answer_until_idle(inbox: &Inbox) {
        loop {
            match inbox.next(Duration::from_millis(50)) {
                Some(Job::Pass(reply, _)) => {
                    let _ = reply.send(Ok(Report::default()));
                }
                Some(_) => {}
                None => {
                    if inbox.end_if_idle().is_ok() {
                        return;
                    }
                }
            }
        }
    }

    /// D13, D14: a part of the host's bound that frees has the repository
    /// it is owed to, one waiting with none under test, pass at once; under
    /// passes when asked, nothing passes by itself.
    #[test]
    fn a_freed_part_wakes_the_repository_it_is_owed_to() {
        use super::super::room::{Room, Shared};
        use sluice_seams::RunKind;
        let (busy, idle) = (
            PathBuf::from("/host/busy.git"),
            PathBuf::from("/host/idle.git"),
        );
        for asked_only in [false, true] {
            let woken: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
            let counted = woken.clone();
            let tasks = Tasks::new(
                Arc::new(move |inbox: Inbox| loop {
                    match inbox.next(Duration::from_millis(50)) {
                        Some(Job::Wake) => locked(&counted).push(inbox.repository.clone()),
                        Some(Job::Pass(reply, _)) => {
                            let _ = reply.send(Ok(Report::default()));
                        }
                        Some(_) => {}
                        None => {
                            if inbox.end_if_idle().is_ok() {
                                return;
                            }
                        }
                    }
                }),
                Arc::new(|_: &Path| Looked {
                    in_play: true,
                    refused: None,
                }),
            );
            let tasks = match asked_only {
                true => tasks.passes_when_asked(),
                false => tasks,
            };
            let room = Shared::new(Room::new(1, 0));
            let told = tasks.clone();
            room.on_free(move |r| told.owed(r));
            let part = room.hold(RunKind::Queue, &busy).unwrap().expect("room");
            assert!(
                room.hold(RunKind::Queue, &idle).unwrap().is_none(),
                "the idle repository waits"
            );
            drop(part);
            if asked_only {
                std::thread::sleep(Duration::from_millis(300));
                assert!(locked(&woken).is_empty(), "{:?}", locked(&woken));
            } else {
                soon("the idle repository's task passes", || {
                    locked(&woken).contains(&idle)
                });
                assert!(!locked(&woken).contains(&busy));
            }
        }
    }

    /// 129 (design D16): a read is answered once a reading of its
    /// repository is current: one ended within its tick at once; with none,
    /// a look's or the task's next pass's, its refusal named; under passes
    /// when asked, at once from the last reading.
    #[test]
    fn a_read_is_answered_once_a_reading_is_current() {
        let repo = PathBuf::from("/host/read.git");
        let looks = Arc::new(AtomicUsize::new(0));
        let counted = looks.clone();
        let passes = Arc::new(AtomicUsize::new(0));
        let passed = passes.clone();
        let body: Body = Arc::new(move |inbox: Inbox| loop {
            match inbox.next(Duration::from_millis(20)) {
                Some(Job::Wake) => {
                    inbox.begin_reading();
                    std::thread::sleep(Duration::from_millis(100));
                    passed.fetch_add(1, Ordering::SeqCst);
                    inbox.end_reading(None);
                }
                Some(Job::Pass(reply, _)) => {
                    let _ = reply.send(Ok(Report::default()));
                }
                Some(_) => {}
                None => {
                    if inbox.end_if_idle().is_ok() {
                        return;
                    }
                }
            }
        });
        let in_play: InPlay = Arc::new(move |_: &Path| {
            counted.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(100));
            Looked {
                in_play: false,
                refused: Some("origin: 503 Service Unavailable".into()),
            }
        });
        let tasks = Tasks::new(body.clone(), in_play.clone());
        let (tick, within) = (Duration::from_millis(500), Duration::from_secs(5));

        // No reading: the look reads now, and its refusal is named.
        let read = tasks.read(&repo, tick, within, false);
        assert!(read.current && read.ended.is_some(), "{read:?}");
        assert_eq!(
            read.refused.as_deref(),
            Some("origin: 503 Service Unavailable")
        );
        assert_eq!(looks.load(Ordering::SeqCst), 1);
        // One ended within its tick: at once, nothing read again.
        let began = Instant::now();
        assert!(tasks.read(&repo, tick, within, false).current);
        assert!(began.elapsed() < Duration::from_millis(50));
        assert_eq!(looks.load(Ordering::SeqCst), 1);
        // Past its tick: read again.
        std::thread::sleep(tick);
        assert!(tasks.read(&repo, tick, within, false).current);
        assert_eq!(looks.load(Ordering::SeqCst), 2);

        // With a task running, its next pass is the reading: a wait's
        // follow asks a pass of its own first (D9).
        let _held = tasks.handle(&repo).follow(FollowKind::Wait);
        soon("the follow's own pass", || {
            passes.load(Ordering::SeqCst) == 1
        });
        std::thread::sleep(tick);
        let read = tasks.read(&repo, tick, within, false);
        assert!(read.current, "{read:?}");
        assert_eq!(read.refused, None);
        assert_eq!(passes.load(Ordering::SeqCst), 2);
        assert_eq!(looks.load(Ordering::SeqCst), 2);

        // Under passes when asked, a look reads, running no pass, though a
        // task holds the repository.
        let asked = Tasks::new(body, in_play).passes_when_asked();
        let _task = asked.handle(&repo).follow(FollowKind::Wait);
        let read = asked.read(&repo, tick, within, false);
        assert!(read.current && read.ended.is_some(), "{read:?}");
        assert_eq!(passes.load(Ordering::SeqCst), 2, "no pass was run");
        assert_eq!(looks.load(Ordering::SeqCst), 3);
        // Asked afresh, it reads again though its reading is current.
        assert!(asked.read(&repo, tick, within, false).current);
        assert_eq!(looks.load(Ordering::SeqCst), 3);
        assert!(asked.read(&repo, tick, within, true).current);
        assert_eq!(looks.load(Ordering::SeqCst), 4);
    }

    /// `check` holds within ten seconds.
    fn soon(what: &str, check: impl Fn() -> bool) {
        let until = Instant::now() + Duration::from_secs(10);
        while !check() {
            assert!(Instant::now() < until, "{what}: not within 10s");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// reader-tick, "An idle tick starts no process" (129, 135; design
    /// D4): the host process's look starts no task for a repository with
    /// nothing in play, starts one for a repository with work, and passes
    /// by one whose task could not start, saying why once, until a
    /// command asks for it.
    #[test]
    fn a_look_starts_a_task_only_where_work_is_in_play() {
        let (idle, busy, refused) = (
            PathBuf::from("/host/idle.git"),
            PathBuf::from("/host/busy.git"),
            PathBuf::from("/host/refused.git"),
        );
        let starts: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
        let counted = starts.clone();
        let refusing = refused.clone();
        let tasks = Tasks::new(
            Arc::new(move |inbox: Inbox| {
                locked(&counted).push(inbox.repository.clone());
                if inbox.repository == refusing {
                    inbox.refuse("its host's systems are not this host's".into());
                    return;
                }
                inbox.started();
                answer_until_idle(&inbox);
            }),
            Arc::new(|r: &Path| Looked {
                in_play: r != Path::new("/host/idle.git"),
                refused: None,
            }),
        );
        let started = |r: &PathBuf| locked(&starts).iter().filter(|s| *s == r).count();

        tasks.look(&idle);
        soon("the idle repository's look ends", || !tasks.looking(&idle));
        assert!(!tasks.holds(&idle), "no task for an idle repository");

        tasks.look(&busy);
        soon("the busy repository's task starts", || started(&busy) == 1);

        tasks.look(&refused);
        soon("the refused task ends", || {
            started(&refused) == 1 && !tasks.holds(&refused)
        });
        tasks.look(&refused);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(started(&refused), 1, "the look passes it by");
        assert_eq!(
            locked(&tasks.0.refused).get(&refused).map(String::as_str),
            Some("its host's systems are not this host's")
        );

        // A command's ask starts it again.
        tasks.handle(&refused);
        soon("a command's ask starts it", || started(&refused) == 2);
        assert_eq!(started(&idle), 0);
    }

    /// host-process, "Each repository's queues are moved apart" (135;
    /// rule core.29): a look whose read of one repository waits on its
    /// remote returns at once, holds no other repository's look or task,
    /// and counts as the process busy until it ends, when it starts that
    /// repository's task where work is in play. A task's start is read
    /// from its body, which counts it, never from whether it holds: one
    /// asked nothing ends within its first idle wait, sooner than a loaded
    /// host may wake the thread that looks.
    #[test]
    fn a_slow_look_holds_no_command_and_no_other_look() {
        let (slow, quick, other) = (
            PathBuf::from("/host/slow.git"),
            PathBuf::from("/host/quick.git"),
            PathBuf::from("/host/other.git"),
        );
        let answer = Arc::new(AtomicBool::new(false));
        let reads = Arc::new(AtomicUsize::new(0));
        let (waiting, counted) = (answer.clone(), reads.clone());
        let starts: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
        let starting = starts.clone();
        let started = |r: &PathBuf| locked(&starts).iter().filter(|s| *s == r).count();
        let tasks = Tasks::new(
            Arc::new(move |inbox: Inbox| {
                locked(&starting).push(inbox.repository.clone());
                answer_until_idle(&inbox);
            }),
            Arc::new(move |r: &Path| {
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
                }
            }),
        );
        let began = Instant::now();
        tasks.look(&slow);
        tasks.look(&quick);
        assert!(
            began.elapsed() < Duration::from_secs(1),
            "the look returns at once"
        );
        soon("the quick repository's task starts", || {
            started(&quick) == 1
        });
        // A command's ask is answered meanwhile.
        assert_eq!(tasks.handle(&other).pass(None), Ok(Report::default()));
        // A second look while the first still reads reads nothing again.
        tasks.look(&slow);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert!(tasks.looking(&slow) && started(&slow) == 0);
        assert!(tasks.any(), "a look under way keeps the process");
        answer.store(true, Ordering::SeqCst);
        soon("the slow repository's task starts", || started(&slow) == 1);
        soon("its look ends", || !tasks.looking(&slow));
    }
}
