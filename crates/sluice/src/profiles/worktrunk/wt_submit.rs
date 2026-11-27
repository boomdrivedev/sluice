//! `wt-submit`: `wt submit <branch> [args]` is `sluice submit <branch>
//! [args]`, with the same output and exit; once a waiting submit exits as
//! landed and sluice's record says its submission landed, the session's
//! worktree is released with `sluice release` and then removed as `wt
//! merge` removes it (C.4; session-runs-1 clause 95), unless `--no-remove`
//! is given or worktrunk's `[merge] remove` is false. A landing whose
//! worktree it removes is confirmed on receipt, since its reader goes with
//! the worktree; any other outcome it reports stays owed (51; record 0102,
//! design D5).
//!
//! A submit from one worktree first reads what stands: the worktree's last
//! submission to the branch, at the revision sluice says a submit from it
//! would send now (124), undecided or landed, or its pull request open and
//! ready there, or landed there. A pull request sent back, failed or in
//! conflict, stands nowhere, on whichever host takes the branch's pull
//! requests in, as a failed submission does (137; record 0096). Where one
//! stands it submits nothing: a waiting run follows
//! it with `sluice wait <branch>`, whose output and exit are its own, and
//! removes the worktree on the same terms once it lands; a `--no-wait` run
//! answers with `sluice submit <branch> --status`; a run naming `--after`
//! or `--priority` is refused, since only a new head submits again. Where
//! none stands it submits; under `--timeout`, without waiting, and then
//! follows the new submission under that limit (C.5 worktrunk; record
//! 0065). Where what stands cannot be read, it submits nothing.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Result;
use sluice::profiles::worktrunk::flow::{self, Tools};
use sluice::profiles::worktrunk::{
    release, remove, removes_on_landing, standing_in, Standing, Submit,
};

fn main() {
    let submit = Submit::parse(std::env::args_os().skip(1));
    std::process::exit(flow::run(&submit, &Processes));
}

/// [`Tools`] as processes: `sluice` and `wt` on the path, and this
/// process's own standard output and error.
struct Processes;

impl Tools for Processes {
    fn sluice(&self, args: &[OsString]) -> Result<i32> {
        let status = Command::new("sluice").args(args).status()?;
        Ok(status.code().unwrap_or(1))
    }

    fn sluice_output(&self, args: &[OsString]) -> Result<(i32, Vec<u8>)> {
        let out = Command::new("sluice")
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .output()?;
        Ok((out.status.code().unwrap_or(1), out.stdout))
    }

    fn standing(&self, branch: &str, worktree: &Path) -> std::result::Result<Standing, String> {
        standing_in(branch, worktree)
    }

    fn removes_on_landing(&self, worktree: &Path) -> bool {
        removes_on_landing(worktree)
    }

    fn release(&self, worktree: &Path) -> Result<()> {
        release(worktree)
    }

    fn remove(&self, worktree: &Path) -> Result<()> {
        remove(worktree)
    }

    fn say(&self, line: &str) {
        eprintln!("{line}");
    }

    fn print(&self, bytes: &[u8]) {
        let _ = std::io::stdout().write_all(bytes);
    }

    fn print_err(&self, bytes: &[u8]) {
        let _ = std::io::stderr().write_all(bytes);
    }
}
