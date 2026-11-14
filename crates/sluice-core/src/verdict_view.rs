//! A submission's report: what its session reads of the combination it
//! was tested in (49, 26), read from the records alone, the same whatever
//! task runner wrote the results (53) and whichever driver shows it (rules
//! core.1, core.2; ADR 0044). A view, never a record (rule core.14), and
//! it decides nothing (27, 30; 118).

use anyhow::Result;
use sluice_seams::{Outcome, Rev};

use crate::line::Line;
use crate::records::{
    parse_lines, ConflictRecord, Event, FailedTask, LogLine, Recorded, ResultKind, ResultLine,
    SetAside, State, Submission,
};

fn contradicted(r: &Recorded) -> Contradicted {
    Contradicted {
        outcome: r.outcome,
        at: r.at.clone(),
        combination: r.combination.clone(),
        worktree: r.worktree.clone(),
    }
}
use crate::Sluice;

pub use sluice_seams::{Contradicted, Row, RowResult, RunShown, Standing, VerdictView};

impl Sluice<'_> {
    /// 49, 26: submission `id`'s report on `branch`, live or finished,
    /// from the records alone; `None` when the branch has no such
    /// submission.
    pub fn verdict_of(&self, branch: &str, id: u64) -> Result<Option<VerdictView>> {
        let snapshot = self.state_read(branch)?;
        let log = Line::read_log(&snapshot)?;
        let line = Line::load(branch, snapshot)?;
        let sub = match line.subs.get(&id) {
            Some(s) => s.clone(),
            None => match self.finished_sub(branch, id)? {
                Some(s) => s,
                None => return Ok(None),
            },
        };
        let ahead: Vec<u64> = line
            .subs
            .values()
            .filter(|s| s.id < id && !s.state.is_out())
            .map(|s| s.id)
            .collect();
        let results: Vec<ResultLine> = parse_lines(&self.seams.state.results(branch)?)?;
        let asides: Vec<SetAside> = parse_lines(&self.seams.state.set_asides()?)?;
        let mut view = verdict_view(&sub, &ahead, &results, &asides, &log);
        if view.standing == Standing::Testing {
            let candidate = line
                .doc
                .candidates
                .iter()
                .find(|c| Some(&c.revision) == view.combination.as_ref());
            if let Some(c) = candidate {
                view.tasks = self
                    .seams
                    .state
                    .progress(branch)?
                    .remove(&c.id)
                    .unwrap_or_default();
                let open: Vec<_> = c.runs.iter().filter(|r| !r.state.done()).collect();
                for r in &open {
                    // A tag names no check until the runner expands it; the
                    // one run under way names its tasks as it goes.
                    let named: Vec<String> = if r.checks.iter().any(|c| c.starts_with('#')) {
                        if open.len() == 1 {
                            view.tasks.iter().map(|t| t.target.clone()).collect()
                        } else {
                            Vec::new()
                        }
                    } else {
                        r.checks.clone()
                    };
                    for task in named {
                        if !view
                            .rows
                            .iter()
                            .any(|x| x.task == task && x.system == r.system)
                        {
                            view.pending.push((task, r.system.clone()));
                        }
                    }
                }
                view.pending.sort();
                view.pending.dedup();
            }
        }
        Ok(Some(view))
    }
}

/// The report of `sub`, behind the live submissions `ahead`, from the
/// results log, the set-asides and its branch's log.
pub fn verdict_view(
    sub: &Submission,
    ahead: &[u64],
    results: &[ResultLine],
    asides: &[SetAside],
    log: &[LogLine],
) -> VerdictView {
    let mut view = VerdictView {
        branch: sub.branch.clone(),
        id: sub.id,
        standing: standing(sub, ahead, log),
        combination: None,
        on: None,
        submissions: Vec::new(),
        runs: Vec::new(),
        rows: Vec::new(),
        tasks: Vec::new(),
        pending: Vec::new(),
    };
    let shows_rows = matches!(
        view.standing,
        Standing::Testing
            | Standing::Passed
            | Standing::Landing
            | Standing::Landed { .. }
            | Standing::Blamed { .. }
    );
    let Some(t) = sub.tested.as_ref().filter(|_| shows_rows) else {
        return view;
    };
    view.combination = Some(t.combination.clone());
    view.on = Some(t.on.clone());
    view.submissions = t.submissions.clone();
    view.runs = runs(log, &t.combination);
    view.rows = rows(sub, &t.combination, results, asides);
    view
}

fn standing(sub: &Submission, ahead: &[u64], log: &[LogLine]) -> Standing {
    match sub.state {
        State::Waiting => Standing::Waiting,
        State::Testing => Standing::Testing,
        State::Passed => Standing::Passed,
        State::Landing => Standing::Landing,
        State::Landed => Standing::Landed {
            landed_as: sub.landed_as.clone().unwrap_or_else(|| sub.head.clone()),
            beside: sub.beside == Some(crate::records::Beside::Landed),
            covered_by: sub.covered_by.clone(),
        },
        State::Failed => Standing::Blamed {
            timed_out: sub.result.as_deref() == Some("timed out"),
        },
        State::Conflict => {
            let c = sub.conflict.clone().unwrap_or(ConflictRecord {
                paths: Vec::new(),
                with: Vec::new(),
            });
            Standing::Conflict {
                paths: c.paths,
                with: c.with,
            }
        }
        State::Withdrawn => Standing::Withdrawn,
        State::Queued if blamed_with(sub, log) => Standing::FormedAgain,
        State::Queued => Standing::Queued {
            ahead: ahead.to_vec(),
        },
    }
}

/// Whether another submission tested in `sub`'s last combination was
/// blamed since that combination was last tested (17, 21).
fn blamed_with(sub: &Submission, log: &[LogLine]) -> bool {
    let Some(t) = &sub.tested else {
        return false;
    };
    let since = log
        .iter()
        .rposition(|l| matches!(&l.event, Event::Testing { combination, .. } if *combination == t.combination))
        .unwrap_or(0);
    log[since..].iter().any(|l| {
        matches!(&l.event, Event::Blamed { id, .. } if *id != sub.id && t.submissions.contains(id))
    })
}

/// What the runner said of each run of `combination`: its latest line per
/// system.
fn runs(log: &[LogLine], combination: &Rev) -> Vec<RunShown> {
    let mut out: Vec<RunShown> = Vec::new();
    for l in log {
        if let Event::Detail {
            combination: c,
            system,
            detail,
            ..
        } = &l.event
        {
            if c != combination || detail.is_empty() {
                continue;
            }
            out.retain(|r| &r.system != system);
            out.push(RunShown {
                system: system.clone(),
                detail: detail.clone(),
            });
        }
    }
    out.sort_by(|a, b| a.system.cmp(&b.system));
    out
}

fn rows(
    sub: &Submission,
    combination: &Rev,
    results: &[ResultLine],
    asides: &[SetAside],
) -> Vec<Row> {
    // The latest result of each check on each system recorded for this
    // combination of this branch (16, 69).
    let mut lines: Vec<&ResultLine> = Vec::new();
    for r in results
        .iter()
        .filter(|r| r.branch == sub.branch && &r.combination == combination && r.worktree.is_none())
    {
        lines.retain(|x| !(x.task == r.task && x.system == r.system));
        lines.push(r);
    }
    let timed_out = sub.result.as_deref() == Some("timed out");
    let failed_for = |task: &str, system: &Option<String>| -> Option<&FailedTask> {
        sub.failed
            .iter()
            .find(|f| f.task == task && (f.system == *system || f.system.is_none()))
    };
    let mut rows: Vec<Row> = lines
        .iter()
        .map(|r| {
            let aside = asides.iter().find(|a| r.set_aside_by(a));
            let failed = failed_for(&r.task, &r.system);
            let result = match (aside, failed, r.outcome) {
                (Some(_), _, _) => RowResult::SetAside,
                (None, Some(f), _) if f.flaky.is_some() => RowResult::Flaky,
                (None, _, Outcome::Fail) => RowResult::Failed,
                (None, _, Outcome::Pass) => RowResult::Passed,
            };
            Row {
                task: r.task.clone(),
                system: r.system.clone(),
                key: Some(r.hash.clone()),
                result,
                replayed: r.kind == ResultKind::Replayed,
                host: r.host.clone().filter(|_| r.kind == ResultKind::Ran),
                signer: r.signer.clone(),
                executor: r.executor.clone(),
                took_ms: r.took_ms,
                attempts: r.attempts,
                flaky: failed
                    .and_then(|f| f.flaky.as_ref())
                    .map(|k| contradicted(&k.other)),
                not_run: None,
                failed_by: Vec::new(),
                set_aside: aside.map(|a| a.reason.clone()),
                log: failed.and_then(|f| f.log.clone()),
            }
        })
        .collect();
    // Checks the records name as failed with no result line: could not
    // run, failed by a task it depends on, timed out, or left without a
    // result (15, 66, 114, 115, 149).
    for f in &sub.failed {
        if rows
            .iter()
            .any(|r| r.task == f.task && r.system == f.system)
        {
            continue;
        }
        let result = if f.not_run.is_some() {
            RowResult::NotRun
        } else if !f.failed_by.is_empty() {
            RowResult::Failed
        } else if timed_out {
            RowResult::TimedOut
        } else if f.hash.is_empty() {
            RowResult::Missing
        } else {
            RowResult::Failed
        };
        rows.push(Row {
            task: f.task.clone(),
            system: f.system.clone(),
            key: Some(f.hash.clone()).filter(|h| !h.is_empty()),
            result,
            replayed: false,
            host: None,
            signer: None,
            executor: None,
            took_ms: None,
            attempts: None,
            flaky: f.flaky.as_ref().map(|k| contradicted(&k.other)),
            not_run: f.not_run.clone(),
            failed_by: f.failed_by.clone(),
            set_aside: None,
            log: f.log.clone(),
        });
    }
    rows.sort_by(|a, b| (a.result, &a.task, &a.system).cmp(&(b.result, &b.task, &b.system)));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(json: serde_json::Value) -> Submission {
        let mut base = serde_json::json!({
            "schema": "sluice/submission@1", "id": 12, "branch": "main", "head": "h12",
            "from": {"agent": "a", "worktree": "pull/12"}, "submitted": "t0",
            "after": [], "priority": 0, "attempt": 1, "state": "failed",
            "tested": {"combination": "c1", "on": "m0", "submissions": [10, 12]}
        });
        base.as_object_mut()
            .unwrap()
            .extend(json.as_object().unwrap().clone());
        serde_json::from_value(base).unwrap()
    }

    fn line(task: &str, system: Option<&str>, kind: &str, outcome: &str) -> ResultLine {
        serde_json::from_value(serde_json::json!({
            "schema": "sluice/result@1", "at": "t1", "kind": kind, "task": task,
            "hash": format!("{task}-k"), "outcome": outcome, "branch": "main", "combination": "c1",
            "host": if kind == "ran" { Some("h1") } else { None },
            "executor": if kind == "ran" { Some(serde_json::json!({"kind": "sandbox", "confined": true})) } else { None },
            "system": system, "signer": system.map(|_| "box"), "took_ms": 1200, "attempts": 1
        }))
        .unwrap()
    }

    fn log(events: serde_json::Value) -> Vec<LogLine> {
        serde_json::from_value(events).unwrap()
    }

    /// Ran and replayed, each with what the records say of where.
    #[test]
    fn ran_and_replayed() {
        let s = sub(serde_json::json!({"state": "passed", "failed": []}));
        let v = verdict_view(
            &s,
            &[],
            &[
                line("app:lint", None, "ran", "pass"),
                line("core:test", None, "replayed", "pass"),
            ],
            &[],
            &[],
        );
        assert_eq!(v.standing, Standing::Passed);
        assert_eq!(v.submissions, vec![10, 12]);
        let lint = &v.rows[0];
        assert_eq!(
            (lint.task.as_str(), lint.result, lint.replayed),
            ("app:lint", RowResult::Passed, false)
        );
        assert_eq!(lint.host.as_deref(), Some("h1"));
        assert_eq!(
            lint.executor.as_ref().map(|e| e.kind.as_str()),
            Some("sandbox")
        );
        assert_eq!(lint.took_ms, Some(1200));
        let core = &v.rows[1];
        assert!(core.replayed && core.host.is_none() && core.executor.is_none());
    }

    /// One row per check per system, with its signer, failed first.
    #[test]
    fn two_systems() {
        let s = sub(serde_json::json!({"failed": [
            {"task": "cli:build", "hash": "cli:build-k", "system": "macos-arm64"}
        ]}));
        let v = verdict_view(
            &s,
            &[],
            &[
                line("cli:build", Some("linux-x64"), "ran", "pass"),
                line("cli:build", Some("macos-arm64"), "ran", "fail"),
            ],
            &[],
            &[],
        );
        let shown: Vec<(&str, Option<&str>, RowResult)> = v
            .rows
            .iter()
            .map(|r| (r.task.as_str(), r.system.as_deref(), r.result))
            .collect();
        assert_eq!(
            shown,
            vec![
                ("cli:build", Some("macos-arm64"), RowResult::Failed),
                ("cli:build", Some("linux-x64"), RowResult::Passed),
            ]
        );
        assert_eq!(v.rows[1].signer.as_deref(), Some("box"));
    }

    /// Could not run, flaky and set aside, each saying why.
    #[test]
    fn could_not_run_flaky_and_set_aside() {
        let s = sub(serde_json::json!({"failed": [
            {"task": "site:check", "not_run": "exited 77, which [tests] not_run names: it could not run here, in 3 runs"},
            {"task": "app:e2e", "hash": "app:e2e-k", "flaky": {"here": "fail", "other":
                {"outcome": "pass", "at": "t-1", "branch": "main", "combination": "c0"}}}
        ]}));
        let mut aside = line("app:lint", None, "ran", "fail");
        aside.at = "t9".into();
        let asides: Vec<SetAside> = vec![serde_json::from_value(serde_json::json!({
            "schema": "sluice/set-aside@1", "at": "t10", "task": "app:lint", "hash": "app:lint-k",
            "run_at": "t9", "branch": "main", "combination": "c1", "reason": "host overloaded", "by": "op@h"
        }))
        .unwrap()];
        let v = verdict_view(
            &s,
            &[],
            &[line("app:e2e", None, "ran", "fail"), aside],
            &asides,
            &[],
        );
        let by = |t: &str| v.rows.iter().find(|r| r.task == t).unwrap();
        assert_eq!(by("app:e2e").result, RowResult::Flaky);
        assert_eq!(by("app:e2e").flaky.as_ref().unwrap().at, "t-1");
        assert_eq!(by("site:check").result, RowResult::NotRun);
        assert!(by("site:check")
            .not_run
            .as_deref()
            .unwrap()
            .contains("3 runs"));
        assert_eq!(by("app:lint").result, RowResult::SetAside);
        assert_eq!(by("app:lint").set_aside.as_deref(), Some("host overloaded"));
    }

    /// 149: a check failed by a task it depends on is failed, with no key
    /// and the task named, never missing; the task's row comes from its
    /// result line, failed with its key and its log.
    #[test]
    fn a_check_failed_by_a_dependency() {
        let s = sub(serde_json::json!({"failed": [
            {"task": "api:integration", "hash": "api:integration-k", "log": "/kept/api.log"},
            {"task": "root:integration", "failed_by": ["api:integration"]}
        ]}));
        let v = verdict_view(
            &s,
            &[],
            &[line("api:integration", None, "ran", "fail")],
            &[],
            &[],
        );
        let by = |t: &str| v.rows.iter().find(|r| r.task == t).unwrap();
        let dep = by("api:integration");
        assert_eq!(dep.result, RowResult::Failed);
        assert_eq!(dep.key.as_deref(), Some("api:integration-k"));
        assert_eq!(dep.log.as_deref(), Some("/kept/api.log"));
        assert!(dep.failed_by.is_empty());
        let check = by("root:integration");
        assert_eq!(check.result, RowResult::Failed);
        assert_eq!(check.key, None);
        assert_eq!(check.failed_by, ["api:integration"]);
        assert!(check.log.is_none() && check.host.is_none());
    }

    /// Behind a blamed submission: formed again, and nothing of the
    /// failed combination (20; 118).
    #[test]
    fn behind_a_blamed_submission() {
        let s = sub(serde_json::json!({"state": "queued", "failed": []}));
        let events = log(serde_json::json!([
            {"at": "t1", "event": "testing", "combination": "c1", "submissions": [10, 12], "place": "p"},
            {"at": "t2", "event": "blamed", "id": 10, "combination": "c0"}
        ]));
        let v = verdict_view(
            &s,
            &[10],
            &[line("app:test", None, "ran", "fail")],
            &[],
            &events,
        );
        assert_eq!(v.standing, Standing::FormedAgain);
        assert!(v.rows.is_empty() && v.combination.is_none() && v.submissions.is_empty());
        let json = serde_json::to_string(&v).unwrap();
        assert!(!json.contains("app:test") && !json.contains("10"), "{json}");
    }

    /// Queued with nothing blamed: what is ahead, and no rows.
    #[test]
    fn queued() {
        let s = sub(serde_json::json!({"state": "queued", "tested": null, "failed": []}));
        let v = verdict_view(&s, &[10, 11], &[], &[], &[]);
        assert_eq!(
            v.standing,
            Standing::Queued {
                ahead: vec![10, 11]
            }
        );
        assert!(v.rows.is_empty());
    }

    /// The runner's own word on each run, by system.
    #[test]
    fn the_runs_detail() {
        let s = sub(serde_json::json!({"state": "passed", "failed": []}));
        let events = log(serde_json::json!([
            {"at": "t1", "event": "detail", "combination": "c1",
             "detail": {"took_ms": 4000, "estimate_ms": 9000}},
            {"at": "t1", "event": "detail", "combination": "c9", "detail": {"took_ms": 1}}
        ]));
        let v = verdict_view(&s, &[], &[], &[], &events);
        assert_eq!(v.runs.len(), 1);
        assert_eq!(v.runs[0].detail.saved_ms(), Some(5000));
    }
}
