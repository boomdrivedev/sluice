//! Session runs (session-runs-1 clauses 89 to 93): a run a session asks
//! for in its own place, held to its branch's policy as a candidate's run
//! is, and recorded under its keys only when its place holds exactly its
//! head (rule core.15: the policy is read from the branch's current
//! revision, never from the session's place).

use anyhow::{anyhow, Result};
use sluice_seams::{
    GoneRelease, Guard, LastResult, Limits, Outcome, Part, Place, ReleasePolicy, Rev, RunHandle,
    RunOutcome, RunReport, RunRequest, Session, SessionPlace, SessionRunKind, Source, TaskProgress,
    TaskState,
};

use crate::policy::{Policy, Rules};
use crate::records::{Event, ResultLine};
use crate::Sluice;

/// What a session run is held to: the branch's revision and the policy
/// and gate read from it.
#[derive(Clone, Debug)]
pub struct SessionPlan {
    pub branch: String,
    pub base: Rev,
    pub policy: Policy,
    pub rules: Rules,
}

/// A session run under way: its place and its run.
#[derive(Clone, Debug)]
pub struct SessionStart {
    pub place: Place,
    pub run: RunHandle,
}

/// What a finished session run left in the records (92).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recorded {
    /// This many results, under their keys.
    Written(usize),
    /// Nothing: the worktree held more than its head.
    NotClean,
    /// Nothing: the worktree's head or files changed while the run ran,
    /// so what ran is not known to be its head (rule core.18): the head it
    /// moved to, when it moved, and the paths it holds beyond its head;
    /// neither when the worktree is gone.
    Changed {
        moved_to: Option<Rev>,
        paths: Vec<String>,
    },
    /// Nothing: a command, or a run that hit its time limit.
    Nothing,
}

/// A finished session run: what it recorded, the report as it was read,
/// and why its result could not be kept on its place, when it could not.
/// Keeping it decides nothing, so a failure to keep it changes nothing
/// else of the run (97).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finished {
    pub recorded: Recorded,
    /// The run's report, its checks left with no outcome where what the
    /// run asked of the host had gone after it (design D5): what the
    /// session is told.
    pub report: RunReport,
    pub unkept: Option<String>,
}

/// A session's place freed because its worktree no longer exists
/// (session-runs-1 clause 97; 123), as the round or run that freed it
/// reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Abandoned {
    pub worktree: String,
    pub branch: String,
    /// Freed; `false` when freeing failed, and `error` says how.
    pub freed: bool,
    /// The release its runs stored was run, once.
    pub ran: bool,
    /// That release failed, and how; it was freed all the same.
    pub failed: Option<String>,
    pub error: Option<String>,
}

/// Why a session run cannot be held to a branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionRefusal {
    NoBranch,
    /// The branch's revision names no policy for it.
    NotManaged,
}

impl Sluice<'_> {
    /// 56, core.15: the branch's revision, policy and gate.
    pub fn session_plan(&self, branch: &str) -> Result<Result<SessionPlan, SessionRefusal>> {
        let Some(base) = self.seams.branch.read(branch)? else {
            return Ok(Err(SessionRefusal::NoBranch));
        };
        let Some(policy) = self.policy_at(&base)? else {
            return Ok(Err(SessionRefusal::NotManaged));
        };
        let Some(rules) = policy.rules_for(branch)?.cloned() else {
            return Ok(Err(SessionRefusal::NotManaged));
        };
        Ok(Ok(SessionPlan {
            branch: branch.into(),
            base,
            policy,
            rules,
        }))
    }

    /// 64: what the session's head would meet were it submitted now, on the
    /// branch's revision the plan read. A session's run is told it before
    /// it runs; it refuses nothing and is never recorded or read as a
    /// verdict. `None` for a place on no branch, which cannot submit (3).
    pub fn session_forecast(
        &self,
        plan: &SessionPlan,
        session: &Session,
    ) -> Result<Option<crate::Forecasted>> {
        if session.detached {
            return Ok(None);
        }
        Ok(Some(self.forecast_onto(
            &session.head,
            &session.from.worktree,
            &plan.branch,
            plan.base.clone(),
            &[],
        )?))
    }

    /// 84, record 0074 (design D6): what this host lacks of what the
    /// branch's runs ask of it, a container engine that answers, looked
    /// at before a session run of the branch's tests, which it refuses
    /// while it lacks one; `None` for a branch whose runs ask for no
    /// engine, which is never looked for. A limit its executor cannot hold
    /// is refused by the run's start, as before. A command's run asks
    /// nothing of it and is never refused for it.
    pub fn session_unmet(&self, plan: &SessionPlan) -> Result<Option<String>> {
        if !plan.policy.limits.docker {
            return Ok(None);
        }
        let engine = Limits {
            docker: true,
            ..Limits::default()
        };
        self.seams.tests.unmet(&engine)
    }

    /// 89, 83: a session run of the checks `head` affects against the
    /// branch, or of `command`, in `place`, under the branch's executor,
    /// limits and time limit; never answered from the cache, since what a
    /// session run records must be observed (16). A command runs in place
    /// of the runner's checks, in a worktree equipped by the runner the
    /// branch's policy names (147).
    pub fn session_request(
        plan: &SessionPlan,
        place: Place,
        head: &Rev,
        command: Option<&str>,
    ) -> RunRequest {
        let mut tests = plan.policy.tests.clone();
        let required = match command {
            None => plan.rules.required.clone(),
            Some(_) => {
                // Could-not-run-1 114: a not-run exit says a check could
                // not run here; a command's exit is its own, so none of
                // its exits is read so.
                tests.remove("not_run");
                Vec::new()
            }
        };
        RunRequest {
            place,
            revision: head.clone(),
            required,
            since: Some(plan.base.clone()),
            limit: plan.rules.timeout,
            limits: plan.policy.limits.clone(),
            tests,
            command: command.map(str::to_string),
            replay: false,
            system: None,
        }
    }

    /// 89, 90: adopt the session's worktree and start its run there, of
    /// the checks or of `command`, in `part`, the room in the host's bound
    /// its caller holds for it (71, 150; rule core.49).
    pub fn start_session(
        &self,
        plan: &SessionPlan,
        worktree: &str,
        head: &Rev,
        command: Option<&str>,
        part: Part,
    ) -> Result<SessionStart> {
        let place = self.seams.checkout.adopt(
            worktree,
            &plan.branch,
            &plan.policy.checkout,
            &self.now(),
        )?;
        let request = Sluice::session_request(plan, place.clone(), head, command);
        let run = self.seams.tests.start(&request, part)?;
        Ok(SessionStart { place, run })
    }

    /// 92: what a finished session run records: the observed results of
    /// a run of the checks in a worktree that holds exactly its head from
    /// the run's start to its end, and nothing of a command, of a run that
    /// hit its time limit, or of a worktree holding more or moved while it
    /// ran. The result names the head the run started on.
    ///
    /// The run saves nothing of its place: what its tools built is shared
    /// through the host's caches, which its runs are given (35).
    ///
    /// Every finished run, recorded or not, then leaves how it ended on its
    /// place (97), after its results, so a reader that finds it recorded
    /// finds the results (rule core.7's order). It decides nothing (rule
    /// core.18), so a failure to keep it is given back, never raised.
    ///
    /// A run of the checks is read with what became of the host during it,
    /// a switch of its system or an engine gone, as a combination's is
    /// (records 0074, design D5, and 0097, design D7); a command's exit is
    /// its own, and never is (record 0074, design D6).
    pub fn finish_session(
        &self,
        plan: &SessionPlan,
        session: &Session,
        started: &SessionStart,
        command: bool,
        report: &RunReport,
    ) -> Result<Finished> {
        let report = &match command {
            true => report.clone(),
            false => self.after_run(&started.run, &plan.policy.limits, report.clone())?,
        };
        let recorded = self.record_finished(plan, session, started, command, report)?;
        let outcome = if report.timed_out {
            RunOutcome::TimedOut
        } else if passed(report) {
            RunOutcome::Pass
        } else if report.failed() {
            RunOutcome::Fail
        } else {
            RunOutcome::NotRun
        };
        let written = matches!(recorded, Recorded::Written(_));
        // ADR 0035: what the run is worth when its head is submitted (93):
        // its passes, recorded, under an executor the gate trusts, read as
        // a ran result is read (56, rule core.15).
        let worth = (!command).then(|| Worth {
            tasks: ended_tasks(report),
            reused: written
                && passed(report)
                && plan
                    .rules
                    .trusts_run(&report.executor.kind, report.executor.confined),
        });
        let unkept = self
            .leave_result(
                &started.place.name,
                &session.head,
                command,
                outcome,
                written,
                worth,
            )
            .err()
            .map(|e| format!("{e:#}"));
        Ok(Finished {
            recorded,
            report: report.clone(),
            unkept,
        })
    }

    /// 97: a session run that ended with no report, lost or stopped, leaves
    /// that on its place; it recorded nothing (92). An error says only that
    /// it could not be kept, which changes nothing else of the run.
    pub fn session_ended(
        &self,
        place: &str,
        head: &Rev,
        command: bool,
        outcome: RunOutcome,
    ) -> Result<()> {
        self.leave_result(place, head, command, outcome, false, None)
    }

    fn leave_result(
        &self,
        place: &str,
        head: &Rev,
        command: bool,
        outcome: RunOutcome,
        recorded: bool,
        worth: Option<Worth>,
    ) -> Result<()> {
        let (tasks, reused) = match worth {
            Some(w) => (w.tasks, Some(w.reused)),
            None => (Vec::new(), None),
        };
        let last = LastResult {
            at: self.now(),
            head: head.clone(),
            kind: if command {
                SessionRunKind::Command
            } else {
                SessionRunKind::Checks
            },
            outcome,
            recorded,
            tasks,
            reused,
        };
        self.seams.checkout.ran(place, &last)
    }

    fn record_finished(
        &self,
        plan: &SessionPlan,
        session: &Session,
        started: &SessionStart,
        command: bool,
        report: &RunReport,
    ) -> Result<Recorded> {
        if command || report.timed_out {
            return Ok(Recorded::Nothing);
        }
        if !session.dirty.is_empty() {
            return Ok(Recorded::NotClean);
        }
        let worktree = started.place.path.to_string_lossy();
        // 92, rule core.18: the worktree must still hold exactly the head
        // the run started on; a commit or an edit while it ran, or a
        // worktree gone, and what ran is not known to be that head.
        match self.seams.driver.session(&worktree) {
            Ok(now) if now.head == session.head && now.dirty.is_empty() => {}
            Ok(now) => {
                return Ok(Recorded::Changed {
                    moved_to: (now.head != session.head).then_some(now.head),
                    paths: now.dirty,
                })
            }
            Err(_) => {
                return Ok(Recorded::Changed {
                    moved_to: None,
                    paths: Vec::new(),
                })
            }
        }
        let n = self.record_session(plan, &session.head, &worktree, report)?;
        Ok(Recorded::Written(n))
    }

    /// 92, 16, core.7: a clean session run's observed results, recorded
    /// under their keys as runs on `head` from `worktree`, a task a check
    /// was failed by as a check (149); a replay is never recorded as a
    /// run. Returns how many were written.
    pub fn record_session(
        &self,
        plan: &SessionPlan,
        head: &Rev,
        worktree: &str,
        report: &RunReport,
    ) -> Result<usize> {
        let now = self.now();
        let have = self.results(&plan.branch)?;
        let mut bytes = Vec::new();
        let mut n = 0;
        for t in report.tasks.iter().chain(&report.dependencies) {
            if !matches!(t.source, Source::Ran { .. }) {
                continue;
            }
            let r = ResultLine::of(
                &now,
                t,
                &report.executor,
                &plan.branch,
                head,
                Some(worktree),
                // The host's own system: a line naming none is read as it.
                None,
            );
            if !have.iter().any(|x| x.same_as(&r)) {
                bytes.extend(crate::records::to_line(&r));
                n += 1;
            }
        }
        if !bytes.is_empty() {
            self.seams.state.record_results(&bytes)?;
        }
        Ok(n)
    }
}

/// What a finished run of the checks is worth when its head is submitted
/// (ADR 0035): its tasks as they ended, and whether the gate takes them.
struct Worth {
    tasks: Vec<TaskProgress>,
    reused: bool,
}

/// Each task of a finished run as it ended: replayed, passed or failed.
fn ended_tasks(report: &RunReport) -> Vec<TaskProgress> {
    report
        .tasks
        .iter()
        .map(|t| TaskProgress {
            target: t.task.clone(),
            state: match (&t.source, &t.outcome) {
                (_, Outcome::Fail) => TaskState::Failed,
                (Source::Replayed { .. }, Outcome::Pass) => TaskState::Replayed,
                (Source::Ran { .. }, Outcome::Pass) => TaskState::Passed,
            },
            started: None,
            ended: None,
            exit: None,
        })
        .collect()
}

/// Every check the run named reported and passed, within its time limit:
/// what `sluice exec` exits 0 on. A check failed by a task it depends on
/// is no pass (149).
fn passed(report: &RunReport) -> bool {
    !report.timed_out && report.not_run.is_empty() && !report.failed()
}

impl Sluice<'_> {
    /// Every session's place the repository holds (session-runs-1 clause
    /// 97), for `sluice status` to list.
    pub fn session_places(&self) -> Result<Vec<SessionPlace>> {
        self.seams.checkout.session_places()
    }

    /// 95, 56: what a session's place on `branch` is released under, read
    /// from the branch's current revision; an error when the branch cannot
    /// be read. A branch whose revision names no policy names no release
    /// command.
    pub fn release_policy(&self, branch: &str) -> Result<ReleasePolicy> {
        let rev = self
            .seams
            .branch
            .read(branch)?
            .ok_or_else(|| anyhow!("no branch {branch}"))?;
        Ok(match self.policy_at(&rev)? {
            Some(p) => ReleasePolicy {
                branch: branch.into(),
                tests: p.tests,
                limits: p.limits,
            },
            None => ReleasePolicy {
                branch: branch.into(),
                tests: Default::default(),
                limits: Default::default(),
            },
        })
    }

    /// 123, rule core.16: free the session's place of `worktree` at its
    /// session's or driver's accepting that its release failed: nothing
    /// run in its name, its runs ended and its scratch directory, ports
    /// and record let go, the worktree left as it is (25); logged
    /// `released` with what was accepted in its branch's records, under
    /// the branch's lock. `false` when sluice holds no place for it.
    pub fn accept_session(&self, worktree: &str) -> Result<bool> {
        let Some(adopted) = self.seams.checkout.adopted(worktree)? else {
            return Ok(false);
        };
        let failed = self
            .seams
            .checkout
            .session_places()?
            .into_iter()
            .find(|p| p.name == adopted.name)
            .and_then(|p| p.release_failed)
            .map(|f| f.failed);
        let _lock = match adopted.branch.is_empty() {
            true => None,
            false => Some(self.seams.state.wait_lock(&adopted.branch)?),
        };
        self.seams.checkout.free(&adopted.name)?;
        self.log_accepted(&adopted.branch, &adopted.name, Some(worktree), failed)?;
        Ok(true)
    }

    /// Session-runs-1 clause 97, 123, rule core.16: release every
    /// session's place whose worktree no longer exists from what sluice
    /// holds for it: the release command its last run stored, run once in
    /// its scratch directory with its ports, no policy read and no
    /// toolchain entered; then free it whatever that did, its runs ended
    /// and its scratch directory, ports and record let go. Each is logged
    /// `gone` in its branch's records with how its release went, and never
    /// tried again. A place whose worktree exists is never released here,
    /// however old its last run.
    ///
    /// Each takes its branch's lock, never waiting for it (rule core.14):
    /// a place whose branch a round holds, or whose branch cannot be read
    /// (26), is left to the next round or run. One that cannot be freed
    /// is reported and the rest go on, and so is one freed whose `gone`
    /// line cannot be written: one broken place never stops the host's
    /// rounds.
    ///
    /// Then every scratch directory no place's record names is removed,
    /// the freed places' own already gone with them (123, rule core.30). A
    /// removal that fails is warned of, and what was freed is returned all
    /// the same; nothing of it is logged on a branch, since such a
    /// directory is no branch's.
    pub fn release_abandoned(&self) -> Result<Vec<Abandoned>> {
        let mut out = Vec::new();
        for p in self.seams.checkout.session_places()? {
            if !p.gone {
                continue;
            }
            let _lock = if p.branch.is_empty() {
                None
            } else {
                match self.release_lock(&p.branch, true) {
                    Some(l) => Some(l),
                    None => continue,
                }
            };
            let worktree = p.worktree.to_string_lossy().into_owned();
            let (gone, error) = match self.seams.checkout.release_gone(&p.name) {
                Ok(g) => (g, None),
                Err(e) => {
                    tracing::warn!(
                        sluice.worktree = %worktree,
                        "freeing a removed worktree's place failed: {e:#}"
                    );
                    (GoneRelease::default(), Some(format!("{e:#}")))
                }
            };
            if error.is_none() {
                tracing::info!(
                    monotonic_counter.sluice.abandoned.released = 1u64,
                    sluice.branch = %p.branch,
                );
                if !p.branch.is_empty() {
                    let now = self.now();
                    let logged = self.update(&p.branch, |l| {
                        l.log(
                            &now,
                            Event::Gone {
                                place: p.name.clone(),
                                worktree: Some(worktree.clone()),
                                ran: gone.ran,
                                failed: gone.failed.clone(),
                            },
                        );
                        Ok(())
                    });
                    // 26: freed all the same; the profile let it go.
                    if let Err(e) = logged {
                        tracing::warn!(
                            sluice.branch = %p.branch,
                            "logging {worktree}'s place freed on {} failed: {e:#}",
                            p.branch
                        );
                    }
                }
            }
            out.push(Abandoned {
                worktree,
                branch: p.branch,
                freed: error.is_none(),
                ran: gone.ran,
                failed: gone.failed,
                error,
            });
        }
        match self.seams.checkout.remove_unnamed_scratch() {
            Ok(removed) if !removed.is_empty() => tracing::info!(
                sluice.scratch.removed = removed.len(),
                "removed scratch no checkout names: {}",
                removed
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!("removing scratch no checkout names failed: {e:#}"),
        }
        Ok(out)
    }

    /// 26, rule core.14: `branch`'s lock for a pass that releases the
    /// places nobody can use, never waited for, its records read under it
    /// when `logged`, since the release is logged there. `None`, the place
    /// left for a later pass, when a round holds the lock, and when the
    /// lock or the records cannot be had, as a branch that cannot be read,
    /// warned with the branch and the words it was refused with.
    pub(crate) fn release_lock(&self, branch: &str, logged: bool) -> Option<Box<dyn Guard + '_>> {
        let held = self.seams.state.lock(branch).and_then(|lock| {
            if lock.is_some() && logged {
                self.read_line(branch)?;
            }
            Ok(lock)
        });
        match held {
            Ok(lock) => lock,
            Err(e) => {
                tracing::warn!(
                    sluice.branch = %branch,
                    "{branch} could not be read, so its place is left for a later pass: {e:#}"
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeZone, Utc};
    use sluice_fakes::Fakes;
    use sluice_seams::{Checkout, Settings};

    use crate::{Clock, Seams, Sluice};

    struct Noon;

    impl Clock for Noon {
        fn now(&self) -> DateTime<Utc> {
            Utc.with_ymd_and_hms(2026, 10, 27, 12, 0, 0).unwrap()
        }
    }

    /// 123, rule core.30: scratch no record names is removed once the
    /// removed worktrees' places are freed, and a removal that fails
    /// leaves every one of them freed and returned.
    #[test]
    fn the_freed_are_returned_when_unnamed_scratch_cannot_be_removed() {
        let f = Fakes::new();
        let rev = f.world.init("main", &[("lib", "1")]);
        for w in ["/w/a", "/w/b"] {
            f.world.session(w, &format!("agent-{w}"), &rev);
            f.checkout
                .adopt(w, "main", &Settings::new(), "2026-10-27T09:00:00Z")
                .unwrap();
            f.world.remove_worktree(w);
        }
        f.world.unnamed_scratch_fails(Some("permission denied"));
        let seams = Seams {
            checkout: &f.checkout,
            tests: &f.tests,
            branch: &f.branch,
            driver: &f.driver,
            state: &f.state,
        };
        let freed = Sluice::new(seams, &Noon).release_abandoned().unwrap();
        let freed: Vec<_> = freed
            .iter()
            .map(|a| (a.worktree.as_str(), a.freed, a.error.clone()))
            .collect();
        assert_eq!(freed, vec![("/w/a", true, None), ("/w/b", true, None)]);
        assert!(f.checkout.session_places().unwrap().is_empty());
        assert_eq!(
            f.world.unnamed_scratch_asks(),
            vec![0],
            "asked once, after both were freed"
        );
    }
}
