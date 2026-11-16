//! GitHub Actions as a pool (88, 107, 111; C.2 launched; ADR 0034): a run
//! no standing host can take starts one job of the workflow the policy's
//! pool names, by `workflow_dispatch`, on the runner its system names,
//! with the inputs `sluice.yml` declares and no other, since GitHub
//! refuses a dispatch carrying an input its workflow does not declare. A
//! job is named by its claim in the run's title, `sluice <claim>`, which
//! the workflow sets, so where it stands is read from the workflow's runs
//! and their jobs (record 0122; design D12). The workflow runs no round,
//! so a wake dispatches nothing (design D11).

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
pub(crate) fn field(kind: &(String, Setting), key: &str) -> Result<String> {
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
pub(crate) fn branch_of(git_ref: &str) -> &str {
    git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref)
}

/// Record 0119 (design D11): the inputs a pool's run is dispatched with,
/// each one `sluice.yml`'s `workflow_dispatch` declares, and every input
/// it requires.
fn run_inputs(launch: &Launch) -> Value {
    json!({
        "claim": launch.claim,
        "branch": launch.branch,
        "combination": launch.combination.as_str(),
        "system": launch.system.clone().unwrap_or_default(),
        "runner": launch.runner,
    })
}

impl Launcher for Actions {
    fn launch(&self, launch: &Launch) -> Result<()> {
        let repo = self.repo(launch.remote.as_deref())?;
        let workflow = Actions::workflow(&launch.kind)?;
        let git_ref = field(&launch.signer, "ref").unwrap_or_else(|_| "refs/heads/main".into());
        self.dispatch(&repo, &workflow, branch_of(&git_ref), run_inputs(launch))
    }

    /// 77, 88, record 0122 (design D12): the workflow's runs listed,
    /// completed ones among them, in one call conditional on what the host
    /// keeps (rule core.32), the newest bearing `sluice <claim>` in its
    /// title read: queued until its `run` job starts, running since that
    /// job's own start, and, completed, recorded where its `sign` job
    /// succeeded.
    fn job(
        &self,
        kind: &(String, Setting),
        _runners: &std::collections::BTreeMap<String, String>,
        claim: &str,
        remote: Option<&str>,
    ) -> Result<Job> {
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
            return Ok(Job::Ended {
                recorded,
                why: None,
            });
        }
        Ok(
            match job(RUN_JOB).and_then(|j| j["started_at"].as_str().map(String::from)) {
                Some(since) => Job::Running { since },
                None => Job::Queued,
            },
        )
    }

    /// 111, record 0119 (design D11): nothing is dispatched. `sluice.yml`
    /// runs a claimed run and signs it, and no round: the job that leads a
    /// branch with every host off, and its wake, are the change
    /// `a-github-action-runs-a-queue-whole`'s. Until then a submission's
    /// wake is lost, which only delays it (59): the branch's next round on
    /// any host advances it.
    fn wake(&self, _kind: &(String, Setting), _branch: &str, _remote: Option<&str>) -> Result<()> {
        Ok(())
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

    /// The inputs `workflow`'s `workflow_dispatch` declares, each with
    /// whether it is required, read from its lines as the template writes
    /// them.
    fn declared(workflow: &str) -> BTreeMap<String, bool> {
        let mut inputs = BTreeMap::new();
        let mut within = false;
        let mut last = None;
        for line in workflow.lines() {
            let indent = line.len() - line.trim_start().len();
            let text = line.trim();
            if text == "inputs:" && indent == 4 {
                within = true;
                continue;
            }
            if !within || text.is_empty() {
                continue;
            }
            match indent {
                6 => {
                    let name = text.trim_end_matches(':').to_string();
                    inputs.insert(name.clone(), false);
                    last = Some(name);
                }
                8 if text == "required: true" => {
                    inputs.insert(last.clone().expect("an input"), true);
                }
                0..=5 => break,
                _ => {}
            }
        }
        inputs
    }

    /// satisfies: record 0119; 88; the actions-pool spec's "A pool's run is
    /// claimed before its job is dispatched": GitHub refuses a dispatch
    /// carrying an input its workflow does not declare, or lacking one it
    /// requires
    #[test]
    fn a_dispatch_sends_the_inputs_sluice_yml_declares() {
        let runners = BTreeMap::from([("macos-arm64".to_string(), "macos-15".to_string())]);
        let workflow = super::super::workflows::sluice("0.0.0", &runners);
        let declared = declared(&workflow);
        assert!(declared.len() >= 5, "{declared:?}");
        let launch = Launch {
            pool: "forge".into(),
            kind: github(&[("workflow", ".github/workflows/sluice.yml")]),
            signer: github(&[("ref", "refs/heads/main")]),
            runner: "macos-15".into(),
            remote: Some("origin".into()),
            branch: "main".into(),
            candidate: "c3".into(),
            combination: sluice_seams::Rev::new("c0ffee"),
            system: Some("macos-arm64".into()),
            claim: "pool/forge/7".into(),
        };
        let inputs = run_inputs(&launch);
        let sent: std::collections::BTreeSet<&String> =
            inputs.as_object().unwrap().keys().collect();
        let names: std::collections::BTreeSet<&String> = declared.keys().collect();
        assert_eq!(sent, names, "each input sent is declared, and only those");
        for (name, required) in &declared {
            if *required {
                assert_ne!(inputs[name], "", "{name} is required");
            }
        }
    }

    #[test]
    fn a_github_table_names_its_workflow_file() {
        let kind = github(&[("workflow", ".github/workflows/sluice.yml")]);
        assert_eq!(Actions::workflow(&kind).unwrap(), "sluice.yml");
        assert!(Actions::workflow(&("ssh".into(), Setting::Text("k".into()))).is_err());
        assert_eq!(branch_of("refs/heads/main"), "main");
    }
}
