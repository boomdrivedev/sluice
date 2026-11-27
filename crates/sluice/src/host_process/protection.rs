//! `sluice host` puts back branch protection's hook (the change
//! `the-git-hook-asks-sluice-host`, design D4;
//! `residents-do-the-network-and-git-work@a2d68cf95ffe`).
//!
//! Each time it binds a repository for a request, it puts back sluice's
//! hook where the hook in place stands for another sluice, by its own
//! executable's path; where a submit, a wait, a step or `sluice run` names
//! the repository, it installs the hook where none stands (1, 124). No
//! command puts back the hook in its own process. The hooks folder of a
//! request's folder is found once, by git, and kept for the host
//! process's life, so a request after the first runs no git for it, and
//! a request that comes while it is found waits for that finding.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::protocol::{FollowKind, Request};
use super::Hosting;

/// The hooks folder of each folder a request named, as git found it. A
/// request that names a folder while its finding goes waits for that
/// finding and starts no git of its own, so no git of a finding starts
/// after the requests that name its folder are answered (reader-tick, "A
/// folder's hooks folder is found once").
#[derive(Default)]
pub(crate) struct Hooks {
    folders: Mutex<BTreeMap<PathBuf, Arc<tokio::sync::Mutex<Option<PathBuf>>>>>,
}

/// What a request has `sluice host` do with the hook of its folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Put {
    /// Install it where none stands, or put it back (1).
    Install,
    /// Put it back where it stands for another sluice (124).
    Back,
}

/// The folders `request` names and what it has done with their hooks: a
/// submit, a wait, a step and `sluice run` install it; every other request
/// that names a folder puts it back. `sluice protect` does its own, the
/// hook's own request puts back nothing, a worktree's release writes
/// nothing (record 0120 as amended 2026-11-17), and a pass, which only a
/// follower whose subscription put the hook back asks, waits on nothing
/// for it.
fn asked(request: &Request) -> Vec<(PathBuf, Put)> {
    match request {
        Request::Submit { folder, .. }
        | Request::Wait { folder, .. }
        | Request::Step { folder, .. } => vec![(folder.clone(), Put::Install)],
        Request::Status { folder, .. }
        | Request::Overview { folder, .. }
        | Request::Home { folder, .. }
        | Request::Forecast { folder, .. }
        | Request::Inbox { folder, .. }
        | Request::Exec { folder, .. }
        | Request::Preview { folder, .. }
        | Request::Equip { folder, .. }
        | Request::Previews { folder, .. }
        | Request::Amend { folder, .. }
        | Request::Withdraw { folder, .. }
        | Request::Confirm { folder, .. }
        | Request::Release { folder, .. }
        | Request::Branch { folder, .. } => vec![(folder.clone(), Put::Back)],
        Request::Subscribe { folders } => folders
            .iter()
            .map(|added| {
                let put = match added.kind {
                    FollowKind::Run | FollowKind::Wait => Put::Install,
                    FollowKind::Watch => Put::Back,
                };
                (added.folder.clone(), put)
            })
            .collect(),
        Request::Protect { .. }
        | Request::ReferenceTransaction { .. }
        | Request::Pass { .. }
        | Request::ReleaseWorktree { .. }
        | Request::Serve { .. }
        | Request::Results { .. }
        | Request::HandOver { .. } => Vec::new(),
    }
}

/// The executable the hook names: `sluice host`'s own, resolved through
/// every link.
fn program(hosting: &Hosting) -> Option<PathBuf> {
    hosting
        .named
        .program
        .clone()
        .or_else(|| std::env::current_exe().and_then(std::fs::canonicalize).ok())
}

/// Put back, or install, the hook of each folder `request` names, before
/// the request is answered, so what the request says reads the hook as it
/// stands after. What fails is said in the host process's log; it holds
/// no request.
pub(crate) async fn put(hosting: &Hosting, request: &Request) {
    let asked = asked(request);
    if asked.is_empty() {
        return;
    }
    let Some(program) = program(hosting) else {
        super::log("sluice host: branch protection: its own executable could not be read");
        return;
    };
    for (folder, put) in asked {
        let hooks = hosting
            .hooks
            .found(&folder, |folder| crate::protect::hooks_of(&folder))
            .await;
        let hooks = match hooks {
            Ok(hooks) => hooks,
            // A folder in no repository is the request's to report, and a
            // tracked hooks folder holds no hook sluice wrote; it is found
            // again at the next request, since either may change.
            Err(e) => {
                if put == Put::Install {
                    super::log(&format!(
                        "sluice host: {}: branch protection not installed: {e:#}",
                        folder.display()
                    ));
                }
                continue;
            }
        };
        let (writing, naming) = (hooks.clone(), program.clone());
        let written = tokio::task::spawn_blocking(move || match put {
            Put::Install => crate::protect::install_at(&writing, &naming),
            Put::Back => crate::protect::reinstall_at(&writing, &naming),
        })
        .await
        .unwrap_or_else(|e| Err(anyhow::anyhow!("writing the hook: {e}")));
        match written {
            Ok(true) => super::log(&format!(
                "sluice host: {}: branch protection's hook names {}",
                hooks.display(),
                program.display()
            )),
            Ok(false) => {}
            Err(e) => super::log(&format!(
                "sluice host: {}: branch protection's hook not written: {e:#}",
                hooks.display()
            )),
        }
    }
}

impl Hooks {
    /// `folder`'s hooks folder: the one kept, or the one `find` finds,
    /// on tokio's pool for blocking work, kept where it is found. One
    /// finding of a folder goes at a time, and a request that comes while
    /// it goes takes what it found.
    async fn found<F>(&self, folder: &Path, find: F) -> anyhow::Result<PathBuf>
    where
        F: FnOnce(PathBuf) -> anyhow::Result<PathBuf> + Send + 'static,
    {
        let slot = self
            .folders
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(folder.to_path_buf())
            .or_default()
            .clone();
        let mut kept = slot.lock().await;
        if let Some(hooks) = kept.as_ref() {
            return Ok(hooks.clone());
        }
        let finding = folder.to_path_buf();
        let hooks = tokio::task::spawn_blocking(move || find(finding))
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("finding the hooks folder: {e}")))?;
        *kept = Some(hooks.clone());
        Ok(hooks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// reader-tick, "A folder's hooks folder is found once": two requests
    /// that name one folder at once find its hooks folder by one finding,
    /// the second waiting for the first's, so no git of the finding starts
    /// after either is answered.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn two_requests_for_one_folder_find_its_hooks_once() {
        let hooks = Hooks::default();
        let finds = Arc::new(AtomicUsize::new(0));
        let find = |finds: Arc<AtomicUsize>| {
            move |folder: PathBuf| {
                finds.fetch_add(1, Ordering::SeqCst);
                // The finding's two gits, slow under load.
                std::thread::sleep(std::time::Duration::from_millis(200));
                Ok(folder.join(".git/hooks"))
            }
        };
        let folder = Path::new("/w/x");
        let (a, b) = tokio::join!(
            hooks.found(folder, find(finds.clone())),
            hooks.found(folder, find(finds.clone())),
        );
        assert_eq!(a.unwrap(), Path::new("/w/x/.git/hooks"));
        assert_eq!(b.unwrap(), Path::new("/w/x/.git/hooks"));
        assert_eq!(finds.load(Ordering::SeqCst), 1, "found once");
        // A request after both finds nothing.
        hooks.found(folder, find(finds.clone())).await.unwrap();
        assert_eq!(finds.load(Ordering::SeqCst), 1, "kept");
    }

    /// A finding that fails is not kept: the next request finds it again,
    /// since the folder may become a repository, or its hooks folder an
    /// untracked one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_finding_that_fails_is_found_again() {
        let hooks = Hooks::default();
        let folder = Path::new("/w/x");
        let failed = hooks
            .found(folder, |_| Err(anyhow::anyhow!("not a git repository")))
            .await;
        assert!(failed.is_err());
        let found = hooks
            .found(folder, |f| Ok(f.join(".git/hooks")))
            .await
            .unwrap();
        assert_eq!(found, Path::new("/w/x/.git/hooks"));
    }

    /// design D4: a submit, a wait, a step and `sluice run` install the
    /// hook; a read puts it back; `sluice protect`, the hook's own request
    /// and a worktree's release do neither.
    #[test]
    fn what_each_request_does_with_the_hook() {
        let folder = PathBuf::from("/w/x");
        let step = Request::Step {
            folder: folder.clone(),
            branch: "main".into(),
        };
        assert_eq!(asked(&step), vec![(folder.clone(), Put::Install)]);
        let status = Request::Status {
            folder: folder.clone(),
            caller: Default::default(),
            target: None,
            all: false,
            json: false,
            branch: None,
        };
        assert_eq!(asked(&status), vec![(folder.clone(), Put::Back)]);
        let protect = Request::Protect {
            folder: folder.clone(),
            caller: Default::default(),
            remove: true,
            json: false,
        };
        assert!(asked(&protect).is_empty());
        let hook = Request::ReferenceTransaction {
            folder: folder.clone(),
            git_dir: None,
            by: "git commit".into(),
            lines: String::new(),
        };
        assert!(asked(&hook).is_empty());
        let run = Request::Subscribe {
            folders: vec![
                super::super::protocol::Added {
                    folder: folder.clone(),
                    kind: FollowKind::Run,
                },
                super::super::protocol::Added {
                    folder: "/w/y".into(),
                    kind: FollowKind::Watch,
                },
            ],
        };
        assert_eq!(
            asked(&run),
            vec![(folder, Put::Install), ("/w/y".into(), Put::Back)]
        );
    }
}
