//! The git the pull request driver reads and writes (C.5 pull request):
//! a remote's address, the default branch and the branches the host
//! holds, the messages of a head's commits, and on a remote, the head of
//! a pull request fetched into the host's own refs and a session's branch
//! pushed. [`GitRefs`] reads the repository with gix and reaches its
//! remotes as [`PushedRefs`] does (129, 134).

use std::cell::OnceCell;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::rc::Rc;

use anyhow::{bail, Context, Result};
use sluice_seams::Rev;

use crate::identity::Identity;
use crate::profiles::local_refs::LocalRefs;
use crate::profiles::pushed_refs::PushedRefs;
use crate::retry;
use crate::workers::Run as _;

/// The repository as the pull request driver reads and writes it.
pub trait RequestRefs {
    /// The address of `remote`.
    fn remote_url(&self, remote: &str) -> Result<String>;
    /// The branch the repository's `HEAD` names, `None` where it names
    /// none.
    fn default_branch(&self) -> Result<Option<String>>;
    /// The branches the host holds.
    fn branch_names(&self) -> Result<Vec<String>>;
    /// The commit messages of `tip..head`, oldest first.
    fn messages(&self, tip: &Rev, head: &Rev) -> Result<Vec<String>>;
    /// `remote` as the driver reaches it; refused where the repository has
    /// no remote of that name.
    fn remote(&self, remote: &str) -> Result<Box<dyn RemoteRefs + '_>>;
}

/// One remote as the pull request driver reaches it.
pub trait RemoteRefs {
    /// The remote's name.
    fn name(&self) -> &str;
    /// The host's own ref `name`, `None` where it has none.
    fn read_ref(&self, name: &str) -> Result<Option<Rev>>;
    /// The remote's ref `src` fetched into the host's ref `dst`, `None`
    /// where the remote has no such ref.
    fn fetch_ref(&self, src: &str, dst: &str) -> Result<Option<Rev>>;
    /// `rev` pushed to the remote's `branch`, whatever it held.
    fn push_branch(&self, rev: &Rev, branch: &str) -> Result<()>;
}

impl RemoteRefs for PushedRefs {
    fn name(&self) -> &str {
        self.remote()
    }

    fn read_ref(&self, name: &str) -> Result<Option<Rev>> {
        PushedRefs::read_ref(self, name)
    }

    fn fetch_ref(&self, src: &str, dst: &str) -> Result<Option<Rev>> {
        PushedRefs::fetch_ref(self, src, dst)
    }

    fn push_branch(&self, rev: &Rev, branch: &str) -> Result<()> {
        PushedRefs::push_branch(self, rev, branch)
    }
}

/// The repository whose git directory is `dir`, read with no process
/// (129) and reaching its remotes as `identity` where it has one, each
/// call under `retry` (134).
pub struct GitRefs {
    dir: PathBuf,
    identity: Option<(Rc<dyn Identity>, PathBuf)>,
    retry: Rc<retry::Policy>,
    /// The repository, as gix reads it, once opened.
    local: OnceCell<LocalRefs>,
}

impl GitRefs {
    pub fn new(
        dir: &Path,
        identity: Option<(Rc<dyn Identity>, PathBuf)>,
        retry: Rc<retry::Policy>,
    ) -> GitRefs {
        GitRefs {
            dir: dir.to_path_buf(),
            identity,
            retry,
            local: OnceCell::new(),
        }
    }

    /// The repository, opened at its first reading.
    fn local(&self) -> Result<&LocalRefs> {
        if let Some(l) = self.local.get() {
            return Ok(l);
        }
        let l = LocalRefs::open(&self.dir)?;
        Ok(self.local.get_or_init(|| l))
    }
}

impl RequestRefs for GitRefs {
    fn remote_url(&self, remote: &str) -> Result<String> {
        self.local()?.remote_url(remote)
    }

    fn default_branch(&self) -> Result<Option<String>> {
        self.local()?.default_branch()
    }

    fn branch_names(&self) -> Result<Vec<String>> {
        self.local()?.branch_names()
    }

    fn messages(&self, tip: &Rev, head: &Rev) -> Result<Vec<String>> {
        let out = crate::workers::command("git", "git")
            .arg("--git-dir")
            .arg(&self.dir)
            .args([
                "log",
                "--reverse",
                "--format=%B%x1e",
                &format!("{tip}..{head}"),
            ])
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null())
            .worker_output()
            .context("running git log")?;
        if !out.status.success() {
            bail!("git log: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .split('\u{1e}')
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .collect())
    }

    fn remote(&self, remote: &str) -> Result<Box<dyn RemoteRefs + '_>> {
        Ok(Box::new(PushedRefs::open_as(
            &self.dir,
            remote,
            self.identity.clone(),
            self.retry.clone(),
        )?))
    }
}
