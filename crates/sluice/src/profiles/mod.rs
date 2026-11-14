//! Binding each seam to a profile (52; ADR 0003). The only directory of
//! this crate that may name a tool (rule core.1).
//!
//! The profiles are `docs/plans/phase-1-integrations.md`'s: the branch is
//! bound to [`local_refs`], or [`pushed_refs`] where its policy names a
//! remote, through [`branches`]; the records to [`local_files`], the check to
//! [`checks`], the place to [`places`] and the driver to [`worktrunk`].

pub mod branches;
pub mod checks;
pub mod github;
pub mod local_files;
pub mod local_refs;
pub mod moon;
pub mod places;
pub mod pushed_refs;
pub mod records;
pub mod refusals;
pub mod remote_refs;
pub mod worktrunk;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;

use anyhow::Result;
use sluice_seams::{Branch, Rev};

use crate::identity::Identity;
use crate::instance::Instance;
use crate::retry;

/// The variables a multiplexer's pane names its session by, as the
/// driver profile reads them (51): what nothing started outside a pane
/// carries, sluice's host process among it (135; design D3).
pub fn session_variables() -> [&'static str; 2] {
    [worktrunk::herdr::PANE, worktrunk::herdr::SOCKET]
}

/// The profiles the walkthrough's setup names, for the repository holding
/// a directory.
pub struct Bound {
    pub checkout: places::Places,
    pub tests: checks::Checks,
    pub branch: branches::Branches,
    pub driver: Drivers,
    pub state: records::Records,
    /// The pull request driver, which also takes pull requests in.
    pub pulls: Rc<github::pulls::Pulls>,
    /// 135: sluice's host process, reached through its socket, or, in its
    /// own repositories' tasks, read in place.
    pub host_process: Box<dyn crate::host_process::HostProcess>,
}

/// The driver seam as an instance binds it: a pull request's place,
/// `pull/<n>`, to the pull request driver, and every other to worktrunk's
/// (C.5).
pub struct Drivers {
    worktrunk: worktrunk::Worktrunk,
    pulls: Rc<github::pulls::Pulls>,
}

impl sluice_seams::Driver for Drivers {
    fn session(&self, worktree: &str) -> Result<sluice_seams::Session> {
        match github::pulls::number(worktree) {
            Some(_) => self.pulls.session(worktree),
            None => self.worktrunk.session(worktree),
        }
    }

    /// 51: a folder is never a pull request's place, so worktrunk's reads
    /// who collects there (C.5; record 0099).
    fn recipient(&self, folder: &str) -> Result<Option<sluice_seams::Submitter>> {
        self.worktrunk.recipient(folder)
    }

    /// 124: a branch named is no pull request's place, so worktrunk's
    /// names its submitter (C.5).
    fn named(&self, branch: &str) -> Result<sluice_seams::Submitter> {
        self.worktrunk.named(branch)
    }

    fn notify(&self, n: &sluice_seams::Notification) -> Result<sluice_seams::Delivery> {
        match github::pulls::number(&n.to.worktree) {
            Some(_) => self.pulls.notify(n),
            None => self.worktrunk.notify(n),
        }
    }
}

impl Bound {
    /// Bind the repository holding `dir`, a worktree or a git directory,
    /// for this executable and this instance's settings, every call to a
    /// remote under the command's `retry` policy (134).
    pub fn open(dir: &Path, retry: Rc<retry::Policy>) -> Result<Self> {
        Bound::open_with(dir, std::env::current_exe()?, Instance::read()?, retry)
    }

    /// `program` is the `sluice` executable that supervises check runs.
    pub fn open_with(
        dir: &Path,
        program: PathBuf,
        instance: Instance,
        retry: Rc<retry::Policy>,
    ) -> Result<Self> {
        Bound::bind(dir, program, instance, false, retry)
    }

    /// 124: bind the repository holding `dir` for branch protection's hook:
    /// the host's settings and each policy read only as far as guarding
    /// needs, every other field passed over (ADR 0059), every call to a
    /// remote under the command's `retry` policy (134).
    pub fn guarding(dir: &Path, retry: Rc<retry::Policy>) -> Result<Self> {
        Bound::bind(
            dir,
            std::env::current_exe()?,
            Instance::guarding()?,
            true,
            retry,
        )
    }

    fn bind(
        dir: &Path,
        program: PathBuf,
        instance: Instance,
        guarding: bool,
        retry: Rc<retry::Policy>,
    ) -> Result<Self> {
        // C.4: the app the host names is sluice's identity on GitHub, and
        // C.5: what reads and posts pull requests there.
        let app = match &instance.forge {
            Some(f) => Some(Rc::new(github::App::open(f, retry.clone())?)),
            None => None,
        };
        let app_for_pool = app.clone();
        let identity = app
            .clone()
            .map(|a| (a as Rc<dyn Identity>, program.clone()));
        let forge = app.map(|a| a as Rc<dyn github::Forge>);
        let branch = branches::Branches::open_as(dir, identity.clone(), retry.clone())?;
        let common = branch.common_dir()?;
        // 136: each branch's open pull requests asked for at the host's
        // interval, what was read of them kept in the host's state.
        let discovery = instance
            .forge
            .as_ref()
            .map_or(crate::instance::DISCOVERY, |f| f.discovery);
        // 72, 137: this host as its peers name it, and its lease, for the
        // branches it keeps on their remotes, whose pull requests it alone
        // takes in (design D21).
        let peer = sluice_core::Peer {
            name: instance.peers.name.clone().unwrap_or_else(checks::hostname),
            lease: instance.peers.lease,
            alive: instance.peers.alive,
            signer: instance.signing.as_ref().map(|s| s.signer.clone()),
            systems: instance.systems.clone(),
            expire_after: instance.expire_after,
        };
        let pulls = Rc::new(github::pulls::Pulls::new(
            branch.git_dir(),
            &common,
            forge,
            identity.clone(),
            retry.clone(),
            github::kept::Kept::new(&crate::instance::state_dir()?),
            discovery,
            peer,
        ));
        // 51: the host a session's submit is recorded on, as its peers
        // name it.
        let host = instance.peers.name.clone().unwrap_or_else(checks::hostname);
        // C.6, record 0122: a branch's records are in the home its records
        // name, and, where none does, on its remote when its policy names
        // signers and this host's own files otherwise.
        let state = records::Records::new(
            &common,
            branches::Branches::open_as(dir, identity.clone(), retry.clone())?,
            instance.signing.clone(),
        )
        .as_host(&host);
        let state = if guarding { state.guarding() } else { state };
        let jobs = instance.jobs();
        let caches = instance.caches.clone();
        Ok(Bound {
            // A session's place's release command is the check seam's to
            // build, and the end of a preview's containers the check seam's
            // to make, at each engine a run's is looked for at; the binding
            // hands both over (rule core.2; record 0121).
            checkout: {
                let places =
                    places::Places::new(&common, crate::runs::Runs::new(&common, program.clone()))
                        .with_release(Box::new(move |place, tests, limits, dir| {
                            checks::release_request(place, tests, limits, jobs, &caches, dir)
                        }))
                        .with_containers(Box::new(|place| {
                            checks::end_containers(&checks::engine_endpoints(), place)
                        }));
                // 35 as amended: the tests' directory standing for the
                // user's, where they name one.
                match std::env::var_os(places::USER_SCRATCH_VAR).filter(|d| !d.is_empty()) {
                    Some(dir) => places.with_scratch_root(dir.into()),
                    None => places,
                }
            },
            tests: {
                let checks = checks::Checks::new(&common, program, instance);
                // 88: with sluice's app, GitHub Actions is the pool this
                // host can start runs on; the binding hands it over (core.2).
                match &app_for_pool {
                    Some(app) => {
                        checks.with_pool(Box::new(github::actions::Actions::new(app.clone(), dir)))
                    }
                    None => checks,
                }
            },
            branch,
            driver: Drivers {
                worktrunk: worktrunk::Worktrunk::new(host),
                pulls: pulls.clone(),
            },
            state,
            pulls,
            host_process: Box::new(crate::host_process::Socketed::default()),
        })
    }

    pub fn seams(&self) -> sluice_core::Seams<'_> {
        sluice_core::Seams {
            checkout: &self.checkout,
            tests: &self.tests,
            branch: &self.branch,
            driver: &self.driver,
            state: &self.state,
        }
    }
}

/// `sluice forge check [<branch>]`: whether sluice's identity on the
/// forge `branch`'s remote is on is set up so only sluice may move the
/// branch there (C.4 pushed refs). `branch` defaults to the one checked
/// out in `dir`. Says each thing checked, or with `json` one document of
/// them; exits 1 when any fails.
pub fn forge_check(
    dir: &Path,
    branch: Option<&str>,
    json: bool,
    out: &mut dyn Write,
    retry: Rc<retry::Policy>,
) -> Result<i32> {
    let branch = match branch {
        Some(b) => b.to_string(),
        None => checked_out(dir)?,
    };
    let found = forge_findings(dir, &branch, retry)?;
    let ok = found.iter().all(|f| f.ok);
    if json {
        let checks: Vec<serde_json::Value> = found
            .iter()
            .map(|f| serde_json::json!({ "ok": f.ok, "message": f.what }))
            .collect();
        writeln!(
            out,
            "{}",
            serde_json::json!({ "branch": branch, "ok": ok, "checks": checks })
        )?;
    } else {
        for f in &found {
            writeln!(
                out,
                "{}  {}",
                if f.ok { "ok    " } else { "failed" },
                f.what
            )?;
        }
    }
    Ok(if ok { 0 } else { 1 })
}

/// Each thing `sluice forge check` checks for `branch`, up to the first
/// that stops the rest.
fn forge_findings(
    dir: &Path,
    branch: &str,
    retry: Rc<retry::Policy>,
) -> Result<Vec<github::check::Finding>> {
    let mut found = Vec::new();
    let mut said = |ok: bool, what: String| found.push(github::check::Finding { ok, what });
    let instance = match Instance::read() {
        Ok(i) => i,
        Err(e) => {
            said(false, format!("{e:#}"));
            return Ok(found);
        }
    };
    let Some(forge) = &instance.forge else {
        said(
            false,
            "the host's config has no [forge] table: sluice pushes with this host's own credentials"
                .into(),
        );
        return Ok(found);
    };
    let branches = branches::Branches::open_as(dir, None, retry.clone())?;
    let remote = match branches.remote(branch)? {
        Some(r) => Some(r),
        // A bolt not yet made is checked before it is: its rules on the
        // forge are read by name, and its remote from main's policy.
        None => {
            let r = branches.remote_if_made_from(branch, "main")?;
            if r.is_some() {
                said(
                    true,
                    format!("{branch} is not made yet; main's policy puts it on its remote"),
                );
            }
            r
        }
    };
    let Some(remote) = remote else {
        said(false, format!("{branch}'s policy names no remote"));
        return Ok(found);
    };
    let url = pushed_refs::remote_url(dir, &remote)?;
    let Some(repo) = github::Repo::from_url(&url) else {
        said(
            false,
            format!("{branch}'s remote {remote} ({url}) is on no forge sluice has an identity on"),
        );
        return Ok(found);
    };
    let app = match github::App::open(forge, retry) {
        Ok(a) => a,
        Err(e) => {
            said(false, format!("{e:#}"));
            return Ok(found);
        }
    };
    // Under a credential command, the check's first finding, that the
    // command gives a token, does this work (record 0108).
    if let crate::instance::Source::Key(key) = &forge.source {
        said(true, format!("[forge] key {} reads", key.display()));
    }
    let operator = operator_token();
    found.extend(github::check::check(
        &app,
        &repo,
        branch,
        operator.as_deref(),
    ));
    Ok(found)
}

/// The repository's default branch, the branch its common directory's
/// HEAD names, a bare repository's or the main checkout's, with its
/// revision in the host's own refs, never fetched: one reading for the
/// overview's order (ADR 0036), the remotes that may hold records and the
/// branches that take pull requests. `None` when HEAD names no branch.
/// Read with no process (129).
pub(crate) fn default_branch(
    local: &local_refs::LocalRefs,
) -> Result<Option<(String, Option<Rev>)>> {
    let Some(branch) = local.default_branch()? else {
        return Ok(None);
    };
    let at = local.read_ref(&format!("refs/heads/{branch}"))?;
    Ok(Some((branch, at)))
}

/// Whether any of `tips` has the policy file (129; rule core.27): a
/// policy in history, in a tag or in a working copy alone is none a branch
/// holds. Read with no process, the first tip holding it ending the
/// reading.
fn holds_policy(local: &local_refs::LocalRefs, tips: &[Rev]) -> Result<bool> {
    for tip in tips {
        if local
            .read_file(tip, sluice_core::policy::POLICY_PATH)?
            .is_some()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the directory `dir` is gone, as a repository's git directory
/// removed or moved away is (129).
fn gone(dir: &Path) -> bool {
    !dir.is_dir()
}

/// The top of the git worktree holding `dir`, as git prints it; `None`
/// where git finds no worktree, as in a bare repository's git directory.
pub fn worktree_top(dir: &Path) -> Result<Option<String>> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    let top = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || top.is_empty() {
        return Ok(None);
    }
    Ok(Some(top))
}

/// The branch checked out in `dir`.
fn checked_out(dir: &Path) -> Result<String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["symbolic-ref", "--short", "-q", "HEAD"])
        .stdin(Stdio::null())
        .output()?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || name.is_empty() {
        anyhow::bail!("no branch is checked out here: name one");
    }
    Ok(name)
}

/// The operator's own GitHub token: `GH_TOKEN`, `GITHUB_TOKEN`, or the one
/// `gh` holds, asked without a prompt.
fn operator_token() -> Option<String> {
    for var in ["GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(t) = std::env::var(var) {
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    let out = Command::new("gh")
        .args(["auth", "token"])
        .env("GH_PROMPT_DISABLED", "1")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !t.is_empty()).then_some(t)
}

impl crate::exec::Host for Bound {
    fn host_process(&self) -> &dyn crate::host_process::HostProcess {
        &*self.host_process
    }

    fn reserved(&self) -> usize {
        self.tests.instance().reserved
    }

    fn peer(&self) -> sluice_core::Peer {
        let i = self.tests.instance();
        sluice_core::Peer {
            name: i.peers.name.clone().unwrap_or_else(checks::hostname),
            lease: i.peers.lease,
            alive: i.peers.alive,
            signer: i.signing.as_ref().map(|s| s.signer.clone()),
            systems: i.systems.clone(),
            expire_after: i.expire_after,
        }
    }

    fn worktree(&self, dir: &str) -> Result<String> {
        let path = std::fs::canonicalize(dir)?;
        path.to_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("{} is not UTF-8", path.display()))
    }

    fn working_copy(&self, dir: &str) -> Result<Option<String>> {
        match worktree_top(Path::new(dir))? {
            Some(top) => self.worktree(&top).map(Some),
            None => Ok(None),
        }
    }

    fn lock_sessions(&self) -> Result<Box<dyn std::any::Any>> {
        Ok(Box::new(self.tests.host_sessions()?.lock()?))
    }

    fn begin_session(
        &self,
        held: &dyn std::any::Any,
        run: &crate::sessions::SessionRun,
    ) -> Result<()> {
        let held = held
            .downcast_ref::<crate::sessions::SessionsLock>()
            .ok_or_else(|| anyhow::anyhow!("the host's session runs are not locked"))?;
        self.tests.host_sessions()?.begin(held, run)
    }

    fn end_session(&self, worktree: &str) -> Result<()> {
        self.tests.host_sessions()?.end(worktree)
    }

    fn session_tasks(&self, worktree: &str, tasks: &[sluice_seams::TaskProgress]) -> Result<()> {
        self.tests.host_sessions()?.keep_tasks(worktree, tasks)
    }

    fn run_dir(&self, run: &sluice_seams::RunHandle) -> Option<PathBuf> {
        Some(self.tests.runs().dir(&run.0))
    }

    fn end_run(&self, run: &sluice_seams::RunHandle) -> Result<()> {
        self.tests.runs().end(&run.0)
    }

    fn revision(&self, rev: &str) -> Result<Option<String>> {
        let out = Command::new("git")
            .args(["rev-parse", "--verify", "--quiet", "--end-of-options"])
            .arg(format!("{rev}^{{commit}}"))
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()?;
        let full = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok((out.status.success() && !full.is_empty()).then_some(full))
    }

    fn name(&self) -> String {
        checks::hostname()
    }

    fn repository(&self) -> Result<String> {
        Ok(self.branch.common_dir()?.to_string_lossy().into_owned())
    }

    fn repository_gone(&self, repository: &str) -> bool {
        gone(Path::new(repository))
    }

    fn tips(&self) -> Result<Vec<Rev>> {
        self.branch.local().tips()
    }

    fn holds_policy(&self, tips: &[Rev]) -> Result<bool> {
        holds_policy(self.branch.local(), tips)
    }

    fn readers(&self) -> Result<PathBuf> {
        Ok(crate::instance::state_dir()?.join("readers"))
    }

    fn next_pass(&self) {
        self.branch.next_pass();
        self.state.next_pass();
        self.pulls.next_pass();
    }

    fn default_branch(&self) -> Result<Option<sluice_core::overview::OrderFrom>> {
        // Its revision here, never fetched: the overview reads it only when
        // the branch has no records.
        Ok(default_branch(self.branch.local())?
            .map(|(branch, local)| sluice_core::overview::OrderFrom { branch, local }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 129: a git directory is there until it is removed.
    #[test]
    fn a_directory_is_gone_once_removed() {
        let dir = std::env::temp_dir().join(format!("sluice-gone-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!gone(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(gone(&dir));
    }
}
