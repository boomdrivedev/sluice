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
    /// What it joined so far. It is kept across a cancelled call, so a
    /// `select!` that drops [`Bursts::next`] loses no event.
    joined: Option<DebounceEventResult>,
}

impl Bursts {
    /// The next burst; `None` once the watch is gone.
    pub async fn next(&mut self) -> Option<DebounceEventResult> {
        if self.joined.is_none() {
            self.joined = Some(self.flushes.recv().await?);
        }
        loop {
            match tokio::time::timeout(JOIN, self.flushes.recv()).await {
                Ok(Some(more)) => {
                    let joined = self.joined.take().expect("a flush");
                    self.joined = Some(match (joined, more) {
                        (Ok(mut a), Ok(b)) => {
                            a.extend(b);
                            Ok(a)
                        }
                        (Err(mut a), Err(b)) => {
                            a.extend(b);
                            Err(a)
                        }
                        (Err(a), Ok(_)) | (Ok(_), Err(a)) => Err(a),
                    });
                }
                _ => return self.joined.take(),
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

/// The host process's watch of its files.
pub struct Watch {
    debouncer: Debouncer<RecommendedWatcher, NoCache>,
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

impl Watch {
    /// A watch, and its bursts of events.
    pub fn new() -> anyhow::Result<(Watch, Bursts)> {
        let (seen, flushes) = mpsc::unbounded_channel();
        let debouncer = new_debouncer_opt::<_, RecommendedWatcher, NoCache>(
            DEBOUNCE,
            None,
            move |result: DebounceEventResult| {
                let _ = seen.send(result);
            },
            NoCache,
            Default::default(),
        )?;
        let bursts = Bursts {
            flushes,
            joined: None,
        };
        Ok((
            Watch {
                debouncer,
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

    fn add(&mut self, path: &Path, mode: RecursiveMode) -> bool {
        match self.debouncer.watch(path, mode) {
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

    fn remove(&mut self, path: &Path) {
        let _ = self.debouncer.unwatch(path);
    }

    /// Watch `state`, the state folder, and `dir`, the host process's
    /// folder in it.
    pub fn state(&mut self, state: &Path, dir: &Path) {
        for p in [state, dir] {
            if self.add(p, RecursiveMode::NonRecursive) {
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
            self.remove(&watched);
        }
        if self.add(&folder, RecursiveMode::NonRecursive) {
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
                self.remove(&p);
            }
            self.names.remove(&r);
        }
        for r in repositories {
            self.repository(r);
        }
    }

    /// Watch `common`'s refs and records, its branches' folders as they
    /// stand now.
    pub fn repository(&mut self, common: &Path) {
        let wanted = repository_paths(common);
        let had = self.repositories.get(common).cloned().unwrap_or_default();
        let wanted_paths: Vec<PathBuf> = wanted.iter().map(|(p, _)| p.clone()).collect();
        if had == wanted_paths {
            return;
        }
        for p in had.iter().filter(|p| !wanted_paths.contains(p)) {
            self.remove(p);
        }
        let mut watched = Vec::new();
        for (p, mode) in wanted {
            if had.contains(&p) || self.add(&p, mode) {
                watched.push(p);
            }
        }
        self.repositories.insert(common.to_path_buf(), watched);
        self.names.insert(common.to_path_buf(), aliases(common));
    }

    /// Watch `registry`, the host's registry of running checks, and each
    /// of `runs`, the run folders it counts now, and no other.
    pub fn runs(&mut self, registry: &Path, runs: &BTreeSet<PathBuf>) {
        if self.registry.as_deref() != Some(registry)
            && std::fs::create_dir_all(registry).is_ok()
            && self.add(registry, RecursiveMode::NonRecursive)
        {
            self.registry = Some(registry.to_path_buf());
            self.registry_names = aliases(registry);
        }
        let gone: Vec<PathBuf> = self.runs.difference(runs).cloned().collect();
        for p in gone {
            self.remove(&p);
            self.runs.remove(&p);
        }
        for p in runs {
            if !self.runs.contains(p) && p.is_dir() && self.add(p, RecursiveMode::NonRecursive) {
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
