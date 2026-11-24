//! The repositories the host process holds, found again when it starts
//! (135; design D4; "Index, on any sluice use", the user's choice): a JSON
//! list of common git directories in `host/repositories`, written whole
//! under `host/start.lock`. A command of sluice's that finds its
//! repository has sluice, records or a policy at a revision the host
//! holds, notes it where it is not listed yet; one listed costs nothing to
//! note. The host process drops a listed repository whose directory is
//! gone, or which has no sluice, read as an idle tick reads it (129).
//! The host process's index is of the folder it holds (design D6;
//! [`super::folder`]), a command's of the folder its path names when it
//! asks.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context, Result};

use super::folder::Folder;
use crate::exec::Host;
use crate::profiles::Bound;

const NAME: &str = "repositories";

/// The index under the host process's folder `dir`.
#[derive(Clone, Debug)]
pub struct Index {
    dir: PathBuf,
    /// The host process's own folder, wherever it is now; a command's
    /// index has none and opens the folder at `dir` for each call.
    held: Option<Arc<Folder>>,
}

impl Index {
    /// A command's: the folder at `dir` as each call finds it.
    pub fn at(dir: &Path) -> Index {
        Index {
            dir: dir.to_path_buf(),
            held: None,
        }
    }

    /// The host process's: the folder it holds, written there alone.
    pub fn held(folder: Arc<Folder>) -> Index {
        Index {
            dir: folder.path().to_path_buf(),
            held: Some(folder),
        }
    }

    fn path(&self) -> PathBuf {
        self.dir.join(NAME)
    }

    /// The folder its calls read and write. The folder is never made
    /// here: a command's [`super::Paths`] makes it, and a host process
    /// whose folder is gone writes nothing there again (135; design D6).
    fn folder(&self) -> std::io::Result<Arc<Folder>> {
        match &self.held {
            Some(f) => Ok(f.clone()),
            None => Folder::open(&self.dir).map(Arc::new),
        }
    }

    /// Every repository listed, in order; none where nothing was noted.
    pub fn list(&self) -> Result<Vec<PathBuf>> {
        match self.folder() {
            Ok(folder) => self.list_in(&folder),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e).with_context(|| format!("reading {}", self.dir.display())),
        }
    }

    fn list_in(&self, folder: &Folder) -> Result<Vec<PathBuf>> {
        match folder.read(NAME) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("reading {}", self.path().display())),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e).with_context(|| format!("reading {}", self.path().display())),
        }
    }

    /// List `repository`, a common git directory, where it is not listed:
    /// whether it was written.
    pub fn note(&self, repository: &Path) -> Result<bool> {
        let folder = self
            .folder()
            .with_context(|| format!("opening {}", self.dir.display()))?;
        if self.list_in(&folder)?.iter().any(|r| r == repository) {
            return Ok(false);
        }
        let _lock = folder.lock("start.lock")?;
        let mut listed = self.list_in(&folder)?;
        if listed.iter().any(|r| r == repository) {
            return Ok(false);
        }
        listed.push(repository.to_path_buf());
        folder.write(NAME, &serde_json::to_vec_pretty(&listed)?)?;
        Ok(true)
    }

    /// Take each repository `drops` says goes out of the index: those it
    /// took out.
    pub fn prune(&self, drops: impl Fn(&Path) -> bool) -> Result<Vec<PathBuf>> {
        let folder = match self.folder() {
            Ok(f) => f,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", self.dir.display())),
        };
        let gone: Vec<PathBuf> = self
            .list_in(&folder)?
            .into_iter()
            .filter(|r| drops(r))
            .collect();
        if gone.is_empty() {
            return Ok(gone);
        }
        let _lock = folder.lock("start.lock")?;
        let listed: Vec<PathBuf> = self
            .list_in(&folder)?
            .into_iter()
            .filter(|r| !gone.contains(r))
            .collect();
        folder.write(NAME, &serde_json::to_vec_pretty(&listed)?)?;
        Ok(gone)
    }
}

/// Whether the repository at the common git directory `repository` still
/// has sluice: its directory is there, and its records list a branch, or a
/// revision the host holds, its own branches' or a remote's tracking
/// branches' as last fetched, has the policy (129). Nothing is fetched and
/// no remote asked: a remote's branches are listed as last fetched (129 as
/// amended 2026-11-19).
pub fn has_sluice(repository: &Path) -> Result<bool> {
    if !repository.is_dir() {
        return Ok(false);
    }
    let bound = Bound::open_with(
        repository,
        std::env::current_exe()?,
        crate::instance::Instance::from_file(None, 1)?,
        Rc::new(crate::retry::Policy::command().recording_where_set()),
    )?;
    sluice_seams::State::hold(&bound.state);
    if !sluice_seams::State::branches(&bound.state)?.is_empty() {
        return Ok(true);
    }
    let tips = bound.tips()?;
    bound.holds_policy(&tips)
}

/// Whether the repository at the common git directory `repository` has
/// work in play, read as an idle tick reads it (129, 135; design D4): where
/// it `asks`, each remote that keeps branches' records asked once, so work
/// another host wrote there is found, and otherwise each read as the last
/// ask fetched it (129 as amended 2026-11-19); then its branches' records
/// and its previews, with no other process started. Records in the host's
/// own files cost no process at all, and a remote that refuses leaves its
/// branches read as last fetched. A repository whose directory is gone has
/// none. The ask is a round's, which passes a spent allowance by rather
/// than waiting it out (rule core.29).
pub fn in_play(
    repository: &Path,
    instance: crate::instance::Instance,
    tokens: &crate::profiles::Tokens,
    asks: bool,
) -> Result<super::tasks::Looked> {
    if !repository.is_dir() {
        return Ok(super::tasks::Looked::default());
    }
    let bound = Bound::open_holding(
        repository,
        std::env::current_exe()?,
        instance,
        Rc::new(crate::retry::Policy::round().recording_where_set()),
        tokens,
    )?;
    // A refusal leaves the branches as last fetched, as a pass's ask does,
    // and is the reading's words (D16). A look that asks nothing reads
    // every remote as the last ask fetched it, and lists no branch by
    // asking it.
    let refused = match asks {
        true => sluice_seams::State::refresh(&bound.state)
            .err()
            .map(|e| format!("{e:#}")),
        false => {
            sluice_seams::State::hold(&bound.state);
            None
        }
    };
    let s = sluice_core::Sluice::new(bound.seams(), &sluice_core::SystemClock);
    // 137 (design D16, D25): the reading reads each pull request this host
    // follows into a branch it does not keep, as each falls due at the
    // host's one bound, so a status and the overview show it as fresh as
    // that bound allows. A read that fails is warned of, and the request
    // shown as last read.
    if let Ok(branches) = crate::PullRequests::branches(&*bound.pulls, bound.seams()) {
        for branch in branches {
            let _ = crate::PullRequests::across(
                &*bound.pulls,
                bound.seams(),
                &sluice_core::SystemClock,
                &branch,
            );
        }
    }
    // 152, record 0124 as amended 2026-11-23 (the change
    // `sluice-host-makes-each-change`, design D3): a held landing is pushed
    // again at the look that asks the remote, once a minute.
    if asks && super::pass::held(&s) {
        let s = sluice_core::Sluice::new(bound.seams(), &sluice_core::SystemClock)
            .as_peer(crate::exec::Host::peer(&bound));
        super::pass::push_held(&s);
    }
    Ok(super::tasks::Looked {
        in_play: super::pass::in_play(&s),
        held: super::pass::held(&s),
        refused,
        stale: bound.state.stale_all(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A scratch folder, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!(
                "sluice-index-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
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

    impl Scratch {
        /// The index in its `host/`, made as a command's `Paths` makes it.
        fn host(&self) -> Index {
            std::fs::create_dir_all(self.0.join("host")).unwrap();
            Index::at(&self.0.join("host"))
        }
    }

    /// A repository with one commit under `dir`, holding `files`: its
    /// common git directory.
    fn repository(dir: &Path, files: &[(&str, &str)]) -> PathBuf {
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .current_dir(dir)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_NAME", "test")
                .env("GIT_AUTHOR_EMAIL", "test@localhost")
                .env("GIT_COMMITTER_NAME", "test")
                .env("GIT_COMMITTER_EMAIL", "test@localhost")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
        };
        std::fs::create_dir_all(dir).unwrap();
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("README"), "scratch\n").unwrap();
        for (path, content) in files {
            let p = dir.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "root"]);
        std::fs::canonicalize(dir.join(".git")).unwrap()
    }

    /// 135 (design D4): a repository is listed once, and noting one
    /// listed writes nothing.
    #[test]
    fn a_repository_is_listed_once() {
        let s = Scratch::new("once");
        let index = s.host();
        assert!(index.list().unwrap().is_empty());
        let (a, b) = (PathBuf::from("/x/a.git"), PathBuf::from("/x/b.git"));
        assert!(index.note(&a).unwrap());
        let written = std::fs::metadata(index.path()).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(!index.note(&a).unwrap());
        assert_eq!(
            std::fs::metadata(index.path()).unwrap().modified().unwrap(),
            written,
            "nothing written for one listed"
        );
        assert!(index.note(&b).unwrap());
        assert_eq!(index.list().unwrap(), [a, b]);
    }

    /// 135 (design D4): a listed repository whose directory is gone
    /// leaves the index.
    #[test]
    fn a_gone_repository_leaves_the_index() {
        let s = Scratch::new("gone");
        let index = s.host();
        let kept = repository(&s.0.join("kept"), &[(".config/sluice.toml", "")]);
        let gone = repository(&s.0.join("gone"), &[(".config/sluice.toml", "")]);
        index.note(&kept).unwrap();
        index.note(&gone).unwrap();
        std::fs::remove_dir_all(s.0.join("gone")).unwrap();
        let out = index.prune(|r| !has_sluice(r).unwrap_or(false)).unwrap();
        assert_eq!(out, [gone]);
        assert_eq!(index.list().unwrap(), [kept]);
    }

    /// 135, 129 (design D4): a listed repository with no records and no
    /// policy at any revision the host holds leaves the index.
    #[test]
    fn a_repository_with_no_sluice_leaves_the_index() {
        let s = Scratch::new("none");
        let index = s.host();
        let set_up = repository(&s.0.join("set-up"), &[(".config/sluice.toml", "")]);
        let none = repository(&s.0.join("none"), &[]);
        assert!(has_sluice(&set_up).unwrap());
        assert!(!has_sluice(&none).unwrap());
        index.note(&set_up).unwrap();
        index.note(&none).unwrap();
        let out = index.prune(|r| !has_sluice(r).unwrap_or(false)).unwrap();
        assert_eq!(out, [none]);
        assert_eq!(index.list().unwrap(), [set_up]);
    }

    /// 135 (design D6): an index whose folder is gone is written nowhere,
    /// and the folder is not made again.
    #[test]
    fn a_gone_folder_is_not_made_again() {
        let s = Scratch::new("folder-gone");
        let index = s.host();
        index.note(Path::new("/x/a.git")).unwrap();
        std::fs::remove_dir_all(s.0.join("host")).unwrap();
        assert!(index.note(Path::new("/x/b.git")).is_err());
        assert!(index.prune(|_| true).is_ok_and(|gone| gone.is_empty()));
        assert!(!s.0.join("host").exists());
    }

    /// 135 (design D6): the host process's index, of the folder it holds,
    /// notes into that folder once it is moved and another is made at its
    /// path, and the other is written nothing: an answer that notes a
    /// repository as the folder goes (bolt bbc6a183's verify, where
    /// `start.lock` and `repositories` were written into the new folder).
    #[test]
    fn a_held_index_writes_nothing_into_a_folder_made_again() {
        let s = Scratch::new("held");
        s.host();
        let (dir, aside) = (s.0.join("host"), s.0.join("aside"));
        let index = Index::held(Arc::new(Folder::open(&dir).unwrap()));
        index.note(Path::new("/x/a.git")).unwrap();
        std::fs::rename(&dir, &aside).unwrap();
        std::fs::create_dir(&dir).unwrap();
        assert!(index.note(Path::new("/x/b.git")).unwrap());
        assert_eq!(index.prune(|r| r.ends_with("a.git")).unwrap().len(), 1);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        assert_eq!(Index::at(&aside).list().unwrap(), [Path::new("/x/b.git")]);
    }
}
