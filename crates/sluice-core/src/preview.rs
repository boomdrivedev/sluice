//! Previews (previews-1 clauses 98 to 103): a running copy of a revision a
//! branch has landed, made by sluice at its caller's asking and released
//! only at its caller's asking, or here when nobody can use it (rule
//! core.16). A preview decides nothing: its command is never recorded as a
//! run, read by a gate, or waited on by a submission (rule core.19).
//!
//! What a preview is brought up with (`[preview]`, `[checkout]`, `[tests]`)
//! is read from the previewed revision, since it describes that revision's
//! stack; what its command may use and reach (`[executor]` and the time
//! limit) from the branch's current revision, since trust is the branch's
//! now (27, 56; rule core.15). Its state is read from its record and its
//! run every time, never kept (rule core.14).

use anyhow::Result;
use sluice_seams::{
    GoneRelease, Limits, Part, Place, PreviewPlace, Progress, Rev, RunHandle, RunRequest,
};

use crate::policy::{Policy, Preview};
use crate::records::Event;
use crate::{Sluice, OUTPUT};

/// The names of a preview begin so.
pub const PREVIEW: &str = "preview/";

/// How much of a failed command's output a preview keeps.
const KEPT: usize = 4000;

/// What a preview of one revision is held to.
#[derive(Clone, Debug)]
pub struct PreviewPlan {
    pub branch: String,
    pub revision: Rev,
    /// The policy of the previewed revision.
    pub policy: Policy,
    pub preview: Preview,
    /// `[executor]` of the branch's current revision.
    pub limits: Limits,
    /// `[preview] timeout`, else the branch's time limit (66).
    pub timeout: Option<std::time::Duration>,
}

/// Why no preview of a revision is made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewRefusal {
    NoBranch,
    /// The branch's revision names no `[branches]` table for it.
    NotManaged,
    /// The revision is not in the branch's history (98, S35).
    NotLanded,
    /// The previewed revision's policy has no `[preview]` table.
    NoTable,
}

/// A preview's state, read from its record and its run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewState {
    /// Its command waits for room in the host's bound (102).
    Waiting,
    /// Its command runs.
    Starting,
    /// Its command exited 0.
    Up,
    /// Its command failed, timed out or was lost: what it printed.
    Failed(String),
}

impl PreviewState {
    pub fn word(&self) -> &'static str {
        match self {
            PreviewState::Waiting => "waiting",
            PreviewState::Starting => "starting",
            PreviewState::Up => "up",
            PreviewState::Failed(_) => "failed",
        }
    }
}

/// A preview with its state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewView {
    pub place: PreviewPlace,
    pub state: PreviewState,
}

/// A preview released here because nobody can use it (101).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dropped {
    pub name: String,
    pub branch: String,
    /// Its command failed: what it printed. `None`: its checkout was
    /// gone, and it was freed (123).
    pub failed: Option<String>,
    /// Its checkout was gone: whether the release its run stored ran, once,
    /// and how it failed; it was freed whatever it did (123).
    pub gone: Option<GoneRelease>,
    /// The release failed, and how.
    pub error: Option<String>,
    /// 123: the release failed on the last try it is given, and the
    /// preview is held until it is released by name.
    pub held: bool,
}

impl Sluice<'_> {
    /// A preview's name: its revision's first twelve characters.
    pub fn preview_name(revision: &Rev) -> String {
        format!(
            "{PREVIEW}{}",
            &revision.as_str()[..revision.as_str().len().min(12)]
        )
    }

    /// 98, S35: what a preview of `revision` through `branch` is held to,
    /// or why there is none.
    pub fn preview_plan(
        &self,
        branch: &str,
        revision: &Rev,
    ) -> Result<Result<PreviewPlan, PreviewRefusal>> {
        let Some(tip) = self.seams.branch.read(branch)? else {
            return Ok(Err(PreviewRefusal::NoBranch));
        };
        let Some(rules) = self.rules_at(branch, &tip)? else {
            return Ok(Err(PreviewRefusal::NotManaged));
        };
        if !self.seams.branch.contains(&tip, revision)? {
            return Ok(Err(PreviewRefusal::NotLanded));
        }
        let current = self
            .policy_at(&tip)?
            .expect("a branch with a gate has a policy");
        let Some(policy) = self.policy_at(revision)? else {
            return Ok(Err(PreviewRefusal::NoTable));
        };
        let Some(preview) = policy.preview.clone() else {
            return Ok(Err(PreviewRefusal::NoTable));
        };
        Ok(Ok(PreviewPlan {
            branch: branch.into(),
            revision: revision.clone(),
            timeout: preview.timeout.or(rules.timeout),
            preview,
            policy,
            limits: current.limits,
        }))
    }

    /// 100: the preview of `revision` on this host, if there is one.
    pub fn preview_of(&self, revision: &Rev) -> Result<Option<PreviewPlace>> {
        let name = Sluice::preview_name(revision);
        Ok(self
            .seams
            .checkout
            .previews()?
            .into_iter()
            .find(|p| p.name == name))
    }

    /// 98, 99: make the preview `plan` names, or give back the one there
    /// is, with its place.
    pub fn make_preview(&self, plan: &PreviewPlan) -> Result<(Place, PreviewPlace)> {
        self.seams.checkout.preview(
            &Sluice::preview_name(&plan.revision),
            &plan.revision,
            &plan.branch,
            &plan.policy.checkout,
            &plan.preview.addresses,
            &self.now(),
        )
    }

    /// 98, 103: the run of a preview's command: the previewed revision's
    /// `[tests]`, its toolchain and release command included, the
    /// checkout equipped by the runner it names (147) and the command run
    /// in place of its checks; the branch's executor and limits; never a
    /// check, so nothing is required and nothing replayed.
    pub fn preview_request(plan: &PreviewPlan, place: Place) -> RunRequest {
        RunRequest {
            place,
            revision: plan.revision.clone(),
            required: Vec::new(),
            since: None,
            limit: plan.timeout,
            limits: plan.limits.clone(),
            tests: plan.policy.tests.clone(),
            command: Some(shell_line(&plan.preview.command)),
            replay: false,
            system: None,
        }
    }

    /// 98, 102: start a preview's command in `part`, the room in the
    /// host's bound its caller holds for it (71, 150; rule core.49).
    pub fn start_preview(&self, plan: &PreviewPlan, place: Place, part: Part) -> Result<RunHandle> {
        let name = place.name.clone();
        let run = self
            .seams
            .tests
            .start(&Sluice::preview_request(plan, place), part)?;
        self.seams.checkout.preview_run(&name, &run)?;
        Ok(run)
    }

    /// A preview's state, read now.
    pub fn preview_state(&self, p: &PreviewPlace) -> Result<PreviewState> {
        if let Some(output) = &p.failed {
            return Ok(PreviewState::Failed(output.clone()));
        }
        let Some(run) = &p.run else {
            return Ok(PreviewState::Waiting);
        };
        Ok(match self.seams.tests.progress(run)? {
            Progress::Running => PreviewState::Starting,
            Progress::Lost => {
                PreviewState::Failed("its command's supervisor stopped before it reported".into())
            }
            Progress::Finished(r) => {
                // 15, 149: a check failed, failed by a task it depends
                // on, or left without a result is no pass.
                if !r.timed_out && !r.failed() {
                    PreviewState::Up
                } else {
                    let task = r
                        .tasks
                        .first()
                        .map(|t| t.task.clone())
                        .unwrap_or_else(|| "preview".into());
                    let text = self
                        .seams
                        .tests
                        .logs(run, &[task])?
                        .into_iter()
                        .map(|l| l.text)
                        .collect::<String>();
                    let start = text.len().saturating_sub(KEPT);
                    let start = (start..=text.len())
                        .find(|i| text.is_char_boundary(*i))
                        .unwrap_or(text.len());
                    let mut output = text[start..].to_string();
                    if r.timed_out {
                        output.push_str("timed out\n");
                    }
                    PreviewState::Failed(output)
                }
            }
        })
    }

    /// 101: every preview on the host, with its state.
    pub fn previews(&self) -> Result<Vec<PreviewView>> {
        let mut out = Vec::new();
        for place in self.seams.checkout.previews()? {
            let state = self.preview_state(&place)?;
            out.push(PreviewView { place, state });
        }
        Ok(out)
    }

    /// 101, rule core.16: release the preview `name` at its caller's
    /// asking; `false` when there is none.
    pub fn release_preview(&self, name: &str) -> Result<bool> {
        let branch = self
            .seams
            .checkout
            .previews()?
            .into_iter()
            .find(|p| p.name == name)
            .map(|p| p.branch);
        let retries = match branch {
            Some(b) => self.release_retries(&b),
            None => crate::step::RELEASE_RETRIES,
        };
        self.seams.checkout.release_preview(name, None, retries)
    }

    /// 123: free the preview `name` at its caller's accepting that its
    /// release failed: nothing run in its name, its checkout removed and
    /// its ports and names let go; logged `released` with what was
    /// accepted in its branch's records, under the branch's lock. `false`
    /// when there is none.
    pub fn accept_preview(&self, name: &str) -> Result<bool> {
        let Some(p) = self
            .seams
            .checkout
            .previews()?
            .into_iter()
            .find(|p| p.name == name)
        else {
            return Ok(false);
        };
        let _lock = self.seams.state.wait_lock(&p.branch)?;
        self.seams.checkout.free(name)?;
        // Where its branch keeps a log, as its release is logged.
        if self.seams.state.branches()?.contains(&p.branch) {
            let failed = p.release_failed.map(|f| f.failed);
            self.log_accepted(&p.branch, name, None, failed)?;
        }
        Ok(true)
    }

    /// 101, rule core.16: release every preview nobody can use: one whose
    /// command failed, timed out or was lost, kept with what it printed,
    /// and one whose checkout is gone, its stored release run once from its
    /// scratch directory and freed whatever that did, logged `gone` with
    /// how it went and never tried again (123). A preview that is up,
    /// starting or waiting with its checkout in place is never released
    /// here, nor one whose checkout is in place held because its release
    /// failed on every try its branch gives it.
    ///
    /// Each release takes its branch's lock, never waiting for it (rule
    /// core.14): a preview whose branch a round holds, or whose branch
    /// cannot be read (26), is left to a later pass. One that fails is
    /// reported and the rest go on, and so is one released whose line
    /// cannot be written.
    pub fn release_unusable_previews(&self) -> Result<Vec<Dropped>> {
        let mut out = Vec::new();
        let checkout = self.seams.checkout;
        for p in checkout.previews()? {
            let held = p.release_failed.as_ref().is_some_and(|f| f.stopped);
            if p.failed.is_some() || (held && !p.gone) {
                continue;
            }
            let failed = match p.gone {
                true => None,
                false => match self.preview_state(&p)? {
                    PreviewState::Failed(output) => Some(output),
                    _ => continue,
                },
            };
            // Where its branch keeps a log, as its release is logged.
            let logged = self.seams.state.branches()?.contains(&p.branch);
            let _lock = match self.release_lock(&p.branch, logged) {
                Some(l) => l,
                None => continue,
            };
            let mut gone = None;
            let done = match &failed {
                None => checkout.release_gone(&p.name).map(|g| {
                    gone = Some(g);
                    true
                }),
                Some(output) => {
                    let retries = self.release_retries(&p.branch);
                    checkout.release_preview(&p.name, Some(output), retries)
                }
            };
            let (released, error) = match done {
                Ok(r) => (r, None),
                Err(e) => {
                    tracing::warn!(sluice.preview = %p.name, "releasing a preview failed: {e:#}");
                    (false, Some(format!("{e:#}")))
                }
            };
            if !released && error.is_none() {
                continue;
            }
            // 123: what the profile kept of a release that failed.
            let kept = match &error {
                Some(_) => checkout
                    .previews()?
                    .into_iter()
                    .find(|q| q.name == p.name)
                    .and_then(|q| q.release_failed),
                None => None,
            };
            // ADR 0022: once, where the failure is kept, for telemetry.
            if let (true, Some(output)) = (released, &failed) {
                tracing::event!(
                    target: OUTPUT,
                    tracing::Level::INFO,
                    sluice.branch = %p.branch,
                    sluice.preview = %p.name,
                    sluice.candidate = %p.revision,
                    sluice.bytes = output.len() as u64,
                    "{}",
                    output
                );
            }
            let event = match (&failed, &kept) {
                (_, Some(f)) => Some(Event::ReleaseFailed {
                    place: p.name.clone(),
                    failed: f.failed.clone(),
                    tries: f.tries,
                    held: f.stopped,
                }),
                _ if !released => None,
                (None, None) => {
                    let g = gone.clone().unwrap_or_default();
                    Some(Event::Gone {
                        place: p.name.clone(),
                        worktree: None,
                        ran: g.ran,
                        failed: g.failed,
                    })
                }
                (Some(_), None) => Some(Event::Released {
                    place: p.name.clone(),
                    worktree: None,
                    accepted: None,
                }),
            };
            if let Some(event) = event.filter(|_| logged) {
                let now = self.now();
                let written = self.update(&p.branch, |l| {
                    l.log(&now, event.clone());
                    Ok(())
                });
                // 26: released all the same; the profile let it go.
                if let Err(e) = written {
                    tracing::warn!(
                        sluice.preview = %p.name,
                        "logging the preview's release on {} failed: {e:#}",
                        p.branch
                    );
                }
            }
            out.push(Dropped {
                name: p.name,
                branch: p.branch,
                held: kept.is_some_and(|f| f.stopped),
                failed,
                gone,
                error,
            });
        }
        Ok(out)
    }
}

/// `argv` as one `sh -c` command line, each word quoted where it needs it.
pub fn shell_line(argv: &[String]) -> String {
    if let [sh, c, line] = argv {
        if sh == "/bin/sh" && c == "-c" {
            return line.clone();
        }
    }
    argv.iter()
        .map(|a| {
            if !a.is_empty()
                && a.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,".contains(c))
            {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', r"'\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
