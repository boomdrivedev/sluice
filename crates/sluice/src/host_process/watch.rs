//! The file events of the host process (the change
//! `sluice-host-runs-on-a-tokio-edge`, design D4;
//! `tokio-streams-before-rxrust@0b9fc11cda8b`).
//!
//! One debouncer of notify-debouncer-full watches:
//!
//! - for each repository the host process holds: the git common folder,
//!   for `packed-refs`; its `refs`; each worktree's `HEAD` under its
//!   `worktrees`; and sluice's records in the host's files, `sluice/` and
//!   each branch's folder in it;
//! - the run folder of each run the host's registry counts, and the
//!   registry itself, so the end of a run frees its part of the bound;
//! - the host process's own folder and the state folder that holds it;
//! - the folder of the host's settings file.
//!
//! The debounce time is 200 ms: many events in one burst are one event.
//! The watch reads no working copy. Where the system refuses a watch, the
//! host process says so once in its log, and the minute's backstop reads
//! that repository.
//!
//! The run folders are watched by a debouncer of their own, as they change
//! with each run's start and end, and on macOS each repository by one of
//! its own. On macOS a debouncer's paths are one FSEvents stream, which
//! each change of its paths starts afresh: what a stream that started
//! afresh watches is read as if an event came for it, since no stream saw
//! the time between the two; and a stream the system would not start is
//! started again a second later, then read so too.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify_debouncer_full::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer_opt, DebounceEventResult, Debouncer, NoCache};
use tokio::sync::mpsc;

use super::log;

/// How long the events of one burst are joined.
pub const DEBOUNCE: Duration = Duration::from_millis(200);

/// How long [`Bursts`] waits for a further flush of the debouncer before
/// it gives what it joined. The debouncer gives each path's events once
/// that path is quiet, at its tick, a quarter of [`DEBOUNCE`], so the
/// paths of one burst can come in flushes a tick or two apart.
pub const JOIN: Duration = Duration::from_millis(120);

/// The debouncer's flushes, joined into one burst while they come less
/// than [`JOIN`] apart.
pub struct Bursts {
    flushes: mpsc::UnboundedReceiver<DebounceEventResult>,
    /// What it joined so far, and when it gives it unless a further flush
    /// comes first. Both are kept across a cancelled call, so a `select!`
    /// that drops [`Bursts::next`] loses no event and waits no longer.
    joined: Option<(DebounceEventResult, tokio::time::Instant)>,
}

impl Bursts {
    /// The next burst; `None` once the watch is gone.
    pub async fn next(&mut self) -> Option<DebounceEventResult> {
        if self.joined.is_none() {
            let first = self.flushes.recv().await?;
            self.joined = Some((first, tokio::time::Instant::now() + JOIN));
        }
        loop {
            let until = self.joined.as_ref().expect("a flush").1;
            match tokio::time::timeout_at(until, self.flushes.recv()).await {
                Ok(Some(more)) => {
                    let (joined, _) = self.joined.take().expect("a flush");
                    let joined = match (joined, more) {
                        (Ok(mut a), Ok(b)) => {
                            a.extend(b);
                            Ok(a)
                        }
                        (Err(mut a), Err(b)) => {
                            a.extend(b);
                            Err(a)
                        }
                        (Err(a), Ok(_)) | (Ok(_), Err(a)) => Err(a),
                    };
                    self.joined = Some((joined, tokio::time::Instant::now() + JOIN));
                }
                _ => return self.joined.take().map(|(joined, _)| joined),
            }
        }
    }
}

/// What a burst of events touched.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// The repositories whose refs or records changed.
    pub repositories: BTreeSet<PathBuf>,
    /// A run ended or began, or the registry changed.
    pub runs: bool,
    /// The host process's folder, or the state folder, changed.
    pub state: bool,
    /// The settings file, or a folder on its way, changed.
    pub settings: bool,
}

impl Seen {
    pub fn is_empty(&self) -> bool {
        self == &Seen::default()
    }
}

/// What one repository is watched by: each path and how.
fn repository_paths(common: &Path) -> Vec<(PathBuf, RecursiveMode)> {
    let mut paths = vec![
        (common.to_path_buf(), RecursiveMode::NonRecursive),
        (common.join("refs"), RecursiveMode::Recursive),
    ];
    let worktrees = common.join("worktrees");
    if worktrees.is_dir() {
        paths.push((worktrees, RecursiveMode::Recursive));
    }
    let records = common.join("sluice");
    if records.is_dir() {
        paths.push((records.clone(), RecursiveMode::NonRecursive));
        for branch in branch_folders(&records) {
            paths.push((branch.join("submissions"), RecursiveMode::NonRecursive));
            paths.push((branch, RecursiveMode::NonRecursive));
        }
    }
    paths.retain(|(p, _)| p.exists());
    paths
}

/// The folders of the branches in the records at `records`: each folder
/// that holds a `branch.json` or a `version`, at any depth, as a branch's
/// name may hold `/`. A folder whose name starts with `.` is no branch's.
fn branch_folders(records: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![records.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "submissions" || !path.is_dir() {
                continue;
            }
            if path.join("branch.json").exists() || path.join("version").exists() {
                out.push(path.clone());
            }
            stack.push(path);
        }
    }
    out
}

/// Whether an event at `path` under the repository `common` tells of a
/// change to its refs or records: a ref, `packed-refs`, a worktree's
/// `HEAD`, or a file of the records that is no lock and no run's.
fn tells(common: &Path, path: &Path) -> bool {
    let Ok(rest) = path.strip_prefix(common) else {
        return false;
    };
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let mut parts = rest.components().map(|c| c.as_os_str().to_string_lossy());
    match parts.next().as_deref() {
        Some("refs") => !name.ends_with(".lock"),
        Some("packed-refs") => rest.components().count() == 1,
        Some("worktrees") => name == "HEAD",
        Some("sluice") => {
            !name.ends_with(".lock")
                && name != "lock"
                && !name.ends_with(".tmp")
                && !rest
                    .components()
                    .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
        }
        _ => false,
    }
}

/// `path` and its real path, through every link: an event names either,
/// as the system reports it (macOS's FSEvents names the real path).
fn aliases(path: &Path) -> Vec<PathBuf> {
    let mut out = vec![path.to_path_buf()];
    if let Ok(real) = std::fs::canonicalize(path) {
        if real != path {
            out.push(real);
        }
    }
    out
}

/// Whether `path` is `at`, or under it, by either of its names.
fn under(path: &Path, at: &[PathBuf]) -> bool {
    at.iter().any(|a| path.starts_with(a))
}

/// Whether a change of a stream's paths starts the stream afresh: macOS's
/// FSEvents watches every path of a stream in one stream, which notify
/// stops and starts again at each change of its paths. The new stream sees
/// no event that came between the old one's end and its own start.
const RESTARTS: bool = cfg!(target_os = "macos");

/// How long a stream the system would not start waits before it is
/// started again.
pub const STALLED_FOR: Duration = Duration::from_secs(1);

/// Whether `e` says the system would not start the stream: notify's
/// FSEvents watcher then has no stream at all, for any of its paths, until
/// its paths next change.
fn stalls(e: &notify_debouncer_full::notify::Error) -> bool {
    matches!(e.kind, notify_debouncer_full::notify::ErrorKind::Generic(_))
}

/// Whether an event of `kind` only says that a file was opened, read or
/// closed after a read, which changes nothing. On Linux, inotify reports
/// each open and read, and `sluice host`'s own read of a repository opens
/// its refs and records: such an event read as a change has the host read
/// the repository again, which opens them again, with no end (macOS's
/// FSEvents reports no read). A close after a write is a change.
fn reads_only(kind: &notify_debouncer_full::notify::EventKind) -> bool {
    use notify_debouncer_full::notify::event::{AccessKind, AccessMode, EventKind};
    matches!(kind, EventKind::Access(a) if *a != AccessKind::Close(AccessMode::Write))
}

/// One debouncer and the paths it watches.
struct Stream {
    debouncer: Debouncer<RecommendedWatcher, NoCache>,
    /// Each path it watches, and how.
    paths: BTreeMap<PathBuf, RecursiveMode>,
    /// The system would not start its stream: none of its paths is seen
    /// until [`Stream::revive`] starts it again.
    stalled: bool,
    /// It started its stream afresh since [`Watch::missed`] last asked.
    restarted: bool,
}

impl Stream {
    fn new(seen: mpsc::UnboundedSender<DebounceEventResult>) -> anyhow::Result<Stream> {
        let debouncer = new_debouncer_opt::<_, RecommendedWatcher, NoCache>(
            DEBOUNCE,
            None,
            move |result: DebounceEventResult| {
                let result = result.map(|events| {
                    events
                        .into_iter()
                        .filter(|e| !reads_only(&e.event.kind))
                        .collect::<Vec<_>>()
                });
                if result.as_ref().is_ok_and(Vec::is_empty) {
                    return;
                }
                let _ = seen.send(result);
            },
            NoCache,
            Default::default(),
        )?;
        Ok(Stream {
            debouncer,
            paths: BTreeMap::new(),
            stalled: false,
            restarted: false,
        })
    }

    /// What a change of its paths answered: its stream started afresh, or
    /// the system would not start it.
    fn answered(&mut self, answer: &notify_debouncer_full::notify::Result<()>) {
        match answer {
            Err(e) if stalls(e) => {
                if !self.stalled {
                    log(&format!(
                        "sluice host: the system would not start its watch of files ({e}); \
                         it starts it again in a second"
                    ));
                }
                self.stalled = true;
            }
            _ => {
                if self.stalled {
                    log("sluice host: its watch of files runs again; it reads what it may have missed");
                    self.restarted = true;
                }
                self.stalled = false;
                self.restarted |= RESTARTS;
            }
        }
    }

    /// Watch `path`: an error where the system refused the path itself. A
    /// path a stalled stream holds is watched once the stream starts.
    fn add(&mut self, path: &Path, mode: RecursiveMode) -> Result<(), String> {
        let answer = self.debouncer.watch(path, mode);
        self.answered(&answer);
        match answer {
            Err(e) if !stalls(&e) => Err(e.to_string()),
            _ => {
                self.paths.insert(path.to_path_buf(), mode);
                Ok(())
            }
        }
    }

    fn remove(&mut self, path: &Path) {
        let answer = self.debouncer.unwatch(path);
        self.paths.remove(path);
        self.answered(&answer);
    }

    /// Start its stream again, where the system would not start it: by
    /// one of its paths watched afresh.
    fn revive(&mut self) {
        let Some((path, mode)) = self.paths.iter().next().map(|(p, m)| (p.clone(), *m)) else {
            self.stalled = false;
            return;
        };
        let _ = self.debouncer.unwatch(&path);
        let answer = self.debouncer.watch(&path, mode);
        if answer.as_ref().is_err_and(|e| !stalls(e)) {
            self.paths.remove(&path);
        }
        self.answered(&answer);
    }
}

/// The host process's watch of its files, in streams kept apart so a
/// change of one's paths starts none of the others afresh: the state
/// folder and the settings in one; the run folders, which change as runs
/// begin and end, in another; and on macOS each repository in one of its
/// own, so a repository a command names, or a branch's folder made, starts
/// afresh no stream of another repository.
pub struct Watch {
    seen: mpsc::UnboundedSender<DebounceEventResult>,
    steady: Stream,
    runs_stream: Stream,
    /// Each repository's own stream, on macOS.
    repository_streams: BTreeMap<PathBuf, Stream>,
    /// Each repository watched, and the paths it is watched by.
    repositories: BTreeMap<PathBuf, Vec<PathBuf>>,
    /// Each repository's names, its own and its real path.
    names: BTreeMap<PathBuf, Vec<PathBuf>>,
    /// The run folders watched, and the registry, by each of their names.
    runs: BTreeSet<PathBuf>,
    run_names: Vec<PathBuf>,
    registry: Option<PathBuf>,
    registry_names: Vec<PathBuf>,
    /// The state folder and the host process's folder, by each of their
    /// names.
    state: Vec<PathBuf>,
    /// The settings file, and the folder watched for it, by each of their
    /// names.
    settings: Option<(PathBuf, PathBuf)>,
    settings_names: Vec<PathBuf>,
    /// The watches the system refused, said once each.
    refused: BTreeSet<PathBuf>,
}

/// Which of [`Watch`]'s streams.
#[derive(Clone, Copy)]
enum Which<'a> {
    Steady,
    Runs,
    Repository(&'a Path),
}

impl Watch {
    /// A watch, and its bursts of events.
    pub fn new() -> anyhow::Result<(Watch, Bursts)> {
        let (seen, flushes) = mpsc::unbounded_channel();
        let bursts = Bursts {
            flushes,
            joined: None,
        };
        Ok((
            Watch {
                steady: Stream::new(seen.clone())?,
                runs_stream: Stream::new(seen.clone())?,
                repository_streams: BTreeMap::new(),
                seen,
                repositories: BTreeMap::new(),
                names: BTreeMap::new(),
                runs: BTreeSet::new(),
                run_names: Vec::new(),
                registry: None,
                registry_names: Vec::new(),
                state: Vec::new(),
                settings: None,
                settings_names: Vec::new(),
                refused: BTreeSet::new(),
            },
            bursts,
        ))
    }

    /// The stream of `which`, a repository's made on macOS where it has
    /// none. A repository whose stream could not be made is watched in the
    /// steady stream.
    fn stream(&mut self, which: Which) -> &mut Stream {
        match which {
            Which::Steady => &mut self.steady,
            Which::Runs => &mut self.runs_stream,
            Which::Repository(r) => {
                if RESTARTS && !self.repository_streams.contains_key(r) {
                    match Stream::new(self.seen.clone()) {
                        Ok(s) => {
                            self.repository_streams.insert(r.to_path_buf(), s);
                        }
                        Err(e) => log(&format!(
                            "sluice host: watching {} apart: {e:#}; it is watched with the state folder",
                            r.display()
                        )),
                    }
                }
                match self.repository_streams.get_mut(r) {
                    Some(s) => s,
                    None => &mut self.steady,
                }
            }
        }
    }

    fn streams(&mut self) -> impl Iterator<Item = &mut Stream> {
        [&mut self.steady, &mut self.runs_stream]
            .into_iter()
            .chain(self.repository_streams.values_mut())
    }

    fn add(&mut self, which: Which, path: &Path, mode: RecursiveMode) -> bool {
        match self.stream(which).add(path, mode) {
            Ok(()) => {
                self.refused.remove(path);
                true
            }
            Err(e) => {
                if self.refused.insert(path.to_path_buf()) {
                    log(&format!(
                        "sluice host: watching {}: {e}; the minute's backstop reads it",
                        path.display()
                    ));
                }
                false
            }
        }
    }

    fn remove(&mut self, which: Which, path: &Path) {
        self.stream(which).remove(path);
    }

    /// Whether the system would not start a stream of the watch, which
    /// [`Watch::revive`] starts again after [`STALLED_FOR`].
    pub fn stalled(&self) -> bool {
        self.steady.stalled
            || self.runs_stream.stalled
            || self.repository_streams.values().any(|s| s.stalled)
    }

    /// Start again each stream the system would not start.
    pub fn revive(&mut self) {
        for s in self.streams() {
            if s.stalled {
                s.revive();
            }
        }
    }

    /// What the watch may have missed since it was last asked: what its
    /// streams watch, for each that started afresh, as a restart sees no
    /// event of the time between two streams, and a stalled stream sees
    /// none at all.
    pub fn missed(&mut self) -> Seen {
        let mut seen = Seen::default();
        if std::mem::take(&mut self.steady.restarted) {
            seen.state = !self.state.is_empty();
            seen.settings = self.settings.is_some();
            seen.repositories = self
                .repositories
                .keys()
                .filter(|r| !self.repository_streams.contains_key(*r))
                .cloned()
                .collect();
        }
        if std::mem::take(&mut self.runs_stream.restarted) {
            seen.runs = true;
        }
        for (r, s) in &mut self.repository_streams {
            if std::mem::take(&mut s.restarted) {
                seen.repositories.insert(r.clone());
            }
        }
        seen
    }

    /// Watch `state`, the state folder, and `dir`, the host process's
    /// folder in it.
    pub fn state(&mut self, state: &Path, dir: &Path) {
        for p in [state, dir] {
            if self.add(Which::Steady, p, RecursiveMode::NonRecursive) {
                self.state.extend(aliases(p));
            }
        }
    }

    /// Watch the folder of the settings file `file`: its own where it is
    /// there, else the nearest folder on its way that is.
    pub fn settings(&mut self, file: Option<&Path>) {
        let Some(file) = file else {
            return;
        };
        let folder = file
            .ancestors()
            .skip(1)
            .find(|a| a.is_dir())
            .map(Path::to_path_buf);
        let Some(folder) = folder else {
            return;
        };
        if let Some((_, watched)) = &self.settings {
            if *watched == folder {
                return;
            }
            let watched = watched.clone();
            self.remove(Which::Steady, &watched);
        }
        if self.add(Which::Steady, &folder, RecursiveMode::NonRecursive) {
            self.settings_names = aliases(&folder);
            self.settings_names.extend(aliases(file));
            if let Some(name) = file.file_name() {
                for f in aliases(&folder) {
                    self.settings_names.push(f.join(name));
                }
            }
            self.settings = Some((file.to_path_buf(), folder));
        }
    }

    /// Watch each of `repositories`, and no other: a repository the index
    /// adds is watched at once, and one it drops is watched no longer. A
    /// repository already watched is watched again where its branches'
    /// folders changed.
    pub fn repositories(&mut self, repositories: &[PathBuf]) {
        let keep: BTreeSet<&PathBuf> = repositories.iter().collect();
        let gone: Vec<PathBuf> = self
            .repositories
            .keys()
            .filter(|r| !keep.contains(r))
            .cloned()
            .collect();
        for r in gone {
            for p in self.repositories.remove(&r).unwrap_or_default() {
                if !self.repository_streams.contains_key(&r) {
                    self.remove(Which::Steady, &p);
                }
            }
            self.repository_streams.remove(&r);
            self.names.remove(&r);
        }
        for r in repositories {
            let _ = self.repository(r);
        }
    }

    /// Watch `common`'s refs and records, its branches' folders as they
    /// stand now: whether it was watched by none of them before.
    pub fn repository(&mut self, common: &Path) -> bool {
        let new = !self.repositories.contains_key(common);
        let wanted = repository_paths(common);
        let had = self.repositories.get(common).cloned().unwrap_or_default();
        let wanted_paths: Vec<PathBuf> = wanted.iter().map(|(p, _)| p.clone()).collect();
        if had == wanted_paths && !new {
            return false;
        }
        for p in had.iter().filter(|p| !wanted_paths.contains(p)) {
            self.remove(Which::Repository(common), p);
        }
        let mut watched = Vec::new();
        for (p, mode) in wanted {
            if had.contains(&p) || self.add(Which::Repository(common), &p, mode) {
                watched.push(p);
            }
        }
        // A repository watched afresh is read by whatever named it: its own
        // stream's starts missed nothing of it.
        if new {
            if let Some(s) = self.repository_streams.get_mut(common) {
                s.restarted = false;
            }
        }
        self.repositories.insert(common.to_path_buf(), watched);
        self.names.insert(common.to_path_buf(), aliases(common));
        new
    }

    /// Watch `registry`, the host's registry of running checks, and each
    /// of `runs`, the run folders it counts now, and no other.
    pub fn runs(&mut self, registry: &Path, runs: &BTreeSet<PathBuf>) {
        if self.registry.as_deref() != Some(registry)
            && std::fs::create_dir_all(registry).is_ok()
            && self.add(Which::Runs, registry, RecursiveMode::NonRecursive)
        {
            self.registry = Some(registry.to_path_buf());
            self.registry_names = aliases(registry);
        }
        let gone: Vec<PathBuf> = self.runs.difference(runs).cloned().collect();
        for p in gone {
            self.remove(Which::Runs, &p);
            self.runs.remove(&p);
        }
        for p in runs {
            if !self.runs.contains(p)
                && p.is_dir()
                && self.add(Which::Runs, p, RecursiveMode::NonRecursive)
            {
                self.runs.insert(p.clone());
            }
        }
        self.run_names = self.runs.iter().flat_map(|r| aliases(r)).collect();
    }

    /// What a burst of events at `paths` touched.
    pub fn seen<'a>(&self, paths: impl IntoIterator<Item = &'a Path>) -> Seen {
        let mut seen = Seen::default();
        for path in paths {
            if self
                .settings_names
                .iter()
                .any(|n| n == path || n.starts_with(path))
            {
                seen.settings = true;
            }
            if self
                .state
                .iter()
                .any(|s| path == s || path.parent() == Some(s.as_path()))
            {
                seen.state = true;
            }
            if under(path, &self.registry_names) || under(path, &self.run_names) {
                seen.runs = true;
            }
            for (common, names) in &self.names {
                if names.iter().any(|n| tells(n, path)) {
                    seen.repositories.insert(common.clone());
                }
            }
        }
        seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An open, a read and a close after a read change nothing; a close
    /// after a write, a write, a create and a rename do.
    #[test]
    fn a_read_is_no_change() {
        use notify_debouncer_full::notify::event::{
            AccessKind, AccessMode, CreateKind, DataChange, EventKind, ModifyKind, RenameMode,
        };
        for kind in [
            EventKind::Access(AccessKind::Open(AccessMode::Any)),
            EventKind::Access(AccessKind::Read),
            EventKind::Access(AccessKind::Close(AccessMode::Read)),
        ] {
            assert!(reads_only(&kind), "{kind:?}");
        }
        for kind in [
            EventKind::Access(AccessKind::Close(AccessMode::Write)),
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            EventKind::Create(CreateKind::File),
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
        ] {
            assert!(!reads_only(&kind), "{kind:?}");
        }
    }

    /// A folder of the test's own, removed as it ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir =
                std::env::temp_dir().join(format!("sluice-watch-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(std::fs::canonicalize(&dir).unwrap())
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A stream started afresh sees no event of the time between the two
    /// streams: what it watches is read as missed on macOS, once. A
    /// repository watched again starts only its own stream afresh, and a
    /// run's folder watched only the runs' stream.
    #[test]
    fn a_streams_start_afresh_is_read_as_missed() {
        // A stream the system would not start, under load, is started
        // again as the host process starts it.
        let running = |watch: &mut Watch| {
            let until = std::time::Instant::now() + Duration::from_secs(30);
            while watch.stalled() {
                assert!(std::time::Instant::now() < until, "the stream starts");
                std::thread::sleep(STALLED_FOR);
                watch.revive();
            }
        };
        let scratch = Scratch::new("missed");
        let (state, dir) = (scratch.0.join("state"), scratch.0.join("state/host"));
        let common = scratch.0.join("repo.git");
        let (registry, run) = (scratch.0.join("runs"), scratch.0.join("runs/1"));
        for p in [&dir, &common.join("refs"), &run] {
            std::fs::create_dir_all(p).unwrap();
        }
        let (mut watch, _bursts) = Watch::new().unwrap();
        watch.state(&state, &dir);
        watch.repositories(std::slice::from_ref(&common));
        running(&mut watch);
        let missed = watch.missed();
        assert_eq!(missed.state, RESTARTS);
        assert!(!missed.runs);
        assert!(watch.missed().is_empty(), "read as missed once");

        // A branch's folder made: the repository is watched again, by its
        // own stream on macOS, which misses nothing of the state folder.
        std::fs::create_dir_all(common.join("sluice/main")).unwrap();
        std::fs::write(common.join("sluice/main/branch.json"), "{}").unwrap();
        assert!(!watch.repository(&common), "watched already");
        running(&mut watch);
        let missed = watch.missed();
        assert_eq!(missed.repositories.contains(&common), RESTARTS);
        assert!(!missed.state && !missed.settings && !missed.runs);

        watch.runs(&registry, &BTreeSet::from([run]));
        running(&mut watch);
        let missed = watch.missed();
        assert_eq!(missed.runs, RESTARTS);
        assert!(!missed.state && !missed.settings && missed.repositories.is_empty());
    }

    /// D4: an event tells of a change to a repository's refs and records,
    /// and not of a lock, a run's file or another file of git's.
    #[test]
    fn an_event_tells_of_refs_and_records_alone() {
        let common = Path::new("/r/repo.git");
        for told in [
            "refs/heads/main",
            "refs/sluice-fetched/origin/main",
            "packed-refs",
            "worktrees/a/HEAD",
            "sluice/main/branch.json",
            "sluice/bolt/x/submissions/3.json",
            "sluice/results.jsonl",
        ] {
            assert!(tells(common, &common.join(told)), "{told}");
        }
        for not in [
            "refs/heads/main.lock",
            "index",
            "FETCH_HEAD",
            "objects/ab/cdef",
            "worktrees/a/index",
            "sluice/main/lock",
            "sluice/.lock",
            "sluice/.progress/main/c.json",
            "sluice/.runs/p/out.log",
            "sluice/main/.pending.json",
        ] {
            assert!(!tells(common, &common.join(not)), "{not}");
        }
        assert!(!tells(common, Path::new("/r/other.git/refs/heads/main")));
    }
}
