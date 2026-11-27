//! Asks of one key joined to one call (the change
//! `sluice-host-answers-every-read`, design D5, D10;
//! `one-read-in-flight@e99acda774d0`).
//!
//! `Joined<K, V>` is a map from each key to one tokio `watch` channel. The
//! value of the channel is the newest answer and the generation of the
//! call that made it. For each key, the map also holds the generation of
//! the call in flight, when that call began, and whether one more call is
//! due after it.
//!
//! An ask of a key needs the generation of the call in flight where that
//! call began after the last change that the asker knows of. Otherwise it
//! needs the next generation: the next call starts at once where no call
//! is in flight, and when the call in flight ends otherwise. The asks that
//! come meanwhile need that same next generation, so they make one more
//! call, not many.
//!
//! The newest answer replaces the older one, so an ask that reads late
//! reads the newest. An answer carries the moment its call began: the
//! calls of a key never overlap, so these moments order its answers as
//! the state that each call read. A key with no receiver and no call in
//! flight leaves the map.
//!
//! This module names no type of sluice. It uses only tokio and tokio-util.

use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::watch;
use tokio::time::Instant;
use tokio_util::task::TaskTracker;

/// A moment in the order of the moments that the process marks. Two
/// moments are always in an order, also where a paused clock gives the
/// two one time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Moment(u64);

static MOMENTS: AtomicU64 = AtomicU64::new(1);

impl Moment {
    /// The next moment.
    pub fn now() -> Moment {
        Moment(MOMENTS.fetch_add(1, Ordering::SeqCst))
    }
}

/// A call's future.
type Call<V> = Pin<Box<dyn Future<Output = V> + Send>>;

/// How a call is started.
type Start<V> = Box<dyn FnOnce() -> Call<V> + Send>;

/// One key's state.
struct Entry<V> {
    /// The newest answer, its generation and when its call began; `None`
    /// before the first.
    answers: watch::Sender<Option<(u64, Moment, V)>>,
    /// The generation of the call in flight, and when it began.
    flight: Option<(u64, Moment)>,
    /// The start of one more call, due when the call in flight ends.
    due: Option<Start<V>>,
    /// The generation of the newest call started.
    last: u64,
}

struct Inner<K, V> {
    keys: Mutex<HashMap<K, Entry<V>>>,
    tracker: TaskTracker,
}

/// Asks of each key joined to one call in flight.
pub struct Joined<K, V>(Arc<Inner<K, V>>);

impl<K, V> Clone for Joined<K, V> {
    fn clone(&self) -> Self {
        Joined(self.0.clone())
    }
}

impl<K, V> Joined<K, V>
where
    K: Eq + Hash + Clone + Send + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// A map whose calls are tasks on `tracker`.
    pub fn new(tracker: TaskTracker) -> Joined<K, V> {
        Joined(Arc::new(Inner {
            keys: Mutex::new(HashMap::new()),
            tracker,
        }))
    }

    fn keys(&self) -> MutexGuard<'_, HashMap<K, Entry<V>>> {
        self.0.keys.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The answer of `key` from a call that began after `changed`, the
    /// last change that the asker knows of. `start` starts the call where
    /// this ask needs one. It waits until `until` at most, and gives
    /// `None` after that.
    pub async fn ask<F, Fut>(
        &self,
        key: K,
        changed: Option<Moment>,
        until: Option<Instant>,
        start: F,
    ) -> Option<V>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = V> + Send + 'static,
    {
        self.ask_dated(key, changed, until, start)
            .await
            .map(|(_, v)| v)
    }

    /// As `ask`, with the moment the answer's call began. Two answers of
    /// one key are in the order of these moments.
    pub async fn ask_dated<F, Fut>(
        &self,
        key: K,
        changed: Option<Moment>,
        until: Option<Instant>,
        start: F,
    ) -> Option<(Moment, V)>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = V> + Send + 'static,
    {
        let start: Start<V> = Box::new(move || Box::pin(start()) as Call<V>);
        let (mut rx, needs) = {
            let mut keys = self.keys();
            let entry = keys.entry(key.clone()).or_insert_with(|| Entry {
                answers: watch::Sender::new(None),
                flight: None,
                due: None,
                last: 0,
            });
            let rx = entry.answers.subscribe();
            let needs = match entry.flight {
                // The call in flight began after the change: it answers.
                Some((generation, began)) if changed.is_none_or(|c| began > c) => generation,
                // One more call after the call in flight.
                Some((generation, _)) => {
                    entry.due = Some(start);
                    generation + 1
                }
                None => {
                    let generation = entry.last + 1;
                    self.begin(&key, entry, generation, start);
                    generation
                }
            };
            (rx, needs)
        };
        let reached =
            |v: &Option<(u64, Moment, V)>| v.as_ref().is_some_and(|(g, _, _)| *g >= needs);
        let answer = match until {
            Some(until) => tokio::time::timeout_at(until, rx.wait_for(reached))
                .await
                .ok()
                .and_then(Result::ok)
                .and_then(|v| v.as_ref().map(|(_, b, v)| (*b, v.clone()))),
            None => rx
                .wait_for(reached)
                .await
                .ok()
                .and_then(|v| v.as_ref().map(|(_, b, v)| (*b, v.clone()))),
        };
        drop(rx);
        self.leave_if_idle(&key);
        answer
    }

    /// Start the call of `generation` for `key`, on the tracker.
    fn begin(&self, key: &K, entry: &mut Entry<V>, generation: u64, start: Start<V>) {
        entry.flight = Some((generation, Moment::now()));
        entry.last = generation;
        let call = start();
        let joined = self.clone();
        let key = key.clone();
        self.0.tracker.spawn(async move {
            let answer = call.await;
            joined.ended(&key, generation, answer);
        });
    }

    /// The call of `generation` for `key` ended with `answer`: the newest
    /// answer replaces the older one, and one more call starts where one
    /// is due.
    fn ended(&self, key: &K, generation: u64, answer: V) {
        let mut keys = self.keys();
        let Some(entry) = keys.get_mut(key) else {
            return;
        };
        // The call that ended is the one in flight: calls never overlap.
        let began = entry.flight.map_or_else(Moment::now, |(_, began)| began);
        entry
            .answers
            .send_replace(Some((generation, began, answer)));
        entry.flight = None;
        if let Some(start) = entry.due.take() {
            let next = entry.last + 1;
            self.begin(key, entry, next, start);
            return;
        }
        if entry.answers.receiver_count() == 0 {
            keys.remove(key);
        }
    }

    /// Remove `key` where no call is in flight and no ask waits.
    fn leave_if_idle(&self, key: &K) {
        let mut keys = self.keys();
        if keys
            .get(key)
            .is_some_and(|e| e.flight.is_none() && e.answers.receiver_count() == 0)
        {
            keys.remove(key);
        }
    }

    /// How many keys the map holds.
    pub fn len(&self) -> usize {
        self.keys().len()
    }

    /// Whether the map holds no key.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    /// A call that counts itself, and ends when `gate` opens, with the
    /// count as its answer.
    fn counted(
        calls: &Arc<AtomicUsize>,
        gate: &watch::Sender<bool>,
    ) -> impl FnOnce() -> Pin<Box<dyn Future<Output = usize> + Send>> + Send + 'static {
        let calls = calls.clone();
        let mut open = gate.subscribe();
        move || {
            Box::pin(async move {
                let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
                let _ = open.wait_for(|o| *o).await;
                n
            })
        }
    }

    async fn settle() {
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
    }

    /// Ten asks that come while one call is in flight share that call.
    #[tokio::test(start_paused = true)]
    async fn asks_in_flight_share_one_call() {
        let joined: Joined<&str, usize> = Joined::new(TaskTracker::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let (gate, _) = watch::channel(false);
        let mut asks = Vec::new();
        for _ in 0..10 {
            let j = joined.clone();
            let call = counted(&calls, &gate);
            asks.push(tokio::spawn(async move {
                j.ask("main", None, None, call).await
            }));
            settle().await;
        }
        gate.send_replace(true);
        for a in asks {
            assert_eq!(a.await.unwrap(), Some(1));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// Asks that come after a change, while a call that began before the
    /// change is in flight, make one more call, and no third.
    #[tokio::test(start_paused = true)]
    async fn an_ask_after_a_change_makes_one_more_call() {
        let joined: Joined<&str, usize> = Joined::new(TaskTracker::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let (gate, _) = watch::channel(false);
        let first = {
            let j = joined.clone();
            let call = counted(&calls, &gate);
            tokio::spawn(async move { j.ask("main", None, None, call).await })
        };
        settle().await;
        let changed = Moment::now();
        let mut later = Vec::new();
        for _ in 0..20 {
            let j = joined.clone();
            let call = counted(&calls, &gate);
            later.push(tokio::spawn(async move {
                j.ask("main", Some(changed), None, call).await
            }));
            settle().await;
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        gate.send_replace(true);
        assert_eq!(first.await.unwrap(), Some(1));
        for a in later {
            assert_eq!(a.await.unwrap(), Some(2));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// The channel holds the newest answer alone.
    #[tokio::test(start_paused = true)]
    async fn the_newest_answer_replaces_the_older() {
        let joined: Joined<&str, usize> = Joined::new(TaskTracker::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let (gate, _) = watch::channel(true);
        for n in 1..=3 {
            let call = counted(&calls, &gate);
            assert_eq!(joined.ask("main", None, None, call).await, Some(n));
        }
        let keys = joined.keys();
        assert!(keys.get("main").is_none(), "no ask holds the key");
    }

    /// An ask that needs call 1, and reads only after call 2 ended, reads
    /// call 2's answer.
    #[tokio::test(start_paused = true)]
    async fn a_late_ask_reads_the_newest_answer() {
        let joined: Joined<&str, usize> = Joined::new(TaskTracker::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let (gate, _) = watch::channel(false);
        let mut first = Box::pin(joined.ask("main", None, None, counted(&calls, &gate)));
        // Polled once: it needs call 1, which is now in flight.
        assert!(tokio::time::timeout(Duration::ZERO, &mut first)
            .await
            .is_err());
        // A change makes call 2 due after call 1.
        let changed = Moment::now();
        let second = {
            let j = joined.clone();
            let call = counted(&calls, &gate);
            tokio::spawn(async move { j.ask("main", Some(changed), None, call).await })
        };
        settle().await;
        gate.send_replace(true);
        assert_eq!(second.await.unwrap(), Some(2));
        // The first ask reads late: the newest answer has replaced call 1's.
        assert_eq!(first.await, Some(2));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// A key with no ask and no call in flight leaves the map; an ask whose
    /// bound passes gives nothing, and its key leaves once its call ends.
    #[tokio::test(start_paused = true)]
    async fn a_key_with_no_ask_leaves_the_map() {
        let joined: Joined<&str, usize> = Joined::new(TaskTracker::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let (gate, _) = watch::channel(false);
        let call = counted(&calls, &gate);
        let until = Instant::now() + Duration::from_secs(1);
        assert_eq!(joined.ask("main", None, Some(until), call).await, None);
        assert_eq!(joined.len(), 1, "its call is still in flight");
        gate.send_replace(true);
        settle().await;
        assert!(joined.is_empty());
        let call = counted(&calls, &gate);
        assert_eq!(joined.ask("main", None, None, call).await, Some(2));
        assert!(joined.is_empty());
    }
}
