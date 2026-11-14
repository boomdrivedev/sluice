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
use super::protocol::{FollowKind, Named};
use super::room::Shared;
use crate::exec::Host;
use crate::profiles::checks::RoomHandle;
use crate::profiles::Bound;
use crate::reader::{Driver, Drives, Reader};
use crate::{Pause, Skipped};

/// How long a host process that ends gives its tasks to finish the round
/// each is in, and its looks to end (D8).
pub const FINISH_FOR: Duration = Duration::from_secs(60);

/// What a task is asked.
pub enum Job {
    /// A pass now, answered once it ends.
    Pass(Sender<Result<Report, String>>),
    /// One round of a branch, answered with the step's document (D10).
    Step(String, Sender<Result<serde_json::Value, String>>),
    /// A pass at once rather than at the next tick.
    Wake,
}

/// What a task's thread runs, given its inbox.
pub type Body = Arc<dyn Fn(Inbox) + Send + Sync>;

/// Whether a repository has work in play, read as an idle tick reads it
/// (129, 135; design D4).
pub type InPlay = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

struct Slot {
    id: u64,
    kind: FollowKind,
    tx: Sender<Report>,
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
    body: Body,
    in_play: InPlay,
    stopping: AtomicBool,
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
            body,
            in_play,
            stopping: AtomicBool::new(false),
            next: AtomicU64::new(1),
        }))
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
        if self.0.stopping.load(Ordering::SeqCst)
            || self.holds(repository)
            || locked(&self.0.refused).contains_key(repository)
            || !locked(&self.0.looking).insert(repository.to_path_buf())
        {
            return;
        }
        let tasks = self.clone();
        let repository = repository.to_path_buf();
        std::thread::spawn(move || {
            let in_play = tasks.0.in_play.clone();
            let busy =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| in_play(&repository)))
                    .unwrap_or(true);
            // Started before the look is let go, so the process never reads
            // itself idle between the two.
            if busy && !tasks.0.stopping.load(Ordering::SeqCst) {
                tasks.handle(&repository);
            }
            locked(&tasks.0.looking).remove(&repository);
        });
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
}

/// A command's way to a repository's task.
pub struct Handle {
    jobs: Sender<Job>,
    followers: Followers,
    tasks: Tasks,
}

impl Handle {
    /// A pass now, once it ends.
    pub fn pass(&self) -> Result<Report, String> {
        let (tx, rx) = mpsc::channel();
        self.jobs
            .send(Job::Pass(tx))
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

    /// Follow the repository's passes while the follower lives.
    pub fn follow(&self, kind: FollowKind) -> Follower {
        let (tx, rx) = mpsc::channel();
        let id = self.tasks.0.next.fetch_add(1, Ordering::SeqCst);
        locked(&self.followers).push(Slot { id, kind, tx });
        // Its first tick may be a second rather than three.
        self.wake_if(kind == FollowKind::Watch);
        Follower {
            rx,
            followers: self.followers.clone(),
            id,
        }
    }

    fn wake_if(&self, wake: bool) {
        if wake {
            self.wake();
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

    /// Tell every follower of `report`.
    pub fn tell(&self, report: &Report) {
        locked(&self.followers).retain(|s| s.tx.send(report.clone()).is_ok());
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
}

/// The body of a repository's task in the host process: its own binding,
/// holding its parts of `room`, passing at its tick or, when `asked_only`,
/// at each ask (design D5, D14).
pub fn repository_body(room: Shared, asked_only: bool, named: Named) -> Body {
    Arc::new(move |inbox: Inbox| repository(inbox, room.clone(), asked_only, named.clone()))
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

fn repository(inbox: Inbox, room: Shared, asked_only: bool, named: Named) {
    let common = inbox.repository.clone();
    let bound = match Bound::open(
        &open_dir(&common),
        Rc::new(crate::retry::Policy::round().recording_where_set()),
    ) {
        Ok(b) => b,
        Err(e) => {
            inbox.refuse(format!(
                "sluice host: {}: could not be opened: {e:#}",
                common.display()
            ));
            return;
        }
    };
    let bound = Bound {
        tests: bound.tests.with_room(RoomHandle::Here(room.clone())),
        host_process: Box::new(Inside { room, named }),
        ..bound
    };
    // In a test's host process, a pass's time moves on by its tick as a
    // waiting command's does (design D14).
    let skipped = Skipped::new(tick(false));
    let clock: &dyn Clock = if asked_only { &skipped } else { &SystemClock };
    // 129: each policy read, by revision, kept across the task's passes.
    let policies = sluice_core::Policies::default();
    let s = Sluice::new(bound.seams(), clock)
        .as_peer(bound.peer())
        .keeping(&policies);
    // 87: a host whose config names a system it is not claims no run here.
    if let Err(e) = s.check_host_systems() {
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
        in_play: pass::in_play(&s),
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
        match job {
            Some(Job::Pass(reply)) => {
                let report = task.pass(&s, &bound, clock, every);
                if asked_only {
                    skipped.pause();
                }
                last = Some(Instant::now());
                if let Ok(r) = &report {
                    inbox.tell(r);
                }
                let _ = reply.send(report);
            }
            Some(Job::Step(branch, reply)) => {
                let _ = reply.send(
                    pass::step(&s, &*bound.pulls, clock, &branch).map_err(|e| format!("{e:#}")),
                );
                task.in_play = pass::in_play(&s);
            }
            Some(Job::Wake) if !asked_only => {
                if let Ok(r) = task.pass(&s, &bound, clock, every) {
                    inbox.tell(&r);
                }
                last = Some(Instant::now());
            }
            Some(Job::Wake) => {}
            None if due => {
                if let Ok(r) = task.pass(&s, &bound, clock, every) {
                    inbox.tell(&r);
                }
                last = Some(Instant::now());
            }
            None => {}
        }
        if !task.in_play && !inbox.followed() {
            match inbox.end_if_idle() {
                Ok(()) => return,
                Err(job) => pending = job,
            }
        }
    }
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
        if self.driver.is_none() {
            self.driver = Driver::try_take(&self.readers, &self.repository).unwrap_or(None);
        }
        if self.driver.is_none() {
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
            Arc::new(|_: &Path| true),
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
        assert_eq!(b.pass(), Ok(Report::default()));
        // The next tick starts it again.
        tasks.tick();
        let a = tasks.handle(&repo);
        assert_eq!(a.pass(), Ok(Report::default()));
        assert!(starts.load(Ordering::SeqCst) >= 3);
    }

    /// A task's body that answers its asks, then ends when idle.
    fn answer_until_idle(inbox: &Inbox) {
        loop {
            match inbox.next(Duration::from_millis(50)) {
                Some(Job::Pass(reply)) => {
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
            Arc::new(|r: &Path| r != Path::new("/host/idle.git")),
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
                true
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
        assert_eq!(tasks.handle(&other).pass(), Ok(Report::default()));
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
