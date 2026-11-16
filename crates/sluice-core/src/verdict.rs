//! Reading a gate over a run's recorded results (15, 16, 19, 66, 69).

use sluice_seams::{Enforced, NotRun, Outcome, RunReport, Source};

use crate::policy::{Flaky, Rules};
use crate::records::{FailedTask, FlakyKey, Recorded, ResultKind, ResultLine, UnprovenKey};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub passed: bool,
    pub timed_out: bool,
    pub failed: Vec<FailedTask>,
    pub ran: usize,
    pub replayed: usize,
    /// 108: the tasks the run replayed with no trusted pass recorded for
    /// their key, which count for nothing, each with its key and why
    /// (design D4 of `a-signed-run-without-a-sandbox-proves-its-passes`).
    pub unproven: Vec<UnprovenKey>,
    /// 87, 28, S18: on a branch whose records are on its remote, the tasks
    /// the run ran with no result the gate counts for their key, the run's
    /// own signed result among none: missing, never failed.
    pub missing: Vec<String>,
    /// 149, 114: the required checks the runner skipped for tasks they
    /// depend on that ran and failed, every one of which reads passed
    /// under its key: never run, judged by nothing, and read as a check
    /// that could not run, with why.
    pub unjudged: Vec<NotRun>,
    /// 60, 114, record 0122 (design D8): the checks the run's report holds
    /// no outcome for, each with why: never failed, nobody blamed.
    pub none: Vec<NotRun>,
}

/// Record 0122 (design D8): why a check the run's report holds nothing for
/// has no outcome.
pub const NO_OUTCOME: &str = "the run's report holds no outcome for it";

/// The gate read over one finished run. `results` is the repository's
/// results log, which already holds this run's results: a verdict is
/// read only from what is recorded (16; rule core.7).
///
/// A check reads as its key reads across every recorded result (69): a
/// pass only passes, a fail only fails, and both read as the policy's
/// `flaky` says, the same for every candidate of the branch (19). Only
/// ran results held to this run's limits and run under its executor's
/// grant are read with it, on a branch whose records are on its remote as
/// on one in a host's own files: a fail with no container engine says
/// nothing of a pass with one (19, 83), a fail under one grant nothing of
/// a pass under another, and a result recorded with no grant is weighed
/// only with others recorded with none (138; rule core.34). A run that
/// hit its time limit reads as failed (66), and so does one that left a
/// required check without a result (15).
///
/// Only results the gate trusts are read (28): a ran result by its
/// recorded executor. A replay is an observation of no one (core.9): it
/// counts only when a trusted ran pass under this run's limits and grant
/// is recorded for its key (108, 138), and otherwise is unproven, for the
/// round to run again with no replay. An unproven replay is named with
/// its key and why no counted pass proves it, the first that holds of: no
/// run of its key is recorded; its key ran only on another system (85);
/// no result of its key is by a signer the branch counts, on a branch
/// whose records are on its remote, or by a run the branch trusts, on one
/// in a host's own files (78, 79); no such result of its key was held to
/// this run's limits (19, 83); none so held was run under this run's
/// grant (138); and no result the branch counts for its key is a pass
/// (design D4 of `a-signed-run-without-a-sandbox-proves-its-passes`). A
/// check left with no trusted result reads as missing: on a
/// branch whose records are on its remote, a check this run ran whose own
/// result does not count there, signed with a key the branch's policy
/// does not list, is missing until a result the gate counts settles it
/// (87, S18), and no pass. There, `proven` names the tasks whose keys the
/// candidate's tree bears out, the only ones signed (C.6): one it leaves
/// out was signed by no one, so it is failed under its key, no pass
/// (15), never missing. A pass the run kept no proof of never reaches
/// it: the step moves it from the run's checks to those it left not run
/// before anything is recorded, so it has no outcome here (114; record
/// 0105; design D5 of the change `a-pass-nothing-proves-blames-no-one`).
///
/// A task a required check depends on that ran and failed, which the
/// runner skipped the check for, is read under its own key exactly as a
/// check is, and counted among those run (149, 16); the skipped check,
/// which no run gave a key, is never read under one (38). The check is
/// failed by those of its tasks that read failed, named with them and
/// with no key of its own. Where none reads failed and one is missing, so
/// is the check's outcome, until the run is counted (87). Where every one
/// reads passed, as `flaky = "pass"` reads a key with a pass and a fail,
/// nothing judged the check: it is unjudged, read as a check that could
/// not run (114, 115).
///
/// Only results on the run's `system` are read, a pass on one system
/// never standing for another (85, 69); a line that names no system is
/// read as on `unnamed`, the host's own.
///
/// 60, 114, record 0122 (design D8): a check the report leaves missing,
/// and a check the run was given, of `given`, that the report names
/// nowhere, has no outcome, [`NO_OUTCOME`], never a failed task; a
/// required check the runner keeps out of its CI mode, by its own word in
/// `kept_out`, is left without a result and is no pass, as before (15;
/// ADR 0050). A tag of `given` names no check until the runner expands
/// it, so the report is asked of no tag.
pub fn read(
    report: &RunReport,
    results: &[ResultLine],
    rules: &Rules,
    system: Option<&str>,
    unnamed: Option<&str>,
    proven: Option<&[String]>,
    given: &[String],
) -> Verdict {
    let flaky = rules.flaky;
    let trusted = |e: &Enforced| rules.trusts_run(&e.kind, e.confined);
    let on_system = |r: &ResultLine| r.system.as_deref().or(unnamed) == system;
    // 78, 79: on a branch whose records are on its remote, a result counts
    // through its signer and the claims that signer may make, and this
    // run's own report counts once it is recorded signed; on one in a
    // host's own files, through its executor.
    let by_trusted = |r: &ResultLine| match (rules.signed, &r.executor) {
        (true, Some(e)) => r
            .signer
            .as_deref()
            .is_some_and(|s| rules.trusts_signed(s, e.confined, r.system.as_deref().or(unnamed))),
        (false, Some(e)) => trusted(e),
        (_, None) => false,
    };
    // 19, 138: on every branch, and only under this run's limits and grant.
    let counts = |r: &ResultLine| {
        by_trusted(r)
            && r.executor
                .as_ref()
                .is_some_and(|e| same_limits(e, &report.executor))
    };
    // Design D4: why no counted pass proves a replay of `task` at `key`.
    let why_unproven = |task: &str, key: &str| -> &'static str {
        let ran: Vec<&ResultLine> = results
            .iter()
            .filter(|r| r.task == task && r.hash == key && r.kind == ResultKind::Ran)
            .collect();
        if ran.is_empty() {
            return "no run of its key is recorded";
        }
        let here: Vec<&ResultLine> = ran.into_iter().filter(|r| on_system(r)).collect();
        if here.is_empty() {
            return "its key ran only on another system";
        }
        let trusted_here: Vec<&ResultLine> = here.into_iter().filter(|r| by_trusted(r)).collect();
        if trusted_here.is_empty() {
            return if rules.signed {
                "no result of its key is signed by a signer the branch counts"
            } else {
                "no result of its key is by a run the branch trusts"
            };
        }
        let held = |r: &ResultLine| {
            r.executor
                .as_ref()
                .is_some_and(|e| held_alike(e, &report.executor))
        };
        if !trusted_here.iter().any(|r| held(r)) {
            return "no result of its key was held to this run's limits";
        }
        if !trusted_here.iter().any(|r| counts(r)) {
            return "no result of its key was run under this run's grant";
        }
        "no result the branch counts for its key is a pass"
    };
    let failed_task = |task: &str, hash: &str, flaky: Option<FlakyKey>| FailedTask {
        task: task.to_string(),
        hash: hash.to_string(),
        log: None,
        flaky,
        system: system.map(str::to_string),
        not_run: None,
        failed_by: Vec::new(),
        unsigned: None,
    };
    let mut failed = Vec::new();
    let mut unproven = Vec::new();
    let mut missing = Vec::new();
    let mut passed = Vec::new();
    let (mut ran, mut replayed) = (0, 0);
    // 149: a task a check was failed by is read as the check would be.
    for t in report.tasks.iter().chain(&report.dependencies) {
        let recorded: Vec<&ResultLine> = results
            .iter()
            .filter(|r| r.task == t.task && r.hash == t.hash && r.kind == ResultKind::Ran)
            .filter(|r| on_system(r))
            .filter(|r| counts(r))
            .collect();
        let this = match t.source {
            Source::Ran { .. } => {
                ran += 1;
                if rules.signed && recorded.is_empty() {
                    if proven.is_some_and(|p| !p.contains(&t.task)) {
                        failed.push(failed_task(&t.task, &t.hash, None));
                    } else {
                        missing.push(t.task.clone());
                    }
                    continue;
                }
                !rules.signed && trusted(&report.executor)
            }
            Source::Replayed { .. } => {
                replayed += 1;
                if !recorded.iter().any(|r| r.outcome == Outcome::Pass) {
                    unproven.push(UnprovenKey {
                        task: t.task.clone(),
                        key: t.hash.clone(),
                        why: why_unproven(&t.task, &t.hash).to_string(),
                    });
                    continue;
                }
                // The pass it replays is already among the recorded.
                false
            }
        };
        let key = recorded
            .iter()
            .map(|r| r.outcome)
            .chain(this.then_some(t.outcome));
        let (mut pass, mut fail) = (false, false);
        for o in key {
            match o {
                Outcome::Pass => pass = true,
                Outcome::Fail => fail = true,
            }
        }
        let passes = match (pass, fail) {
            (true, true) => flaky == Flaky::Pass,
            (true, false) => true,
            _ => false,
        };
        if passes {
            passed.push(t.task.as_str());
        } else {
            // 19: read as failed for holding both outcomes, so the
            // submitter is shown the result that contradicts this run's,
            // with the host that ran it. This run's own time and host are
            // its recorded line's, which the step reads.
            let flaky = (pass && fail)
                .then(|| recorded.iter().rev().find(|r| r.outcome != t.outcome))
                .flatten()
                .map(|r| FlakyKey {
                    here: t.outcome,
                    at: None,
                    host: None,
                    other: Recorded {
                        outcome: r.outcome,
                        at: r.at.clone(),
                        branch: r.branch.clone(),
                        combination: r.combination.clone(),
                        host: r.host.clone(),
                        worktree: r.worktree.clone(),
                        log: None,
                    },
                });
            failed.push(failed_task(&t.task, &t.hash, flaky));
        }
    }
    // 149: a check its runner skipped for tasks it depends on that ran and
    // failed is failed by those that read failed.
    let read_failed: Vec<String> = failed.iter().map(|f| f.task.clone()).collect();
    let mut unjudged = Vec::new();
    for skipped in &report.failed_by {
        let by: Vec<String> = skipped
            .by
            .iter()
            .filter(|b| read_failed.contains(b))
            .cloned()
            .collect();
        if !by.is_empty() {
            failed.push(FailedTask {
                failed_by: by,
                ..failed_task(&skipped.task, "", None)
            });
        } else if skipped.by.iter().any(|b| missing.contains(b)) {
            // 87, S18: the run is open again for a host whose results
            // count, and the check is run again with it.
        } else if let Some(first) = skipped
            .by
            .first()
            .filter(|_| skipped.by.iter().all(|b| passed.contains(&b.as_str())))
        {
            unjudged.push(NotRun {
                task: skipped.task.clone(),
                reason: format!(
                    "its dependency {first} failed, and its key reads passed, \
                     as flaky = \"pass\" reads a key with a pass and a fail"
                ),
                key: None,
            });
        } else {
            // 15: a task the check was failed by that the run gave no
            // result of leaves the check without one.
            failed.push(failed_task(&skipped.task, "", None));
        }
    }
    // 15, ADR 0050: a required check the runner keeps out of its CI mode
    // is left without a result, and no pass.
    let mut none = Vec::new();
    for task in &report.missing {
        if report.kept_out.contains(task) {
            failed.push(failed_task(task, "", None));
        } else {
            none.push(NotRun {
                task: task.clone(),
                reason: NO_OUTCOME.into(),
                key: None,
            });
        }
    }
    // Record 0122 (design D8): a check given the run that its report names
    // nowhere has no outcome either.
    let named = |c: &str| {
        report.tasks.iter().any(|t| t.task == c)
            || report.dependencies.iter().any(|t| t.task == c)
            || report.missing.iter().any(|m| m == c)
            || report.not_run.iter().any(|n| n.task == c)
            || report.failed_by.iter().any(|f| f.task == c)
            || report.kept_out.iter().any(|k| k == c)
    };
    for check in given.iter().filter(|c| !c.starts_with('#')) {
        if !named(check) && !none.iter().any(|n| n.task == *check) {
            none.push(NotRun {
                task: check.clone(),
                reason: NO_OUTCOME.into(),
                key: None,
            });
        }
    }
    Verdict {
        passed: failed.is_empty()
            && unproven.is_empty()
            && missing.is_empty()
            && unjudged.is_empty()
            && none.is_empty()
            && !report.timed_out,
        timed_out: report.timed_out,
        failed,
        ran,
        replayed,
        unproven,
        missing,
        unjudged,
        none,
    }
}

/// Whether two runs were held to the same limits (83) and run under one
/// grant (138): the executor, the memory, the network, the container
/// engine, and what the executor let the run reach, so a key's results
/// are weighed together, and a replay proven, only under one grant (19;
/// rule core.34). Two results recorded with no grant are under one, and a
/// result with none is under no grant a run records. The cores a run was
/// given vary with the instance's load, so they are not one of them.
fn same_limits(a: &Enforced, b: &Enforced) -> bool {
    held_alike(a, b) && a.grant == b.grant
}

/// Whether two runs were held to the same limits but for the grant: what
/// tells an unproven replay's two whys apart.
fn held_alike(a: &Enforced, b: &Enforced) -> bool {
    (&a.kind, a.confined, a.memory, a.network, a.docker)
        == (&b.kind, b.confined, b.memory, b.network, b.docker)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::RESULT_SCHEMA;

    /// A run read on no named system.
    fn read(report: &RunReport, results: &[ResultLine], rules: &Rules) -> Verdict {
        super::read(report, results, rules, None, None, None, &[])
    }

    /// 60, 114, record 0122 (design D8): a check in the report's missing,
    /// and a check the run was given that the report names nowhere, has no
    /// outcome, saying the report holds none for it, and is never a failed
    /// task; a required check the runner keeps out of its CI mode is left
    /// without a result, as before; a tag given is asked of no report.
    #[test]
    fn a_check_the_report_holds_nothing_for_has_no_outcome() {
        let pass = report(Source::Ran { host: "h".into() }, enforced("open", false));
        let run = RunReport {
            missing: vec!["b:test".into(), "k:test".into()],
            kept_out: vec!["k:test".into()],
            ..pass
        };
        let given: Vec<String> = ["a:test", "b:test", "c:test", "k:test", "#gate"]
            .iter()
            .map(|c| c.to_string())
            .collect();
        let v = super::read(&run, &[], &rules(&["open"]), None, None, None, &given);
        assert!(!v.passed);
        let none: Vec<(&str, &str)> = v
            .none
            .iter()
            .map(|n| (n.task.as_str(), n.reason.as_str()))
            .collect();
        assert_eq!(
            none,
            [
                ("b:test", "the run's report holds no outcome for it"),
                ("c:test", "the run's report holds no outcome for it"),
            ]
        );
        let failed: Vec<&str> = v.failed.iter().map(|f| f.task.as_str()).collect();
        assert_eq!(failed, ["k:test"], "kept out of CI, as before");
        assert!(v.unproven.is_empty() && v.missing.is_empty() && v.unjudged.is_empty());

        let whole = report(Source::Ran { host: "h".into() }, enforced("open", false));
        let v = super::read(
            &whole,
            &[],
            &rules(&["open"]),
            None,
            None,
            None,
            &given[..1],
        );
        assert!(v.passed && v.none.is_empty(), "{v:?}");
    }

    /// The tasks a verdict reads unproven (108).
    fn unproven(v: &Verdict) -> Vec<&str> {
        v.unproven.iter().map(|u| u.task.as_str()).collect()
    }
    use sluice_seams::TaskResult;

    fn rules(trust: &[&str]) -> Rules {
        Rules {
            trust: trust.iter().map(|t| t.to_string()).collect(),
            ..Rules::default()
        }
    }

    fn enforced(kind: &str, confined: bool) -> Enforced {
        Enforced {
            kind: kind.into(),
            confined,
            processors: None,
            memory: None,
            network: None,
            docker: false,
            grant: None,
        }
    }

    fn report(source: Source, executor: Enforced) -> RunReport {
        RunReport {
            detail: None,
            kept_out: Vec::new(),
            tasks: vec![TaskResult {
                took_ms: None,
                attempts: None,
                task: "a:test".into(),
                hash: "h".into(),
                outcome: Outcome::Pass,
                source,
            }],
            missing: Vec::new(),
            not_run: Vec::new(),
            dependencies: Vec::new(),
            failed_by: Vec::new(),
            timed_out: false,
            executor,
            refusals_unread: None,
        }
    }

    fn recorded(kind: ResultKind, executor: Option<Enforced>, outcome: Outcome) -> ResultLine {
        ResultLine {
            schema: RESULT_SCHEMA.into(),
            at: "t".into(),
            kind,
            task: "a:test".into(),
            hash: "h".into(),
            outcome,
            host: None,
            executor,
            branch: "main".into(),
            combination: sluice_seams::Rev::new("c"),
            worktree: None,
            system: None,
            signer: None,
            took_ms: None,
            attempts: None,
        }
    }

    /// Clause 119: a runner's time and attempts decide
    /// nothing; a pass tried twice is a pass, not a flake (19).
    #[test]
    fn a_runners_detail_decides_nothing() {
        let plain = report(Source::Ran { host: "h".into() }, enforced("sandbox", false));
        let mut detailed = plain.clone();
        detailed.tasks[0].took_ms = Some(123_000);
        detailed.tasks[0].attempts = Some(2);
        detailed.detail = Some(sluice_seams::RunDetail {
            took_ms: Some(250_000),
            estimate_ms: Some(580_000),
            changed: vec!["app/a.rs".into()],
        });
        let r = rules(&["sandbox"]);
        let mut with = recorded(
            ResultKind::Ran,
            Some(enforced("sandbox", false)),
            Outcome::Pass,
        );
        with.took_ms = Some(123_000);
        with.attempts = Some(2);
        let without = recorded(
            ResultKind::Ran,
            Some(enforced("sandbox", false)),
            Outcome::Pass,
        );
        let a = read(&plain, &[without], &r);
        let b = read(&detailed, &[with], &r);
        assert_eq!(a, b);
        assert!(b.passed);
        assert!(b.failed.iter().all(|f| f.flaky.is_none()));
    }

    /// A result line carries its runner's detail, and a line written
    /// before it reads without.
    #[test]
    fn result_lines_carry_detail_and_old_ones_read() {
        let mut t =
            report(Source::Ran { host: "h".into() }, enforced("open", false)).tasks[0].clone();
        t.took_ms = Some(2_003);
        t.attempts = Some(2);
        let l = ResultLine::of(
            "t",
            &t,
            &enforced("open", false),
            "main",
            &sluice_seams::Rev::new("c"),
            None,
            None,
        );
        assert_eq!((l.took_ms, l.attempts), (Some(2_003), Some(2)));
        let json = serde_json::to_string(&l).unwrap();
        assert!(json.contains("\"took_ms\":2003"));
        let old = r#"{"schema":"sluice/result@1","at":"t","kind":"ran","task":"a:test","hash":"h","outcome":"pass","branch":"main","combination":"c"}"#;
        let back: ResultLine = serde_json::from_str(old).unwrap();
        assert_eq!((back.took_ms, back.attempts), (None, None));
    }

    /// 108 (design D4 of `a-signed-run-without-a-sandbox-proves-its-
    /// passes`), the run-signing spec's "Each why": five replays read
    /// against results holding, in turn, no run of the first key, a run of
    /// the second on another system alone, a run of the third by an
    /// executor or signer the branch does not count, a trusted run of the
    /// fourth held to other limits, and a counted fail of the fifth, are
    /// each unproven with their key and why.
    #[test]
    fn an_unproven_replay_says_why_by_its_key() {
        let replays = ["a:test", "b:test", "c:test", "d:test", "e:test"];
        let run = RunReport {
            tasks: replays
                .iter()
                .map(|t| TaskResult {
                    took_ms: None,
                    attempts: None,
                    task: t.to_string(),
                    hash: format!("{t}-key"),
                    outcome: Outcome::Pass,
                    source: Source::Replayed { from_run_at: None },
                })
                .collect(),
            ..report(
                Source::Replayed { from_run_at: None },
                enforced("open", false),
            )
        };
        let line = |task: &str, executor: Enforced, system: &str, outcome| ResultLine {
            task: task.into(),
            hash: format!("{task}-key"),
            system: Some(system.into()),
            signer: Some("box".into()),
            ..recorded(ResultKind::Ran, Some(executor), outcome)
        };
        let docker = Enforced {
            docker: true,
            ..enforced("open", false)
        };
        let results = [
            line("b:test", enforced("open", false), "macos", Outcome::Pass),
            ResultLine {
                signer: Some("stranger".into()),
                ..line(
                    "c:test",
                    enforced("container", false),
                    "linux",
                    Outcome::Pass,
                )
            },
            line("d:test", docker, "linux", Outcome::Pass),
            line("e:test", enforced("open", false), "linux", Outcome::Fail),
        ];
        let whys = |v: &Verdict| -> Vec<(String, String, String)> {
            v.unproven
                .iter()
                .map(|u| (u.task.clone(), u.key.clone(), u.why.clone()))
                .collect()
        };
        let expect = |whys: &[(&str, &str)]| -> Vec<(String, String, String)> {
            whys.iter()
                .map(|(t, why)| (t.to_string(), format!("{t}-key"), why.to_string()))
                .collect()
        };

        // A branch in a host's own files.
        let v = super::read(
            &run,
            &results,
            &rules(&["open"]),
            Some("linux"),
            None,
            None,
            &[],
        );
        assert!(!v.passed);
        assert_eq!(
            whys(&v),
            expect(&[
                ("a:test", "no run of its key is recorded"),
                ("b:test", "its key ran only on another system"),
                (
                    "c:test",
                    "no result of its key is by a run the branch trusts"
                ),
                (
                    "d:test",
                    "no result of its key was held to this run's limits"
                ),
                (
                    "e:test",
                    "no result the branch counts for its key is a pass"
                ),
            ])
        );

        // A branch whose records are on its remote, counting `box`'s
        // signed results held to this run's limits, as `d:test`'s pass is
        // there, so it proves it (19).
        let signed = Rules {
            trust: vec!["box".into()],
            signed: true,
            claims: [("box".to_string(), (false, None))].into_iter().collect(),
            ..Rules::default()
        };
        let mut results = results;
        results[2] = line("d:test", enforced("open", false), "linux", Outcome::Pass);
        let v = super::read(&run, &results, &signed, Some("linux"), None, None, &[]);
        assert_eq!(
            whys(&v),
            expect(&[
                ("a:test", "no run of its key is recorded"),
                ("b:test", "its key ran only on another system"),
                (
                    "c:test",
                    "no result of its key is signed by a signer the branch counts"
                ),
                (
                    "e:test",
                    "no result the branch counts for its key is a pass"
                ),
            ])
        );
    }

    /// 28: a result from an executor the gate does not trust never
    /// satisfies it; a replay counts only through a trusted pass recorded
    /// for its key (108), and is otherwise unproven.
    #[test]
    fn only_trusted_results_are_read() {
        let ran = |kind| report(Source::Ran { host: "h".into() }, enforced(kind, false));
        assert!(read(&ran("sandbox"), &[], &rules(&["sandbox"])).passed);
        assert!(!read(&ran("open"), &[], &rules(&["confined", "sandbox"])).passed);
        assert!(read(&ran("container"), &[], &rules(&["container"])).passed);

        let replay = report(
            Source::Replayed { from_run_at: None },
            enforced("open", false),
        );
        let none = read(&replay, &[], &rules(&["open"]));
        assert!(!none.passed && none.failed.is_empty());
        assert_eq!(unproven(&none), ["a:test"]);
        let pass = |kind| recorded(ResultKind::Ran, Some(enforced(kind, false)), Outcome::Pass);
        let proven = read(&replay, &[pass("open")], &rules(&["open"]));
        assert!(proven.passed && proven.unproven.is_empty());
        // A pass by an executor the gate does not trust proves nothing,
        // and neither does an earlier replay (core.9).
        assert_eq!(
            unproven(&read(&replay, &[pass("open")], &rules(&["sandbox"]))),
            ["a:test"]
        );
        let replayed = recorded(ResultKind::Replayed, None, Outcome::Pass);
        assert_eq!(
            unproven(&read(&replay, &[replayed], &rules(&["open"]))),
            ["a:test"]
        );

        // An untrusted fail at the key is not read either; nor is a
        // trusted one from runs held to other limits (19).
        let open_fail = recorded(
            ResultKind::Ran,
            Some(enforced("open", false)),
            Outcome::Fail,
        );
        assert!(
            read(
                &ran("sandbox"),
                std::slice::from_ref(&open_fail),
                &rules(&["sandbox"])
            )
            .passed
        );
        assert!(
            read(
                &ran("sandbox"),
                std::slice::from_ref(&open_fail),
                &rules(&["sandbox", "open"])
            )
            .passed
        );
        let sandbox_fail = recorded(
            ResultKind::Ran,
            Some(enforced("sandbox", false)),
            Outcome::Fail,
        );
        assert!(!read(&ran("sandbox"), &[sandbox_fail], &rules(&["sandbox"])).passed);
    }

    /// 19: a key's fail under one run's limits and its pass under
    /// another's are not flaky; under the same limits they are, whatever
    /// cores each was given.
    #[test]
    fn a_key_is_flaky_only_under_the_same_limits() {
        let docker = Enforced {
            docker: true,
            processors: Some(4),
            ..enforced("sandbox", false)
        };
        let run = report(Source::Ran { host: "h".into() }, docker.clone());
        let before = recorded(
            ResultKind::Ran,
            Some(enforced("sandbox", false)),
            Outcome::Fail,
        );
        assert!(read(&run, &[before], &rules(&["sandbox"])).passed);
        let same = recorded(
            ResultKind::Ran,
            Some(Enforced {
                processors: Some(8),
                ..docker
            }),
            Outcome::Fail,
        );
        assert!(!read(&run, &[same], &rules(&["sandbox"])).passed);
    }

    /// `executor` under `grant`.
    fn granted(executor: Enforced, grant: Option<&str>) -> Enforced {
        Enforced {
            grant: grant.map(str::to_string),
            ..executor
        }
    }

    /// A branch whose records are on its remote, counting `box`'s signed
    /// results, reading a key with a pass and a fail as `flaky` says.
    fn signed_by_box(flaky: Flaky) -> Rules {
        Rules {
            trust: vec!["box".into()],
            signed: true,
            claims: [("box".to_string(), (false, None))].into_iter().collect(),
            flaky,
            ..Rules::default()
        }
    }

    /// 19, 138, rule core.34 (the check-grant spec's "A fail under a
    /// narrower profile", "A fail under the same grant", "A branch whose
    /// state is on its remote", "An old fail" and "Two old lines"): a key's
    /// fail is weighed against a run's pass only under the run's grant, on
    /// a signed branch as on a host's own; a fail recorded with no grant is
    /// weighed with no run that records one, and two lines recorded with
    /// none are weighed together as before.
    #[test]
    fn a_key_is_flaky_only_under_the_same_grant() {
        let sandbox = |grant| granted(enforced("sandbox", false), grant);
        let run = |grant| report(Source::Ran { host: "h".into() }, sandbox(grant));
        let fail = |grant| recorded(ResultKind::Ran, Some(sandbox(grant)), Outcome::Fail);
        let fails = flaky(&["sandbox"], Flaky::Fail);

        // A fail under a narrower profile.
        let v = read(&run(Some("sb1:g2")), &[fail(Some("sb1:g1"))], &fails);
        assert!(v.passed, "{v:?}");
        assert!(v.failed.iter().all(|f| f.flaky.is_none()));

        // A fail under the same grant.
        let v = read(&run(Some("sb1:g1")), &[fail(Some("sb1:g1"))], &fails);
        assert!(!v.passed);
        let k = v.failed[0].flaky.clone().expect("read as flaky");
        assert_eq!((k.here, k.other.outcome), (Outcome::Pass, Outcome::Fail));

        // A branch whose state is on its remote: the run's own signed pass
        // under its grant, and a counted signer's fail under another.
        let signed = |line: ResultLine| ResultLine {
            signer: Some("box".into()),
            ..line
        };
        let own = signed(recorded(
            ResultKind::Ran,
            Some(sandbox(Some("sb1:g2"))),
            Outcome::Pass,
        ));
        let v = read(
            &run(Some("sb1:g2")),
            &[signed(fail(Some("sb1:g1"))), own.clone()],
            &signed_by_box(Flaky::Fail),
        );
        assert!(v.passed, "{v:?}");
        let v = read(
            &run(Some("sb1:g2")),
            &[signed(fail(Some("sb1:g2"))), own],
            &signed_by_box(Flaky::Fail),
        );
        assert!(!v.passed, "the same grant's fail is weighed");

        // An old fail.
        let v = read(&run(Some("sb1:g2")), &[fail(None)], &fails);
        assert!(v.passed, "{v:?}");
        let v = read(&run(Some("open:1")), &[fail(None)], &fails);
        assert!(v.passed, "{v:?}");

        // Two old lines.
        let old_pass = recorded(ResultKind::Ran, Some(sandbox(None)), Outcome::Pass);
        let both = [old_pass, fail(None)];
        let v = read(&run(None), &both, &fails);
        assert!(!v.passed);
        assert!(v.failed[0].flaky.is_some());
        assert!(read(&run(None), &both, &flaky(&["sandbox"], Flaky::Pass)).passed);
    }

    /// 108, 138 (the check-grant spec's "A replay under another grant"):
    /// on a branch in a host's own files and on a signed one, a replay
    /// whose only counted pass is under another grant is unproven, and one
    /// with a pass under its own grant is proven.
    #[test]
    fn a_replay_is_proven_only_by_a_pass_under_its_grant() {
        let open = |grant| granted(enforced("open", false), grant);
        let replay = report(Source::Replayed { from_run_at: None }, open(Some("open:1")));
        let pass = |grant, signer: Option<&str>| ResultLine {
            signer: signer.map(str::to_string),
            ..recorded(ResultKind::Ran, Some(open(grant)), Outcome::Pass)
        };
        for (rules, signer) in [
            (rules(&["open"]), None),
            (signed_by_box(Flaky::Fail), Some("box")),
        ] {
            let v = read(&replay, &[pass(Some("sb1:g1"), signer)], &rules);
            assert_eq!(unproven(&v), ["a:test"], "signed: {}", rules.signed);
            assert!(!v.passed);
            let v = read(&replay, &[pass(None, signer)], &rules);
            assert_eq!(unproven(&v), ["a:test"], "signed: {}", rules.signed);
            let v = read(
                &replay,
                &[pass(Some("sb1:g1"), signer), pass(Some("open:1"), signer)],
                &rules,
            );
            assert!(v.passed && v.unproven.is_empty(), "{v:?}");
        }
    }

    /// 108, 138 (the check-grant spec's "Another grant" and "Other limits
    /// on a signed branch"): a replay whose key holds a counted pass held
    /// to the run's limits under another grant is unproven for its grant,
    /// on a branch in a host's own files and on a signed one; one whose key
    /// holds only a counted signer's pass held to other limits, for its
    /// limits.
    #[test]
    fn an_unproven_replay_names_another_grant() {
        let open = |grant| granted(enforced("open", false), grant);
        let replay = report(Source::Replayed { from_run_at: None }, open(Some("open:1")));
        let pass = |executor: Enforced, signer: Option<&str>| ResultLine {
            signer: signer.map(str::to_string),
            ..recorded(ResultKind::Ran, Some(executor), Outcome::Pass)
        };
        let why =
            |v: &Verdict| -> Vec<String> { v.unproven.iter().map(|u| u.why.clone()).collect() };
        let grant = "no result of its key was run under this run's grant";
        let limits = "no result of its key was held to this run's limits";

        let v = read(
            &replay,
            &[pass(open(Some("sb1:g1")), None)],
            &rules(&["open"]),
        );
        assert_eq!(why(&v), [grant]);
        let v = read(
            &replay,
            &[pass(open(Some("sb1:g1")), Some("box"))],
            &signed_by_box(Flaky::Fail),
        );
        assert_eq!(why(&v), [grant]);

        let docker = Enforced {
            docker: true,
            ..open(Some("open:1"))
        };
        let v = read(
            &replay,
            &[pass(docker, Some("box"))],
            &signed_by_box(Flaky::Fail),
        );
        assert_eq!(why(&v), [limits]);
    }

    /// 19, 83, record 0072 (the check-grant spec's "Another network"): on
    /// a branch whose state is on its remote, a counted signer's fail of a
    /// key held to no network is not weighed against a run held to
    /// `network = "localhost"` that passes it.
    #[test]
    fn a_signed_key_is_weighed_only_under_the_runs_limits() {
        let localhost = Enforced {
            network: Some(sluice_seams::Network::Localhost),
            ..granted(enforced("sandbox", false), Some("sb1:g1"))
        };
        let run = report(Source::Ran { host: "h".into() }, localhost.clone());
        let signed = |executor: Enforced, outcome| ResultLine {
            signer: Some("box".into()),
            ..recorded(ResultKind::Ran, Some(executor), outcome)
        };
        let unheld = Enforced {
            network: None,
            ..localhost.clone()
        };
        let results = [
            signed(unheld, Outcome::Fail),
            signed(localhost.clone(), Outcome::Pass),
        ];
        let v = read(&run, &results, &signed_by_box(Flaky::Fail));
        assert!(v.passed, "{v:?}");
        assert!(v.failed.is_empty());

        let results = [
            signed(localhost.clone(), Outcome::Fail),
            signed(localhost, Outcome::Pass),
        ];
        assert!(!read(&run, &results, &signed_by_box(Flaky::Fail)).passed);
    }

    /// 19: a task read as failed for its key holding a pass and a fail
    /// names what this run observed and the newest result that says
    /// otherwise; one that failed on its own names neither.
    #[test]
    fn a_flaky_task_names_the_result_it_contradicts() {
        let rules = rules(&["sandbox"]);
        let run = report(Source::Ran { host: "h".into() }, enforced("sandbox", false));
        let fail = |at: &str, on: &str| ResultLine {
            at: at.into(),
            combination: sluice_seams::Rev::new(on),
            host: Some(format!("host-{on}")),
            ..recorded(
                ResultKind::Ran,
                Some(enforced("sandbox", false)),
                Outcome::Fail,
            )
        };
        let v = read(&run, &[fail("t1", "c1"), fail("t2", "c2")], &rules);
        assert!(!v.passed);
        let k = v.failed[0].flaky.clone().expect("read as flaky");
        assert_eq!(k.here, Outcome::Pass);
        assert_eq!(
            (k.other.outcome, k.other.at.as_str()),
            (Outcome::Fail, "t2")
        );
        assert_eq!(k.other.combination, sluice_seams::Rev::new("c2"));
        // The host of the run it contradicts, not this run's `h`; this
        // run's own are its recorded line's, which the step fills.
        assert_eq!(k.other.host.as_deref(), Some("host-c2"));
        assert_eq!((k.at, k.host), (None, None));

        let failing = RunReport {
            tasks: vec![TaskResult {
                outcome: Outcome::Fail,
                ..run.tasks[0].clone()
            }],
            ..run.clone()
        };
        let v = read(&failing, &[], &rules);
        assert_eq!(v.failed[0].flaky, None);
    }

    /// 85, 86, 69: a pass on one system never stands for another, and a
    /// line that names no system is the host's own.
    #[test]
    fn a_pass_on_one_system_proves_nothing_on_another() {
        let replay = report(
            Source::Replayed { from_run_at: None },
            enforced("open", false),
        );
        let on = |system: Option<&str>| ResultLine {
            system: system.map(str::to_string),
            ..recorded(
                ResultKind::Ran,
                Some(enforced("open", false)),
                Outcome::Pass,
            )
        };
        let r = rules(&["open"]);
        let v = super::read(
            &replay,
            &[on(Some("linux-x64"))],
            &r,
            Some("macos-arm64"),
            None,
            None,
            &[],
        );
        assert_eq!(unproven(&v), ["a:test"]);
        let v = super::read(
            &replay,
            &[on(Some("linux-x64"))],
            &r,
            Some("linux-x64"),
            None,
            None,
            &[],
        );
        assert!(v.passed);
        // An old line is the host's own system.
        let v = super::read(
            &replay,
            &[on(None)],
            &r,
            Some("linux-x64"),
            Some("linux-x64"),
            None,
            &[],
        );
        assert!(v.passed);
        let v = super::read(
            &replay,
            &[on(None)],
            &r,
            Some("macos-arm64"),
            Some("linux-x64"),
            None,
            &[],
        );
        assert!(!v.passed);
        // A failure names its system.
        let fail = report(Source::Ran { host: "h".into() }, enforced("open", false));
        let fail = RunReport {
            tasks: vec![TaskResult {
                outcome: Outcome::Fail,
                ..fail.tasks[0].clone()
            }],
            ..fail
        };
        let v = super::read(&fail, &[], &r, Some("linux-arm64"), None, None, &[]);
        assert_eq!(v.failed[0].system.as_deref(), Some("linux-arm64"));
    }

    /// 87, 28; S18: on a branch whose records are on its remote, a check
    /// this run ran whose own result does not count, signed with a key the
    /// branch does not list, is missing and no pass, never failed; a
    /// result the gate counts settles it.
    #[test]
    fn a_check_with_no_counted_result_is_missing_never_failed() {
        let r = Rules {
            trust: vec!["box".into()],
            signed: true,
            claims: [("box".to_string(), (false, None))].into_iter().collect(),
            ..Rules::default()
        };
        let ran = report(Source::Ran { host: "h".into() }, enforced("open", false));
        let v = read(&ran, &[], &r);
        assert!(!v.passed);
        assert!(v.failed.is_empty());
        assert_eq!(v.missing, ["a:test"]);
        let mut by_box = recorded(
            ResultKind::Ran,
            Some(enforced("open", false)),
            Outcome::Pass,
        );
        by_box.signer = Some("box".into());
        let v = read(&ran, &[by_box], &r);
        assert!(v.passed);
        assert!(v.missing.is_empty());
    }
    /// A run of `root:integration`, skipped for `api:integration`, which
    /// ran and failed under the key `d`, held to `executor`.
    fn failed_by(executor: Enforced) -> RunReport {
        RunReport {
            tasks: Vec::new(),
            dependencies: vec![TaskResult {
                took_ms: None,
                attempts: None,
                task: "api:integration".into(),
                hash: "d".into(),
                outcome: Outcome::Fail,
                source: Source::Ran { host: "h".into() },
            }],
            failed_by: vec![sluice_seams::FailedBy {
                task: "root:integration".into(),
                by: vec!["api:integration".into()],
            }],
            ..report(Source::Ran { host: "h".into() }, executor)
        }
    }

    /// A result recorded under `task`'s key `hash`, by `executor`.
    fn at(task: &str, hash: &str, executor: Enforced, outcome: Outcome) -> ResultLine {
        ResultLine {
            task: task.into(),
            hash: hash.into(),
            ..recorded(ResultKind::Ran, Some(executor), outcome)
        }
    }

    /// `rules(trust)` reading a key with a pass and a fail as `flaky` says.
    fn flaky(trust: &[&str], flaky: Flaky) -> Rules {
        Rules {
            flaky,
            ..rules(trust)
        }
    }

    /// 149, 16: a task a check was failed by is read under its own key
    /// and counted among the run's; the check is failed by it, with no
    /// key of its own.
    #[test]
    fn a_check_is_failed_by_the_task_it_depends_on() {
        let v = read(&failed_by(enforced("open", false)), &[], &rules(&["open"]));
        assert!(!v.passed);
        assert_eq!(v.ran, 1);
        assert_eq!(v.failed.len(), 2);
        let (dep, check) = (&v.failed[0], &v.failed[1]);
        assert_eq!(
            (dep.task.as_str(), dep.hash.as_str()),
            ("api:integration", "d")
        );
        assert!(dep.failed_by.is_empty() && dep.flaky.is_none());
        assert_eq!(check.task, "root:integration");
        assert_eq!(check.hash, "");
        assert_eq!(check.failed_by, ["api:integration"]);
        assert!(check.flaky.is_none() && check.log.is_none() && check.not_run.is_none());
        assert!(v.unjudged.is_empty() && v.missing.is_empty());
    }

    /// 149, 19: a dependency whose key holds a trusted pass, under
    /// `flaky = "fail"`, is flaky, naming the pass, and fails the check.
    #[test]
    fn a_flaky_dependency_read_as_failed_fails_the_check() {
        let open = enforced("open", false);
        let pass = at("api:integration", "d", open.clone(), Outcome::Pass);
        let v = read(&failed_by(open), &[pass], &flaky(&["open"], Flaky::Fail));
        assert!(!v.passed);
        let k = v.failed[0].flaky.clone().expect("read as flaky");
        assert_eq!((k.here, k.other.outcome), (Outcome::Fail, Outcome::Pass));
        assert_eq!(v.failed[1].failed_by, ["api:integration"]);
    }

    /// 149, 114: the same under `flaky = "pass"` reads the dependency
    /// passed, so nothing judged the check: unjudged, saying why, and
    /// nothing failed.
    #[test]
    fn a_flaky_dependency_read_as_passed_leaves_the_check_unjudged() {
        let open = enforced("open", false);
        let pass = at("api:integration", "d", open.clone(), Outcome::Pass);
        let v = read(&failed_by(open), &[pass], &flaky(&["open"], Flaky::Pass));
        assert!(!v.passed);
        assert!(v.failed.is_empty());
        assert_eq!(
            v.unjudged,
            [NotRun {
                task: "root:integration".into(),
                reason: "its dependency api:integration failed, and its key reads passed, \
                         as flaky = \"pass\" reads a key with a pass and a fail"
                    .into(),
                key: None,
            }]
        );
    }

    /// 149, 19, 83: a dependency's pass under other limits is not
    /// weighed with its fail, so the check is failed by it.
    #[test]
    fn a_dependencys_pass_under_other_limits_is_not_weighed() {
        let docker = Enforced {
            docker: true,
            ..enforced("open", false)
        };
        let pass = at(
            "api:integration",
            "d",
            enforced("open", false),
            Outcome::Pass,
        );
        let v = read(&failed_by(docker), &[pass], &flaky(&["open"], Flaky::Pass));
        assert!(!v.passed && v.unjudged.is_empty());
        assert!(v.failed[0].flaky.is_none());
        assert_eq!(v.failed[1].failed_by, ["api:integration"]);
    }

    /// 149, 87, S18: on a branch whose records are on its remote, a
    /// dependency whose result no listed key signed is missing, and the
    /// check is named nowhere until the run is counted; one whose key the
    /// tree did not bear out is failed under it, and fails the check (C.6).
    #[test]
    fn a_dependency_with_no_counted_result_is_missing_and_names_no_check() {
        let r = Rules {
            trust: vec!["box".into()],
            signed: true,
            claims: [("box".to_string(), (false, None))].into_iter().collect(),
            ..Rules::default()
        };
        let run = failed_by(enforced("open", false));
        let v = read(&run, &[], &r);
        assert!(!v.passed);
        assert_eq!(v.missing, ["api:integration"]);
        assert!(v.failed.is_empty() && v.unjudged.is_empty());

        let v = super::read(&run, &[], &r, None, None, Some(&[]), &[]);
        assert!(v.missing.is_empty());
        assert_eq!(
            (v.failed[0].task.as_str(), v.failed[0].hash.as_str()),
            ("api:integration", "d")
        );
        assert_eq!(v.failed[1].failed_by, ["api:integration"]);

        let mut by_box = at(
            "api:integration",
            "d",
            enforced("open", false),
            Outcome::Fail,
        );
        by_box.signer = Some("box".into());
        let v = read(&run, &[by_box], &r);
        assert!(v.missing.is_empty());
        assert_eq!(v.failed[1].failed_by, ["api:integration"]);
    }

    /// 149: a check failed by two, one read failed and one read passed,
    /// is failed by the first alone.
    #[test]
    fn a_check_failed_by_two_names_those_read_failed() {
        let open = enforced("open", false);
        let mut run = failed_by(open.clone());
        run.dependencies.push(TaskResult {
            task: "ui:integration".into(),
            hash: "u".into(),
            ..run.dependencies[0].clone()
        });
        run.failed_by[0].by.push("ui:integration".into());
        let pass = at("ui:integration", "u", open, Outcome::Pass);
        let v = read(&run, &[pass], &flaky(&["open"], Flaky::Pass));
        assert!(!v.passed);
        assert_eq!(v.ran, 2);
        let names: Vec<&str> = v.failed.iter().map(|f| f.task.as_str()).collect();
        assert_eq!(names, ["api:integration", "root:integration"]);
        assert_eq!(v.failed[1].failed_by, ["api:integration"]);
    }

    /// 149, 38: the skipped check's own key is never read: a pass or a
    /// fail recorded under its name makes it neither flaky nor passed.
    #[test]
    fn the_skipped_checks_own_key_is_never_read() {
        let open = enforced("open", false);
        let lines = [
            at("root:integration", "r", open.clone(), Outcome::Pass),
            at("root:integration", "", open.clone(), Outcome::Pass),
        ];
        let v = read(&failed_by(open), &lines, &flaky(&["open"], Flaky::Pass));
        assert!(!v.passed);
        let check = &v.failed[1];
        assert_eq!(check.task, "root:integration");
        assert_eq!((check.hash.as_str(), check.flaky.as_ref()), ("", None));
        assert_eq!(check.failed_by, ["api:integration"]);
        assert_eq!(v.ran, 1);
    }
}
