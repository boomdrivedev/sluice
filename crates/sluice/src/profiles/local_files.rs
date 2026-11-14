//! The records seam's `local files` profile (C.6): one host's records,
//! plain JSON under the repository's common git directory, readable by
//! every worktree with no sluice process running (26, core.11).
//! `docs/plans/phase-1-integrations.md` section 6 is the reference.
//!
//! ```text
//! <common>/sluice/
//!   results.jsonl  .next-id  .lock
//!   <branch>/  version  branch.json  submissions/<id>.json  log.jsonl
//!              lock  .pending.json
//!   .progress/<branch>/<candidate>.json   a run's tasks (ADR 0028)
//!   .refused/<branch>.json                a refusal of the branch's
//!                                         remote that stands (133)
//! ```

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sluice_seams::{
    Changes, Guard, ResultAsk, Snapshot, State, TaskLog, TaskProgress, Written, FINISHED, KEPT_LOGS,
};

use crate::runs::flat_name;

const VERSION: &str = "version";
const LOCK: &str = "lock";
const PENDING: &str = ".pending.json";
const RESULTS: &str = "results.jsonl";
/// Could-not-run-1 116: beside the results log, never in it, so a reader
/// of the results log alone reads what it always did.
const SET_ASIDE: &str = "set-aside.jsonl";
const PROGRESS: &str = ".progress";
/// 133: this host's refusals of its branches' remotes, beside the records
/// and never in them; the dot keeps them out of [`State::branches`].
const REFUSED: &str = ".refused";
/// 133 (design D27): this host's refusal of each branch's intake.
const INTAKE_REFUSED: &str = ".intake";
const PROGRESS_SCHEMA: &str = "sluice/progress@1";

/// One candidate's tasks as kept: the candidate is named inside, since a
/// long name's file is shortened.
#[derive(Serialize, Deserialize)]
struct Kept {
    schema: String,
    candidate: String,
    tasks: Vec<TaskProgress>,
}

pub struct LocalFiles {
    root: PathBuf,
    /// The branches whose lock this process holds through a live guard:
    /// `write` refuses any other.
    held: RefCell<BTreeSet<String>>,
    /// 129: each branch's records as last read whole, answered again while
    /// their version stands and no write is pending.
    kept: RefCell<BTreeMap<String, Snapshot>>,
}

/// A write in progress, written before any file it changes and removed
/// once every file holds it, so a crash between leaves the whole write to
/// finish (63). The records are JSON, so the bytes are UTF-8.
#[derive(Serialize, Deserialize)]
struct Pending {
    version: u64,
    put: BTreeMap<String, String>,
    append: BTreeMap<String, String>,
    /// Each appended log's length before the append.
    lengths: BTreeMap<String, u64>,
    /// Files the write takes away, after it has put the rest.
    #[serde(default)]
    remove: BTreeSet<String>,
}

struct BranchGuard<'a> {
    files: &'a LocalFiles,
    branch: String,
    _lock: File,
}

impl Guard for BranchGuard<'_> {}

impl Drop for BranchGuard<'_> {
    fn drop(&mut self) {
        self.files.held.borrow_mut().remove(&self.branch);
    }
}

impl LocalFiles {
    /// The records of the repository whose common git directory is
    /// `common`.
    pub fn new(common: &Path) -> Self {
        LocalFiles::at(common.join("sluice"))
    }

    /// Records under `root`, for what a host keeps of a remote's records:
    /// its locks, its runs' tasks and the logs it kept.
    pub fn at(root: PathBuf) -> Self {
        LocalFiles {
            root,
            held: RefCell::new(BTreeSet::new()),
            kept: RefCell::default(),
        }
    }

    /// Record 0122 (design D5; record 0114): raise the host's count past
    /// `number`, so it gives no number up to it; a count already past it is
    /// left as it is.
    pub fn raise_ids_past(&self, number: u64) -> Result<()> {
        std::fs::create_dir_all(&self.root)
            .with_context(|| format!("creating {}", self.root.display()))?;
        let lock = open_lock(&self.root.join(".lock"))?;
        lock.lock().context("locking the repository's records")?;
        let path = self.root.join(".next-id");
        let next: u64 = match std::fs::read_to_string(&path) {
            Ok(s) => s
                .trim()
                .parse()
                .with_context(|| format!("{} holds {s:?}", path.display()))?,
            Err(e) if e.kind() == ErrorKind::NotFound => 1,
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        if next <= number {
            write_atomic(
                &self.root,
                ".next-id",
                format!("{}\n", number + 1).as_bytes(),
            )?;
        }
        Ok(())
    }

    fn dir(&self, branch: &str) -> Result<PathBuf> {
        if branch.is_empty()
            || branch == RESULTS
            || branch
                .split('/')
                .any(|c| c.is_empty() || c.starts_with('.'))
        {
            bail!("{branch:?} cannot name a branch's records");
        }
        Ok(self.root.join(branch))
    }

    /// 133: this host's document `branch`'s file under `dir` keeps, a
    /// refusal of its remote or of its intake (design D27).
    fn read_kept(&self, dir: &str, branch: &str) -> Result<Option<Vec<u8>>> {
        self.dir(branch)?;
        let path = self.root.join(dir).join(format!("{branch}.json"));
        match std::fs::read(&path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// Keep `doc` as `branch`'s file under `dir`, or remove it.
    fn keep_kept(&self, dir: &str, branch: &str, doc: Option<&[u8]>) -> Result<()> {
        self.dir(branch)?;
        let dir = self.root.join(dir);
        let name = format!("{branch}.json");
        match doc {
            Some(doc) => write_atomic(&dir, &name, doc),
            None => match std::fs::remove_file(dir.join(&name)) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e).with_context(|| format!("removing {name}")),
            },
        }
    }

    /// 69: where the results log is (design D17).
    pub(crate) fn results_path(&self) -> PathBuf {
        self.root.join(RESULTS)
    }

    /// 69: the results log as written, one JSON document per line.
    pub(crate) fn results_log(&self) -> Result<Vec<u8>> {
        match std::fs::read(self.root.join(RESULTS)) {
            Ok(b) => Ok(b),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e).context("reading the results log"),
        }
    }

    /// Refuse a branch whose directory passes through another branch's:
    /// records outlive their branch, so `a`'s may still exist when `a/b`
    /// is written.
    fn check_nesting(&self, branch: &str) -> Result<()> {
        let mut at = self.root.clone();
        let parts: Vec<&str> = branch.split('/').collect();
        for part in &parts[..parts.len() - 1] {
            at.push(part);
            if at.join(VERSION).exists() {
                bail!(
                    "{branch}'s records would lie inside {}'s",
                    at.strip_prefix(&self.root).unwrap_or(&at).display()
                );
            }
        }
        Ok(())
    }

    /// Finish a write a crash left part done (steps 3 to 5).
    /// The branch's lock, taken: finish a write a crash left part done and
    /// mark the branch held by this process.
    fn locked<'a>(&'a self, branch: &str, dir: &Path, file: File) -> Result<Box<dyn Guard + 'a>> {
        self.finish_pending(dir)?;
        self.held.borrow_mut().insert(branch.to_string());
        Ok(Box::new(BranchGuard {
            files: self,
            branch: branch.to_string(),
            _lock: file,
        }))
    }

    fn finish_pending(&self, dir: &Path) -> Result<()> {
        let path = dir.join(PENDING);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let pending: Pending = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing {}", path.display()))?;
        apply(dir, &pending)?;
        std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        sync_dir(dir)
    }

    /// Keep `candidate`'s tasks on `branch`, or drop them with `None`,
    /// while `lock` holds the branch's lock through a live guard: these
    /// records' own lock, or, for what a host keeps of a remote's records,
    /// the host's one lock of the branch, which the records seam takes
    /// (rule core.14).
    pub(crate) fn put_progress_under(
        &self,
        lock: &LocalFiles,
        branch: &str,
        candidate: &str,
        tasks: Option<&[TaskProgress]>,
    ) -> Result<()> {
        // Beside the branch's records, as its kept logs are: no read of the
        // branch sees them, and they move no version (ADR 0028). Kept by a
        // round, under the branch's lock (rule core.14).
        if !lock.held.borrow().contains(branch) {
            bail!("keeping {branch}'s tasks without holding its lock");
        }
        self.dir(branch)?;
        let dir = self.root.join(PROGRESS).join(branch);
        let name = format!("{}.json", flat_name(candidate));
        match tasks {
            Some(tasks) => {
                let kept = Kept {
                    schema: PROGRESS_SCHEMA.into(),
                    candidate: candidate.into(),
                    tasks: tasks.to_vec(),
                };
                let mut bytes = serde_json::to_vec_pretty(&kept)?;
                bytes.push(b'\n');
                write_atomic(&dir, &name, &bytes)
            }
            None => match std::fs::remove_file(dir.join(&name)) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e).with_context(|| format!("removing {name}")),
            },
        }
    }
}

fn apply(dir: &Path, p: &Pending) -> Result<()> {
    for (name, text) in &p.put {
        write_atomic(dir, name, text.as_bytes())?;
    }
    for (name, text) in &p.append {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        // The truncate makes a repeated append after a crash write once.
        f.set_len(p.lengths.get(name).copied().unwrap_or(0))?;
        let mut f = f;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    // Last, so a crash before leaves the file both live and finished,
    // which the pending write's replay makes finished alone.
    for name in &p.remove {
        match std::fs::remove_file(dir.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("removing {name}")),
        }
    }
    write_atomic(dir, VERSION, format!("{}\n", p.version).as_bytes())
}

/// The name of `path` under the finished records, if it is one of them.
fn finished_name(path: &str) -> Option<&str> {
    path.strip_prefix(FINISHED)?.strip_prefix('/')
}

/// One file replaced all or nothing: a temporary file beside it, synced,
/// renamed over it, and the rename synced.
fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let path = dir.join(name);
    let parent = path.parent().unwrap_or(dir);
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let file = path.file_name().and_then(|n| n.to_str()).unwrap_or(name);
    let tmp = parent.join(format!(".{file}.tmp.{}", std::process::id()));
    let mut f = File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(&tmp, &path).with_context(|| format!("renaming into {}", path.display()))?;
    sync_dir(parent)
}

/// Make a rename durable. Some network and FUSE filesystems refuse to
/// sync a directory; that is "not supported", not a failed write.
fn sync_dir(dir: &Path) -> Result<()> {
    match File::open(dir).and_then(|d| d.sync_all()) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(22) => Ok(()), // EINVAL
        Err(e) => Err(e).with_context(|| format!("syncing {}", dir.display())),
    }
}

fn open_lock(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    // Never deleted: a delete races a concurrent open, and two steps could
    // each lock a different file.
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))
}

/// The files of a branch's records, by path relative to its directory,
/// leaving out the version, the lock, a pending write, temporary files and
/// any nested branch's records.
fn walk(dir: &Path, base: &Path, out: &mut BTreeMap<String, Vec<u8>>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("listing {}", dir.display())),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if entry.file_type()?.is_dir() {
            if !path.join(VERSION).exists() {
                walk(&path, base, out)?;
            }
            continue;
        }
        if dir == base && (name == VERSION || name == LOCK) {
            continue;
        }
        let rel = path
            .strip_prefix(base)
            .map_err(|e| anyhow!("{e}"))?
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        out.insert(
            rel,
            std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?,
        );
    }
    Ok(())
}

fn read_version(dir: &Path) -> Result<u64> {
    match std::fs::read_to_string(dir.join(VERSION)) {
        Ok(s) => s
            .trim()
            .parse()
            .with_context(|| format!("{}/{VERSION} holds {s:?}", dir.display())),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(0),
        Err(e) => Err(e).with_context(|| format!("reading {}/{VERSION}", dir.display())),
    }
}

fn text(bytes: Vec<u8>, path: &str) -> Result<String> {
    String::from_utf8(bytes)
        .map_err(|_| anyhow!("{path}: records are JSON, and these bytes are not UTF-8"))
}

impl State for LocalFiles {
    fn lock<'a>(&'a self, branch: &str) -> Result<Option<Box<dyn Guard + 'a>>> {
        let dir = self.dir(branch)?;
        let file = open_lock(&dir.join(LOCK))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(e)) => {
                return Err(e).with_context(|| format!("locking {branch}"))
            }
        }
        Ok(Some(self.locked(branch, &dir, file)?))
    }

    fn wait_lock<'a>(&'a self, branch: &str) -> Result<Box<dyn Guard + 'a>> {
        let dir = self.dir(branch)?;
        let file = open_lock(&dir.join(LOCK))?;
        file.lock().with_context(|| format!("locking {branch}"))?;
        self.locked(branch, &dir, file)
    }

    /// 129: read whole only once the version moved since the branch was
    /// last read, or a write is pending. Every write moves the version
    /// only once its files are in place, and a pending write stands from
    /// before its first file changes until after the version moved, so a
    /// version that stands with no write pending is records that stand.
    fn read(&self, branch: &str) -> Result<Snapshot> {
        let dir = self.dir(branch)?;
        let at = read_version(&dir)?;
        let pending_at = dir.join(PENDING).exists();
        if !pending_at {
            if let Some(kept) = self.kept.borrow().get(branch).filter(|k| k.version == at) {
                return Ok(kept.clone());
            }
        }
        let mut files = BTreeMap::new();
        walk(&dir, &dir, &mut files)?;
        let mut version = read_version(&dir)?;
        let mut pending = false;
        // With no lock held, a write a crash left part done is read as
        // whole: what is read is what sluice last wrote (51).
        if let Ok(bytes) = std::fs::read(dir.join(PENDING)) {
            pending = true;
            let p: Pending = serde_json::from_slice(&bytes).context("parsing a pending write")?;
            for (name, t) in &p.put {
                if finished_name(name).is_none() {
                    files.insert(name.clone(), t.clone().into_bytes());
                }
            }
            for (name, t) in &p.append {
                let f = files.entry(name.clone()).or_default();
                f.truncate(p.lengths.get(name).copied().unwrap_or(0) as usize);
                f.extend_from_slice(t.as_bytes());
            }
            for name in &p.remove {
                files.remove(name);
            }
            version = p.version;
        }
        let read = Snapshot { version, files };
        // Kept only when no write was pending, or finished, while it was
        // walked: nothing kept is part of a write.
        if !pending_at && !pending && version == at {
            self.kept
                .borrow_mut()
                .insert(branch.to_string(), read.clone());
        }
        Ok(read)
    }

    /// The host's own files: nothing to ask.
    fn refresh(&self) -> Result<()> {
        Ok(())
    }

    /// The host's own files are the records: as [`LocalFiles::read`].
    fn read_held(&self, branch: &str) -> Result<Snapshot> {
        self.read(branch)
    }

    fn read_finished(&self, branch: &str, name: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.read_all_finished(branch)?.remove(name))
    }

    fn read_all_finished(&self, branch: &str) -> Result<BTreeMap<String, Vec<u8>>> {
        let dir = self.dir(branch)?;
        let here = dir.join(FINISHED);
        let mut files = BTreeMap::new();
        match std::fs::read_dir(&here) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry?;
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') || !entry.file_type()?.is_file() {
                        continue;
                    }
                    let path = entry.path();
                    files.insert(
                        name,
                        std::fs::read(&path)
                            .with_context(|| format!("reading {}", path.display()))?,
                    );
                }
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("listing {}", here.display())),
        }
        // As `read`: a write a crash left part done is read as whole (51).
        if let Ok(bytes) = std::fs::read(dir.join(PENDING)) {
            let p: Pending = serde_json::from_slice(&bytes).context("parsing a pending write")?;
            for (name, t) in &p.put {
                if let Some(n) = finished_name(name) {
                    files.insert(n.to_string(), t.clone().into_bytes());
                }
            }
        }
        Ok(files)
    }

    fn write(&self, branch: &str, read_at: u64, changes: Changes) -> Result<Written> {
        if !self.held.borrow().contains(branch) {
            bail!("writing {branch}'s records without holding its lock");
        }
        let dir = self.dir(branch)?;
        self.check_nesting(branch)?;
        if read_version(&dir)? != read_at {
            return Ok(Written::Refused);
        }
        let mut lengths = BTreeMap::new();
        for name in changes.append.keys() {
            let len = match std::fs::metadata(dir.join(name)) {
                Ok(m) => m.len(),
                Err(e) if e.kind() == ErrorKind::NotFound => 0,
                Err(e) => return Err(e).with_context(|| format!("reading {name}")),
            };
            lengths.insert(name.clone(), len);
        }
        let pending = Pending {
            version: read_at + 1,
            put: changes
                .put
                .into_iter()
                .map(|(p, b)| Ok((p.clone(), text(b, &p)?)))
                .collect::<Result<_>>()?,
            append: changes
                .append
                .into_iter()
                .map(|(p, b)| Ok((p.clone(), text(b, &p)?)))
                .collect::<Result<_>>()?,
            lengths,
            remove: changes.remove,
        };
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        write_atomic(&dir, PENDING, &serde_json::to_vec(&pending)?)?;
        self.finish_pending(&dir)?;
        Ok(Written::Written(read_at + 1))
    }

    fn branches(&self) -> Result<Vec<String>> {
        fn visit(dir: &Path, root: &Path, out: &mut Vec<String>) -> Result<()> {
            let entries = match std::fs::read_dir(dir) {
                Ok(e) => e,
                Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
                Err(e) => return Err(e).with_context(|| format!("listing {}", dir.display())),
            };
            for entry in entries {
                let entry = entry?;
                if !entry.file_type()?.is_dir()
                    || entry.file_name().to_string_lossy().starts_with('.')
                {
                    continue;
                }
                let path = entry.path();
                if path.join(VERSION).exists() {
                    let rel = path.strip_prefix(root).map_err(|e| anyhow!("{e}"))?;
                    out.push(
                        rel.to_string_lossy()
                            .replace(std::path::MAIN_SEPARATOR, "/"),
                    );
                } else {
                    visit(&path, root, out)?;
                }
            }
            Ok(())
        }
        let mut out = Vec::new();
        visit(&self.root, &self.root, &mut out)?;
        out.sort();
        Ok(out)
    }

    /// The host's count, one for every branch whose records are its own
    /// (record 0114).
    fn allocate_id(&self, _branch: &str) -> Result<u64> {
        let lock = open_lock(&self.root.join(".lock"))?;
        lock.lock().context("locking the repository's records")?;
        let path = self.root.join(".next-id");
        let id = match std::fs::read_to_string(&path) {
            Ok(s) => s
                .trim()
                .parse()
                .with_context(|| format!("{} holds {s:?}", path.display()))?,
            Err(e) if e.kind() == ErrorKind::NotFound => 1,
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        write_atomic(&self.root, ".next-id", format!("{}\n", id + 1).as_bytes())?;
        Ok(id)
    }

    /// The results this host recorded that `ask` names, whichever branch
    /// reads them: none is signed, so no signer's key is read.
    fn results_for(&self, _: &str, ask: &ResultAsk) -> Result<Vec<u8>> {
        Ok(ask.answer(&self.results_log()?))
    }

    fn keep_logs(&self, branch: &str, candidate: &str, logs: &[TaskLog]) -> Result<Vec<TaskLog>> {
        // Beside the branch's records, never among them: a step reads every
        // record of its branch, and a log is read only by whoever asks.
        self.dir(branch)?;
        let dir = self.root.join(".logs").join(branch);
        let here = dir.join(flat_name(candidate));
        std::fs::create_dir_all(&here).with_context(|| format!("creating {}", here.display()))?;
        let mut kept = Vec::new();
        for l in logs {
            let name = format!("{}.log", flat_name(&l.task));
            write_atomic(&here, &name, l.text.as_bytes())?;
            kept.push(TaskLog {
                task: l.task.clone(),
                text: here.join(&name).display().to_string(),
            });
        }
        // Only the last KEPT_LOGS candidates keep theirs.
        let mut all = Vec::new();
        for e in std::fs::read_dir(&dir)? {
            let e = e?;
            if e.file_type()?.is_dir() {
                all.push((e.metadata()?.modified()?, e.path()));
            }
        }
        all.sort_by_key(|a| std::cmp::Reverse(a.0));
        for (_, old) in all.into_iter().skip(KEPT_LOGS) {
            if old != here {
                std::fs::remove_dir_all(old)?;
            }
        }
        Ok(kept)
    }

    fn refusal(&self, branch: &str) -> Result<Option<Vec<u8>>> {
        self.read_kept(REFUSED, branch)
    }

    fn keep_refusal(&self, branch: &str, doc: Option<&[u8]>) -> Result<()> {
        self.keep_kept(REFUSED, branch, doc)
    }

    fn intake_refusal(&self, branch: &str) -> Result<Option<Vec<u8>>> {
        self.read_kept(INTAKE_REFUSED, branch)
    }

    fn keep_intake_refusal(&self, branch: &str, doc: Option<&[u8]>) -> Result<()> {
        self.keep_kept(INTAKE_REFUSED, branch, doc)
    }

    fn progress(&self, branch: &str) -> Result<BTreeMap<String, Vec<TaskProgress>>> {
        self.dir(branch)?;
        let dir = self.root.join(PROGRESS).join(branch);
        let mut out = BTreeMap::new();
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
        };
        for e in entries {
            let path = e?.path();
            let named = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if named.starts_with('.') || !named.ends_with(".json") {
                continue;
            }
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(e) if e.kind() == ErrorKind::NotFound => continue,
                Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
            };
            let kept: Kept = serde_json::from_slice(&bytes)
                .with_context(|| format!("reading {}", path.display()))?;
            out.insert(kept.candidate, kept.tasks);
        }
        Ok(out)
    }

    fn put_progress(
        &self,
        branch: &str,
        candidate: &str,
        tasks: Option<&[TaskProgress]>,
    ) -> Result<()> {
        self.put_progress_under(self, branch, candidate, tasks)
    }

    fn record_results(&self, lines: &[u8]) -> Result<()> {
        let lock = open_lock(&self.root.join(".lock"))?;
        lock.lock().context("locking the repository's records")?;
        let mut have = self.results_log()?;
        // A line a crash cut short is no record: it is dropped before
        // anything is appended after it.
        if let Some(end) = have
            .iter()
            .rposition(|b| *b == b'\n')
            .map(|i| i + 1)
            .or(Some(0))
        {
            if end < have.len() {
                let f = OpenOptions::new()
                    .write(true)
                    .open(self.root.join(RESULTS))?;
                f.set_len(end as u64)?;
                have.truncate(end);
            }
        }
        let present: BTreeSet<&[u8]> = have.split(|b| *b == b'\n').collect();
        let mut add = Vec::new();
        for line in lines.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
            // Never appended twice: a step that crashed after recording
            // records the same run again.
            if !present.contains(line) {
                add.extend_from_slice(line);
                add.push(b'\n');
            }
        }
        if add.is_empty() {
            return Ok(());
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join(RESULTS))
            .context("opening the results log")?;
        f.write_all(&add)?;
        f.sync_all()?;
        Ok(())
    }

    fn set_asides(&self) -> Result<Vec<u8>> {
        match std::fs::read(self.root.join(SET_ASIDE)) {
            Ok(b) => Ok(b),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e).context("reading the results set aside"),
        }
    }

    fn set_aside(&self, line: &[u8]) -> Result<()> {
        let lock = open_lock(&self.root.join(".lock"))?;
        lock.lock().context("locking the repository's records")?;
        // A line a crash cut short is no record, as in the results log.
        let have = self.set_asides()?;
        if !have.is_empty() && !have.ends_with(b"\n") {
            let end = have.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
            OpenOptions::new()
                .write(true)
                .open(self.root.join(SET_ASIDE))?
                .set_len(end as u64)?;
        }
        let mut line = line.to_vec();
        if !line.ends_with(b"\n") {
            line.push(b'\n');
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join(SET_ASIDE))
            .context("opening the results set aside")?;
        f.write_all(&line)?;
        f.sync_all()?;
        Ok(())
    }
}
