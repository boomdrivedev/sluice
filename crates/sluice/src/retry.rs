//! The one retry policy every call to a remote runs under (134; rule
//! core.29; ADR 0062): a git fetch, listing or push, and every call to a
//! forge's API. Its backoff is `backon`'s, exponential with jitter, a
//! second first and thirty at most between tries, bounded by what called
//! it: five minutes for a waiting submit or `sluice wait`, ending at the
//! wait's deadline or Ctrl-C, and thirty seconds for anything else. Each
//! try is bounded too, by [`TRY_FOR`] whatever called it, so a remote that
//! never answers holds a try, and whatever the try holds, no longer than
//! that (record 0109).
//!
//! The call site classes each failure, since only it knows the protocol;
//! the policy decides from the class alone what is tried again: what the
//! remote never judged and a credential it refused, for every call, and a
//! failure not known for a read alone, since reading changes nothing.
//! What the remote answered is never tried again. Each try is traced at
//! debug level and nothing of it is written to a branch's log: a refusal
//! that stands is logged once by whoever reports it (133).
//!
//! A forge's word that its allowance of calls is spent is a class of its
//! own, never backed off (134 as amended 2026-10-28; rule core.29; ADR
//! 0071): the policy sleeps until the time the forge named when it may,
//! a wait to its deadline or without end, and otherwise stops at once, a
//! round and a submit that does not wait among them (the user,
//! 2026-10-28).

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use backon::{BlockingRetryable, ExponentialBuilder};
use chrono::{DateTime, Local, Utc};

/// How long a call of a command that does not wait is tried for:
/// seconds (134), room for two tokens bought afresh and settled, and four
/// or five pauses.
pub const COMMAND_FOR: Duration = Duration::from_secs(30);

/// How long one try of a call to a remote may take before it is ended,
/// whatever called it: a forge call's, a credential command's and each try
/// of a git call's, git ended with every process it started and the
/// repository's fetch lock let go (134; rule core.29; record 0109). A
/// fetch waits for that lock no longer either. It is the family's limit,
/// weir-bounded's, one value for every tool (record 0109 as amended
/// 2026-11-11; agentplot/weir record 0008).
pub const TRY_FOR: Duration = weir_bounded::TRY_FOR;

/// The first pause, and the longest the backoff grows to before jitter.
const FIRST: Duration = Duration::from_secs(1);
const LONGEST: Duration = Duration::from_secs(30);

/// How a failed try of a call to a remote stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Judged {
    /// The remote never judged it: no connection was made, or the remote
    /// could not answer before anything was sent.
    Never,
    /// The remote refused the call's credential before it judged the call.
    Credential,
    /// The remote answered it: tried again, it would be answered again.
    Answered,
    /// Not known whether the remote judged it: a connection cut, or a
    /// server error, after the call was sent.
    Unknown,
    /// The remote's allowance of calls is spent until `until`: no call is
    /// made before it, and none to find out whether it has ended.
    Limited { until: DateTime<Utc> },
}

/// A call not made, or refused, because the forge's allowance of calls is
/// spent until `until` (134; ADR 0071): `words` are the forge's own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limited {
    pub app: u64,
    pub until: DateTime<Utc>,
    pub words: String,
}

impl fmt::Display for Limited {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "GitHub's allowance of calls for app {} is spent until {}: {}",
            self.app,
            clock(self.until),
            self.words
        )
    }
}

impl std::error::Error for Limited {}

/// `t` as the host's clock shows it, hours and minutes.
pub fn clock(t: DateTime<Utc>) -> String {
    t.with_timezone(&Local).format("%H:%M").to_string()
}

/// The spent allowance `e` was stopped on, if it was.
pub fn limited(e: &anyhow::Error) -> Option<&Limited> {
    e.chain().find_map(|c| c.downcast_ref::<Limited>())
}

/// What a call does to the remote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Changes nothing: a fetch, a listing, a forge `GET` or query.
    Read,
    /// A git push.
    Push,
    /// A forge call that changes something.
    Write,
}

/// What a try is told before it is made.
#[derive(Clone, Copy, Debug)]
pub struct Try {
    /// 1 for the first.
    pub number: u32,
    /// How the try before it failed; `None` for the first.
    pub after: Option<Judged>,
}

impl Try {
    /// Whether the try before was refused its credential, so this one is
    /// to carry one bought afresh.
    pub fn renew(&self) -> bool {
        self.after == Some(Judged::Credential)
    }
}

/// A try that failed, and how it stands.
pub struct Failed<E> {
    pub judged: Judged,
    pub error: E,
}

impl<E> Failed<E> {
    pub fn new(judged: Judged, error: E) -> Self {
        Failed { judged, error }
    }
}

/// A call the policy stopped trying: its last try's failure, and, when
/// the policy's time ran out or Ctrl-C stopped it rather than the class
/// saying stop, how long it tried.
pub struct Stopped<E> {
    pub judged: Judged,
    pub error: E,
    pub spent: Option<Spent>,
}

impl Stopped<anyhow::Error> {
    /// The last try's error, under a [`Spent`] context when the policy's
    /// time ran out on it.
    pub fn into_error(self) -> anyhow::Error {
        match self.spent {
            Some(s) => self.error.context(s),
            None => self.error,
        }
    }
}

/// The context of a call whose tries ran out: how many were made, and how
/// long from the first to the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spent {
    pub tries: u32,
    pub lasted: Duration,
}

impl fmt::Display for Spent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "tried {} times over {}",
            self.tries,
            crate::progress::duration(self.lasted.as_secs() as i64)
        )
    }
}

/// How long `e`'s call was tried, from its [`Spent`] context; zero when it
/// has none.
pub fn lasted(e: &anyhow::Error) -> Duration {
    e.downcast_ref::<Spent>()
        .map_or(Duration::ZERO, |s| s.lasted)
}

/// `e`'s words as `{:#}` says them, without its [`Spent`] context, for a
/// line that says how long the call was tried in its own words.
pub fn unspent(e: &anyhow::Error) -> String {
    let spent = e.downcast_ref::<Spent>().map(Spent::to_string);
    e.chain()
        .map(|c| c.to_string())
        .filter(|w| Some(w) != spent.as_ref())
        .collect::<Vec<_>>()
        .join(": ")
}

/// The retry policy: its bound, the instant it ends at whatever the bound
/// leaves, and how it pauses and reads the time.
pub struct Policy {
    bound: Duration,
    /// How long one try may take: [`TRY_FOR`], or a test's.
    try_for: Duration,
    until: Option<Instant>,
    /// Whether it waits out a spent allowance: to `until`, or without end.
    waits: bool,
    sleep: Rc<dyn Fn(Duration) -> bool>,
    now: Rc<dyn Fn() -> Instant>,
    /// An instant of `now` and the time of day then, so the time of day
    /// moves as `now` does, a recorded pause's included.
    origin: (Instant, DateTime<Utc>),
    seed: Option<u64>,
}

/// Set to `record` in tests, the retry policy's pauses are skipped.
pub const SLEEP_VAR: &str = "SLUICE_RETRY_SLEEP";

impl Policy {
    /// Tries for `bound` of pauses, pausing in earnest until Ctrl-C.
    pub fn new(bound: Duration) -> Policy {
        Policy {
            bound,
            try_for: TRY_FOR,
            until: None,
            waits: true,
            sleep: Rc::new(interruptible),
            now: Rc::new(Instant::now),
            origin: (Instant::now(), Utc::now()),
            seed: None,
        }
    }

    /// A waiting submit's or `sluice wait`'s: five minutes, ending at the
    /// wait's `deadline` when it has one, or at Ctrl-C.
    pub fn waiting(deadline: Option<Instant>) -> Policy {
        let bound = crate::UNANSWERED_FOR.to_std().expect("five minutes");
        Policy::new(bound).until(deadline)
    }

    /// Any other command's: thirty seconds, waiting out a spent allowance
    /// to its `until`, or without end.
    pub fn command() -> Policy {
        Policy::new(COMMAND_FOR)
    }

    /// A round's, of `sluice run` or `sluice step`: a command's thirty
    /// seconds, never waiting out a spent allowance (133, 134).
    pub fn round() -> Policy {
        Policy::command().unpaused()
    }

    /// `self`, stopping at once on a spent allowance: a round's, and a
    /// submit's that does not wait (the user, 2026-10-28).
    pub fn unpaused(mut self) -> Policy {
        self.waits = false;
        self
    }

    /// The time of day as the policy reads it: moved on by every pause it
    /// made, a skipped one's included.
    pub fn wall(&self) -> DateTime<Utc> {
        let (at, then) = self.origin;
        let since = (self.now)().saturating_duration_since(at);
        then + chrono::TimeDelta::from_std(since).unwrap_or_default()
    }

    /// Whether the policy waits out a spent allowance.
    pub fn waits(&self) -> bool {
        self.waits
    }

    /// `self`, ending at `until` whatever its bound leaves.
    pub fn until(mut self, until: Option<Instant>) -> Policy {
        self.until = until;
        self
    }

    /// `self`, pausing with `sleep`, which returns `false` to stop the
    /// tries at once, and reading the time from `now`.
    pub fn pausing(
        mut self,
        sleep: impl Fn(Duration) -> bool + 'static,
        now: impl Fn() -> Instant + 'static,
    ) -> Policy {
        self.sleep = Rc::new(sleep);
        self.now = Rc::new(now);
        self.origin = ((self.now)(), Utc::now());
        self
    }

    /// `self`, [`Policy::recording`] where the process's environment sets
    /// [`SLEEP_VAR`] to `record`, as a test that runs the binary does: for
    /// every policy the process binds, a command's and each repository's
    /// in sluice's host process alike (135).
    pub fn recording_where_set(self) -> Policy {
        match std::env::var(SLEEP_VAR).as_deref() {
            Ok("record") => self.recording(),
            _ => self,
        }
    }

    /// `self`, skipping every pause and reading the time as if it had
    /// paused: for tests that run the binary, whose fakes count the tries.
    pub fn recording(self) -> Policy {
        let skipped = Rc::new(Cell::new(Duration::ZERO));
        let s = skipped.clone();
        self.pausing(
            move |d| {
                s.set(s.get() + d);
                true
            },
            move || Instant::now() + skipped.get(),
        )
    }

    /// How long one try of a call may take before it is ended.
    pub fn try_for(&self) -> Duration {
        self.try_for
    }

    /// `self`, each try given `limit` in place of [`TRY_FOR`]: for tests.
    pub fn trying_for(mut self, limit: Duration) -> Policy {
        self.try_for = limit;
        self
    }

    /// `self`, its jitter drawn from `seed`: for tests.
    pub fn seeded(mut self, seed: u64) -> Policy {
        self.seed = Some(seed);
        self
    }

    /// Whether a failure judged `judged` of a call of `kind` is tried
    /// again.
    pub fn again(kind: Kind, judged: Judged) -> bool {
        match judged {
            Judged::Never | Judged::Credential => true,
            Judged::Unknown => kind == Kind::Read,
            Judged::Answered | Judged::Limited { .. } => false,
        }
    }

    /// Make the call `op` until a try succeeds, or a try fails in a way
    /// [`Policy::again`] does not try again, or the pauses would pass the
    /// bound, or the bound has passed since the first try, or the end
    /// instant has come, or the sleeper says stop: no try starts once the
    /// call's time is spent, however long its tries took (134; record
    /// 0109).
    /// `op` is told each try's number and how the one before failed. A
    /// try that met a spent allowance is never backed off: the policy
    /// sleeps until its time and tries again when it waits a pause and
    /// its end instant is later, sleeps to its end instant and stops when
    /// that is sooner, and stops at once when it waits none.
    pub fn run<T, E>(
        &self,
        kind: Kind,
        mut op: impl FnMut(&Try) -> Result<T, Failed<E>>,
    ) -> Result<T, Stopped<E>> {
        let started = (self.now)();
        let tries = Cell::new(0u32);
        let after = Cell::new(None);
        let last: RefCell<Option<E>> = RefCell::new(None);
        // Set when a pause was stopped: no try is made after it.
        let cut = Rc::new(Cell::new(false));
        // Set when the policy, not the class, ended the tries.
        let ran_out = Cell::new(false);
        let mut attempt = || -> Result<T, Judged> {
            let judged = match after.get() {
                Some(j) if cut.get() => return Err(j),
                _ => {
                    let t = Try {
                        number: tries.get() + 1,
                        after: after.get(),
                    };
                    tries.set(t.number);
                    match op(&t) {
                        Ok(v) => return Ok(v),
                        Err(f) => {
                            *last.borrow_mut() = Some(f.error);
                            f.judged
                        }
                    }
                }
            };
            after.set(Some(judged));
            Err(judged)
        };
        let result = loop {
            // The call's time runs from its first try, or from the end of
            // a spent allowance's pause.
            let begun = (self.now)();
            let mut backoff = ExponentialBuilder::new()
                .with_jitter()
                .with_min_delay(FIRST)
                .with_max_delay(LONGEST)
                .without_max_times()
                .with_total_delay(Some(self.bound));
            if let Some(seed) = self.seed {
                backoff = backoff.with_jitter_seed(seed);
            }
            let (sleep, now, until, stop) = (
                self.sleep.clone(),
                self.now.clone(),
                self.until,
                cut.clone(),
            );
            let pause = move |d: Duration| {
                let d = match until {
                    Some(u) => d.min(u.saturating_duration_since(now())),
                    None => d,
                };
                if !sleep(d) {
                    stop.set(true);
                }
            };
            let result = (&mut attempt)
                .retry(backoff)
                .sleep(pause)
                .when(|j| {
                    if cut.get() {
                        ran_out.set(true);
                        return false;
                    }
                    if !Policy::again(kind, *j) {
                        return false;
                    }
                    if self.until.is_some_and(|u| (self.now)() >= u)
                        || (self.now)().saturating_duration_since(begun) >= self.bound
                    {
                        ran_out.set(true);
                        return false;
                    }
                    // The backoff may yield no pause; whether it does is
                    // known only after this, so assume it will run out.
                    ran_out.set(true);
                    true
                })
                .notify(|j, d| {
                    ran_out.set(false);
                    tracing::debug!(
                        kind = ?kind,
                        judged = ?j,
                        tried = tries.get(),
                        pause_ms = d.as_millis() as u64,
                        "a call to a remote is tried again"
                    );
                })
                .call();
            let until = match result {
                Err(Judged::Limited { until }) if !cut.get() && self.waits => until,
                r => break r,
            };
            let mut wait = (until - self.wall()).to_std().unwrap_or_default();
            let mut last = false;
            if let Some(end) = self.until {
                let left = end.saturating_duration_since((self.now)());
                if left <= wait {
                    (wait, last) = (left, true);
                }
            }
            tracing::debug!(
                kind = ?kind,
                tried = tries.get(),
                pause_ms = wait.as_millis() as u64,
                "a call to a remote waits out a spent allowance"
            );
            if !(self.sleep)(wait) || last {
                ran_out.set(true);
                break result;
            }
        };
        match result {
            Ok(v) => Ok(v),
            Err(judged) => Err(Stopped {
                judged,
                error: last.into_inner().expect("a failed try's error"),
                spent: ran_out.get().then(|| Spent {
                    tries: tries.get(),
                    lasted: (self.now)().saturating_duration_since(started),
                }),
            }),
        }
    }
}

/// Pause for `d`, stopping at Ctrl-C; `false` when it stopped.
fn interruptible(d: Duration) -> bool {
    let until = Instant::now() + d;
    while Instant::now() < until {
        if crate::INTERRUPTED.load(Ordering::SeqCst) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50).min(until - Instant::now()));
    }
    !crate::INTERRUPTED.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A policy whose pauses are recorded and skipped, its clock moved on
    /// by each, its jitter fixed.
    fn recorded(bound: Duration) -> (Policy, Rc<RefCell<Vec<Duration>>>, Rc<Cell<Instant>>) {
        recorded_as(Policy::new(bound))
    }

    /// `policy`, its pauses recorded and skipped as [`recorded`]'s are.
    fn recorded_as(policy: Policy) -> (Policy, Rc<RefCell<Vec<Duration>>>, Rc<Cell<Instant>>) {
        let pauses = Rc::new(RefCell::new(Vec::new()));
        let clock = Rc::new(Cell::new(Instant::now()));
        let (p, c) = (pauses.clone(), clock.clone());
        let now = clock.clone();
        let policy = policy.seeded(7).pausing(
            move |d| {
                p.borrow_mut().push(d);
                c.set(c.get() + d);
                true
            },
            move || now.get(),
        );
        (policy, pauses, clock)
    }

    /// A call that fails `judged` `fails` times, then succeeds; how many
    /// tries it took, and how it ended.
    fn call(
        policy: &Policy,
        kind: Kind,
        judged: Judged,
        fails: u32,
    ) -> (u32, Result<(), Stopped<String>>) {
        let mut made = 0;
        let r = policy.run(kind, |t| {
            made += 1;
            assert_eq!(t.number, made);
            if made <= fails {
                Err(Failed::new(judged, format!("try {made}")))
            } else {
                Ok(())
            }
        });
        (made, r)
    }

    #[test]
    fn never_judged_and_a_refused_credential_are_tried_again_for_every_kind() {
        for kind in [Kind::Read, Kind::Push, Kind::Write] {
            for judged in [Judged::Never, Judged::Credential] {
                let (policy, pauses, _) = recorded(COMMAND_FOR);
                let (made, r) = call(&policy, kind, judged, 2);
                assert!(r.is_ok(), "{kind:?} {judged:?}");
                assert_eq!(made, 3, "{kind:?} {judged:?}");
                assert_eq!(pauses.borrow().len(), 2);
            }
        }
    }

    #[test]
    fn a_failure_not_known_is_tried_again_for_a_read_alone() {
        let (policy, _, _) = recorded(COMMAND_FOR);
        let (made, r) = call(&policy, Kind::Read, Judged::Unknown, 2);
        assert!(r.is_ok());
        assert_eq!(made, 3);
        for kind in [Kind::Push, Kind::Write] {
            let (policy, pauses, _) = recorded(COMMAND_FOR);
            let (made, r) = call(&policy, kind, Judged::Unknown, 2);
            let e = r.expect_err("not tried again");
            assert_eq!(
                (made, e.judged, e.error.as_str()),
                (1, Judged::Unknown, "try 1")
            );
            assert!(e.spent.is_none(), "the class ended it, not the policy");
            assert!(pauses.borrow().is_empty());
        }
    }

    #[test]
    fn an_answered_call_is_made_once() {
        for kind in [Kind::Read, Kind::Push, Kind::Write] {
            let (policy, pauses, _) = recorded(COMMAND_FOR);
            let (made, r) = call(&policy, kind, Judged::Answered, 5);
            let e = r.expect_err("answered");
            assert_eq!((made, e.judged), (1, Judged::Answered));
            assert!(e.spent.is_none());
            assert!(pauses.borrow().is_empty());
        }
    }

    #[test]
    fn a_try_is_told_how_the_one_before_failed() {
        let (policy, _, _) = recorded(COMMAND_FOR);
        let mut seen = Vec::new();
        let _ = policy.run(Kind::Read, |t| {
            seen.push((t.after, t.renew()));
            match t.number {
                1 => Err(Failed::new(Judged::Credential, ())),
                2 => Err(Failed::new(Judged::Never, ())),
                _ => Ok(()),
            }
        });
        assert_eq!(
            seen,
            [
                (None, false),
                (Some(Judged::Credential), true),
                (Some(Judged::Never), false)
            ]
        );
    }

    #[test]
    fn the_pauses_grow_and_stay_within_the_bound() {
        for bound in [COMMAND_FOR, Duration::from_secs(300)] {
            let (policy, pauses, _) = recorded(bound);
            let (made, r) = call(&policy, Kind::Read, Judged::Never, u32::MAX);
            let e = r.expect_err("never answered");
            let pauses = pauses.borrow();
            assert_eq!(made as usize, pauses.len() + 1);
            assert!(pauses.len() >= 3, "{bound:?}: {pauses:?}");
            assert!(pauses[0] >= FIRST && pauses[0] < 2 * FIRST, "{pauses:?}");
            // Each pause is longer than the one before while the backoff
            // grows: jitter adds less than the pause itself.
            for w in pauses.windows(2).take_while(|w| w[0] < LONGEST) {
                assert!(w[1] > w[0], "{pauses:?}");
            }
            assert!(pauses.iter().all(|p| *p < 2 * LONGEST), "{pauses:?}");
            let total: Duration = pauses.iter().sum();
            assert!(total <= bound, "{total:?} past {bound:?}");
            assert!(e.spent.is_some());
        }
    }

    /// 134, record 0109: no try starts once the call's time is spent,
    /// however long its tries took, so a remote that never answers holds a
    /// command's call for its first slow tries, not for one try per pause
    /// the bound has room for.
    #[test]
    fn no_try_starts_once_the_calls_time_is_spent() {
        let (policy, pauses, clock) = recorded(COMMAND_FOR);
        let r: Result<(), _> = policy.run(Kind::Read, |t| {
            clock.set(clock.get() + Duration::from_secs(20));
            Err(Failed::new(Judged::Unknown, t.number))
        });
        let e = r.expect_err("never answered");
        assert_eq!(e.error, 2, "a second try, begun 21s in, and no third");
        assert_eq!(pauses.borrow().len(), 1);
        assert_eq!(e.spent.map(|s| s.tries), Some(2));
    }

    #[test]
    fn the_end_instant_stops_the_tries() {
        let (policy, pauses, clock) = recorded(Duration::from_secs(300));
        let policy = policy.until(Some(clock.get() + Duration::from_secs(10)));
        let (made, r) = call(&policy, Kind::Read, Judged::Never, u32::MAX);
        let e = r.expect_err("never answered");
        let total: Duration = pauses.borrow().iter().sum();
        assert_eq!(
            total,
            Duration::from_secs(10),
            "the last pause ends at the instant"
        );
        assert_eq!(made as usize, pauses.borrow().len() + 1);
        assert_eq!(e.spent.map(|s| s.lasted), Some(Duration::from_secs(10)));
    }

    #[test]
    fn a_sleeper_that_says_stop_stops_at_once() {
        let made = Cell::new(0);
        let pauses = Rc::new(Cell::new(0));
        let p = pauses.clone();
        let policy = Policy::new(COMMAND_FOR).pausing(
            move |_| {
                p.set(p.get() + 1);
                false
            },
            Instant::now,
        );
        let r: Result<(), _> = policy.run(Kind::Read, |_| {
            made.set(made.get() + 1);
            Err(Failed::new(Judged::Never, "unanswered"))
        });
        let e = r.expect_err("stopped");
        assert_eq!((made.get(), pauses.get()), (1, 1));
        assert_eq!((e.judged, e.error), (Judged::Never, "unanswered"));
        assert_eq!(e.spent.map(|s| s.tries), Some(1));
    }

    #[test]
    fn the_spent_error_carries_its_tries_and_how_long() {
        let (policy, pauses, _) = recorded(COMMAND_FOR);
        let r: Result<(), _> = policy.run(Kind::Read, |t| {
            Err(Failed::new(
                Judged::Never,
                anyhow::anyhow!("503 on try {}", t.number),
            ))
        });
        let stopped = r.expect_err("never answered");
        let spent = stopped.spent.expect("ran out");
        let slept: Duration = pauses.borrow().iter().sum();
        assert_eq!(spent.tries as usize, pauses.borrow().len() + 1);
        assert_eq!(spent.lasted, slept);
        let e = stopped.into_error();
        assert_eq!(lasted(&e), slept);
        let words = format!("{e:#}");
        assert!(
            words.ends_with(&format!("503 on try {}", spent.tries)),
            "{words}"
        );
        assert_eq!(unspent(&e), format!("503 on try {}", spent.tries));
        let e = e.context("following #7");
        assert_eq!(lasted(&e), slept);
        assert_eq!(
            unspent(&e),
            format!("following #7: 503 on try {}", spent.tries)
        );
        assert_eq!(lasted(&anyhow::anyhow!("a fake's")), Duration::ZERO);
        assert_eq!(unspent(&anyhow::anyhow!("a fake's")), "a fake's");
    }

    /// A call met by a spent allowance until `ahead` from the policy's
    /// time, `fails` times, then answered: how many tries it took and how
    /// it ended.
    fn limited_call(
        policy: &Policy,
        ahead: Duration,
        fails: u32,
    ) -> (u32, Result<(), Stopped<String>>) {
        let mut made = 0;
        let r = policy.run(Kind::Read, |_| {
            made += 1;
            if made <= fails {
                let until = policy.wall() + chrono::TimeDelta::from_std(ahead).unwrap();
                Err(Failed::new(
                    Judged::Limited { until },
                    format!("try {made}"),
                ))
            } else {
                Ok(())
            }
        });
        (made, r)
    }

    const PAUSE: Duration = Duration::from_secs(20 * 60);

    #[test]
    fn a_wait_with_no_deadline_sleeps_to_the_allowance_and_tries_again() {
        let (policy, pauses, _) = recorded_as(Policy::waiting(None));
        let (made, r) = limited_call(&policy, PAUSE, 1);
        assert!(r.is_ok());
        assert_eq!(made, 2);
        assert_eq!(*pauses.borrow(), [PAUSE], "no backoff, the pause alone");
    }

    #[test]
    fn a_command_sleeps_to_the_allowance_without_end() {
        let (policy, pauses, _) = recorded_as(Policy::command());
        let (made, r) = limited_call(&policy, Duration::from_secs(3600), 2);
        assert!(r.is_ok());
        assert_eq!(made, 3);
        assert_eq!(pauses.borrow().len(), 2);
        assert!(pauses
            .borrow()
            .iter()
            .all(|p| *p == Duration::from_secs(3600)));
    }

    #[test]
    fn a_wait_whose_deadline_comes_first_stops_at_it_with_no_try() {
        let (policy, pauses, clock) = recorded_as(Policy::waiting(None));
        let policy = policy.until(Some(clock.get() + Duration::from_secs(300)));
        let (made, r) = limited_call(&policy, PAUSE, u32::MAX);
        let e = r.expect_err("the deadline came first");
        assert_eq!(made, 1);
        assert_eq!(*pauses.borrow(), [Duration::from_secs(300)]);
        assert!(matches!(e.judged, Judged::Limited { .. }));
        assert_eq!(
            e.spent,
            Some(Spent {
                tries: 1,
                lasted: Duration::from_secs(300)
            })
        );
    }

    #[test]
    fn a_round_stops_at_once_on_a_spent_allowance() {
        for policy in [Policy::round(), Policy::command().unpaused()] {
            let (policy, pauses, _) = recorded_as(policy);
            let (made, r) = limited_call(&policy, PAUSE, u32::MAX);
            let e = r.expect_err("no wait");
            assert_eq!((made, e.error.as_str()), (1, "try 1"));
            assert!(matches!(e.judged, Judged::Limited { .. }));
            assert!(e.spent.is_none());
            assert!(pauses.borrow().is_empty());
        }
    }

    #[test]
    fn ctrl_c_stops_the_wait_for_an_allowance() {
        let slept = Rc::new(RefCell::new(Vec::new()));
        let s = slept.clone();
        let policy = Policy::waiting(None).pausing(
            move |d| {
                s.borrow_mut().push(d);
                false
            },
            Instant::now,
        );
        let (made, r) = limited_call(&policy, PAUSE, u32::MAX);
        let e = r.expect_err("stopped");
        assert_eq!(made, 1);
        assert_eq!(slept.borrow().len(), 1);
        assert!(e.spent.is_some());
    }

    #[test]
    fn the_policys_time_moves_with_its_pauses() {
        let (policy, _, clock) = recorded(COMMAND_FOR);
        let before = policy.wall();
        clock.set(clock.get() + Duration::from_secs(90));
        assert_eq!(policy.wall() - before, chrono::TimeDelta::seconds(90));
    }

    #[test]
    fn a_spent_allowance_says_until_when_in_the_hosts_time() {
        let until = DateTime::parse_from_rfc3339("2026-10-28T14:20:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let e = anyhow::Error::new(Limited {
            app: 4242,
            until,
            words: "API rate limit exceeded".into(),
        })
        .context("listing o/r's pull requests");
        let l = limited(&e).expect("limited");
        assert_eq!(l.until, until);
        assert_eq!(
            l.to_string(),
            format!(
                "GitHub's allowance of calls for app 4242 is spent until {}: API rate limit exceeded",
                until.with_timezone(&Local).format("%H:%M")
            )
        );
        assert!(limited(&anyhow::anyhow!("other")).is_none());
    }
}
