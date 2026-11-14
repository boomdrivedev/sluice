//! The host's running checks (71, S60): one count of the checks running
//! on the host, read by every sluice on it whatever repository or branch
//! it serves, and the one count a run is admitted against.
//!
//! A running check is a fact about this host, never about a branch, so it
//! is kept beside the host's blocks of ports and never in the records,
//! whose profiles may live on a remote or a server:
//!
//! ```text
//! $XDG_STATE_HOME/sluice/running/     (default ~/.local/state)
//!   .lock                   held only across counting and admitting one run
//!   <name>.json             one admitted run: its kind, repository, place,
//!                           run directory, starter and when it was admitted
//!   <name>.hold             locked by the process that admitted the run,
//!                           until its supervisor holds the run directory
//! ```
//!
//! A run's room is admitted before its run directory is known, as a round
//! holds a combination's part from the count that chose it to its start
//! (71; rule core.49; record 0087): its entry is named for the admission
//! and names, until the run begins, a run directory under the registry
//! that never exists, and is written again with the run's own once it
//! begins. An older sluice names its entries by their run directories;
//! the names only keep entries apart.
//!
//! An entry counts while its hold is locked or a supervisor holds its run
//! directory (`crate::runs`). One that is neither, a run that ended or a
//! starter that died before its supervisor started, counts for nothing
//! and is removed by the next admission.
//!
//! The hold is what keeps the lock short: a starter takes `.lock`, counts,
//! locks its hold, writes its entry and lets `.lock` go, then sets up and
//! starts its run, which may take minutes; once the run's supervisor holds
//! the run directory, the starter lets the hold go and the supervisor's own
//! lock keeps the entry counted. Two starters, of any repositories, never
//! both take the bound's last run.
//!
//! Nothing but the entries and the limit decides an admission: no load or
//! use of the host is read (71). The limit is the host's bound, less the
//! part kept for submissions' runs when the run is a session's or a
//! preview's (session-runs-1 clause 91, previews-1 clause 102).

use std::fs::File;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sluice_seams::Running;

use crate::runs::supervised;

/// What a running check is run for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A submission's combination.
    Queue,
    /// A session's run (`sluice exec`) or a preview's command.
    Session,
}

/// One admitted run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub kind: Kind,
    /// The repository, as its common git directory.
    pub repository: PathBuf,
    /// The name of the place the run is given, which names a
    /// combination's branch.
    pub place: String,
    /// Its run directory: the run counts while a supervisor holds this
    /// directory's lock.
    pub run_dir: PathBuf,
    /// The process that admitted it.
    pub pid: u32,
    /// When it was admitted, RFC 3339.
    pub admitted: String,
}

/// The running checks of the host whose state is under `dir`.
#[derive(Clone, Debug)]
pub struct HostCount {
    dir: PathBuf,
}

/// A run admitted and not yet handed to its supervisor: its hold stays
/// locked until [`Admission::started`]. Dropped without it, the start
/// failed, and its entry goes.
#[must_use = "an admission dropped before its run starts gives its part of the bound back"]
pub struct Admission {
    dir: PathBuf,
    name: String,
    entry: Entry,
    hold: Option<File>,
}

impl HostCount {
    pub fn new(dir: PathBuf) -> Self {
        HostCount { dir }
    }

    /// This host's: `$XDG_STATE_HOME/sluice/running`.
    pub fn host() -> Result<Self> {
        Ok(HostCount::new(
            crate::instance::state_dir()?.join("running"),
        ))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Admit `entry` while fewer than `limit` checks run on the host, both
    /// kinds counted, under the host's lock; `None` at the limit. Every
    /// entry that counts for nothing is removed on the way. An entry given
    /// no run directory, a run's room held before its run begins, names one
    /// under the registry that never exists until [`Admission::begin`].
    pub fn admit(&self, entry: &Entry, limit: usize) -> Result<Option<Admission>> {
        let _lock = self.lock()?;
        let name = admission_name();
        let mut entry = entry.clone();
        if entry.run_dir.as_os_str().is_empty() {
            entry.run_dir = self.dir.join(format!("{name}.unstarted"));
        }
        let mut live = 0;
        for (n, e) in self.recorded()? {
            if self.live(&n, &e)? {
                if e.run_dir == entry.run_dir {
                    bail!("the run in {} is admitted already", e.run_dir.display());
                }
                live += 1;
            } else {
                self.remove(&n)?;
            }
        }
        if live >= limit {
            return Ok(None);
        }
        let entry = &entry;
        // The hold first: an entry is never seen without it.
        let path = self.dir.join(format!("{name}.hold"));
        let hold = File::create(&path).with_context(|| format!("creating {}", path.display()))?;
        match hold.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                bail!("the run in {} is admitted already", entry.run_dir.display())
            }
            Err(std::fs::TryLockError::Error(e)) => {
                return Err(e).with_context(|| format!("locking {}", path.display()))
            }
        }
        let tmp = self.dir.join(format!(".{name}.tmp"));
        let path = self.dir.join(format!("{name}.json"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(entry)?)
            .with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))?;
        Ok(Some(Admission {
            dir: self.dir.clone(),
            name,
            entry: entry.clone(),
            hold: Some(hold),
        }))
    }

    /// The checks running on the host: the entries whose hold is locked or
    /// whose run directory a supervisor holds.
    pub fn running(&self) -> Result<Running> {
        let mut out = Running::default();
        for (name, e) in self.recorded()? {
            if self.live(&name, &e)? {
                match e.kind {
                    Kind::Queue => out.queue += 1,
                    Kind::Session => out.sessions += 1,
                }
            }
        }
        Ok(out)
    }

    /// Each repository's checks running on the host, both kinds counted:
    /// its runs under test (record 0123; D13).
    pub fn under_test(&self) -> Result<std::collections::BTreeMap<PathBuf, usize>> {
        let mut out = std::collections::BTreeMap::new();
        for (name, e) in self.recorded()? {
            if self.live(&name, &e)? {
                *out.entry(e.repository).or_default() += 1;
            }
        }
        Ok(out)
    }

    /// The host's lock, once whatever holds it lets go.
    fn lock(&self) -> Result<File> {
        lock(&self.dir)
    }

    /// Every entry, live or not, by its file's name.
    fn recorded(&self) -> Result<Vec<(String, Entry)>> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("listing {}", self.dir.display())),
        };
        let mut out = Vec::new();
        for entry in entries {
            let path = entry?.path();
            let file = path.file_name().unwrap_or_default().to_string_lossy();
            let Some(name) = file
                .strip_suffix(".json")
                .filter(|_| !file.starts_with('.'))
            else {
                continue;
            };
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                // Removed between the listing and the read.
                Err(e) if e.kind() == ErrorKind::NotFound => continue,
                Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
            };
            let e: Entry = serde_json::from_slice(&bytes)
                .with_context(|| format!("parsing {}", path.display()))?;
            out.push((name.to_string(), e));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    /// Whether the entry `name` counts. The hold is read first: it is let
    /// go only once the supervisor holds the run directory, so a run
    /// between the two is never missed.
    fn live(&self, name: &str, entry: &Entry) -> Result<bool> {
        Ok(held(&self.dir.join(format!("{name}.hold")))? || supervised(&entry.run_dir)?)
    }

    fn remove(&self, name: &str) -> Result<()> {
        remove(&self.dir, name)
    }
}

impl Admission {
    /// What kind of run the room was admitted for.
    pub fn kind(&self) -> Kind {
        self.entry.kind
    }

    /// The run begins in `run_dir`, in the place `place`: its entry names
    /// them from now, written whole, while its hold keeps it counted.
    pub fn begin(&mut self, place: &str, run_dir: &Path) -> Result<()> {
        self.entry.place = place.to_string();
        self.entry.run_dir = run_dir.to_path_buf();
        let _lock = lock(&self.dir)?;
        let tmp = self.dir.join(format!(".{}.tmp", self.name));
        let path = self.dir.join(format!("{}.json", self.name));
        std::fs::write(&tmp, serde_json::to_vec_pretty(&self.entry)?)
            .with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))
    }

    /// The run's supervisor holds its run directory, which keeps the
    /// entry counted from now: let the hold go.
    pub fn started(mut self) {
        self.hold = None;
    }
}

/// A name for an admission no other on the host takes: this process's,
/// its count of admissions and the time.
fn admission_name() -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("run-{}-{n}-{at}", std::process::id())
}

impl Drop for Admission {
    /// A start that failed gives its part of the bound back at once; one
    /// that cannot is given back by the next admission.
    fn drop(&mut self) {
        if self.hold.is_some() {
            if let Ok(_lock) = lock(&self.dir) {
                let _ = remove(&self.dir, &self.name);
            }
        }
    }
}

/// The lock over the registry in `dir`, once whatever holds it lets go.
fn lock(dir: &Path) -> Result<File> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let file = File::create(dir.join(".lock"))?;
    file.lock().context("locking the host's running checks")?;
    Ok(file)
}

/// Whether some process holds the lock of the file at `path`.
fn held(path: &Path) -> Result<bool> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e).with_context(|| format!("opening {}", path.display())),
    };
    match file.try_lock() {
        Ok(()) => Ok(false),
        Err(std::fs::TryLockError::WouldBlock) => Ok(true),
        Err(std::fs::TryLockError::Error(e)) => {
            Err(e).with_context(|| format!("reading the lock of {}", path.display()))
        }
    }
}

/// Remove the entry `name` and its hold, the entry first.
fn remove(dir: &Path, name: &str) -> Result<()> {
    for path in [
        dir.join(format!("{name}.json")),
        dir.join(format!("{name}.hold")),
    ] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("removing {}", path.display())),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};

    /// A scratch state directory, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("sluice-host-count-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        fn count(&self) -> HostCount {
            HostCount::new(self.0.join("running"))
        }

        /// A run directory, `n`, of the repository under the scratch.
        fn run_dir(&self, n: &str) -> PathBuf {
            let dir = self.0.join("repo/sluice/.runs").join(n);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn entry(kind: Kind, run_dir: &Path) -> Entry {
        Entry {
            kind,
            repository: run_dir.join("../../.."),
            place: "sluice/try/main/1".into(),
            run_dir: run_dir.to_path_buf(),
            pid: std::process::id(),
            admitted: "2026-10-23T00:00:00Z".into(),
        }
    }

    /// The registry's entries, by the run directory each names.
    fn named(count: &HostCount) -> Vec<PathBuf> {
        count
            .recorded()
            .unwrap()
            .into_iter()
            .map(|(_, e)| e.run_dir)
            .collect()
    }

    /// Hold `dir`'s lock as its supervisor would, until dropped.
    fn supervise(dir: &Path) -> File {
        let lock = File::create(dir.join("lock")).unwrap();
        lock.lock().unwrap();
        lock
    }

    /// 71: a run is admitted while fewer than the limit run, and refused at
    /// it; a started run stays counted by its supervisor, and a start that
    /// failed gives its part back.
    #[test]
    fn an_admission_under_the_limit_is_granted_and_one_at_it_refused() {
        let s = Scratch::new("limit");
        let count = s.count();
        let (a, b, c) = (s.run_dir("a"), s.run_dir("b"), s.run_dir("c"));
        let first = count.admit(&entry(Kind::Queue, &a), 2).unwrap().unwrap();
        let second = count.admit(&entry(Kind::Session, &b), 2).unwrap().unwrap();
        assert_eq!(
            count.running().unwrap(),
            Running {
                queue: 1,
                sessions: 1
            }
        );
        assert!(count.admit(&entry(Kind::Queue, &c), 2).unwrap().is_none());

        let _supervisor = supervise(&a);
        first.started();
        assert_eq!(count.running().unwrap().queue, 1, "its supervisor holds it");
        assert!(count.admit(&entry(Kind::Queue, &c), 2).unwrap().is_none());

        drop(second);
        assert_eq!(count.running().unwrap().sessions, 0, "its start failed");
        assert!(!named(&count).contains(&b));
        let third = count.admit(&entry(Kind::Queue, &c), 2).unwrap();
        assert!(third.is_some());
    }

    /// 71 (record 0087; design D12): the host process's room counts an
    /// older sluice's admission through the registry, and its own parts
    /// are counted by an older sluice's.
    #[test]
    fn a_room_counts_an_older_sluices_admission() {
        use crate::host_process::room::{Asked, Asker, Room};
        let s = Scratch::new("older");
        let older = s.run_dir("older");
        let repository = s.0.join("repo");
        let now = std::time::Instant::now();
        let mut room = Room::over(s.count(), 1, 0);
        let admitted = s
            .count()
            .admit(&entry(Kind::Session, &older), 1)
            .unwrap()
            .expect("the older sluice's exec is admitted");
        let asker = Asker::Queue(repository.clone());
        assert!(matches!(
            room.ask(&asker, &repository, now).unwrap(),
            Asked::Refused(_)
        ));
        drop(admitted);
        let Asked::Held(id) = room.ask(&asker, &repository, now).unwrap() else {
            panic!("the room is free once the older run's start failed");
        };
        assert!(
            s.count()
                .admit(&entry(Kind::Session, &older), 1)
                .unwrap()
                .is_none(),
            "the room's part counts for the older sluice"
        );
        room.give_back(id);
        assert!(s
            .count()
            .admit(&entry(Kind::Session, &older), 1)
            .unwrap()
            .is_some());
    }

    /// 71, 135 (design D6, D12): a room made afresh, as a host process
    /// that starts after another stopped, counts the runs whose supervisors
    /// still hold them, and a run begun from a part names its directory.
    #[test]
    fn a_new_room_counts_the_runs_still_supervised() {
        use crate::host_process::room::{Asked, Asker, Room};
        let s = Scratch::new("afresh");
        let dir = s.run_dir("outlived");
        let repository = s.0.join("repo");
        let asker = Asker::Queue(repository.clone());
        let now = std::time::Instant::now();
        let supervisor = {
            let mut room = Room::over(s.count(), 1, 0);
            let Asked::Held(id) = room.ask(&asker, &repository, now).unwrap() else {
                panic!("an empty host has room");
            };
            room.begin(id, "sluice/try/main/1", &dir).unwrap();
            assert_eq!(named(&s.count()), std::slice::from_ref(&dir));
            let supervisor = supervise(&dir);
            room.started(id);
            supervisor
            // The process that held the room stops.
        };
        let mut room = Room::over(s.count(), 1, 0);
        assert_eq!(room.running().unwrap().queue, 1);
        assert!(matches!(
            room.ask(&asker, &repository, now).unwrap(),
            Asked::Refused(_)
        ));
        drop(supervisor);
        assert!(matches!(
            room.ask(&asker, &repository, now).unwrap(),
            Asked::Held(_)
        ));
    }

    /// The variable naming the one test a run of this binary is for.
    const ALONE: &str = "SLUICE_TEST_ALONE";

    /// Whether this process runs the test `name` alone. If it does not,
    /// the test is run again in a process of this binary's own that runs
    /// nothing else, and fails here where it fails there: a lock let go
    /// in a process where other tests run is held a moment longer by the
    /// copy of its descriptor a child they spawn holds until it execs,
    /// the copy of a hold's, a supervisor's or a read's own lock among
    /// them, so a lock this test reads as free may read as held.
    fn alone(name: &str) -> bool {
        if std::env::var_os(ALONE).is_some_and(|n| n == name) {
            return true;
        }
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name])
            .env(ALONE, name)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("test result: ok. 1 passed;"),
            "{name}, run alone:\n{stdout}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        false
    }

    /// The spec's "A killed run frees its part of the bound": an entry
    /// whose hold and run directory nobody holds counts nothing, and the
    /// next admission removes it.
    #[test]
    fn an_entry_whose_hold_and_run_are_free_counts_nothing_and_goes() {
        if !alone("host_count::tests::an_entry_whose_hold_and_run_are_free_counts_nothing_and_goes")
        {
            return;
        }
        let s = Scratch::new("dead");
        let count = s.count();
        let (dead, next) = (s.run_dir("dead"), s.run_dir("next"));
        {
            let supervisor = supervise(&dead);
            count
                .admit(&entry(Kind::Queue, &dead), 1)
                .unwrap()
                .unwrap()
                .started();
            assert_eq!(count.running().unwrap().queue, 1);
            // Its supervisor is killed.
            drop(supervisor);
        }
        assert_eq!(named(&count), std::slice::from_ref(&dead));
        assert_eq!(count.running().unwrap(), Running::default());
        let admitted = count.admit(&entry(Kind::Queue, &next), 1).unwrap();
        assert!(
            admitted.is_some(),
            "the dead entry holds no part of the bound"
        );
        assert_eq!(named(&count), std::slice::from_ref(&next));
        let holds = std::fs::read_dir(count.dir())
            .unwrap()
            .filter(|e| {
                let name = e.as_ref().unwrap().file_name();
                name.to_string_lossy().ends_with(".hold")
            })
            .count();
        assert_eq!(holds, 1, "the dead entry's hold went with it");
    }

    /// The spec's "Two rounds race for one free run": starters admitting
    /// at once against a limit of one leave exactly one admitted.
    #[test]
    fn two_threads_admitting_against_a_limit_of_one_leave_one_admitted() {
        let s = Scratch::new("race");
        for round in 0..20 {
            let dirs = [
                s.run_dir(&format!("{round}-a")),
                s.run_dir(&format!("{round}-b")),
            ];
            let barrier = Arc::new(Barrier::new(2));
            let threads: Vec<_> = dirs
                .iter()
                .map(|d| {
                    let (count, barrier, e) = (s.count(), barrier.clone(), entry(Kind::Queue, d));
                    std::thread::spawn(move || {
                        barrier.wait();
                        count.admit(&e, 1).unwrap()
                    })
                })
                .collect();
            let admitted: Vec<Admission> = threads
                .into_iter()
                .filter_map(|t| t.join().unwrap())
                .collect();
            assert_eq!(admitted.len(), 1, "round {round}");
            assert_eq!(s.count().running().unwrap().queue, 1, "round {round}");
        }
    }

    /// The spec's "Nothing but the count decides": both kinds count against
    /// one limit, a file that is no entry is passed over, and a host whose
    /// every core is busy is answered as an idle one.
    #[test]
    fn the_decision_reads_only_the_entries_and_the_limit() {
        let s = Scratch::new("only");
        let count = s.count();
        let decide = |count: &HostCount| {
            let (a, b, c) = (s.run_dir("a"), s.run_dir("b"), s.run_dir("c"));
            let first = count.admit(&entry(Kind::Session, &a), 3).unwrap();
            let second = count.admit(&entry(Kind::Queue, &b), 3).unwrap();
            let at = count.admit(&entry(Kind::Queue, &c), 2).unwrap();
            let under = count.admit(&entry(Kind::Queue, &c), 3).unwrap();
            let got = (
                first.is_some(),
                second.is_some(),
                at.is_some(),
                under.is_some(),
            );
            drop((first, second, at, under));
            got
        };
        std::fs::create_dir_all(count.dir()).unwrap();
        std::fs::write(count.dir().join("load"), "95.0 90.0 80.0").unwrap();
        let idle = decide(&count);
        assert_eq!(idle, (true, true, false, true));

        let stop = Arc::new(AtomicBool::new(false));
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        let busy: Vec<_> = (0..cores)
            .map(|_| {
                let stop = stop.clone();
                std::thread::spawn(move || {
                    let mut n = 0u64;
                    while !stop.load(Ordering::Relaxed) {
                        n = std::hint::black_box(n.wrapping_add(1));
                    }
                })
            })
            .collect();
        let loaded = decide(&count);
        stop.store(true, Ordering::Relaxed);
        for t in busy {
            t.join().unwrap();
        }
        assert_eq!(loaded, idle);
    }
}
