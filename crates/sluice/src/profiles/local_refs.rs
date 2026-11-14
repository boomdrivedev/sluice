//! The branch seam's `local refs` profile (C.4): one host's repository.
//! gix reads refs and objects, writes commits and moves a branch from its
//! expected revision; the git command forms candidates and answers
//! ancestry, so a conflict is exactly what `git merge` would report (ADR
//! 0009). `docs/plans/phase-1-integrations.md` section 1 is the reference.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{anyhow, bail, Context, Result};
use gix::refs::file::transaction::prepare::ReferenceOutOfDate;
use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};
use gix::refs::Target;
use gix::ObjectId;
use sluice_seams::{Branch, Formed, HostCopy, LandingCommit, Moved, Rev};

/// Who sluice's commits are by: fixed, never read from git config.
const NAME: &str = "sluice";
const EMAIL: &str = "sluice@localhost";

pub struct LocalRefs {
    repo: gix::Repository,
    git_dir: PathBuf,
    /// The repository as its configuration was when `<common>/config` had
    /// the length and modification time kept with it: gix reads the
    /// configuration once, when it opens a repository, so a remote added
    /// since is seen by opening it again (129).
    config: RefCell<(Option<Stamp>, gix::Repository)>,
}

/// A file's length and modification time, `None` when it can't be read.
type Stamp = (u64, std::time::SystemTime);

fn stamp(path: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

impl LocalRefs {
    /// The repository holding `path`, a worktree or a git directory.
    pub fn open(path: &Path) -> Result<Self> {
        let repo = gix::open(path)
            .map_err(|e| anyhow!("opening the repository at {}: {e}", path.display()))?;
        let git_dir = repo.git_dir().to_path_buf();
        // Taken after the opening, so a configuration that moved between
        // the two is read again at the next asking.
        let at = stamp(&repo.common_dir().join("config"));
        let config = RefCell::new((at, repo.clone()));
        Ok(LocalRefs {
            repo,
            git_dir,
            config,
        })
    }

    /// The repository with its configuration as `<common>/config` holds it
    /// now, opened again only when that file's length or modification time
    /// moved since it was last read (129).
    fn configured(&self) -> Result<std::cell::Ref<'_, gix::Repository>> {
        let now = stamp(&self.repo.common_dir().join("config"));
        if self.config.borrow().0 != now {
            let repo = gix::open(&self.git_dir).map_err(|e| {
                anyhow!("opening the repository at {}: {e}", self.git_dir.display())
            })?;
            *self.config.borrow_mut() = (now, repo);
        }
        Ok(std::cell::Ref::map(self.config.borrow(), |(_, r)| r))
    }

    /// The names of the repository's remotes, as `git remote` lists them,
    /// read with no process (129).
    pub fn remotes(&self) -> Result<Vec<String>> {
        Ok(self
            .configured()?
            .remote_names()
            .into_iter()
            .map(|n| n.to_string())
            .collect())
    }

    /// The fetch address of the remote `remote` as configured, before any
    /// `insteadOf`: the forge it names is the one an identity is for. Read
    /// with no process (129).
    pub fn remote_url(&self, remote: &str) -> Result<String> {
        let repo = self.configured()?;
        let url = repo
            .try_find_remote_without_url_rewrite(remote)
            .transpose()
            .map_err(|e| anyhow!("reading the remote {remote:?}: {e}"))?
            .and_then(|r| r.url(gix::remote::Direction::Fetch).map(|u| u.to_bstring()));
        match url {
            Some(url) => Ok(url.to_string()),
            None => bail!("the remote {remote:?} has no url"),
        }
    }

    /// The repository's default branch: the branch its common directory's
    /// HEAD names, a bare repository's or the main checkout's, as `git
    /// symbolic-ref --short -q HEAD` names it there. `None` when HEAD names
    /// no branch. Read with no process (129).
    pub fn default_branch(&self) -> Result<Option<String>> {
        let head = self
            .repo
            .try_find_reference("main-worktree/HEAD")
            .map_err(|e| anyhow!("reading the repository's HEAD: {e}"))?;
        Ok(head.and_then(|h| match h.target() {
            gix::refs::TargetRef::Symbolic(name) => Some(name.shorten().to_string()),
            gix::refs::TargetRef::Object(_) => None,
        }))
    }

    /// The repository's branches, `refs/heads/*`, by name, read with no
    /// process (129).
    pub fn branch_names(&self) -> Result<Vec<String>> {
        let refs = self
            .repo
            .references()
            .map_err(|e| anyhow!("reading the branches: {e}"))?;
        let mut names = Vec::new();
        for r in refs
            .local_branches()
            .map_err(|e| anyhow!("reading the branches: {e}"))?
        {
            let r = r.map_err(|e| anyhow!("reading the branches: {e}"))?;
            if let Some(name) = r.name().as_bstr().to_string().strip_prefix("refs/heads/") {
                names.push(name.to_string());
            }
        }
        Ok(names)
    }

    /// The tips of every branch the host holds, its own and each remote's
    /// tracking branch as last fetched, each once, read with no process
    /// (129).
    pub fn tips(&self) -> Result<Vec<Rev>> {
        let refs = self
            .repo
            .references()
            .map_err(|e| anyhow!("reading the branches: {e}"))?;
        let mut tips: Vec<Rev> = Vec::new();
        let listed = refs
            .local_branches()
            .and_then(|l| Ok(l.chain(refs.remote_branches()?)))
            .map_err(|e| anyhow!("reading the branches: {e}"))?;
        for r in listed {
            let mut r = r.map_err(|e| anyhow!("reading the branches: {e}"))?;
            // A ref that names nothing, as git's listing passes it by.
            let Ok(id) = r.peel_to_id() else { continue };
            let tip = Rev::new(id.to_string());
            if !tips.contains(&tip) {
                tips.push(tip);
            }
        }
        Ok(tips)
    }

    /// The repository's common git directory, the same from every
    /// worktree, canonicalized (section 1, *Common dir and opening*).
    pub fn common_dir(&self) -> Result<PathBuf> {
        std::fs::canonicalize(self.repo.common_dir())
            .context("canonicalizing the common git directory")
    }

    /// The git directory of the worktree it was opened from.
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    /// The revision a full ref name points at, or `None` when it does not
    /// exist.
    pub fn read_ref(&self, name: &str) -> Result<Option<Rev>> {
        let found = self
            .repo
            .try_find_reference(name)
            .map_err(|e| anyhow!("reading {name}: {e}"))?;
        let Some(mut r) = found else { return Ok(None) };
        let id = r.peel_to_id().map_err(|e| anyhow!("peeling {name}: {e}"))?;
        Ok(Some(Rev::new(id.to_string())))
    }

    /// Every ref under `prefix`, such as `refs/sluice-fetched/origin/state/`,
    /// by the rest of its name, with the revision it names; read with no
    /// process (129).
    pub fn refs_under(&self, prefix: &str) -> Result<Vec<(String, Rev)>> {
        let refs = self
            .repo
            .references()
            .map_err(|e| anyhow!("reading {prefix}: {e}"))?;
        let mut out = Vec::new();
        for r in refs
            .prefixed(prefix)
            .map_err(|e| anyhow!("reading {prefix}: {e}"))?
        {
            let mut r = r.map_err(|e| anyhow!("reading {prefix}: {e}"))?;
            let name = r.name().as_bstr().to_string();
            let Some(rest) = name.strip_prefix(prefix) else {
                continue;
            };
            let id = r.peel_to_id().map_err(|e| anyhow!("peeling {name}: {e}"))?;
            out.push((rest.to_string(), Rev::new(id.to_string())));
        }
        Ok(out)
    }

    /// Every file of `commit`'s tree, by path, with its blob and its
    /// bytes, read with no process (129): what `git ls-tree -r` lists and
    /// `git cat-file --batch` reads of it.
    pub fn tree_files(&self, commit: &str) -> Result<BTreeMap<String, (String, Vec<u8>)>> {
        let id = ObjectId::from_hex(commit.as_bytes())
            .map_err(|e| anyhow!("{commit} is not an object id: {e}"))?;
        let tree = self
            .repo
            .find_object(id)
            .map_err(|e| anyhow!("reading {commit}: {e}"))?
            .peel_to_tree()
            .map_err(|e| anyhow!("{commit} has no tree: {e}"))?;
        let mut files = BTreeMap::new();
        self.files_of(&tree, "", &mut files)?;
        Ok(files)
    }

    /// Every file of a commit's tree, by path, with its blob's id, no
    /// blob read: what an index that reads only blobs it has not seen
    /// lists (design D17).
    pub fn tree_entries(&self, commit: &str) -> Result<Vec<(String, String)>> {
        let id = ObjectId::from_hex(commit.as_bytes())
            .map_err(|e| anyhow!("{commit} is not an object id: {e}"))?;
        let tree = self
            .repo
            .find_object(id)
            .map_err(|e| anyhow!("reading {commit}: {e}"))?
            .peel_to_tree()
            .map_err(|e| anyhow!("{commit} has no tree: {e}"))?;
        let mut entries = Vec::new();
        self.entries_of(&tree, "", &mut entries)?;
        Ok(entries)
    }

    fn entries_of(
        &self,
        tree: &gix::Tree<'_>,
        under: &str,
        entries: &mut Vec<(String, String)>,
    ) -> Result<()> {
        let decoded = tree
            .decode()
            .map_err(|e| anyhow!("reading the tree {}: {e}", tree.id))?;
        for e in &decoded.entries {
            let path = format!("{under}{}", e.filename);
            if e.mode.is_tree() {
                let object = self
                    .repo
                    .find_object(e.oid)
                    .map_err(|err| anyhow!("reading {path}: {err}"))?;
                self.entries_of(&object.into_tree(), &format!("{path}/"), entries)?;
            } else if e.mode.is_blob_or_symlink() {
                entries.push((path, e.oid.to_string()));
            }
        }
        Ok(())
    }

    /// The blob `oid`'s bytes, read with no process.
    pub fn blob(&self, oid: &str) -> Result<Vec<u8>> {
        let id = ObjectId::from_hex(oid.as_bytes())
            .map_err(|e| anyhow!("{oid} is not an object id: {e}"))?;
        Ok(self
            .repo
            .find_object(id)
            .map_err(|e| anyhow!("reading {oid}: {e}"))?
            .data
            .clone())
    }

    fn files_of(
        &self,
        tree: &gix::Tree<'_>,
        under: &str,
        files: &mut BTreeMap<String, (String, Vec<u8>)>,
    ) -> Result<()> {
        let decoded = tree
            .decode()
            .map_err(|e| anyhow!("reading the tree {}: {e}", tree.id))?;
        for e in &decoded.entries {
            let path = format!("{under}{}", e.filename);
            let object = self
                .repo
                .find_object(e.oid)
                .map_err(|err| anyhow!("reading {path}: {err}"))?;
            if e.mode.is_tree() {
                self.files_of(&object.into_tree(), &format!("{path}/"), files)?;
            } else if e.mode.is_blob_or_symlink() {
                files.insert(path, (e.oid.to_string(), object.data.clone()));
            }
        }
        Ok(())
    }

    /// Set `branch`, the host's copy of a branch kept on a remote, to `to`
    /// from exactly `expected`, or from its absence when `expected` is
    /// `None` (124); a copy found elsewhere is reported, never forced (rule
    /// core.5).
    pub fn level_copy(&self, branch: &str, expected: Option<&Rev>, to: &Rev) -> Result<Moved> {
        let previous = match expected {
            Some(rev) => PreviousValue::MustExistAndMatch(Target::Object(self.id(rev)?)),
            None => PreviousValue::MustNotExist,
        };
        let message = format!("sluice: level {branch} to its landing");
        match self.edit_as(branch, previous, to, &message) {
            Ok(Moved::Moved) => Ok(Moved::Moved),
            // A copy found elsewhere is reported with what it holds, however
            // the refusal came; one still at `expected` failed for a reason
            // of its own, git's lock among them.
            other => match self.read(branch)? {
                found if found.as_ref() != expected => Ok(Moved::Elsewhere(found)),
                _ => other,
            },
        }
    }

    /// Move `branch` to `to` from `expected`; a branch found elsewhere is
    /// reported, never retried blind (24, rule core.5).
    fn edit(&self, branch: &str, expected: PreviousValue, to: &Rev) -> Result<Moved> {
        self.edit_as(branch, expected, to, &format!("sluice: land on {branch}"))
    }

    /// As [`LocalRefs::edit`], with `message` in the reflog.
    fn edit_as(
        &self,
        branch: &str,
        expected: PreviousValue,
        to: &Rev,
        message: &str,
    ) -> Result<Moved> {
        let new = self.id(to)?;
        let change = Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: false,
                message: message.into(),
            },
            expected,
            new: Target::Object(new),
        };
        self.transact(&full_name(branch), change)
    }

    /// Set the ref `name` to `to`, whatever it holds: a copy of a remote's
    /// ref sluice just pushed, so the next read finds it with no fetch.
    pub fn write_ref(&self, name: &str, to: &Rev) -> Result<()> {
        let change = Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: false,
                message: "sluice: pushed".into(),
            },
            expected: PreviousValue::Any,
            new: Target::Object(self.id(to)?),
        };
        match self.transact(name, change)? {
            Moved::Moved => Ok(()),
            Moved::Elsewhere(_) => bail!("{name} could not be written"),
        }
    }

    /// Remove the ref `name`, whatever it holds: a pushed branch's local
    /// copies once the remote's is gone. Removing one that is absent does
    /// nothing.
    pub fn delete_ref(&self, name: &str) -> Result<()> {
        if self.read_ref(name)?.is_none() {
            return Ok(());
        }
        let change = Change::Delete {
            expected: PreviousValue::Any,
            log: RefLog::AndReference,
        };
        match self.transact(name, change)? {
            Moved::Moved => Ok(()),
            Moved::Elsewhere(_) => bail!("{name} could not be removed"),
        }
    }

    /// One ref edit, as sluice; a ref found other than `change` expects is
    /// reported, never retried blind.
    fn transact(&self, name: &str, change: Change) -> Result<Moved> {
        let edit = RefEdit {
            change,
            name: name
                .to_string()
                .try_into()
                .map_err(|e| anyhow!("{name} is not a ref name: {e}"))?,
            deref: false,
        };
        let branch = name;
        let who = Self::signature(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .context("the clock is before 1970")?
                .as_secs() as i64,
        );
        let mut buf = gix::date::parse::TimeBuf::default();
        match self
            .repo
            .edit_references_as([edit], Some(who.to_ref(&mut buf)))
        {
            Ok(_) => Ok(Moved::Moved),
            Err(e) => {
                if let Some(out) = e.downcast_any_ref::<ReferenceOutOfDate>() {
                    let actual = match &out.actual {
                        Target::Object(id) => Some(Rev::new(id.to_string())),
                        Target::Symbolic(_) => None,
                    };
                    Ok(Moved::Elsewhere(actual))
                } else if e.classify().is_not_found() {
                    Ok(Moved::Elsewhere(None))
                } else if e
                    .downcast_any_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::AlreadyExists)
                {
                    Err(anyhow!("git holds the lock on {name}: {e}"))
                } else {
                    Err(anyhow!("moving {branch}: {e}"))
                }
            }
        }
    }

    fn id(&self, rev: &Rev) -> Result<ObjectId> {
        let id = ObjectId::from_hex(rev.as_str().as_bytes())
            .map_err(|e| anyhow!("{rev} is not an object id: {e}"))?;
        self.repo
            .find_object(id)
            .map_err(|e| anyhow!("no object {rev}: {e}"))?;
        Ok(id)
    }

    fn signature(seconds: i64) -> gix::actor::Signature {
        gix::actor::Signature {
            name: NAME.into(),
            email: EMAIL.into(),
            time: gix::date::Time { seconds, offset: 0 },
        }
    }

    fn write_commit(
        &self,
        tree: ObjectId,
        parents: [ObjectId; 2],
        message: &str,
        seconds: i64,
    ) -> Result<Rev> {
        let who = Self::signature(seconds);
        let commit = gix::objs::Commit {
            tree,
            parents: parents.into_iter().collect(),
            author: who.clone(),
            committer: who,
            encoding: None,
            message: message.into(),
            extra_headers: vec![],
        };
        let id = self
            .repo
            .write_object(&commit)
            .map_err(|e| anyhow!("writing a commit: {e}"))?
            .detach();
        Ok(Rev::new(id.to_string()))
    }

    /// The git command, for what gix has no equivalent of. Its identity and
    /// configuration are pinned, nothing it inherits from a hook or a
    /// session points it elsewhere, and it reads nothing from stdin.
    fn git(&self, args: &[&str]) -> Result<Output> {
        Command::new("git")
            .arg("--git-dir")
            .arg(&self.git_dir)
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", NAME)
            .env("GIT_AUTHOR_EMAIL", EMAIL)
            .env("GIT_COMMITTER_NAME", NAME)
            .env("GIT_COMMITTER_EMAIL", EMAIL)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("running git {}", args.join(" ")))
    }
}

fn full_name(branch: &str) -> String {
    format!("refs/heads/{branch}")
}

fn is_object_id(field: &[u8]) -> bool {
    matches!(field.len(), 40 | 64) && field.iter().all(u8::is_ascii_hexdigit)
}

impl Branch for LocalRefs {
    fn read(&self, branch: &str) -> Result<Option<Rev>> {
        self.read_ref(&full_name(branch))
    }

    /// The host's ref is the branch here: as [`LocalRefs::read`] reads it.
    fn held(&self, branch: &str) -> Result<Option<Rev>> {
        self.read(branch)
    }

    /// 64: an object lookup; a revision that names no object id is none
    /// the host holds.
    fn holds(&self, rev: &Rev) -> Result<bool> {
        Ok(ObjectId::from_hex(rev.as_str().as_bytes()).is_ok_and(|id| self.repo.has_object(id)))
    }

    /// The host's ref is the branch here: it has no copy apart from it.
    fn copy(&self, _branch: &str) -> Result<HostCopy> {
        Ok(HostCopy::Itself)
    }

    fn level(&self, branch: &str, _expected: Option<&Rev>, _to: &Rev) -> Result<Moved> {
        bail!("{branch} is kept in this host's own refs, which have no copy of it to level")
    }

    fn read_file(&self, rev: &Rev, path: &str) -> Result<Option<Vec<u8>>> {
        let id = self.id(rev)?;
        let tree = self
            .repo
            .find_object(id)
            .map_err(|e| anyhow!("{e}"))?
            .peel_to_tree()
            .map_err(|e| anyhow!("{rev} has no tree: {e}"))?;
        let Some(entry) = tree
            .lookup_entry_by_path(path)
            .map_err(|e| anyhow!("looking up {path} in {rev}: {e}"))?
        else {
            return Ok(None);
        };
        let object = entry
            .object()
            .map_err(|e| anyhow!("reading {path} in {rev}: {e}"))?;
        Ok(Some(object.data.clone()))
    }

    fn form(&self, onto: &Rev, head: &Rev) -> Result<Formed> {
        let (base, top) = (self.id(onto)?, self.id(head)?);
        let out = self.git(&[
            "merge-tree",
            "--write-tree",
            "-z",
            "--name-only",
            "--no-messages",
            &base.to_string(),
            &top.to_string(),
        ])?;
        let mut fields = out.stdout.split(|b| *b == 0);
        let first = fields.next().unwrap_or_default();
        let fail = || {
            anyhow!(
                "git merge-tree {onto} {head} exited {:?}: {}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim()
            )
        };
        if !is_object_id(first) {
            return Err(fail());
        }
        match out.status.code() {
            Some(0) => {
                let tree = ObjectId::from_hex(first).map_err(|e| anyhow!("{e}"))?;
                // A candidate's time is fixed, so one combination forms one
                // commit whenever it is formed.
                let rev = self.write_commit(tree, [base, top], "sluice candidate\n", 0)?;
                Ok(Formed::Revision(rev))
            }
            Some(1) => {
                let mut paths: Vec<String> = Vec::new();
                for f in fields.take_while(|f| !f.is_empty()) {
                    let p = String::from_utf8_lossy(f).into_owned();
                    if !paths.contains(&p) {
                        paths.push(p);
                    }
                }
                Ok(Formed::Conflict { paths })
            }
            _ => Err(fail()),
        }
    }

    fn commit(&self, commit: &LandingCommit) -> Result<Rev> {
        let tested = self.id(&commit.tree_of)?;
        let tree = self
            .repo
            .find_commit(tested)
            .map_err(|e| anyhow!("{} is not a commit: {e}", commit.tree_of))?
            .tree_id()
            .map_err(|e| anyhow!("{e}"))?
            .detach();
        let parents = [self.id(&commit.parent)?, self.id(&commit.head)?];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context("the clock is before 1970")?
            .as_secs() as i64;
        self.write_commit(tree, parents, &commit.message, now)
    }

    fn contains(&self, tip: &Rev, rev: &Rev) -> Result<bool> {
        let tip = self.id(tip)?;
        // 121: a commit this host does not have, as another host's landing
        // the remote has since lost, is in no revision read here.
        let rev = ObjectId::from_hex(rev.as_str().as_bytes())
            .map_err(|e| anyhow!("{rev} is not an object id: {e}"))?;
        let found = self
            .repo
            .try_find_object(rev)
            .map_err(|e| anyhow!("reading {rev}: {e}"))?;
        if !found.is_some_and(|o| o.kind == gix::object::Kind::Commit) {
            return Ok(false);
        }
        let out = self.git(&[
            "merge-base",
            "--is-ancestor",
            &rev.to_string(),
            &tip.to_string(),
        ])?;
        match out.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            code => bail!(
                "git merge-base --is-ancestor exited {code:?}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }
    }

    /// 122: the last of `git rev-list --first-parent --ancestry-path
    /// rev..tip` is the oldest commit of the line descended from `rev`: the
    /// commit that landed it, or, where its first parent is `rev`, the
    /// commit after `rev` on the line, which then landed itself.
    fn landed_by(&self, tip: &Rev, rev: &Rev) -> Result<Option<Rev>> {
        if !self.contains(tip, rev)? {
            return Ok(None);
        }
        let (top, of) = (self.id(tip)?, self.id(rev)?);
        let range = format!("{of}..{top}");
        let out = self.git(&["rev-list", "--first-parent", "--ancestry-path", &range])?;
        if !out.status.success() {
            bail!(
                "git rev-list {range} exited {:?}: {}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let Some(oldest) = text.lines().last() else {
            return Ok(Some(rev.clone()));
        };
        let parent = self.git(&["rev-parse", "--verify", "-q", &format!("{oldest}^1")])?;
        let parent = String::from_utf8_lossy(&parent.stdout);
        if ObjectId::from_hex(parent.trim().as_bytes()).is_ok_and(|p| p == of) {
            return Ok(Some(rev.clone()));
        }
        Ok(Some(Rev::new(oldest.to_string())))
    }

    fn move_to(&self, branch: &str, expected: &Rev, to: &Rev) -> Result<Moved> {
        let from = self.id(expected)?;
        self.edit(
            branch,
            PreviousValue::MustExistAndMatch(Target::Object(from)),
            to,
        )
    }

    fn make(&self, branch: &str, at: &Rev) -> Result<Moved> {
        match self.edit(branch, PreviousValue::MustNotExist, at) {
            Ok(Moved::Moved) => Ok(Moved::Moved),
            // A branch present is reported with what it holds, however the
            // refusal came.
            other => match self.read(branch)? {
                Some(found) => Ok(Moved::Elsewhere(Some(found))),
                None => other,
            },
        }
    }

    fn remove(&self, branch: &str, expected: &Rev) -> Result<Moved> {
        let from = self.id(expected)?;
        let change = Change::Delete {
            expected: PreviousValue::MustExistAndMatch(Target::Object(from)),
            log: RefLog::AndReference,
        };
        match self.transact(&full_name(branch), change) {
            Ok(Moved::Moved) => Ok(Moved::Moved),
            other => match self.read(branch)? {
                found if found.as_ref() != Some(expected) => Ok(Moved::Elsewhere(found)),
                _ => other,
            },
        }
    }
}
