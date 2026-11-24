//! `sluice host` judges the transaction that branch protection's hook
//! sends, and writes each refused write (the change
//! `the-git-hook-asks-sluice-host`, design D1;
//! `reads-go-through-the-resident@69134e5825d7`,
//! `residents-do-the-network-and-git-work@a2d68cf95ffe`).
//!
//! The connection's task answers it before any read or change, from the
//! state that the host keeps: one call on the pool for blocking work binds
//! the repository git names for guarding, judges each line by the branch's
//! ref on the host and its policy, and writes each refusal under the
//! branch's lock. It waits for no round and no reading, so a round that
//! holds the repository holds no commit.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use sluice_core::{Sluice, SystemClock};

use super::protocol::Output;
use super::Hosting;
use crate::exec::Host as _;

/// The folder git names: `folder`, where git ran the hook, joined with
/// its `GIT_DIR` where it set one.
pub fn named(folder: &Path, git_dir: Option<&Path>) -> PathBuf {
    match git_dir {
        Some(d) if !d.as_os_str().is_empty() => folder.join(d),
        _ => folder.to_path_buf(),
    }
}

/// Whether any line of the transaction `lines` updates a branch: one that
/// updates none is let go with nothing bound.
fn names_a_branch(lines: &str) -> bool {
    lines
        .lines()
        .filter_map(|l| l.split(' ').nth(2))
        .any(|name| name.starts_with("refs/heads/"))
}

/// Answer the hook's `reference-transaction` (design D1): its refusal's
/// words on standard error, and the exit, 0 or 1.
pub(crate) async fn answer(
    hosting: &Arc<Hosting>,
    folder: PathBuf,
    git_dir: Option<PathBuf>,
    by: String,
    lines: String,
) -> Output {
    let dir = named(&folder, git_dir.as_deref());
    let began = std::time::Instant::now();
    let output = match names_a_branch(&lines) {
        false => Output::default(),
        true => {
            let held = hosting.clone();
            let judging = dir.clone();
            let said = by.clone();
            tokio::task::spawn_blocking(move || judge(&held, &judging, &said, &lines))
                .await
                .unwrap_or_else(|e| Output {
                    stdout: String::new(),
                    stderr: vec![crate::protect::unread(&anyhow::anyhow!(
                        "the judgment ended: {e}"
                    ))],
                    code: 0,
                })
        }
    };
    super::log(&format!(
        "sluice host: {}: reference transaction of `{by}`: {} in {} ms",
        dir.display(),
        match output.code {
            0 => "let go",
            _ => "refused",
        },
        began.elapsed().as_millis()
    ));
    output
}

/// The call on the pool for blocking work: bind `dir` for guarding as the
/// host holds it, judge `lines`, write each refusal, and give what it said
/// and its exit. A repository it cannot bind is let go, said once, as a
/// transaction it cannot read is (124).
fn judge(hosting: &Hosting, dir: &Path, by: &str, lines: &str) -> Output {
    let (code, stderr) = crate::telemetry::capturing(|| {
        let program = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("sluice"));
        let bound = match crate::profiles::Bound::guarding(
            dir,
            program,
            Rc::new(crate::retry::Policy::command().recording_where_set()),
            &hosting.tokens,
        ) {
            Ok(bound) => bound,
            Err(e) => {
                crate::telemetry::say(&crate::protect::unread(&e));
                return 0;
            }
        };
        let s = Sluice::new(bound.seams(), &SystemClock)
            .as_peer(bound.peer())
            .guarding();
        crate::protect::judged(&s, &bound, lines, by)
    });
    Output {
        stdout: String::new(),
        stderr,
        code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// design D1: git names the folder it ran in, or `GIT_DIR` there.
    #[test]
    fn the_folder_is_the_one_git_names() {
        assert_eq!(named(Path::new("/w/x"), None), PathBuf::from("/w/x"));
        assert_eq!(
            named(Path::new("/w/x"), Some(Path::new(".git"))),
            PathBuf::from("/w/x/.git")
        );
        assert_eq!(
            named(Path::new("/w/x"), Some(Path::new("/r.git"))),
            PathBuf::from("/r.git")
        );
        assert_eq!(
            named(Path::new("/w/x"), Some(Path::new(""))),
            PathBuf::from("/w/x")
        );
    }

    /// A transaction that updates no branch binds nothing.
    #[test]
    fn a_transaction_of_no_branch_is_let_go() {
        let zero = "0".repeat(40);
        assert!(!names_a_branch(&format!("{zero} a refs/tags/v1\n")));
        assert!(!names_a_branch(&format!("{zero} a HEAD\n")));
        assert!(names_a_branch(&format!("{zero} a refs/heads/main\n")));
    }
}
