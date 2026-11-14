//! Verdicts, attribution and the window: A.3 and A.4.

use std::path::PathBuf;

use sluice_conformance::{policy, shared, Harness, Rig, BRANCH};
use sluice_core::records::{Event, ResultKind, ResultLine, State};
use sluice_core::{KeptOut, SubmitRefusal};
use sluice_seams::{Decided, Outcome};

fn results(h: &Harness) -> Vec<ResultLine> {
    sluice_core::records::parse_lines(h.w().results_log().as_bytes()).unwrap()
}

/// satisfies: 16, 37, 38
#[test]
fn every_run_is_recorded_before_its_verdict_a_fail_exactly_as_a_pass() {
    shared::every_run_is_recorded_before_its_verdict_a_fail_exactly_as_a_pass(&Harness::bolt());
}

/// satisfies: 15, 38, 69
#[test]
fn a_key_that_holds_a_pass_is_read_from_it_on_any_line_and_recorded_as_a_replay() {
    let h = Harness::bolt();
    let other = "bolt/y";
    h.w().init(
        other,
        &[
            (".config/sluice.toml", &policy(false, "fail", 4)),
            ("a/lib", "a1"),
            ("b/lib", "b1"),
            ("c/lib", "c1"),
        ],
    );
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    let ran = h.w().observed().len();

    // The same work on another branch: every key already holds a pass.
    let head = h.w().work(&h.w().tip(other), &[("a/lib", Some("a2"))]);
    h.w().session("unit/y", "agent-y", &head);
    let s = h.sluice();
    let two = s
        .submit(&sluice_core::SubmitRequest {
            head: sluice_core::SubmitHead::Worktree("unit/y".into()),
            branch: other.into(),
            after: vec![],
            priority: 0,
            subject: None,
        })
        .unwrap()
        .unwrap()
        .id;
    for _ in 0..6 {
        s.step(other).unwrap();
    }
    assert_eq!(h.sub(two).state, State::Landed);
    // Nothing was run again (15), and each is a replay, not a run (16, 38).
    assert_eq!(h.w().observed().len(), ran);
    let r = results(&h);
    let replays: Vec<&ResultLine> = r.iter().filter(|l| l.branch == other).collect();
    assert_eq!(replays.len(), 3);
    assert!(replays
        .iter()
        .all(|l| l.kind == ResultKind::Replayed && l.host.is_none()));
}

/// satisfies: 70, 15, 36
#[test]
fn a_revision_with_no_verdict_affects_every_required_check() {
    let h = Harness::bolt();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    h.settle();
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    h.submit("unit/b");
    h.settle();
    h.w().hand_commit(BRANCH, &[("c/lib", Some("by hand"))]);
    h.work("unit/c", "agent-c", &[("c/lib", Some("c3"))]);
    h.submit("unit/c");
    h.settle();

    let since: Vec<bool> = h.w().requests().iter().map(|r| r.since.is_some()).collect();
    // A new branch, then a branch sluice landed, then a branch moved by hand.
    assert_eq!(since, vec![false, true, false]);
    // Only the affected check ran on the landed branch.
    let b_run: Vec<_> = h
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Ran { task, .. } | Event::Replayed { task, .. } => Some(task),
            _ => None,
        })
        .collect();
    assert_eq!(b_run[3], "b:test");
    assert_eq!(b_run.len(), 3 + 1 + 3);
}

/// satisfies: 18, 17, 55
#[test]
fn no_blame_until_every_candidate_ahead_has_a_verdict() {
    let h = Harness::bolt();
    h.w().hold_runs(true);
    h.w().fails_with("a:test", "a/two", "bad");
    h.w().fails_with("a:test", "a/one", "bad");
    h.work("unit/1", "agent-1", &[("a/one", Some("fine"))]);
    h.work("unit/2", "agent-2", &[("a/two", Some("bad"))]);
    let (one, two) = (h.submit("unit/1"), h.submit("unit/2"));
    h.step();
    let c2 = h.sub(two).tested.unwrap().combination;
    h.w().complete(&c2);
    h.step();
    h.step();
    // #2's combination failed, but #1's has no verdict: no one is blamed,
    // nothing lands.
    assert!(!h.events().iter().any(|e| matches!(e, Event::Blamed { .. })));
    assert!(h.w().delivered().is_empty());
    assert!(h.w().moves().is_empty());
    assert_eq!(h.sub(two).state, State::Testing);

    h.w().hold_runs(false);
    h.w().complete_all();
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.sub(two).state, State::Failed);
}

/// satisfies: 66, 20, 32
#[test]
fn a_run_past_its_time_limit_is_recorded_as_timed_out_and_read_as_failed() {
    let h = Harness::bolt();
    h.w().hangs("a:test");
    h.work("unit/a", "agent-a", &[("a/lib", Some("slow"))]);
    let one = h.submit("unit/a");
    h.settle();

    assert_eq!(
        h.w().requests()[0].limit,
        Some(std::time::Duration::from_secs(1200))
    );
    assert!(h.events().iter().any(|e| matches!(e,
        Event::Result { result, .. } if result == "timed out")));
    let s = h.sub(one);
    assert_eq!(s.state, State::Failed);
    assert_eq!(s.result.as_deref(), Some("timed out"));
    match &h.w().delivered_to("agent-a")[0].decided {
        Decided::Failed { timed_out, .. } => assert!(timed_out),
        other => panic!("delivered {other:?}"),
    }
    assert_eq!(h.w().live_runs(), 0);
    assert!(h.w().live_places().is_empty());
}

/// A check the time limit cut off, on a host too slow that day, has no
/// outcome: nothing is recorded under its key, so once the host keeps up
/// the same head lands, and is never read as flaky. What ended before the
/// limit keeps its result.
/// satisfies: time-limit-1 66, 19, 20, 69; rule core.7
#[test]
fn a_check_the_time_limit_cut_off_is_recorded_under_no_key_and_the_same_head_lands() {
    let h = Harness::bolt();
    h.w().hangs("b:test");
    h.work(
        "unit/ab",
        "agent-a",
        &[("a/lib", Some("a2")), ("b/lib", Some("b2"))],
    );
    let one = h.submit("unit/ab");
    h.settle();

    let s = h.sub(one);
    assert_eq!(s.state, State::Failed);
    assert_eq!(s.result.as_deref(), Some("timed out"));
    let named: Vec<(&str, &str)> = s
        .failed
        .iter()
        .map(|f| (f.task.as_str(), f.hash.as_str()))
        .collect();
    assert_eq!(named, [("b:test", "")], "cut off, with no key");
    let lines = results(&h);
    assert!(lines
        .iter()
        .any(|r| r.task == "a:test" && r.outcome == Outcome::Pass));
    assert!(
        !lines.iter().any(|r| r.task == "b:test"),
        "nothing under b:test's key: {lines:?}"
    );
    match &h.w().delivered_to("agent-a")[0].decided {
        Decided::Failed {
            failed, timed_out, ..
        } => {
            assert!(timed_out);
            assert_eq!(failed, &["b:test"]);
        }
        other => panic!("delivered {other:?}"),
    }

    h.w().ends_in_time("b:test");
    let two = h.submit("unit/ab");
    h.settle();
    let s = h.sub(two);
    assert_eq!(s.state, State::Landed, "{:?}", s.failed);
    assert!(results(&h)
        .iter()
        .any(|r| r.task == "b:test" && r.outcome == Outcome::Pass));
}

/// satisfies: 13, 71
#[test]
fn the_window_shrinks_on_a_failure_grows_on_passes_and_the_host_bounds_it() {
    let h = Harness::bolt();
    h.w().hold_runs(true);
    h.w().fails_with("a:test", "a/1", "bad");
    for n in 1..=5 {
        h.work(
            &format!("unit/{n}"),
            "agent",
            &[(&format!("a/{n}"), Some(if n == 1 { "bad" } else { "ok" }))],
        );
        h.submit(&format!("unit/{n}"));
    }
    h.step();
    // Four at once, the policy's bound, across the branch.
    assert_eq!(h.w().live_runs(), 4);
    h.w().complete_all();
    h.step();
    // #1 failed and was blamed: the window halves to two.
    let window = || h.sluice().status(BRANCH).unwrap().window;
    assert_eq!(window(), 2);
    assert_eq!(h.w().live_runs(), 2);
    h.w().complete_all();
    h.step();
    assert_eq!(window(), 4);

    // A host bound of one holds every branch to one run at a time (71).
    let h = Harness::bolt();
    h.w().host_limit(1);
    h.w().hold_runs(true);
    for n in 1..=3 {
        h.work(
            &format!("unit/{n}"),
            "agent",
            &[(&format!("a/{n}"), Some("ok"))],
        );
        h.submit(&format!("unit/{n}"));
    }
    h.step();
    assert_eq!(h.w().live_runs(), 1);
}

/// satisfies: 60, S15, 37
#[test]
fn a_run_whose_executor_stopped_has_no_outcome_and_runs_again() {
    let h = Harness::bolt();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    let c = h.sub(one).tested.unwrap().combination;
    h.w().lose(&c);
    h.step();
    assert!(h.events().iter().any(|e| matches!(e, Event::Lost { .. })));
    h.w().hold_runs(false);
    h.w().complete_all();
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.w().started().len(), 2);
}

/// satisfies: 55, 70, 56
#[test]
fn a_landed_change_to_the_gate_runs_every_check_it_now_requires() {
    shared::a_landed_change_to_the_gate_runs_every_check_it_now_requires(&Harness::new(
        &policy(false, "fail", 4).replace("[\"#gate\"]", "[\"a:test\"]"),
    ));
}

/// satisfies: 70, 55
#[test]
fn a_report_holding_none_of_the_required_checks_is_no_pass() {
    let h = Harness::new(&policy(false, "fail", 4).replace("#gate", "#nothing"));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Failed);
}

/// Two bolt branches, each with two submissions, under a bound of two;
/// how many runs are live after rounds of both branches.
fn live_runs_of_two_branches() -> usize {
    let h = Harness::bolt();
    let other = "bolt/y";
    h.w().init(
        other,
        &[
            (".config/sluice.toml", &policy(false, "fail", 4)),
            ("a/lib", "a1"),
        ],
    );
    h.w().host_limit(2);
    h.w().hold_runs(true);
    for n in 1..=2 {
        h.work(
            &format!("unit/x{n}"),
            "agent",
            &[(&format!("a/x{n}"), Some("ok"))],
        );
        h.submit(&format!("unit/x{n}"));
        let head = h
            .w()
            .work(&h.w().tip(other), &[(&format!("a/y{n}"), Some("ok"))]);
        h.w().session(&format!("unit/y{n}"), "agent", &head);
        h.sluice()
            .submit(&sluice_core::SubmitRequest {
                head: sluice_core::SubmitHead::Worktree(format!("unit/y{n}")),
                branch: other.into(),
                after: vec![],
                priority: 0,
                subject: None,
            })
            .unwrap()
            .unwrap();
    }
    let s = h.sluice();
    for _ in 0..3 {
        s.step(BRANCH).unwrap();
        s.step(other).unwrap();
    }
    h.w().live_runs()
}

/// satisfies: 71
#[test]
fn every_instance_on_the_host_shares_its_bound() {
    // Rounds of either branch hold each run's part of the host's bound
    // (rule core.49): two runs across both.
    assert_eq!(live_runs_of_two_branches(), 2);
}

/// satisfies: 71; session-runs-1 89
#[test]
fn session_runs_hold_the_bound() {
    let h = Harness::bolt();
    h.w().host_limit(2);
    h.w().session_run("/work/a", true);
    h.w().session_run("/work/b", true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    let s = h.sluice();
    s.step(BRANCH).unwrap();
    s.step(BRANCH).unwrap();
    assert!(h.w().started().is_empty(), "started into a full bound");
    // A session run whose process is gone holds nothing.
    h.w().kill_session("/work/a");
    s.step(BRANCH).unwrap();
    assert_eq!(h.w().started().len(), 1);
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: 71; session-runs-1 89, 91
#[test]
fn session_runs_never_take_the_reserve() {
    let h = Harness::bolt();
    h.w().host_limit(4);
    let s = h.sluice();
    assert_eq!(s.session_capacity(1).unwrap(), 3);
    assert_eq!(s.session_capacity(0).unwrap(), 4);
    for n in 0..3 {
        h.w().session_run(&format!("/work/{n}"), true);
    }
    assert_eq!(s.session_capacity(1).unwrap(), 0);
    // A submission's run still starts into the reserve.
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    h.step();
    assert_eq!(h.w().live_runs(), 1);
    // And a candidate under test on any branch counts against sessions.
    h.w().kill_session("/work/0");
    assert_eq!(s.session_capacity(1).unwrap(), 0);
    assert_eq!(s.session_capacity(0).unwrap(), 1);
}

/// satisfies: 16, 38; session-runs-1 92
#[test]
fn a_session_run_records_what_it_ran_and_never_a_replay() {
    use sluice_seams::{Enforced, Outcome, RunReport, Source, TaskResult};
    let h = Harness::bolt();
    let s = h.sluice();
    let plan = s.session_plan(BRANCH).unwrap().unwrap();
    let head = h.w().tip(BRANCH);
    let task = |name: &str, source: Source| TaskResult {
        took_ms: None,
        attempts: None,
        task: name.into(),
        hash: format!("{name}-hash"),
        outcome: Outcome::Pass,
        source,
    };
    let report = RunReport {
        detail: None,
        kept_out: Vec::new(),
        tasks: vec![
            task(
                "a:test",
                Source::Ran {
                    host: "here".into(),
                },
            ),
            task("b:test", Source::Replayed { from_run_at: None }),
        ],
        executor: Enforced {
            kind: "sandbox".into(),
            confined: false,
            processors: None,
            memory: None,
            network: None,
            docker: false,
            grant: None,
        },
        missing: vec![],
        not_run: vec![],
        dependencies: Vec::new(),
        failed_by: Vec::new(),
        timed_out: false,
        refusals_unread: None,
    };
    assert_eq!(s.record_session(&plan, &head, "/w/a", &report).unwrap(), 1);
    // The same run again is never recorded twice.
    assert_eq!(s.record_session(&plan, &head, "/w/a", &report).unwrap(), 0);
    let r = results(&h);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].task, "a:test");
    assert_eq!(r[0].kind, ResultKind::Ran);
    let e = r[0].executor.as_ref().unwrap();
    assert_eq!((e.kind.as_str(), e.confined), ("sandbox", false));
    assert_eq!(r[0].worktree.as_deref(), Some("/w/a"));
}

/// satisfies: 84, 16
#[test]
fn every_observed_result_records_what_its_executor_enforced() {
    shared::every_observed_result_records_what_its_executor_enforced(&Harness::bolt());
}

/// satisfies: 83, 84, 114: the run is refused before anything of it
/// runs, and recorded with no result, naming the limit
#[test]
fn a_limit_the_executor_cannot_hold_refuses_the_run() {
    let h = Harness::new(&format!(
        "{}\n[executor]\nmemory = \"1G\"\n",
        policy(false, "fail", 4)
    ));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    let said: Vec<String> = h
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Lost { not_run, .. } => Some(not_run.into_iter().map(|n| n.reason)),
            _ => None,
        })
        .flatten()
        .collect();
    assert!(
        !said.is_empty() && said.iter().all(|w| w.contains("memory")),
        "{said:?}"
    );
    assert!(h.w().started().is_empty());
    assert!(results(&h).is_empty());
    assert_ne!(h.sub(one).state, State::Landed);
}

/// satisfies: 15, 55
#[test]
fn a_required_check_the_run_left_without_a_result_is_no_pass() {
    let h = Harness::bolt();
    h.w().skips("b:test");
    h.work("unit/a", "agent-a", &[("b/lib", Some("b2"))]);
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Failed);
    assert!(results(&h).iter().all(|r| r.task != "b:test"));
    match &h.w().delivered()[0].decided {
        Decided::Failed { failed, .. } => assert_eq!(failed, &vec!["b:test".to_string()]),
        other => panic!("delivered {other:?}"),
    }
}

/// satisfies: 15, 27 (ADR 0044)
#[test]
fn a_required_check_kept_out_of_ci_is_missing_and_named() {
    let h = Harness::bolt();
    h.work("unit/a", "agent-a", &[("b/lib", Some("b2"))]);
    h.w().at_path("unit/a", "/work/unit-a");
    let one = h.submit("unit/a");
    // Kept out only once the submission is taken, as an entry ahead of it
    // changing the runner's settings would.
    h.w().keeps_out("b:test");
    h.settle();
    assert_eq!(h.sub(one).state, State::Failed);
    assert!(results(&h).iter().all(|r| r.task != "b:test"));
    let status = h.sluice().status(BRANCH).unwrap();
    assert!(
        status
            .warnings
            .iter()
            .any(|w| w.contains("b:test") && w.contains("keeps out of CI")),
        "{:?}",
        status.warnings
    );
}

/// satisfies: 2, 27, 56 (ADR 0050)
#[test]
fn a_submission_whose_gate_requires_a_check_kept_out_of_ci_is_refused() {
    let h = Harness::bolt();
    h.w().keeps_out("b:test");
    // Refused whatever the submission affects: the policy is what is wrong.
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.w().at_path("unit/a", "/work/unit-a");
    match h.try_submit("unit/a", &[], 0) {
        Err(SubmitRefusal::KeptOut(kept)) => assert_eq!(
            kept,
            [KeptOut {
                task: "b:test".into(),
                entry: "#gate".into(),
                line: 7,
            }]
        ),
        other => panic!("submitted: {other:?}"),
    }
    // Asked in the submitting worktree; nothing queued, nothing published.
    assert_eq!(h.w().asked_kept_out(), [PathBuf::from("/work/unit-a")]);
    assert_eq!(h.version(), 0);
    assert!(h.w().published().is_empty());
    assert!(h.sluice().status(BRANCH).unwrap().submissions.is_empty());
}

/// satisfies: 2, 27, 56 (ADR 0050)
#[test]
fn a_submission_that_lets_the_check_run_is_taken() {
    let h = Harness::bolt();
    h.w().keeps_out("b:test");
    h.work("unit/a", "agent-a", &[("b/lib", Some("b2"))]);
    h.w().at_path("unit/a", "/work/unit-a");
    assert!(matches!(
        h.try_submit("unit/a", &[], 0),
        Err(SubmitRefusal::KeptOut(_))
    ));
    // The worktree's head now lets it run in CI.
    h.w().lets_run("b:test");
    let accepted = h.try_submit("unit/a", &[], 0).expect("accepted");
    assert!(accepted.warnings.is_empty(), "{:?}", accepted.warnings);
    h.settle();
    assert_eq!(h.sub(accepted.id).state, State::Landed);
    assert!(results(&h).iter().any(|r| r.task == "b:test"));
}

/// satisfies: 27 (ADR 0050)
#[test]
fn a_runner_that_cannot_be_asked_takes_the_submission_with_a_warning() {
    let h = Harness::bolt();
    h.w()
        .kept_out_fails(Some("[tests] toolchain exited 1: no devenv"));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.w().at_path("unit/a", "/work/unit-a");
    let accepted = h.try_submit("unit/a", &[], 0).expect("accepted");
    match accepted.warnings.as_slice() {
        [w] => assert!(
            w.contains("could not check") && w.contains("no devenv"),
            "{w}"
        ),
        other => panic!("warned {other:?}"),
    }
    assert_eq!(h.sub(accepted.id).state, State::Queued);
}

/// satisfies: 27, 29
#[test]
fn a_gate_no_run_of_its_executor_can_satisfy_starts_nothing() {
    let strict = policy(false, "fail", 4).replace(
        "trust = [\"open\", \"sandbox\", \"container\"]",
        "trust = [\"confined\"]",
    );
    let h = Harness::new(&strict);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let id = h.submit("unit/a");
    let tip = h.w().tip(BRANCH);
    for _ in 0..3 {
        let err = h.sluice().step(BRANCH).unwrap_err().to_string();
        assert!(err.contains("trusts"), "{err}");
    }
    // Nothing ran, nobody was blamed, nothing landed.
    assert!(h.w().requests().is_empty());
    assert!(h.w().observed().is_empty());
    assert!(!matches!(h.sub(id).state, State::Failed | State::Landed));
    assert_eq!(h.w().tip(BRANCH), tip);
}

/// satisfies: 108, 16, 38; S42
#[test]
fn a_replay_no_trusted_pass_proves_runs_again_with_no_replay() {
    let h = Harness::bolt();
    let other = "bolt/y";
    // Runs on bolt/y are held to a container engine, so the passes the
    // first branch's runs recorded, held to none, prove none of its keys
    // (19, 83).
    let docker = format!("{}\n[executor]\ndocker = true\n", policy(false, "fail", 4));
    h.w().init(
        other,
        &[
            (".config/sluice.toml", &docker),
            ("a/lib", "a1"),
            ("b/lib", "b1"),
            ("c/lib", "c1"),
        ],
    );
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    let ran = h.w().observed().len();

    // The same work on bolt/y: the runner replays every key from its
    // cache, no trusted pass proves them, and the checks run again.
    let head = h.w().work(&h.w().tip(other), &[("a/lib", Some("a2"))]);
    h.w().session("unit/y", "agent-y", &head);
    let s = h.sluice();
    let two = s
        .submit(&sluice_core::SubmitRequest {
            head: sluice_core::SubmitHead::Worktree("unit/y".into()),
            branch: other.into(),
            after: vec![],
            priority: 0,
            subject: None,
        })
        .unwrap()
        .unwrap()
        .id;
    for _ in 0..8 {
        s.step(other).unwrap();
    }
    assert_eq!(h.sub(two).state, State::Landed);
    let requests = h.w().requests();
    assert!(requests.iter().any(|r| r.replay));
    assert!(requests.iter().any(|r| !r.replay));
    assert_eq!(h.w().observed().len(), ran + 3);
    let snap = s.seams().state.read(other).unwrap();
    let events: Vec<Event> = sluice_core::line::Line::read_log(&snap)
        .unwrap()
        .into_iter()
        .map(|l| l.event)
        .collect();
    let (tasks, keys) = events
        .iter()
        .find_map(|e| match e {
            Event::Unproven { tasks, keys, .. } => Some((tasks.clone(), keys.clone())),
            _ => None,
        })
        .expect("an unproven line");
    assert_eq!(tasks.len(), 3);
    // The first attempt's replays are recorded as replays (core.9), and
    // the runs as runs.
    let r = results(&h);
    let mine: Vec<&ResultLine> = r.iter().filter(|l| l.branch == other).collect();
    let replayed: Vec<&&ResultLine> = mine
        .iter()
        .filter(|l| l.kind == ResultKind::Replayed)
        .collect();
    assert_eq!(replayed.len(), 3);
    assert_eq!(mine.iter().filter(|l| l.kind == ResultKind::Ran).count(), 3);
    // Design D4 of `a-signed-run-without-a-sandbox-proves-its-passes`, the
    // run-signing spec's "The unproven line": each replay is named under
    // `keys` with the key the run gave it, and why.
    let mut named: Vec<(String, String, String)> = keys
        .iter()
        .map(|k| (k.task.clone(), k.key.clone(), k.why.clone()))
        .collect();
    named.sort();
    let mut given: Vec<(String, String, String)> = replayed
        .iter()
        .map(|l| {
            (
                l.task.clone(),
                l.hash.clone(),
                "no result of its key was held to this run's limits".to_string(),
            )
        })
        .collect();
    given.sort();
    assert_eq!(named, given);
    let mut named_tasks = tasks.clone();
    named_tasks.sort();
    assert_eq!(
        named_tasks,
        given.iter().map(|(t, _, _)| t.clone()).collect::<Vec<_>>()
    );
}

/// A policy naming two systems, its bolts' rules ending in `extra`.
fn systems_policy(extra: &str) -> String {
    format!(
        "[tests]\nrunner = \"fake\"\n\n[checkout]\n\n\
         [systems]\nlinux-x64 = {{ os = \"linux\", arch = \"x86_64\" }}\n\
         macos-arm64 = {{ os = \"macos\", arch = \"aarch64\" }}\n\n\
         [branches.\"bolt/*\"]\nrequired = [\"a:test\", \"b:test\"]\nparallel = 4\n\
         timeout = \"20m\"\n{extra}"
    )
}

fn runs_of(h: &Harness) -> Vec<sluice_core::records::SystemRun> {
    let s = h.sluice();
    let snap = s.seams().state.read(BRANCH).unwrap();
    let line = sluice_core::line::Line::load(BRANCH, snap).unwrap();
    line.doc.candidates[0].runs.clone()
}

/// satisfies: 86; the check-systems spec's "Runs of one combination"
#[test]
fn a_combination_opens_one_run_per_system() {
    let h = Harness::new(&systems_policy(
        "systems = [\"linux-x64\"]\n\n[branches.\"bolt/*\".systems_for]\n\
         \"b:test\" = [\"linux-x64\", \"macos-arm64\"]\n",
    ));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    h.settle();
    let runs = runs_of(&h);
    let shape: Vec<(Option<&str>, Vec<&str>)> = runs
        .iter()
        .map(|r| {
            (
                r.system.as_deref(),
                r.checks.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        shape,
        [
            (Some("linux-x64"), vec!["a:test", "b:test"]),
            (Some("macos-arm64"), vec!["b:test"])
        ]
    );
    // This host is linux-x64: it runs that run alone, and the other stays
    // open for an executor of its system (87).
    assert_eq!(runs[0].state, sluice_core::records::RunState::Passed);
    assert_eq!(runs[1].state, sluice_core::records::RunState::Open);
    let req = &h.w().requests()[0];
    assert_eq!(req.system.as_deref(), Some("linux-x64"));
    assert_eq!(req.required, ["a:test", "b:test"]);
}

/// satisfies: 85, 86, 87, 66; S41's half with no executor; the
/// check-systems spec's "Nobody runs the system"
#[test]
fn a_run_no_executor_takes_reads_as_not_passed_at_the_time_limit() {
    let h = Harness::new(&systems_policy(
        "systems = [\"linux-x64\", \"macos-arm64\"]\n",
    ));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.settle();
    // A pass on linux-x64 stands for nothing on macos-arm64.
    assert_eq!(h.sub(one).state, State::Testing);
    let s = h.sluice();
    h.w().advance(chrono::Duration::minutes(21));
    for _ in 0..4 {
        s.step(BRANCH).unwrap();
    }
    let sub = h.sub(one);
    assert_eq!(sub.state, State::Failed);
    assert_eq!(sub.result.as_deref(), Some("timed out"));
    let missing: Vec<(&str, Option<&str>)> = sub
        .failed
        .iter()
        .map(|f| (f.task.as_str(), f.system.as_deref()))
        .collect();
    assert_eq!(
        missing,
        [
            ("a:test", Some("macos-arm64")),
            ("b:test", Some("macos-arm64"))
        ]
    );
    // Each result names the system it ran on.
    assert!(results(&h)
        .iter()
        .all(|l| l.system.as_deref() == Some("linux-x64")));
}

/// satisfies: 86; the check-systems spec's "No systems named"
#[test]
fn a_check_with_no_system_named_takes_a_pass_from_the_host_it_ran_on() {
    let h = Harness::new(&systems_policy(""));
    h.w().platform("macos", "aarch64");
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.w().requests()[0].system, None);
    assert!(results(&h)
        .iter()
        .all(|l| l.system.as_deref() == Some("macos-arm64")));
}

/// satisfies: 19, 138; the check-grant spec's "Through the queue": a
/// submission blamed for `a:test`'s fail under one grant, submitted again
/// where its host's runs are under another, where `a:test` passes, lands
/// with no flake named on a branch with `flaky = "fail"` (rule core.34)
#[test]
fn a_fail_under_another_grant_is_never_weighed_against_a_pass() {
    let h = Harness::new(&policy(false, "fail", 4));
    h.w().flaky("a:test", &[Outcome::Fail, Outcome::Pass]);
    h.w().grant(Some("g1"));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let first = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(first).state, State::Failed);

    h.w().grant(Some("g2"));
    let again = h.submit("unit/a");
    h.settle();
    let sub = h.sub(again);
    assert_eq!(sub.state, State::Landed, "{sub:?}");
    assert!(sub.failed.iter().all(|f| f.flaky.is_none()), "{sub:?}");
    // One key, its fail and its pass each under its own grant.
    let a: Vec<(Outcome, Option<String>, String)> = results(&h)
        .into_iter()
        .filter(|l| l.task == "a:test")
        .map(|l| (l.outcome, l.executor.and_then(|e| e.grant), l.hash))
        .collect();
    assert_eq!(a.len(), 2, "{a:?}");
    assert_eq!(a[0].2, a[1].2, "one key");
    assert_eq!(
        (a[0].0, a[0].1.as_deref(), a[1].0, a[1].1.as_deref()),
        (Outcome::Fail, Some("g1"), Outcome::Pass, Some("g2"))
    );
}
