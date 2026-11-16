//! A run's life, written in its branch's home before anything acts on it
//! (record 0122; the run-life spec): its five states, each act's proof,
//! the limit from its own start, follow-up runs, a change of lead, and a
//! pool's run through the same states.
//!
//! Two harnesses: [`signed`], a branch whose records are on its remote and
//! whose policy names `[signers.box]`, as `peers.rs` has it, and [`local`],
//! a branch in the host's own files.

use std::time::Duration;

use sluice_conformance::{policy, Crash, Harness, Rig, BRANCH};
use sluice_core::records::{Candidate, CheckOutcome, Event, RunState, State, SystemRun};
use sluice_core::{Peer, Sluice};

/// A branch whose records are on its remote, trusting the signer `box`.
#[allow(dead_code)]
fn signed() -> Harness {
    signed_with("")
}

/// [`signed`], its policy ending in `more`.
#[allow(dead_code)]
fn signed_with(more: &str) -> Harness {
    let p = policy(false, "fail", 4).replace(
        "trust = [\"open\", \"sandbox\", \"container\"]\n",
        "trust = [\"box\"]\nremote = \"origin\"\n",
    ) + "\n[signers.box]\nssh = \"box ssh-ed25519 AAAA\"\n"
        + more;
    let h = Harness::new(&p);
    h.w().signs_as("box");
    h
}

/// A branch in the host's own files.
fn local() -> Harness {
    Harness::bolt()
}

/// Instance `name` of the rig, on its own host, with a lease of five
/// minutes and a sign of life bound of two.
#[allow(dead_code)]
fn on<'a>(h: &'a Harness, name: &str) -> Sluice<'a> {
    h.w().be_host(name);
    h.sluice().as_peer(Peer {
        name: name.into(),
        lease: Duration::from_secs(300),
        alive: Duration::from_secs(120),
        signer: Some("box".into()),
        systems: None,
        expire_after: sluice_core::EXPIRE_AFTER,
    })
}

/// The branch's candidates, as its records hold them.
fn candidates(h: &Harness) -> Vec<Candidate> {
    h.doc().candidates
}

/// The runs of the candidate holding exactly `ids`.
fn runs_of(h: &Harness, ids: &[u64]) -> Vec<SystemRun> {
    candidates(h)
        .into_iter()
        .find(|c| c.submissions == ids)
        .map(|c| c.runs)
        .unwrap_or_default()
}

/// satisfies: record 0122; rule core.14; the run-life spec's "Nothing acts
/// on a run before the state naming the act is written", a host that stops
/// after starting the checks
#[test]
fn a_run_started_before_its_running_was_written_is_not_started_again() {
    let h = local();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.w().crash(Crash::AfterStart);
    assert!(h.sluice().step(BRANCH).is_err(), "the step stopped");
    let run = &runs_of(&h, &[one])[0];
    assert_eq!(run.state, RunState::Claimed);
    let place = run.place.clone().expect("its checkout named at the claim");
    assert_eq!(h.w().started().len(), 1, "its checks were started");
    let began = h.w().run_starts().pop();

    // The next round, minutes on, finds the supervisor's run.
    h.w().advance(chrono::Duration::minutes(3));
    h.step();
    let run = &runs_of(&h, &[one])[0];
    assert_eq!(run.state, RunState::Running);
    assert_eq!(
        run.running, began,
        "running from the run's own start, not the round's"
    );
    assert!(run.run.is_some());
    assert_eq!(h.w().started().len(), 1, "nothing started again");
    assert_eq!(h.w().made_places(), std::slice::from_ref(&place));
    assert_eq!(h.releases_of(&place), 0, "nothing released");

    h.w().complete_all();
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.w().started().len(), 1);
}

/// satisfies: record 0122; rule core.16; the run-life spec's "Nothing acts
/// on a run before the state naming the act is written", a host that stops
/// after making the checkout
#[test]
fn a_checkout_made_before_the_start_is_used_once() {
    let h = local();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.w().crash(Crash::AfterCheckout);
    assert!(h.sluice().step(BRANCH).is_err(), "the step stopped");
    let run = &runs_of(&h, &[one])[0];
    assert_eq!(run.state, RunState::Claimed);
    let place = run.place.clone().expect("its checkout named at the claim");
    assert_eq!(h.w().made_places(), std::slice::from_ref(&place));
    assert!(h.w().started().is_empty(), "no checks started");

    h.step();
    assert_eq!(h.w().started().len(), 1, "started once, in the checkout");
    assert_eq!(
        h.w().made_places(),
        std::slice::from_ref(&place),
        "no second checkout"
    );
    assert_eq!(h.releases_of(&place), 0);
    assert_eq!(runs_of(&h, &[one])[0].state, RunState::Running);
    h.w().complete_all();
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.releases_of(&place), 1);
    assert_eq!(h.w().made_places(), [place]);
    assert_eq!(h.w().started().len(), 1);
}

/// The run-life lines of the branch's log, in order, as `(word, run)`.
fn life(h: &Harness) -> Vec<(&'static str, Option<usize>)> {
    h.events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Opened { run, .. } => Some(("opened", Some(run))),
            Event::Claimed { .. } => Some(("claimed", None)),
            Event::Running { run, .. } => Some(("running", Some(run))),
            Event::Ended { run, .. } => Some(("ended", Some(run))),
            Event::Closed { run, .. } => Some(("closed", Some(run))),
            _ => None,
        })
        .collect()
}

/// satisfies: record 0122; 75, 88; the run-life spec's "A run is in one of
/// five states, written in its branch's home"
#[test]
fn a_runs_states_are_written_in_order() {
    let h = local();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    // The round that reads the run's end stops before the branch moves, so
    // the run's record is read as it ends, before its landing takes it.
    h.w().crash(Crash::BeforeMove);
    assert!(
        (0..8).any(|_| h.sluice().step(BRANCH).is_err()),
        "the round that lands stopped"
    );
    assert_eq!(
        life(&h),
        [
            ("opened", Some(0)),
            ("claimed", None),
            ("running", Some(0)),
            ("ended", Some(0)),
            ("closed", Some(0)),
        ]
    );
    let run = &runs_of(&h, &[one])[0];
    assert_eq!(run.state, RunState::Closed);
    assert!(run.opened.is_some() && run.running.is_some() && run.closed.is_some());
    let ended = run.ended.as_ref().expect("its report under ended");
    assert_eq!(ended.outcomes.get("#gate"), Some(&CheckOutcome::Passed));
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: record 0122; rule core.7; the run-life spec's "Each act leaves
/// a proof the next reader checks", a host that stops after recording the
/// results
#[test]
fn results_recorded_before_the_end_was_written_are_not_recorded_again() {
    let h = local();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    assert_eq!(runs_of(&h, &[one])[0].state, RunState::Running);
    h.w().complete_all();
    h.w().crash(Crash::AfterResults);
    assert!(h.sluice().step(BRANCH).is_err(), "the step stopped");
    let recorded = h.results();
    assert!(!recorded.is_empty(), "its results were recorded");
    assert_eq!(runs_of(&h, &[one])[0].state, RunState::Running);

    h.step();
    let run = &runs_of(&h, &[one]);
    assert!(
        run.is_empty() || run[0].ended.is_some(),
        "its end written from the same report"
    );
    assert_eq!(h.results(), recorded, "no result recorded twice");
    assert_eq!(life(&h).iter().filter(|(w, _)| *w == "ended").count(), 1);
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.results(), recorded);
}

/// satisfies: record 0122; 123; rule core.16; the run-life spec's "Each act
/// leaves a proof the next reader checks", a host that stops after
/// releasing
#[test]
fn a_release_found_done_is_closed_and_not_run_again() {
    let h = local();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    let place = runs_of(&h, &[one])[0].place.clone().expect("its checkout");
    h.w().complete_all();
    h.w().crash(Crash::AfterRelease);
    assert!(h.sluice().step(BRANCH).is_err(), "the step stopped");
    let run = &runs_of(&h, &[one])[0];
    assert_eq!(run.state, RunState::Ended);
    assert_eq!(h.releases_of(&place), 1);
    assert!(h.w().live_places().is_empty(), "its checkout is gone");

    h.step();
    assert_eq!(h.releases_of(&place), 1, "no release command run again");
    assert_eq!(
        life(&h).iter().filter(|(w, _)| *w == "closed").count(),
        1,
        "{:?}",
        life(&h)
    );
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.releases_of(&place), 1);
}

/// Instance `a`, leading with no room of its own, so other hosts take its
/// runs.
fn lead(h: &Harness) -> Sluice<'_> {
    let s = on(h, "a");
    h.w().no_room();
    s
}

/// The run-life lines a step of `s` adds to the branch's log.
fn written_by(h: &Harness, s: Sluice<'_>) -> Vec<&'static str> {
    let before = life(h).len();
    s.step(BRANCH).expect("a step runs");
    life(h)[before..].iter().map(|(w, _)| *w).collect()
}

/// satisfies: record 0122; 75, 77; rule core.14; the run-life spec's "A
/// run's state is written by its claim holder or the lead that opened it"
#[test]
fn a_claim_holder_writes_its_own_runs_states() {
    let h = signed();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    // A leads with no room of its own; B takes the run.
    assert_eq!(written_by(&h, lead(&h)), ["opened"]);
    assert_eq!(written_by(&h, on(&h, "b")), ["claimed", "running"]);
    let run = &runs_of(&h, &[one])[0];
    assert_eq!((run.state, run.holder()), (RunState::Running, Some("b")));
    h.w().complete_all();
    assert_eq!(written_by(&h, lead(&h)), Vec::<&str>::new());
    assert_eq!(written_by(&h, on(&h, "b")), ["ended", "closed"]);
    let run = &runs_of(&h, &[one])[0];
    assert_eq!(run.state, RunState::Closed);
    assert!(run.ended.is_some());
    for _ in 0..4 {
        assert!(written_by(&h, lead(&h)).is_empty());
    }
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: record 0122; 60, 114; the run-life spec's "A run's report is
/// read only once the run has ended": its runner's word that a check
/// passed, there to read while the run goes, is neither recorded nor read
#[test]
fn a_report_is_not_read_while_its_run_goes() {
    use sluice_seams::{TaskProgress, TaskState};
    let h = local();
    h.w().hold_runs(true);
    h.work(
        "unit/a",
        "agent-a",
        &[("a/lib", Some("a2")), ("b/lib", Some("b2"))],
    );
    let one = h.submit("unit/a");
    h.step();
    let combination = candidates(&h)[0].revision.clone();
    h.w().says_tasks(
        &combination,
        &[TaskProgress {
            target: "a:test".into(),
            state: TaskState::Passed,
            started: None,
            ended: None,
            exit: Some(0),
        }],
    );
    for _ in 0..3 {
        h.w().advance(chrono::Duration::minutes(1));
        h.step();
    }
    assert!(h.results().is_empty(), "nothing of the run recorded");
    assert!(!h.events().iter().any(|e| matches!(
        e,
        Event::Ran { .. } | Event::Result { .. } | Event::Ended { .. }
    )));
    assert_eq!(runs_of(&h, &[one])[0].state, RunState::Running);
    assert_eq!(h.sub(one).state, State::Testing);

    h.w().complete_all();
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert!(!h.results().is_empty());
}
