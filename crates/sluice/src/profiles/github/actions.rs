//! GitHub Actions as a pool (88, 107, 111; C.2 launched; ADR 0034): a run
//! no standing host can take starts one job of the workflow the policy's
//! pool names, by `workflow_dispatch`, on the runner its system names; a
//! wake starts a job that advances a branch. A job is named by its claim
//! in the run's title, `sluice <claim>`, which the workflow sets, so
//! where it stands is read from the workflow's runs and their jobs
//! (record 0122; design D12).

use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use sluice_seams::{Job, Launch, Setting};

use super::app::{App, Bearer, Call, Repo};
use crate::profiles::checks::Launcher;
use crate::profiles::pushed_refs::remote_url;

/// Record 0119, 0122 (design D11): the name of a pool run's job that runs
/// the checks, holding no identity.
pub const RUN_JOB: &str = "run";

/// Record 0119, 0122 (design D11): the name of a pool run's job that signs
/// and records the run's end.
pub const SIGN_JOB: &str = "sign";

pub struct Actions {
    app: Rc<App>,
    dir: PathBuf,
}

impl Actions {
    /// The pool of the repository holding `dir`, reached as `app`.
    pub fn new(app: Rc<App>, dir: &Path) -> Actions {
        Actions {
            app,
            dir: dir.to_path_buf(),
        }
    }

    fn repo(&self, remote: Option<&str>) -> Result<Repo> {
        let remote = remote.unwrap_or("origin");
        let url = remote_url(&self.dir, remote)?;
        Repo::from_url(&url).ok_or_else(|| anyhow!("{remote} ({url}) is no repository on GitHub"))
    }

    /// The workflow a `github` table names, as the API names it: its file.
    fn workflow(kind: &(String, Setting)) -> Result<String> {
        let path = field(kind, "workflow")?;
        Ok(path.rsplit('/').next().unwrap_or(&path).to_string())
    }

    /// Start a run of `workflow` on `git_ref` (134; rule core.29): a call
    /// that starts something, which nothing reads back, so one whose
    /// answer was not read is never sent again and fails
    /// [`sluice_seams::Unanswered`] for its caller to judge.
    fn dispatch(&self, repo: &Repo, workflow: &str, git_ref: &str, inputs: Value) -> Result<()> {
        let path = format!(
            "/repos/{}/{}/actions/workflows/{workflow}/dispatches",
            repo.owner, repo.name
        );
        let body = json!({ "ref": git_ref, "inputs": inputs }).to_string();
        self.app
            .post_api(Call::Make, &path, Bearer::Repo(repo), &body)?;
        Ok(())
    }
}

/// A key of a `github = { ... }` table.
fn field(kind: &(String, Setting), key: &str) -> Result<String> {
    if kind.0 != "github" {
        bail!("a {} pool is not GitHub Actions", kind.0);
    }
    match &kind.1 {
        Setting::Tables(t) => match t.first().and_then(|t| t.get(key)) {
            Some(Setting::Text(v)) => Ok(v.clone()),
            _ => bail!("the github table names no {key}"),
        },
        _ => bail!("github must be a table naming a workflow"),
    }
}

/// The branch a ref names: `refs/heads/main` is `main`.
fn branch_of(git_ref: &str) -> &str {
    git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref)
}

impl Launcher for Actions {
    fn launch(&self, launch: &Launch) -> Result<()> {
        let repo = self.repo(launch.remote.as_deref())?;
        let workflow = Actions::workflow(&launch.kind)?;
        let git_ref = field(&launch.signer, "ref").unwrap_or_else(|_| "refs/heads/main".into());
        self.dispatch(
            &repo,
            &workflow,
            branch_of(&git_ref),
            json!({
                "mode": "run",
                "branch": launch.branch,
                "claim": launch.claim,
                "combination": launch.combination.as_str(),
                "system": launch.system.clone().unwrap_or_default(),
                "runner": launch.runner,
            }),
        )
    }

    /// 77, 88, record 0122 (design D12): the workflow's runs listed,
    /// completed ones among them, in one call conditional on what the host
    /// keeps (rule core.32), the newest bearing `sluice <claim>` in its
    /// title read: queued until its `run` job starts, running since that
    /// job's own start, and, completed, recorded where its `sign` job
    /// succeeded.
    fn job(&self, kind: &(String, Setting), claim: &str, remote: Option<&str>) -> Result<Job> {
        let repo = self.repo(remote)?;
        let workflow = Actions::workflow(kind)?;
        let title = format!("sluice {claim}");
        let path = format!(
            "/repos/{}/{}/actions/workflows/{workflow}/runs?per_page=100",
            repo.owner, repo.name
        );
        let v = self.app.get(&path, Bearer::Repo(&repo))?;
        let Some(run) = v["workflow_runs"].as_array().and_then(|runs| {
            runs.iter()
                .filter(|r| r["display_title"] == title.as_str())
                .max_by_key(|r| r["id"].as_u64().unwrap_or(0))
        }) else {
            return Ok(Job::None);
        };
        let status = run["status"].as_str().unwrap_or_default();
        if matches!(status, "queued" | "waiting" | "pending" | "requested") {
            return Ok(Job::Queued);
        }
        let id = run["id"]
            .as_u64()
            .ok_or_else(|| anyhow!("a run of {workflow} with no id"))?;
        let path = format!(
            "/repos/{}/{}/actions/runs/{id}/jobs?per_page=100",
            repo.owner, repo.name
        );
        let jobs = self.app.get(&path, Bearer::Repo(&repo))?;
        let job = |name: &str| {
            jobs["jobs"]
                .as_array()
                .and_then(|j| j.iter().find(|x| x["name"] == name).cloned())
        };
        if status == "completed" {
            let recorded = job(SIGN_JOB).is_some_and(|j| j["conclusion"] == "success");
            return Ok(Job::Ended { recorded });
        }
        Ok(
            match job(RUN_JOB).and_then(|j| j["started_at"].as_str().map(String::from)) {
                Some(since) => Job::Running { since },
                None => Job::Queued,
            },
        )
    }

    fn wake(&self, kind: &(String, Setting), branch: &str, remote: Option<&str>) -> Result<()> {
        let repo = self.repo(remote)?;
        let workflow = Actions::workflow(kind)?;
        let git_ref = field(kind, "ref").unwrap_or_else(|_| "main".into());
        self.dispatch(
            &repo,
            &workflow,
            branch_of(&git_ref),
            json!({ "mode": "round", "branch": branch, "claim": format!("round/{branch}") }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn github(pairs: &[(&str, &str)]) -> (String, Setting) {
        let t: BTreeMap<String, Setting> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), Setting::Text(v.to_string())))
            .collect();
        ("github".into(), Setting::Tables(vec![t]))
    }

    #[test]
    fn a_github_table_names_its_workflow_file() {
        let kind = github(&[("workflow", ".github/workflows/sluice.yml")]);
        assert_eq!(Actions::workflow(&kind).unwrap(), "sluice.yml");
        assert!(Actions::workflow(&("ssh".into(), Setting::Text("k".into()))).is_err());
        assert_eq!(branch_of("refs/heads/main"), "main");
    }
}
