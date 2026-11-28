//! The git a place is made and removed by (C.1's git worktree row; ADR
//! 0013): a repository's worktrees, as git keeps them. [`GitWorktrees`]
//! reads them from the repository's files with gix, with no process
//! started (129; rule core.31), where git's own list says enough, and
//! starts git to read its porcelain list and a checkout's status, and to
//! add, unlock, remove and prune a checkout, with nothing of a caller's
//! repository or hooks.

use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};

use anyhow::{anyhow, bail, Context as _, Result};
use sluice_seams::{CheckedOut, Rev};

use super::{parse_worktree_entries, Listed};
use crate::workers::Run as _;

/// A repository's worktrees as the places profile makes and removes them.
pub trait Worktrees {
    /// Records 0120 and 0115: whether git lists the folder `path` as one
    /// of the repository's worktrees, detached or not, matched by what it
    /// is on disk and never by how its path is spelled.
    fn listed(&self, path: &Path) -> Result<bool>;
    /// Record 0122 (design D3, D4): what git's list says of the folder
    /// `path`, the revision it holds and whether, and why, git keeps it
    /// locked; `None` where git lists no such folder.
    fn entry(&self, path: &Path) -> Result<Option<Listed>>;
    /// Every worktree with a branch checked out, as `git worktree list`
    /// lists them (ADR 0024).
    fn checked_out(&self) -> Result<Vec<CheckedOut>>;
    /// A checkout of exactly `rev` made at `path`, detached, with no hook
    /// run, since nothing of the candidate's may run outside the executor.
    fn add(&self, path: &Path, rev: &Rev) -> Result<()>;
    /// Whether the checkout at `path` holds nothing beyond its revision,
    /// as git made it: nothing changed, untracked or ignored in it.
    fn clean(&self, path: &Path) -> Result<bool>;
    /// The lock git keeps on the checkout at `path` taken off.
    fn unlock(&self, path: &Path) -> Result<()>;
    /// The checkout at `path` removed by force: `Err` of git's own words
    /// where it refused.
    fn remove(&self, path: &Path) -> Result<std::result::Result<(), String>>;
    /// What git keeps of checkouts whose folders are gone let go.
    fn prune(&self) -> Result<()>;
}

/// The worktrees of the repository whose common directory is `common`,
/// as git keeps them.
pub struct GitWorktrees {
    common: PathBuf,
    /// The repository, as gix reads it, once opened: its worktrees, read
    /// with no process (129).
    repo: std::cell::OnceCell<gix::Repository>,
}

impl GitWorktrees {
    pub fn new(common: &Path) -> GitWorktrees {
        GitWorktrees {
            common: common.to_path_buf(),
            repo: std::cell::OnceCell::new(),
        }
    }

    /// The repository at its common directory, opened at its first
    /// reading.
    fn repo(&self) -> Result<&gix::Repository> {
        if let Some(r) = self.repo.get() {
            return Ok(r);
        }
        let r = gix::open(&self.common)
            .map_err(|e| anyhow!("opening the repository at {}: {e}", self.common.display()))?;
        Ok(self.repo.get_or_init(|| r))
    }

    /// `git` in the common dir, with nothing of a caller's repository or
    /// hooks: a hook's `GIT_DIR` would point it elsewhere.
    fn git(&self, args: &[&str]) -> Result<Output> {
        crate::workers::command("git", "git")
            .arg("-C")
            .arg(&self.common)
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR")
            .stdin(Stdio::null())
            .worker_output()
            .context("running git")
    }

    /// git in the common dir, `what` naming the call in its failure.
    fn run(&self, what: &str, args: &[&str]) -> Result<()> {
        let out = self.git(args)?;
        if !out.status.success() {
            bail!("{what}: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(())
    }
}

/// `path` as git is given it.
fn text(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow!("{} is not UTF-8", path.display()))
}

impl Worktrees for GitWorktrees {
    /// Records 0120 and 0115: whether git lists the folder `path` as one
    /// of the repository's worktrees, detached or not: its main worktree
    /// where it has one, or the folder an entry under `<common>/worktrees/`
    /// names. Read from the repository's files as `git worktree list` reads
    /// them, with no process started and no object read (129; rule
    /// core.31), each folder matched by what it is on disk ([`Folder`]),
    /// never by how its path is spelled. An entry whose `gitdir` file
    /// cannot be read, or whose folder cannot be, names nothing; worktrees
    /// that cannot be listed and a `path` that cannot be read are an error,
    /// never read as unlisted.
    fn listed(&self, path: &Path) -> Result<bool> {
        let want = Folder::of(path).with_context(|| format!("reading {}", path.display()))?;
        let repo = self.repo()?;
        let is = |dir: &Path| Folder::of(dir).is_ok_and(|f| f == want);
        if !repo.is_bare() && repo.workdir().is_some_and(is) {
            return Ok(true);
        }
        let linked = repo
            .worktrees()
            .map_err(|e| anyhow!("listing the worktrees: {e}"))?;
        Ok(linked.into_iter().any(|w| w.base().is_ok_and(|b| is(&b))))
    }

    /// Record 0122 (design D3, D4): what `git worktree list --porcelain`
    /// says of the folder `path`, matched by what it is on disk
    /// ([`Folder`]) or, where it is gone, by its spelling: the revision it
    /// holds and whether, and why, git keeps it locked. `None` where git
    /// lists no such folder.
    fn entry(&self, path: &Path) -> Result<Option<Listed>> {
        let out = self.git(&["worktree", "list", "--porcelain", "-z"])?;
        if !out.status.success() {
            bail!(
                "git worktree list: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let want = Folder::of(path).ok();
        Ok(parse_worktree_entries(&out.stdout)
            .into_iter()
            .find(|(at, _)| match &want {
                Some(w) => Folder::of(at).is_ok_and(|f| f == *w),
                None => at == path,
            })
            .map(|(_, listed)| listed))
    }

    /// Read as `git worktree list` lists them with no process started
    /// (129): the main worktree where the repository is not bare, then
    /// each linked one whose directory is there or which is locked; one
    /// whose HEAD names no branch is left out, as
    /// [`super::parse_worktree_list`] leaves it.
    fn checked_out(&self) -> Result<Vec<CheckedOut>> {
        let repo = self.repo()?;
        let mut found = Vec::new();
        if !repo.is_bare() {
            if let Some(dir) = repo.workdir() {
                found.extend(on_branch(repo, "main-worktree/HEAD", dir.to_path_buf())?);
            }
        }
        let linked = repo
            .worktrees()
            .map_err(|e| anyhow!("listing the worktrees: {e}"))?;
        for w in linked {
            // Its directory gone, as `git worktree prune` would remove it.
            if w.is_prunable() {
                continue;
            }
            let (Ok(base), Ok(id)) = (w.base(), w.id()) else {
                continue;
            };
            found.extend(on_branch(repo, &format!("worktrees/{id}/HEAD"), base)?);
        }
        Ok(found
            .into_iter()
            .map(|mut c| {
                // As a session's place names its worktree (`session_name`).
                if let Ok(p) = std::fs::canonicalize(&c.worktree) {
                    c.worktree = p;
                }
                c
            })
            .collect())
    }

    fn add(&self, path: &Path, rev: &Rev) -> Result<()> {
        self.run(
            &format!("git worktree add {}", path.display()),
            &[
                "-c",
                "core.hooksPath=/dev/null",
                "worktree",
                "add",
                "--detach",
                text(path)?,
                rev.as_str(),
            ],
        )
    }

    fn clean(&self, path: &Path) -> Result<bool> {
        let status = self.git(&[
            "-C",
            text(path)?,
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--ignored",
        ])?;
        Ok(status.status.success() && status.stdout.is_empty())
    }

    fn unlock(&self, path: &Path) -> Result<()> {
        self.run(
            &format!("git worktree unlock {}", path.display()),
            &["worktree", "unlock", text(path)?],
        )
    }

    fn remove(&self, path: &Path) -> Result<std::result::Result<(), String>> {
        let out = self.git(&["worktree", "remove", "--force", text(path)?])?;
        Ok(match out.status.success() {
            true => Ok(()),
            false => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        })
    }

    fn prune(&self) -> Result<()> {
        self.run("git worktree prune", &["worktree", "prune"])
    }
}

/// A folder as it is on disk, whatever spells its path: the device and
/// inode of the folder, or, where it no longer exists, of its nearest
/// ancestor that does, with the rest of its path below that ancestor. So
/// `/var/…` and `/private/var/…`, or a path spelled through a link, are
/// one folder, and a folder deleted is still the one its entry names
/// (record 0115).
#[derive(Debug, PartialEq, Eq)]
struct Folder {
    dev: u64,
    ino: u64,
    rest: PathBuf,
}

impl Folder {
    fn of(path: &Path) -> std::io::Result<Folder> {
        use std::os::unix::fs::MetadataExt as _;
        let mut at = path;
        let mut rest: Vec<&std::ffi::OsStr> = Vec::new();
        loop {
            // The folder itself is not followed, as git never follows it; an
            // ancestor is, since the rest of the path is reached through it.
            let meta = if rest.is_empty() {
                std::fs::symlink_metadata(at)
            } else {
                std::fs::metadata(at)
            };
            match meta {
                Ok(m) => {
                    return Ok(Folder {
                        dev: m.dev(),
                        ino: m.ino(),
                        rest: rest.iter().rev().collect(),
                    })
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let (Some(parent), Some(name)) = (at.parent(), at.file_name()) else {
                        return Err(e);
                    };
                    rest.push(name);
                    at = if parent.as_os_str().is_empty() {
                        Path::new(".")
                    } else {
                        parent
                    };
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// The worktree at `worktree` whose HEAD is the ref `head`, with the
/// branch it names and that branch's revision, as `git worktree list`
/// reads them: `None` when HEAD is detached or names no branch, and the
/// null revision for a branch not yet made.
fn on_branch(repo: &gix::Repository, head: &str, worktree: PathBuf) -> Result<Option<CheckedOut>> {
    let found = repo
        .try_find_reference(head)
        .map_err(|e| anyhow!("reading {head}: {e}"))?;
    let Some(found) = found else {
        return Ok(None);
    };
    let gix::refs::TargetRef::Symbolic(name) = found.target() else {
        return Ok(None);
    };
    let full = name.as_bstr().to_string();
    let Some(branch) = full.strip_prefix("refs/heads/") else {
        return Ok(None);
    };
    let at = match repo
        .try_find_reference(name)
        .map_err(|e| anyhow!("reading {full}: {e}"))?
    {
        Some(mut r) => r
            .peel_to_id()
            .map_err(|e| anyhow!("peeling {full}: {e}"))?
            .to_string(),
        None => gix::ObjectId::null(repo.object_hash()).to_string(),
    };
    Ok(Some(CheckedOut {
        worktree,
        branch: branch.to_string(),
        head: Rev(at),
    }))
}
