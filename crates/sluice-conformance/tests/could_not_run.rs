//! A run that did not judge its check: no outcome, nothing under its
//! key, and the check runs again (60; could-not-run-1 in agentplot/
//! blueprints, 114 to 116, amended 84, S43, S44).

use sluice_conformance::{policy, Harness, Rig, BRANCH};
use sluice_core::records::{Event, ResultLine, State};
use sluice_seams::{Decided, Outcome, Progress, RunOutcome, RunReport};

fn results(h: &Harness) -> Vec<ResultLine> {
    sluice_core::records::parse_lines(h.w().results_log().as_bytes()).unwrap()
}

/// The checks each lost run left not run, in order.
fn not_run_events(h: &Harness) -> Vec<Vec<(String, String)>> {
    h.events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Lost { not_run, .. } if !not_run.is_empty() => {
                Some(not_run.into_iter().map(|n| (n.task, n.reason)).collect())
            }
            _ => None,
        })
        .collect()
}

/// The bolt policy with `[tests]` holding `line`.
fn with_tests(line: &str) -> String {
    policy(false, "fail", 4).replace(
        "runner = \"fake\"\n",
        &format!("runner = \"fake\"\n{line}\n"),
    )
}

/// What a `[tests] not_run` exit says of a check that could not run.
const NOT_RUN: &str = "exited 77, which [tests] not_run names: it could not run here";

/// satisfies: could-not-run-1 114, 60, 18
#[test]
fn a_check_that_could_not_run_records_nothing_and_runs_again() {
    let h = Harness::bolt();
    h.w().cannot_run("a:test", Some("the engine is down"));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    for _ in 0..8 {
        if !not_run_events(&h).is_empty() {
            break;
        }
        h.step();
    }
    assert_eq!(
        not_run_events(&h),
        [vec![(
            "a:test".to_string(),
            "the engine is down".to_string()
        )]]
    );
    assert!(results(&h).iter().all(|r| r.task != "a:test"));
    assert_ne!(h.sub(one).state, State::Failed);
    assert!(h.w().delivered().is_empty(), "nobody is blamed");

    h.w().cannot_run("a:test", None);
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert!(results(&h)
        .iter()
        .any(|r| r.task == "a:test" && r.outcome == Outcome::Pass));
}

/// satisfies: could-not-run-1 115, 17, 20
#[test]
fn checks_that_could_not_run_three_runs_in_a_row_are_blamed_with_why() {
    let h = Harness::bolt();
    h.w().cannot_run("a:test", Some("the engine is down"));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Failed);
    assert_eq!(h.w().started().len(), 3);
    assert_eq!(not_run_events(&h).len(), 2);
    let failed = &h.sub(one).failed;
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert_eq!(failed[0].task, "a:test");
    assert!(failed[0].hash.is_empty(), "no key: it has no result");
    assert_eq!(failed[0].not_run.as_deref(), Some("the engine is down"));
    match &h.delivered_to("agent-a")[0].decided {
        Decided::Failed { failed, .. } => assert_eq!(failed, &vec!["a:test".to_string()]),
        other => panic!("delivered {other:?}"),
    }
    assert!(results(&h).iter().all(|r| r.task != "a:test"));
}

/// satisfies: could-not-run-1 114, 17
#[test]
fn a_check_that_failed_beside_one_that_could_not_run_fails_as_before() {
    let h = Harness::bolt();
    h.fails_with("a:test", "a/lib", "broken");
    h.w().cannot_run("b:test", Some("the engine is down"));
    h.work(
        "unit/a",
        "agent-a",
        &[("a/lib", Some("broken")), ("b/lib", Some("b2"))],
    );
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Failed);
    assert_eq!(h.w().started().len(), 1, "a fail of its own is a verdict");
    let failed = &h.sub(one).failed;
    let a = failed.iter().find(|f| f.task == "a:test").unwrap();
    assert!(!a.hash.is_empty() && a.not_run.is_none());
    let b = failed.iter().find(|f| f.task == "b:test").unwrap();
    assert_eq!(b.not_run.as_deref(), Some("the engine is down"));
    assert!(results(&h).iter().all(|r| r.task != "b:test"));
}

/// satisfies: could-not-run-1 114, S43, session-runs-1 92
#[test]
fn s43_a_session_run_whose_check_could_not_run_records_only_what_ran() {
    let h = Harness::bolt();
    let head = h.work(
        "unit/a",
        "agent-a",
        &[("a/lib", Some("a2")), ("b/lib", Some("b2"))],
    );
    h.w().cannot_run("b:test", Some(NOT_RUN));
    let (report, _) = h.session_run("unit/a");
    assert_eq!(report.not_run.len(), 1);
    assert_eq!(report.not_run[0].task, "b:test");
    let mine = |h: &Harness| -> Vec<ResultLine> {
        results(h)
            .into_iter()
            .filter(|r| r.combination == head)
            .collect()
    };
    assert!(mine(&h).iter().any(|r| r.task == "a:test"));
    assert!(mine(&h).iter().all(|r| r.task != "b:test"));
    let places = h.sluice().session_places().unwrap();
    let last = places
        .iter()
        .find_map(|p| p.last_result.clone())
        .expect("the run's last result is kept");
    assert_eq!(last.outcome, RunOutcome::NotRun);

    // The next run of the same head passes, and the key reads passed.
    h.w().cannot_run("b:test", None);
    let (report, _) = h.session_run("unit/a");
    assert!(report.not_run.is_empty());
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: could-not-run-1 114, session-runs-1 89
#[test]
fn a_command_run_is_held_to_no_not_run_exit() {
    let h = Harness::new(&with_tests("not_run = [77]"));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let (report, _) = h.session_exec("unit/a", Some("true"));
    assert!(report.not_run.is_empty());
    let asked = h.requests().pop().expect("the command ran");
    assert!(!asked.tests.contains_key("not_run"));
}

/// satisfies: could-not-run-1 84 (amended 2026-10-30), 114; run-outcome
/// "A combination on a host without what a test needs"
#[test]
fn a_policy_naming_ready_is_tested_once_and_lands() {
    let h = Harness::new(&with_tests("ready = \"false\""));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.w().started().len(), 1, "no start was refused");
}

/// satisfies: could-not-run-1 116, S44, 19, 69; rule core.7
#[test]
fn s44_a_fail_set_aside_is_read_by_no_gate_and_its_line_stays() {
    let h = Harness::bolt();
    h.w().flaky("a:test", &[Outcome::Fail, Outcome::Pass]);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Failed);
    let two = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(two).state, State::Failed, "the key reads flaky");
    assert!(h.sub(two).failed.iter().any(|f| f.flaky.is_some()));

    let fail = results(&h)
        .into_iter()
        .find(|r| r.task == "a:test" && r.outcome == Outcome::Fail)
        .unwrap();
    let s = h.sluice();
    assert!(s
        .set_aside("a:test", &fail.at, None, " ", "op@host")
        .is_err());
    let aside = s
        .set_aside("a:test", &fail.at, None, "the engine was down", "op@host")
        .unwrap();
    assert_eq!(aside.hash, fail.hash);
    assert_eq!(aside.reason, "the engine was down");
    let again = s.set_aside("a:test", &fail.at, None, "twice", "op@host");
    assert!(again.is_err(), "a result is set aside once");

    let three = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(three).state, State::Landed);
    assert!(results(&h).contains(&fail), "the fail's line stays");
}

/// The bolt policy, its runs asking for the container engine (84).
fn docker() -> String {
    policy(false, "fail", 4) + "\n[executor]\ndocker = true\n"
}

/// What the fake's look at a host whose engine has gone says.
const GONE: &str = "no container engine answers: /var/run/docker.sock refused the connection";

/// What a check that did not pass after the engine went is left with.
fn engine_gone() -> String {
    format!("after the run, {GONE}")
}

/// satisfies: 114, 139; record 0074 (design D5); the host-requirements
/// spec's "The engine stopped during the run" and "The engine still
/// answers"
#[test]
fn a_check_failed_after_the_engine_went_has_no_outcome_and_a_pass_stands() {
    let h = Harness::new(&docker());
    h.w().hold_runs(true);
    h.fails_with("a:test", "a/lib", "broken");
    let head = h.work(
        "unit/a",
        "agent-a",
        &[("a/lib", Some("broken")), ("b/lib", Some("b2"))],
    );
    let one = h.submit("unit/a");
    h.step();
    assert_eq!(h.w().started().len(), 1);
    h.w().engine_unmet(Some(GONE));
    h.w().complete_all();
    h.step();
    assert_eq!(
        not_run_events(&h),
        [vec![("a:test".to_string(), engine_gone())]]
    );
    let c = h.w().started()[0].clone();
    let mine: Vec<ResultLine> = results(&h)
        .into_iter()
        .filter(|r| r.combination == c)
        .collect();
    assert!(mine
        .iter()
        .any(|r| r.task == "b:test" && r.outcome == Outcome::Pass));
    assert!(mine.iter().all(|r| r.task != "a:test"), "{mine:?}");
    assert_ne!(h.sub(one).state, State::Failed);
    assert!(h.w().delivered().is_empty(), "nobody is blamed");

    // The engine answers again: the combination is tested again, and its
    // fail, after which the engine still answers, is recorded and blamed.
    h.w().engine_unmet(None);
    h.w().hold_runs(false);
    h.settle();
    assert_eq!(h.w().started().len(), 2);
    assert_eq!(h.sub(one).state, State::Failed);
    let a = h
        .sub(one)
        .failed
        .into_iter()
        .find(|f| f.task == "a:test")
        .unwrap();
    assert!(!a.hash.is_empty() && a.not_run.is_none(), "{a:?}");
    assert!(results(&h)
        .iter()
        .any(|r| r.task == "a:test" && r.outcome == Outcome::Fail));
    assert_ne!(head, c);
}

/// satisfies: record 0074 (design D5); the host-requirements spec's
/// "Every test passed"
#[test]
fn a_run_whose_checks_all_passed_never_looks_at_the_engine() {
    let h = Harness::new(&docker());
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    let looked = h.w().engine_looks().len();
    h.w().engine_unmet(Some(GONE));
    h.w().complete_all();
    h.step();
    assert_eq!(h.w().engine_looks().len(), looked, "no look after a pass");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: 66; record 0074 (design D5); the host-requirements spec's
/// "The time limit ended the run"
#[test]
fn a_run_its_time_limit_ended_never_looks_at_the_engine() {
    let h = Harness::new(&docker());
    h.w().hold_runs(true);
    h.w().hangs("a:test");
    h.work("unit/a", "agent-a", &[("a/lib", Some("slow"))]);
    let one = h.submit("unit/a");
    h.step();
    let looked = h.w().engine_looks().len();
    h.w().engine_unmet(Some(GONE));
    h.w().complete_all();
    h.step();
    assert_eq!(h.w().engine_looks().len(), looked, "no look after a cut");
    assert_eq!(h.sub(one).state, State::Failed);
    assert_eq!(h.sub(one).result.as_deref(), Some("timed out"));
}

/// satisfies: 92, 114, 139; record 0074 (design D5, D6); the
/// host-requirements spec's "A command runs"
#[test]
fn a_session_run_failed_after_the_engine_went_records_no_fail() {
    let h = Harness::new(&docker());
    h.fails_with("a:test", "a/lib", "broken");
    let head = h.work(
        "unit/a",
        "agent-a",
        &[("a/lib", Some("broken")), ("b/lib", Some("b2"))],
    );
    h.w().engine_unmet(Some(GONE));
    let (report, _) = h.session_run("unit/a");
    assert_eq!(report.not_run.len(), 1, "{report:?}");
    assert_eq!(report.not_run[0].task, "a:test");
    assert_eq!(report.not_run[0].reason, engine_gone());
    let mine: Vec<ResultLine> = results(&h)
        .into_iter()
        .filter(|r| r.combination == head)
        .collect();
    assert!(mine.iter().any(|r| r.task == "b:test"));
    assert!(mine.iter().all(|r| r.task != "a:test"), "{mine:?}");

    // A command's fail is its own, whatever the host lacks.
    let looked = h.w().engine_looks().len();
    h.fails_with("false", "a/lib", "broken");
    let (report, _) = h.session_exec("unit/a", Some("false"));
    assert!(report.not_run.is_empty(), "{report:?}");
    assert_eq!(
        report
            .tasks
            .iter()
            .map(|t| (t.task.as_str(), t.outcome))
            .collect::<Vec<_>>(),
        [("false", Outcome::Fail)]
    );
    assert_eq!(h.w().engine_looks().len(), looked, "never looked at");
}

/// The systems a Nix-managed host runs, as the link Nix keeps for the
/// active system names them (record 0097).
const AAA: &str = "/nix/store/aaa-darwin-system";
const BBB: &str = "/nix/store/bbb-darwin-system";

/// What a check that did not pass in a run during which the host's system
/// moved from `from` to `to` is left with (record 0097, design D5).
fn switched(from: &str, to: &str) -> String {
    format!("the host's system was switched during the run, from {from} to {to}")
}

/// Steps until `n` runs have started.
fn until_started(h: &Harness, n: usize) {
    for _ in 0..8 {
        if h.w().started().len() >= n {
            break;
        }
        h.step();
    }
    assert_eq!(h.w().started().len(), n, "runs started");
}

/// `a:test`, which fails, and `b:test`, which passes, submitted from
/// `unit/a` as one held run starts, on a host that runs the system
/// [`AAA`]; the switch to [`BBB`] and the run's end are the caller's.
fn one_run_on_aaa(h: &Harness) -> u64 {
    h.w().nix_system(Some(AAA));
    h.w().hold_runs(true);
    h.fails_with("a:test", "a/lib", "broken");
    h.work(
        "unit/a",
        "agent-a",
        &[("a/lib", Some("broken")), ("b/lib", Some("b2"))],
    );
    let one = h.submit("unit/a");
    until_started(h, 1);
    one
}

/// The first run's switch to [`BBB`] and its end, read.
fn switch_and_end(h: &Harness) {
    h.w().nix_system(Some(BBB));
    h.w().complete_all();
    h.step();
}

/// The first combination's results.
fn first_results(h: &Harness) -> Vec<ResultLine> {
    let c = h.w().started()[0].clone();
    results(h)
        .into_iter()
        .filter(|r| r.combination == c)
        .collect()
}

/// satisfies: 84, 114, 139; record 0097 (design D3, D5); the
/// host-requirements spec's "The system was switched during the run" and
/// "The system stood"
#[test]
fn a_check_failed_after_the_hosts_system_was_switched_has_no_outcome_and_a_pass_stands() {
    let h = Harness::new(&docker());
    let one = one_run_on_aaa(&h);
    switch_and_end(&h);
    assert_eq!(
        not_run_events(&h),
        [vec![("a:test".to_string(), switched(AAA, BBB))]]
    );
    let mine = first_results(&h);
    assert!(mine
        .iter()
        .any(|r| r.task == "b:test" && r.outcome == Outcome::Pass));
    assert!(mine.iter().all(|r| r.task != "a:test"), "{mine:?}");
    assert_ne!(h.sub(one).state, State::Failed);
    assert!(h.w().delivered().is_empty(), "nobody is blamed");

    // The system stands: the combination is tested again, and its fail is
    // recorded and blamed.
    h.w().hold_runs(false);
    h.w().complete_all();
    h.settle();
    assert_eq!(h.w().started().len(), 2);
    assert_eq!(h.sub(one).state, State::Failed);
    let a = h
        .sub(one)
        .failed
        .into_iter()
        .find(|f| f.task == "a:test")
        .unwrap();
    assert!(!a.hash.is_empty() && a.not_run.is_none(), "{a:?}");
    assert!(results(&h)
        .iter()
        .any(|r| r.task == "a:test" && r.outcome == Outcome::Fail));
    assert_eq!(h.delivered_to("agent-a").len(), 1, "blamed");
}

/// satisfies: record 0097 (design D3); the host-requirements spec's
/// "Every test passed"
#[test]
fn a_run_whose_checks_all_passed_never_reads_the_system() {
    let h = Harness::bolt();
    h.w().nix_system(Some(AAA));
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    until_started(&h, 1);
    let looked = h.w().system_looks().len();
    switch_and_end(&h);
    assert_eq!(h.w().system_looks().len(), looked, "no read after a pass");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: 66; record 0097 (design D3); the host-requirements spec's
/// "The time limit ended the run"
#[test]
fn a_run_its_time_limit_ended_never_reads_the_system() {
    let h = Harness::bolt();
    h.w().nix_system(Some(AAA));
    h.w().hold_runs(true);
    h.w().hangs("a:test");
    h.work("unit/a", "agent-a", &[("a/lib", Some("slow"))]);
    let one = h.submit("unit/a");
    until_started(&h, 1);
    let looked = h.w().system_looks().len();
    switch_and_end(&h);
    assert_eq!(h.w().system_looks().len(), looked, "no read after a cut");
    assert_eq!(h.sub(one).state, State::Failed);
    assert_eq!(h.sub(one).result.as_deref(), Some("timed out"));
}

/// satisfies: 84, 114; record 0097 (design D3); the host-requirements
/// spec's "A policy that asks no engine"
#[test]
fn a_switch_is_read_whatever_the_policy_asks_of_the_host() {
    let h = Harness::bolt();
    let one = one_run_on_aaa(&h);
    switch_and_end(&h);
    assert_eq!(
        not_run_events(&h),
        [vec![("a:test".to_string(), switched(AAA, BBB))]]
    );
    assert!(first_results(&h).iter().all(|r| r.task != "a:test"));
    assert_ne!(h.sub(one).state, State::Failed);
    assert!(h.w().delivered().is_empty(), "nobody is blamed");
    assert!(h.w().engine_looks().is_empty(), "no engine asked");
}

/// satisfies: 84, 114, 139; record 0097 (design D3); the
/// host-requirements spec's "The engine gone too"
#[test]
fn a_switch_and_an_engine_gone_name_the_switch() {
    let h = Harness::new(&docker());
    let one = one_run_on_aaa(&h);
    let looked = h.w().engine_looks().len();
    h.w().engine_unmet(Some(GONE));
    switch_and_end(&h);
    assert_eq!(
        not_run_events(&h),
        [vec![("a:test".to_string(), switched(AAA, BBB))]]
    );
    // The one look is the one before the run again is claimed (84),
    // which finds the engine gone and claims nothing; none follows the run.
    assert_eq!(
        h.w().engine_looks().len(),
        looked + 1,
        "the engine not looked at after the run"
    );
    assert_eq!(h.w().started().len(), 1, "no run again while it is gone");
    assert_ne!(h.sub(one).state, State::Failed);
    assert!(h.w().delivered().is_empty(), "nobody is blamed");
}

/// satisfies: 84; record 0097 (design D4); the host-requirements spec's
/// "No Nix system"
#[test]
fn a_host_nix_does_not_manage_reads_nothing_and_blames_as_before() {
    let h = Harness::bolt();
    h.w().nix_system(None);
    h.fails_with("a:test", "a/lib", "broken");
    h.work("unit/a", "agent-a", &[("a/lib", Some("broken"))]);
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.w().started().len(), 1, "a verdict, not a run again");
    assert!(not_run_events(&h).is_empty());
    assert_eq!(h.sub(one).state, State::Failed);
    let failed = &h.sub(one).failed;
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert_eq!(failed[0].task, "a:test");
    assert!(!failed[0].hash.is_empty() && failed[0].not_run.is_none());
    assert!(results(&h)
        .iter()
        .any(|r| r.task == "a:test" && r.outcome == Outcome::Fail));
    assert_eq!(h.delivered_to("agent-a").len(), 1, "blamed");
}

/// satisfies: 34, 84, 114, 123; record 0097 (design D6); the
/// host-requirements spec's "Tested again in a new checkout"
#[test]
fn a_switched_run_is_tested_again_in_a_new_checkout_once_its_own_is_released() {
    let h = Harness::bolt();
    one_run_on_aaa(&h);
    let broken = h.w().live_at_each_start()[0].0.clone();
    switch_and_end(&h);
    assert_eq!(not_run_events(&h).len(), 1);
    until_started(&h, 2);
    let (again, live) = h.w().live_at_each_start()[1].clone();
    assert_ne!(again, broken, "tested again in another checkout");
    assert!(
        !live.contains(&broken),
        "{broken} released before the run again started: {live:?}"
    );
    assert!(h.w().released_places().contains(&broken));
}

/// satisfies: 81, 84, 114, 123; record 0097 (design D6); the
/// host-requirements spec's "The broken run's release fails"
#[test]
fn a_switched_run_whose_release_fails_is_tested_again_in_a_new_checkout() {
    let h = Harness::bolt();
    one_run_on_aaa(&h);
    let broken = h.w().live_at_each_start()[0].0.clone();
    h.w().fail_next_release();
    switch_and_end(&h);
    until_started(&h, 2);
    let held = h.sluice().status(BRANCH).unwrap().failed_releases;
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0].place, broken);
    assert!(held[0].failed.contains("failed"), "{held:?}");
    assert!(h.w().live_places().contains(&broken), "held");
    let again = h.w().live_at_each_start()[1].0.clone();
    assert_ne!(again, broken, "tested again in another checkout");
}

/// satisfies: 84, 114, 115, 17, 20; record 0097; the host-requirements
/// spec's "A host that keeps switching"
#[test]
fn a_host_that_keeps_switching_blames_with_why_after_three_runs() {
    let h = Harness::bolt();
    let one = one_run_on_aaa(&h);
    for (n, to) in [(1, BBB), (2, AAA), (3, BBB)] {
        until_started(&h, n);
        h.w().nix_system(Some(to));
        h.w().complete_all();
        h.step();
    }
    h.w().hold_runs(false);
    h.settle();
    assert_eq!(h.w().started().len(), 3);
    assert_eq!(h.sub(one).state, State::Failed);
    let a = h
        .sub(one)
        .failed
        .into_iter()
        .find(|f| f.task == "a:test")
        .unwrap();
    assert!(a.hash.is_empty(), "no key: it has no result");
    assert_eq!(a.not_run.as_deref(), Some(switched(AAA, BBB).as_str()));
    match &h.delivered_to("agent-a")[0].decided {
        Decided::Failed { failed, .. } => assert!(failed.contains(&"a:test".to_string())),
        other => panic!("delivered {other:?}"),
    }
    assert!(results(&h).iter().all(|r| r.task != "a:test"));
}

/// A session run of the checks, or of `command`, as the harness's own,
/// the host's system switched to `to` while it runs.
fn session_exec_switched(
    h: &Harness,
    worktree: &str,
    command: Option<&str>,
    to: &str,
) -> RunReport {
    let s = h.sluice();
    let place = h.place(worktree);
    let plan = s.session_plan(BRANCH).unwrap().expect("managed");
    let session = s.seams().driver.session(&place).unwrap();
    let started = s
        .start_session(
            &plan,
            &place,
            &session.head,
            command,
            sluice_conformance::shared::session_part(&s),
        )
        .unwrap();
    h.w().nix_system(Some(to));
    let report = match s.seams().tests.progress(&started.run).unwrap() {
        Progress::Finished(r) => r,
        other => panic!("{other:?}"),
    };
    let finished = s
        .finish_session(&plan, &session, &started, command.is_some(), &report)
        .unwrap();
    assert_eq!(finished.unkept, None, "its result is kept");
    finished.report
}

/// satisfies: 89, 92, 114, 139; record 0097 (design D7); the
/// host-requirements spec's "The tests" and "A command"
#[test]
fn a_session_run_failed_after_a_switch_records_no_fail() {
    let h = Harness::bolt();
    h.w().nix_system(Some(AAA));
    h.fails_with("a:test", "a/lib", "broken");
    let head = h.work(
        "unit/a",
        "agent-a",
        &[("a/lib", Some("broken")), ("b/lib", Some("b2"))],
    );
    let report = session_exec_switched(&h, "unit/a", None, BBB);
    assert_eq!(report.not_run.len(), 1, "{report:?}");
    assert_eq!(report.not_run[0].task, "a:test");
    assert_eq!(report.not_run[0].reason, switched(AAA, BBB));
    let mine: Vec<ResultLine> = results(&h)
        .into_iter()
        .filter(|r| r.combination == head)
        .collect();
    assert!(mine.iter().any(|r| r.task == "b:test"));
    assert!(mine.iter().all(|r| r.task != "a:test"), "{mine:?}");

    // A command's fail is its own, whatever became of the host.
    let looked = h.w().system_looks().len();
    h.fails_with("false", "a/lib", "broken");
    let report = session_exec_switched(&h, "unit/a", Some("false"), AAA);
    assert!(report.not_run.is_empty(), "{report:?}");
    assert_eq!(
        report
            .tasks
            .iter()
            .map(|t| (t.task.as_str(), t.outcome))
            .collect::<Vec<_>>(),
        [("false", Outcome::Fail)]
    );
    assert_eq!(h.w().system_looks().len(), looked, "never read");
}

/// What the fake's sandbox reports refusing a run (design D6, D9).
const REFUSED: &str = "sluice's sandbox refused the run network-outbound \
                       /private/var/run/mDNSResponder (a name lookup, by bun)";

/// `a:test`, which fails, and `b:test`, which passes, submitted from
/// `unit/a` as one held run starts; what the run is refused and its end
/// are the caller's.
fn one_held_run(h: &Harness) -> u64 {
    h.w().hold_runs(true);
    h.fails_with("a:test", "a/lib", "broken");
    h.work(
        "unit/a",
        "agent-a",
        &[("a/lib", Some("broken")), ("b/lib", Some("b2"))],
    );
    let one = h.submit("unit/a");
    until_started(h, 1);
    one
}

/// satisfies: 114, 139, 149; record 0072 (design D7); rule core.35; the
/// run-outcome spec's "A refused run" and "No refusal"
#[test]
fn a_check_failed_in_a_refused_run_has_no_outcome_and_a_pass_stands() {
    let h = Harness::bolt();
    let one = one_held_run(&h);
    h.w().refuses(Some(REFUSED));
    h.w().complete_all();
    h.step();
    assert_eq!(
        not_run_events(&h),
        [vec![("a:test".to_string(), REFUSED.to_string())]]
    );
    let mine = first_results(&h);
    assert!(mine
        .iter()
        .any(|r| r.task == "b:test" && r.outcome == Outcome::Pass));
    assert!(mine.iter().all(|r| r.task != "a:test"), "{mine:?}");
    assert_ne!(h.sub(one).state, State::Failed);
    assert!(h.w().delivered().is_empty(), "nobody is blamed");

    // The next run is refused nothing: its fail is recorded and blamed.
    h.w().refuses(None);
    h.w().hold_runs(false);
    h.w().complete_all();
    h.settle();
    assert_eq!(h.w().started().len(), 2, "tested again");
    assert_eq!(h.sub(one).state, State::Failed);
    let a = h
        .sub(one)
        .failed
        .into_iter()
        .find(|f| f.task == "a:test")
        .unwrap();
    assert!(!a.hash.is_empty() && a.not_run.is_none(), "{a:?}");
    assert!(results(&h)
        .iter()
        .any(|r| r.task == "a:test" && r.outcome == Outcome::Fail));
    assert_eq!(h.delivered_to("agent-a").len(), 1, "blamed");
}

/// satisfies: 66, 139; record 0072 (design D7); rule core.35; the
/// run-outcome spec's "A passing run, or one its time limit ended"
#[test]
fn a_run_that_passed_or_timed_out_never_reads_its_refusals() {
    let h = Harness::bolt();
    h.w().refuses(Some(REFUSED));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert!(h.w().refusal_looks().is_empty(), "no read after a pass");

    let h = Harness::bolt();
    h.w().refuses(Some(REFUSED));
    h.w().hold_runs(true);
    h.w().hangs("a:test");
    h.work("unit/a", "agent-a", &[("a/lib", Some("slow"))]);
    let one = h.submit("unit/a");
    until_started(&h, 1);
    h.w().complete_all();
    h.step();
    assert_eq!(h.sub(one).state, State::Failed);
    assert_eq!(h.sub(one).result.as_deref(), Some("timed out"));
    assert!(h.w().refusal_looks().is_empty(), "no read after a cut");
}

/// satisfies: 84, 114, 139; records 0072 and 0097 (design D7); the
/// run-outcome spec's "A switch and a refusal"
#[test]
fn a_switch_and_a_refusal_name_the_switch() {
    let h = Harness::bolt();
    let one = one_run_on_aaa(&h);
    h.w().refuses(Some(REFUSED));
    switch_and_end(&h);
    assert_eq!(
        not_run_events(&h),
        [vec![("a:test".to_string(), switched(AAA, BBB))]]
    );
    assert!(h.w().refusal_looks().is_empty(), "the refusals not read");
    assert_ne!(h.sub(one).state, State::Failed);
    assert!(h.w().delivered().is_empty(), "nobody is blamed");
}

/// satisfies: 84, 114, 139; records 0072 and 0074 (design D7); the
/// run-outcome spec's "A refusal and an engine gone"
#[test]
fn a_refusal_and_an_engine_gone_name_the_refusal() {
    let h = Harness::new(&docker());
    let one = one_held_run(&h);
    let looked = h.w().engine_looks().len();
    h.w().engine_unmet(Some(GONE));
    h.w().refuses(Some(REFUSED));
    h.w().complete_all();
    h.step();
    assert_eq!(
        not_run_events(&h),
        [vec![("a:test".to_string(), REFUSED.to_string())]]
    );
    // The one look is the one before the run again is claimed (84),
    // which finds the engine gone and claims nothing; none follows the run.
    assert_eq!(
        h.w().engine_looks().len(),
        looked + 1,
        "the engine not looked at after the run"
    );
    assert_eq!(h.w().refusal_looks().len(), 1);
    assert_ne!(h.sub(one).state, State::Failed);
    assert!(h.w().delivered().is_empty(), "nobody is blamed");
}

/// satisfies: 89, 92, 114, 139; records 0072 and 0074 (design D7); the
/// run-outcome spec's "A session's run refused something"
#[test]
fn a_session_run_refused_something_records_no_fail() {
    let h = Harness::bolt();
    h.fails_with("a:test", "a/lib", "broken");
    let head = h.work(
        "unit/a",
        "agent-a",
        &[("a/lib", Some("broken")), ("b/lib", Some("b2"))],
    );
    h.w().refuses(Some(REFUSED));
    let (report, _) = h.session_run("unit/a");
    assert_eq!(report.not_run.len(), 1, "{report:?}");
    assert_eq!(report.not_run[0].task, "a:test");
    assert_eq!(report.not_run[0].reason, REFUSED);
    let mine: Vec<ResultLine> = results(&h)
        .into_iter()
        .filter(|r| r.combination == head)
        .collect();
    assert!(mine.iter().any(|r| r.task == "b:test"));
    assert!(mine.iter().all(|r| r.task != "a:test"), "{mine:?}");

    // A command's fail is its own, whatever its executor refused it.
    let looked = h.w().refusal_looks().len();
    h.fails_with("false", "a/lib", "broken");
    let (report, _) = h.session_exec("unit/a", Some("false"));
    assert!(report.not_run.is_empty(), "{report:?}");
    assert_eq!(
        report
            .tasks
            .iter()
            .map(|t| (t.task.as_str(), t.outcome))
            .collect::<Vec<_>>(),
        [("false", Outcome::Fail)]
    );
    assert_eq!(h.w().refusal_looks().len(), looked, "never read");
}

/// satisfies: 114, 115, 17, 20, 139; record 0072 (design D7); the
/// run-outcome spec's "A refusal in every run"
#[test]
fn a_refusal_in_every_run_reads_not_passed_at_the_bound_naming_it() {
    let h = Harness::bolt();
    h.w().refuses(Some(REFUSED));
    h.fails_with("a:test", "a/lib", "broken");
    h.work("unit/a", "agent-a", &[("a/lib", Some("broken"))]);
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.w().started().len(), 3);
    assert_eq!(h.sub(one).state, State::Failed);
    let a = h
        .sub(one)
        .failed
        .into_iter()
        .find(|f| f.task == "a:test")
        .unwrap();
    assert!(a.hash.is_empty(), "no key: it has no result");
    assert_eq!(a.not_run.as_deref(), Some(REFUSED));
    match &h.delivered_to("agent-a")[0].decided {
        Decided::Failed { failed, .. } => assert!(failed.contains(&"a:test".to_string())),
        other => panic!("delivered {other:?}"),
    }
    assert!(results(&h).iter().all(|r| r.task != "a:test"));
}

/// satisfies: 139; record 0072 (design D8); the run-outcome spec's
/// "Through the queue"
#[test]
fn a_run_whose_refusals_could_not_be_read_is_read_as_before_and_says_so() {
    let h = Harness::bolt();
    h.w().refusals_unread(Some("log: exited 1"));
    h.fails_with("a:test", "a/lib", "broken");
    h.work("unit/a", "agent-a", &[("a/lib", Some("broken"))]);
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.w().started().len(), 1, "a verdict, not a run again");
    assert!(not_run_events(&h).is_empty());
    assert_eq!(h.sub(one).state, State::Failed);
    assert!(results(&h)
        .iter()
        .any(|r| r.task == "a:test" && r.outcome == Outcome::Fail));
    assert_eq!(h.delivered_to("agent-a").len(), 1, "blamed");
    let said: Vec<Option<String>> = h
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Detail {
                refusals_unread, ..
            } => Some(refusals_unread),
            _ => None,
        })
        .collect();
    assert_eq!(said, [Some("log: exited 1".to_string())]);
}
