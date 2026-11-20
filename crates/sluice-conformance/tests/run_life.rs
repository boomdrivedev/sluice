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
        repository: "this-repository".into(),
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

/// How a claimed run whose checkout was made comes to end with no outcome
/// before any check of it runs.
#[derive(Clone, Copy, Debug)]
enum Unstarted {
    /// Its start is refused at the host's bound (71).
    AtBound,
    /// Its start fails, as a toolchain that does not enter (114).
    StartFails,
    /// A step that made its checkout stopped, and the next cannot make it
    /// (record 0118).
    Unmade,
}

/// satisfies: record 0122; rule core.65; the run-life spec's "Nothing acts
/// on a run before the state naming the act is written", its checkout
/// released only once the run is written ended: a start refused at the
/// bound, a start that fails, and a checkout a stopped step made that
/// cannot be made again, each stopped right after the release
#[test]
fn a_checkout_is_released_only_once_its_run_is_written_ended() {
    for how in [Unstarted::AtBound, Unstarted::StartFails, Unstarted::Unmade] {
        let h = local();
        h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
        let one = h.submit("unit/a");
        match how {
            Unstarted::AtBound => h.w().refuse_next_start(),
            Unstarted::StartFails => h.w().start_fails(Some("devenv: no such file")),
            Unstarted::Unmade => {
                h.w().crash(Crash::AfterCheckout);
                assert!(
                    h.sluice().step(BRANCH).is_err(),
                    "{how:?}: the step stopped"
                );
                assert_eq!(runs_of(&h, &[one])[0].state, RunState::Claimed);
                h.w().checkout_fails(Some("no room left on the device"));
            }
        }
        h.w().crash(Crash::AfterRelease);
        assert!(
            h.sluice().step(BRANCH).is_err(),
            "{how:?}: the step stopped"
        );
        let run = &runs_of(&h, &[one])[0];
        let place = run.place.clone().expect("its checkout named until closed");
        assert_eq!(h.releases_of(&place), 1, "{how:?}: its checkout released");
        assert_eq!(
            run.state,
            RunState::Ended,
            "{how:?}: written ended before its checkout was released"
        );
        assert!(
            run.ended.as_ref().is_some_and(|e| e
                .outcomes
                .values()
                .all(|o| matches!(o, CheckOutcome::None { .. }))),
            "{how:?}: {run:?}"
        );

        h.w().start_fails(None);
        h.w().checkout_fails(None);
        h.step();
        let run = &runs_of(&h, &[one])[0];
        assert_eq!(run.state, RunState::Closed, "{how:?}");
        assert_eq!(h.releases_of(&place), 1, "{how:?}: not released again");
        h.settle();
        assert_eq!(h.sub(one).state, State::Landed, "{how:?}");
    }
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

/// `bolt/y`, its runs held to a container engine, so the passes `bolt/x`'s
/// runs record, held to none, prove none of its keys (19, 83); `bolt/x`
/// has landed work in `a/`, so a run on `bolt/y` replays `b:test` and
/// `c:test` from the runner's cache, with no counted pass behind them.
fn replaying(h: &Harness) -> &'static str {
    let other = "bolt/y";
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
    other
}

/// A submission of `changes` from `unit/y` to `branch`.
fn submit_to(h: &Harness, branch: &str, changes: &[(&str, Option<&str>)]) -> u64 {
    let head = h.w().work(&h.w().tip(branch), changes);
    h.w().session("unit/y", "agent-y", &head);
    h.sluice()
        .submit(&sluice_core::SubmitRequest {
            head: sluice_core::SubmitHead::Worktree("unit/y".into()),
            branch: branch.into(),
            after: vec![],
            priority: 0,
            subject: None,
            trace: None,
        })
        .unwrap()
        .unwrap()
        .id
}

/// The candidates of `branch`, as its records hold them.
fn candidates_on(h: &Harness, branch: &str) -> Vec<Candidate> {
    let s = h.sluice();
    sluice_core::line::Line::load(branch, s.seams().state.read(branch).unwrap())
        .unwrap()
        .doc
        .candidates
}

/// The events of `branch`'s log.
fn events_on(h: &Harness, branch: &str) -> Vec<Event> {
    let s = h.sluice();
    sluice_core::line::Line::read_log(&s.seams().state.read(branch).unwrap())
        .unwrap()
        .into_iter()
        .map(|l| l.event)
        .collect()
}

/// satisfies: record 0122; 108, S42; the run-life spec's "A follow-up run
/// holds only what a run left undecided"
#[test]
fn an_unproven_replay_runs_again_alone() {
    let h = local();
    let other = replaying(&h);
    let two = submit_to(&h, other, &[("a/lib", Some("a3"))]);
    let s = h.sluice();
    s.step(other).unwrap();
    s.step(other).unwrap();
    let runs = candidates_on(&h, other)[0].runs.clone();
    let ended = runs[0].ended.as_ref().expect("the first run ended");
    assert_eq!(ended.outcomes.get("a:test"), Some(&CheckOutcome::Passed));
    assert_eq!(ended.outcomes.get("b:test"), Some(&CheckOutcome::Unproven));
    assert_eq!(ended.outcomes.get("c:test"), Some(&CheckOutcome::Unproven));
    let follow = &runs[1];
    assert_eq!(follow.follows, Some(0));
    assert_eq!(
        follow.checks,
        ["b:test", "c:test"],
        "the unproven ones alone"
    );
    assert!(follow.no_replay);
    for _ in 0..8 {
        s.step(other).unwrap();
    }
    assert_eq!(h.sub(two).state, State::Landed);
    let asked: Vec<(Vec<String>, bool)> = h
        .w()
        .requests()
        .into_iter()
        .filter(|r| r.revision == candidates_or_landed(&h, other, two))
        .map(|r| (r.required, r.replay))
        .collect();
    assert_eq!(
        asked,
        [
            (vec!["#gate".to_string()], true),
            (vec!["b:test".to_string(), "c:test".to_string()], false),
        ]
    );
}

/// The combination submission `id` of `branch` was tested in.
fn candidates_or_landed(h: &Harness, _branch: &str, id: u64) -> sluice_seams::Rev {
    h.sub(id).tested.expect("tested").combination
}

/// satisfies: record 0122; 108, 17; the run-life spec's "A combination's
/// verdict is read over a run and its follow-ups", a fail beside an
/// unproven replay
#[test]
fn a_fail_beside_an_unproven_replay_is_read_at_once() {
    let h = local();
    let other = replaying(&h);
    h.w().fails_with("a:test", "a/lib", "a3");
    let two = submit_to(&h, other, &[("a/lib", Some("a3"))]);
    let s = h.sluice();
    for _ in 0..4 {
        s.step(other).unwrap();
    }
    assert_eq!(h.sub(two).state, State::Failed);
    let failed: Vec<String> = h.sub(two).failed.iter().map(|f| f.task.clone()).collect();
    assert_eq!(failed, ["a:test"]);
    assert!(
        !events_on(&h, other)
            .iter()
            .any(|e| matches!(e, Event::Opened { run, .. } if *run > 0)),
        "no follow-up opened"
    );
}

/// satisfies: record 0122; 60, 114; rule core.22; the run-life spec's "A
/// check the report holds no outcome for has none"
#[test]
fn a_check_the_report_holds_no_outcome_for_is_never_failed() {
    let h = local();
    h.w().skips("b:test");
    h.w().skips("c:test");
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    h.step();
    let runs = runs_of(&h, &[one]);
    let ended = runs[0].ended.as_ref().expect("the first run ended");
    assert_eq!(ended.outcomes.get("a:test"), Some(&CheckOutcome::Passed));
    for check in ["b:test", "c:test"] {
        assert_eq!(
            ended.outcomes.get(check),
            Some(&CheckOutcome::None {
                why: "the run's report holds no outcome for it".into()
            })
        );
    }
    assert!(h.results().iter().any(|r| r.task == "a:test"));
    assert_eq!(runs[1].follows, Some(0));
    assert_eq!(runs[1].checks, ["b:test", "c:test"]);
    assert_ne!(h.sub(one).state, State::Failed);
    assert!(h.sub(one).failed.is_empty());
    assert!(!h.events().iter().any(|e| matches!(e, Event::Blamed { .. })));
    h.w().stops_skipping("b:test");
    h.w().stops_skipping("c:test");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: record 0122; 66; the run-life spec's "A run's time limit
/// counts from its own running", a follow-up run's limit
#[test]
fn a_follow_up_run_has_a_limit_of_its_own() {
    let h = local();
    h.w().hold_runs(true);
    h.w().skips("b:test");
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    // The first run takes 15 minutes of the branch's 20.
    h.w().advance(chrono::Duration::minutes(15));
    h.w().complete_all();
    h.step();
    h.w().stops_skipping("b:test");
    let runs = runs_of(&h, &[one]);
    let follow = &runs[1];
    assert_eq!(follow.follows, Some(0));
    assert_eq!(follow.state, RunState::Running);
    let began =
        |at: &Option<String>| chrono::DateTime::parse_from_rfc3339(at.as_deref().unwrap()).unwrap();
    assert!(
        began(&follow.running) - began(&runs[0].running) >= chrono::Duration::minutes(15),
        "its own start"
    );
    let asked = h.w().requests();
    assert_eq!(asked.len(), 2);
    assert_eq!(
        asked[1].limit,
        Some(Duration::from_secs(20 * 60)),
        "the whole limit from its own start"
    );
    // It runs 10 minutes: 25 since the combination opened, 10 of its own.
    h.w().advance(chrono::Duration::minutes(10));
    h.w().complete_all();
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert!(!h.events().iter().any(|e| matches!(
        e,
        Event::Result { result, .. } if result == "timed out"
    )));
}

/// satisfies: record 0122; 78; rule core.22; the run-life spec's "A run's
/// written end stands", a passed run with no signed result
#[test]
fn a_passed_run_with_no_signed_result_stands_and_is_followed_up() {
    use sluice_core::records::{CandidateState, Ended};
    let h = signed();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    on(&h, "a").step(BRANCH).unwrap();
    // A host that can write the state writes the run ended passed, with no
    // signed result behind it.
    let state = h.sluice().seams().state;
    let mut line = sluice_core::line::Line::load(BRANCH, state.read(BRANCH).unwrap()).unwrap();
    for c in &mut line.doc.candidates {
        c.state = CandidateState::Passed;
        for r in &mut c.runs {
            r.state = RunState::Ended;
            r.ended = Some(Ended {
                at: "2026-11-16T09:00:00Z".into(),
                outcomes: r
                    .checks
                    .iter()
                    .map(|c| (c.clone(), CheckOutcome::Passed))
                    .collect(),
                timed_out: false,
                why: None,
                untaken: false,
            });
            r.place = None;
            r.run = None;
        }
    }
    {
        let _g = state.lock(BRANCH).unwrap().unwrap();
        assert!(matches!(
            state.write(BRANCH, line.version, line.changes()).unwrap(),
            sluice_seams::Written::Written(_)
        ));
    }
    h.w().complete_all();
    on(&h, "a").step(BRANCH).unwrap();
    assert!(h
        .events()
        .iter()
        .any(|e| matches!(e, Event::Unsigned { .. })));
    let c = &candidates(&h)[0];
    assert_eq!(c.state, CandidateState::Testing, "under test again");
    let first = &c.runs[0];
    assert!(first.state.done(), "its end stands: {first:?}");
    assert_eq!(
        first.ended.as_ref().unwrap().outcomes.get("#gate"),
        Some(&CheckOutcome::Passed)
    );
    let follow = &c.runs[1];
    assert_eq!(follow.follows, Some(0));
    assert_eq!(follow.checks, ["#gate"]);
    assert_ne!(h.sub(one).state, State::Landed);
    assert!(h.w().moves().is_empty(), "nothing lands");

    for _ in 0..4 {
        h.w().complete_all();
        on(&h, "a").step(BRANCH).unwrap();
    }
    assert_eq!(h.sub(one).state, State::Landed);
}

/// The marker of `host`'s last sign of life, as the remote holds it.
fn beat_of(h: &Harness, host: &str) -> Option<String> {
    use sluice_seams::State as _;
    h.f.state.beats().unwrap().remove(host).map(|(m, _)| m)
}

/// `host` shows its sign of life with no round of the branch, as a host
/// busy elsewhere does.
fn beats(h: &Harness, host: &str) {
    use sluice_seams::State as _;
    h.f.state.beat(host, b"{}").unwrap();
}

/// satisfies: record 0122; 73, 77; the run-claims spec's "Every claim
/// holder shows its sign of life", the lead among them
#[test]
fn a_lead_shows_its_sign_of_life_as_any_claim_holder() {
    let h = signed();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    on(&h, "a").step(BRANCH).unwrap();
    assert_eq!(h.doc().lead.as_deref(), Some("a"));
    assert_eq!(runs_of(&h, &[one])[0].holder(), Some("a"));
    let first = beat_of(&h, "a").expect("the lead's sign of life");
    h.w().advance(chrono::Duration::minutes(1));
    on(&h, "a").step(BRANCH).unwrap();
    let second = beat_of(&h, "a").expect("the lead's sign of life");
    assert_ne!(first, second, "written each round");
}

/// satisfies: record 0122; 72, 73, 77; the run-life spec's "A change of
/// lead changes no run", the lead moves under a running claim
#[test]
fn a_change_of_lead_keeps_a_live_claim() {
    let h = signed();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    on(&h, "a").step(BRANCH).unwrap();
    assert_eq!(runs_of(&h, &[one])[0].holder(), Some("a"));
    // a writes nothing of the branch for the lease, but stays alive; b
    // takes the lead.
    for _ in 0..7 {
        h.w().advance(chrono::Duration::minutes(1));
        beats(&h, "a");
        on(&h, "b").step(BRANCH).unwrap();
    }
    assert_eq!(h.doc().lead.as_deref(), Some("b"));
    for _ in 0..3 {
        h.w().advance(chrono::Duration::minutes(1));
        beats(&h, "a");
        on(&h, "b").step(BRANCH).unwrap();
    }
    assert!(!h.events().iter().any(|e| matches!(e, Event::Lapsed { .. })));
    assert_eq!(runs_of(&h, &[one])[0].holder(), Some("a"));
    assert_eq!(runs_of(&h, &[one])[0].state, RunState::Running);
    // a's run ends, and a writes its end under its own claim.
    h.w().complete_all();
    on(&h, "a").step(BRANCH).unwrap();
    let run = &runs_of(&h, &[one])[0];
    assert_eq!(run.state, RunState::Closed);
    assert_eq!(
        run.ended.as_ref().unwrap().outcomes.get("#gate"),
        Some(&CheckOutcome::Passed)
    );
    for _ in 0..3 {
        on(&h, "b").step(BRANCH).unwrap();
    }
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.w().requests().len(), 1, "run once");
}

/// satisfies: record 0122; 73, 77; the run-life spec's "A change of lead
/// changes no run", a lead that stops
#[test]
fn a_stopped_leads_claim_lapses_by_its_own_sign_of_life() {
    let h = signed();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    on(&h, "a").step(BRANCH).unwrap();
    // a stops. b reads the branch, and takes it once the lease has passed.
    on(&h, "b").step(BRANCH).unwrap();
    h.w().advance(chrono::Duration::minutes(6));
    on(&h, "b").step(BRANCH).unwrap();
    assert_eq!(h.doc().lead.as_deref(), Some("b"));
    // b's own observation of a's sign of life begins as it leads: nothing
    // lapses before the bound of its own reading.
    h.w().advance(chrono::Duration::minutes(1));
    on(&h, "b").step(BRANCH).unwrap();
    assert!(!h.events().iter().any(|e| matches!(e, Event::Lapsed { .. })));
    h.w().advance(chrono::Duration::minutes(2));
    on(&h, "b").step(BRANCH).unwrap();
    assert!(h
        .events()
        .iter()
        .any(|e| matches!(e, Event::Lapsed { host, .. } if host == "a")));
    let runs = runs_of(&h, &[one]);
    assert_eq!(runs[0].state, RunState::Closed);
    let ended = runs[0].ended.as_ref().unwrap();
    assert_eq!(ended.why.as_deref(), Some("a's claim lapsed"));
    assert_eq!(
        ended.outcomes.get("#gate"),
        Some(&CheckOutcome::None {
            why: "a's claim lapsed".into()
        })
    );
    assert_eq!(runs[1].follows, Some(0));
    assert_eq!(runs[1].checks, runs[0].checks);
    for _ in 0..4 {
        h.w().complete_all();
        on(&h, "b").step(BRANCH).unwrap();
    }
    assert_eq!(h.sub(one).state, State::Landed);
}

/// A branch whose records are on its remote, its required checks needing
/// `macos-arm64`, which the fakes' host is not.
fn on_macos() -> Harness {
    let p = policy(false, "fail", 4).replace(
        "trust = [\"open\", \"sandbox\", \"container\"]\n",
        "trust = [\"box\"]\nremote = \"origin\"\nsystems = [\"macos-arm64\"]\n",
    ) + "\n[signers.box]\nssh = \"box ssh-ed25519 AAAA\"\n\n[systems]\n\
         linux-x64 = { os = \"linux\", arch = \"x86_64\" }\n\
         macos-arm64 = { os = \"macos\", arch = \"aarch64\" }\n";
    let h = Harness::new(&p);
    h.w().signs_as("box");
    h
}

/// Instance `name`, taking the runs of `systems` (`[runs] systems`).
fn taking<'a>(h: &'a Harness, name: &str, systems: &[&str]) -> Sluice<'a> {
    h.w().be_host(name);
    h.sluice().as_peer(Peer {
        name: name.into(),
        repository: "this-repository".into(),
        lease: Duration::from_secs(300),
        alive: Duration::from_secs(120),
        signer: Some("box".into()),
        systems: Some(systems.iter().map(|s| s.to_string()).collect()),
        expire_after: sluice_core::EXPIRE_AFTER,
    })
}

/// satisfies: record 0122; 66, 87; the run-life spec's "A run's time limit
/// counts from its own running", a run that waited for room
#[test]
fn a_run_that_waited_for_room_has_its_whole_limit() {
    let h = on_macos();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    // The lead is no host of macos-arm64; m, the one that is, has no room
    // for 30 minutes of a 20-minute limit.
    lead(&h).step(BRANCH).unwrap();
    for _ in 0..6 {
        let m = taking(&h, "m", &["macos-arm64"]);
        h.w().no_room();
        m.step(BRANCH).unwrap();
        lead(&h).step(BRANCH).unwrap();
        h.w().advance(chrono::Duration::minutes(5));
    }
    assert_eq!(runs_of(&h, &[one])[0].state, RunState::Open, "it waits");
    // m has room: it runs 10 minutes and passes.
    let m = taking(&h, "m", &["macos-arm64"]);
    h.w().has_room();
    m.step(BRANCH).unwrap();
    assert_eq!(runs_of(&h, &[one])[0].state, RunState::Running);
    h.w().advance(chrono::Duration::minutes(10));
    h.w().complete_all();
    taking(&h, "m", &["macos-arm64"]).step(BRANCH).unwrap();
    for _ in 0..3 {
        lead(&h).step(BRANCH).unwrap();
    }
    assert_eq!(h.sub(one).state, State::Landed);
    assert!(h.sub(one).failed.is_empty());
    assert!(!h.events().iter().any(|e| matches!(
        e,
        Event::Result { result, .. } if result != "passed"
    )));
}

/// satisfies: record 0122; 26; the run-progress spec's "A combination with
/// no run running reads each run's state" and the run-life spec's "A status
/// reads each run's state": a run naming a system reads waiting for room
/// while a live host of that system may take it, and waiting for a host of
/// it once none may, by the lead's reading, from the records alone
#[test]
fn a_waiting_run_reads_whether_a_live_executor_may_take_it() {
    let h = on_macos();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    lead(&h).step(BRANCH).unwrap();
    // m, the one host of macos-arm64, is live and has no room.
    let m = taking(&h, "m", &["macos-arm64"]);
    h.w().no_room();
    m.step(BRANCH).unwrap();
    lead(&h).step(BRANCH).unwrap();
    let opened = candidates(&h)[0].runs[0]
        .opened
        .clone()
        .expect("when it opened");
    let read = |h: &Harness| {
        let view = h.sluice().branch_view_of(BRANCH).unwrap();
        let now = chrono::Utc::now();
        sluice_core::overview::run_time(&view.candidates[0], now).expect("a line of its runs")
    };
    assert_eq!(read(&h), format!("waiting for room since {opened}"));
    assert!(!candidates(&h)[0].runs[0].no_executor);

    // m shows no sign of life for longer than the bound: none may take it.
    h.w().advance(chrono::Duration::minutes(3));
    lead(&h).step(BRANCH).unwrap();
    assert_eq!(candidates(&h)[0].runs[0].state, RunState::Open);
    assert!(candidates(&h)[0].runs[0].no_executor);
    assert_eq!(
        read(&h),
        format!("waiting for a host of macos-arm64 since {opened}")
    );

    // m is back: it may take the run again.
    let m = taking(&h, "m", &["macos-arm64"]);
    h.w().no_room();
    m.step(BRANCH).unwrap();
    lead(&h).step(BRANCH).unwrap();
    assert_eq!(read(&h), format!("waiting for room since {opened}"));
}

/// satisfies: record 0122; 66, 87, 115; rule core.22; the run-life spec's
/// "A run no live executor may take ends with no outcome" and "A
/// combination whose run no executor took is not passed and blames no one"
#[test]
fn a_run_no_live_executor_may_take_blames_no_one() {
    use sluice_core::records::CandidateState;
    let h = on_macos();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    lead(&h).step(BRANCH).unwrap();
    let first = candidates(&h)[0].clone();
    assert_eq!(first.runs[0].system.as_deref(), Some("macos-arm64"));
    h.w().advance(chrono::Duration::minutes(19));
    lead(&h).step(BRANCH).unwrap();
    assert_eq!(
        candidates(&h)[0].runs[0].state,
        RunState::Open,
        "within its limit"
    );
    h.w().advance(chrono::Duration::minutes(2));
    lead(&h).step(BRANCH).unwrap();
    let c = candidates(&h)
        .into_iter()
        .find(|c| c.id == first.id)
        .expect("the combination, read not passed");
    assert_eq!(c.state, CandidateState::NotPassed);
    let ended = c.runs[0].ended.as_ref().expect("its run ended");
    assert!(ended.untaken);
    assert_eq!(
        ended.outcomes.get("#gate"),
        Some(&CheckOutcome::None {
            why: "no host that may take it was live".into()
        })
    );
    assert_eq!(c.runs[0].state, RunState::Closed);
    assert!(c.failed.is_empty(), "no check failed");
    assert!(h.results().is_empty(), "results.jsonl gains nothing");
    let sub = h.sub(one);
    assert_eq!(sub.state, State::Queued, "not blamed, and queued");
    assert!(sub.failed.is_empty());
    assert!(!h.events().iter().any(|e| matches!(e, Event::Blamed { .. })));
    assert_eq!(c.not_run_runs, 0, "it counts toward nothing");

    // The next round forms it again.
    lead(&h).step(BRANCH).unwrap();
    let again = candidates(&h);
    assert_eq!(again.len(), 1);
    assert_ne!(again[0].id, first.id);
    assert_eq!(again[0].revision, first.revision);
    assert_eq!(again[0].state, CandidateState::Testing);
    assert_eq!(again[0].runs[0].state, RunState::Open);
    assert_eq!(h.sub(one).state, State::Testing);
}

/// A branch whose records are on its remote, its required checks needing
/// `macos-arm64`, which no host is, with a pool `forge` that runs them; the
/// lead signs as `box`.
fn with_pool() -> Harness {
    let p = policy(false, "fail", 4).replace(
        "trust = [\"open\", \"sandbox\", \"container\"]\n",
        "trust = [\"box\", \"forge\"]\nremote = \"origin\"\nsystems = [\"macos-arm64\"]\n",
    ) + "\n[signers.box]\nssh = \"box ssh-ed25519 AAAA\"\n\
         \n[signers.forge]\ngithub = { workflow = \".github/workflows/sluice.yml\", ref = \"refs/heads/main\" }\n\
         \n[pools.forge]\ngithub = { workflow = \".github/workflows/sluice.yml\" }\nsigner = \"forge\"\n\
         runners = { macos-arm64 = \"macos-15\" }\n\
         \n[systems]\nlinux-x64 = { os = \"linux\", arch = \"x86_64\" }\n\
         macos-arm64 = { os = \"macos\", arch = \"aarch64\" }\n";
    let h = Harness::new(&p);
    h.w().signs_as("box");
    h
}

/// The one run of the branch's first combination.
fn pool_run(h: &Harness) -> SystemRun {
    candidates(h)[0].runs.last().cloned().expect("a run")
}

/// satisfies: record 0122; 88, 107; the actions-pool spec's "A pool's run is
/// claimed before its job is dispatched"
#[test]
fn a_pools_run_is_claimed_before_its_job_is_dispatched() {
    let h = with_pool();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    h.w().crash(Crash::BeforeDispatch);
    assert!(lead(&h).step(BRANCH).is_err(), "the step stopped");
    let run = pool_run(&h);
    assert_eq!(run.state, RunState::Claimed);
    let claim = run.claim.clone().expect("the pool's claim, written");
    assert!(claim.by.starts_with("pool/forge/"), "{claim:?}");
    assert_eq!(claim.dispatched, None);
    assert!(h.w().launches().is_empty(), "nothing dispatched");

    lead(&h).step(BRANCH).unwrap();
    let launches = h.w().launches();
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].claim, claim.by);
    let run = pool_run(&h);
    assert!(run.claim.as_ref().unwrap().dispatched.is_some());
}

/// satisfies: record 0122; 88, 134; rule core.29; the actions-pool spec's
/// "A dispatch found done is not made again"
#[test]
fn a_pools_dispatch_found_done_is_not_made_again() {
    let h = with_pool();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    h.w().crash(Crash::AfterDispatch);
    assert!(lead(&h).step(BRANCH).is_err(), "the step stopped");
    assert_eq!(h.w().launches().len(), 1, "dispatched");
    assert_eq!(pool_run(&h).claim.unwrap().dispatched, None);
    for _ in 0..3 {
        lead(&h).step(BRANCH).unwrap();
    }
    assert_eq!(h.w().launches().len(), 1, "found done, sent no more");
    assert!(pool_run(&h).claim.unwrap().dispatched.is_some());
}

/// satisfies: record 0122; 88; the actions-pool spec's "A pool's run is
/// running from its run job's start"
#[test]
fn a_pools_run_is_running_from_its_run_jobs_start() {
    let h = with_pool();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    lead(&h).step(BRANCH).unwrap();
    let claim = pool_run(&h).claim.unwrap().by;
    lead(&h).step(BRANCH).unwrap();
    assert_eq!(pool_run(&h).state, RunState::Claimed, "queued, not running");
    let since = "2026-10-11T09:03:00Z";
    h.w().move_job(
        &claim,
        sluice_seams::Job::Running {
            since: since.into(),
        },
    );
    lead(&h).step(BRANCH).unwrap();
    let run = pool_run(&h);
    assert_eq!(run.state, RunState::Running);
    assert_eq!(run.running.as_deref(), Some(since));
    assert!(life(&h).contains(&("running", Some(0))));
}

/// satisfies: record 0122; 77, 88; the actions-pool spec's "A cancelled
/// workflow run": its sign job skipped, the workflow run ended with no
/// record of the run, which ends with no outcome saying so, nothing of it
/// recorded and nobody blamed, its follow-up claimed for the pool again
#[test]
fn a_pools_job_ended_with_no_record_is_a_lapsed_claim() {
    let h = with_pool();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    lead(&h).step(BRANCH).unwrap();
    let claim = pool_run(&h).claim.unwrap().by;
    h.w().move_job(
        &claim,
        sluice_seams::Job::Ended {
            recorded: false,
            why: None,
        },
    );
    lead(&h).step(BRANCH).unwrap();
    let runs = candidates(&h)[0].runs.clone();
    assert_eq!(runs[0].state, RunState::Closed);
    let why = format!("{claim}'s job ended with no record of its run");
    let ended = runs[0].ended.as_ref().expect("ended");
    assert_eq!(ended.why.as_deref(), Some(why.as_str()));
    assert!(ended
        .outcomes
        .values()
        .all(|o| *o == CheckOutcome::None { why: why.clone() }));
    assert!(h.results().is_empty(), "nothing of it recorded");
    assert!(
        !h.events().iter().any(|e| matches!(e, Event::Blamed { .. })),
        "nobody blamed"
    );
    assert!(h.sub(one).failed.is_empty());
    assert_ne!(h.sub(one).state, State::Failed);
    assert_eq!(runs[1].follows, Some(0));
    let again = runs[1].claim.clone().expect("claimed for the pool again");
    assert_ne!(again.by, claim, "under a new claim");
    let launches = h.w().launches();
    assert_eq!(launches.len(), 2);
    assert_eq!(launches[1].claim, again.by);
}

/// satisfies: 115 as amended 2026-11-18, 77, 88; record 0031 as amended
/// 2026-11-18; the run-life spec's "A pool's job lost in runs in a row":
/// each job lost counts toward the runs in a row, and at the bound the
/// combination reads not passed, each check named with the job's words,
/// nothing recorded under a key and no job dispatched again
/// (boomdrivedev/sluice-actions-trial on 0.97.1, 2026-11-18: pool/forge/1
/// lost, and dispatched again as 3, 4 and 5 until withdrawn)
#[test]
fn a_pools_jobs_lost_in_a_row_read_the_combination_not_passed() {
    let h = with_pool();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    lead(&h).step(BRANCH).unwrap();
    let mut whys = Vec::new();
    for _ in 0..3 {
        let claim = pool_run(&h).claim.expect("claimed for the pool").by;
        let why = format!("{claim}'s job ended with no record of its run; its log is at https://github.com/o/r/actions/runs/1");
        h.w().move_job(
            &claim,
            sluice_seams::Job::Ended {
                recorded: false,
                why: Some(why.clone()),
            },
        );
        lead(&h).step(BRANCH).unwrap();
        whys.push(why);
    }
    assert_eq!(
        h.w().launches().len(),
        3,
        "no job dispatched past the bound"
    );
    assert!(
        h.events()
            .iter()
            .any(|e| matches!(e, Event::Result { result, .. } if result == "failed")),
        "the combination read failed"
    );
    lead(&h).step(BRANCH).unwrap();
    assert_eq!(h.w().launches().len(), 3, "nor on the next round");
    assert_eq!(h.sub(one).state, State::Failed);
    let failed = &h.sub(one).failed;
    assert!(!failed.is_empty());
    for f in failed {
        assert!(f.hash.is_empty(), "no key: it has no result");
        assert_eq!(f.not_run.as_deref(), Some(whys[2].as_str()), "{f:?}");
    }
    assert!(h.results().is_empty(), "nothing of it recorded");
}

/// satisfies: record 0122; 77, 88; the codebuild-pool spec's "The lead's
/// lapse gives the pool's why": a pool's job that ended with no record
/// and says why ends its run with no outcome under the pool's words,
/// nobody blamed, and a follow-up run holds its checks
#[test]
fn a_pools_job_ended_with_its_why_lapses_with_it() {
    let h = with_pool();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    lead(&h).step(BRANCH).unwrap();
    let claim = pool_run(&h).claim.unwrap().by;
    let why = format!("{claim}'s run build sluice-run:1 was stopped, which starts no sign build");
    h.w().move_job(
        &claim,
        sluice_seams::Job::Ended {
            recorded: false,
            why: Some(why.clone()),
        },
    );
    lead(&h).step(BRANCH).unwrap();
    let runs = candidates(&h)[0].runs.clone();
    assert_eq!(runs[0].state, RunState::Closed);
    let ended = runs[0].ended.as_ref().expect("ended");
    assert_eq!(ended.why.as_deref(), Some(why.as_str()));
    assert!(ended
        .outcomes
        .values()
        .all(|o| *o == CheckOutcome::None { why: why.clone() }));
    assert!(h.results().is_empty(), "nothing of it recorded");
    assert!(
        !h.events().iter().any(|e| matches!(e, Event::Blamed { .. })),
        "nobody blamed"
    );
    assert!(h.sub(one).failed.is_empty());
    assert_eq!(runs[1].follows, Some(0), "a follow-up holds its checks");
    assert!(runs[1].claim.is_some());
}

/// satisfies: 77, 133; rule core.28; the codebuild-pool spec's "The pool
/// refuses every read for a while": a lead's reads of a running pool run's
/// job fail for three rounds with one cause, then answer; the branch's log
/// holds one line of the cause and one of its end, and the run is still
/// running under its claim
#[test]
fn a_pools_read_refused_is_logged_once_and_lapses_nothing() {
    let h = with_pool();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    lead(&h).step(BRANCH).unwrap();
    let claim = pool_run(&h).claim.unwrap().by;
    let since = "2026-10-11T09:03:00Z";
    h.w().move_job(
        &claim,
        sluice_seams::Job::Running {
            since: since.into(),
        },
    );
    lead(&h).step(BRANCH).unwrap();
    assert_eq!(pool_run(&h).state, RunState::Running);

    let said = "BatchGetBuilds: AccessDeniedException: User: \
                arn:aws:sts::111122223333:assumed-role/sluice-lead/host is not authorized to \
                perform: codebuild:BatchGetBuilds";
    h.w().refuse_jobs(Some(said));
    for _ in 0..3 {
        lead(&h).step(BRANCH).unwrap();
        let run = pool_run(&h);
        assert_eq!(run.state, RunState::Running, "read as it stood");
        assert_eq!(run.claim.as_ref().unwrap().by, claim);
    }
    h.w().refuse_jobs(None);
    lead(&h).step(BRANCH).unwrap();

    let refused: Vec<Event> = h
        .events()
        .into_iter()
        .filter(|e| matches!(e, Event::ReadRefused { pool: true, .. }))
        .collect();
    assert_eq!(
        refused,
        [Event::ReadRefused {
            host: "a".into(),
            remote: "forge".into(),
            asked: "pool forge's jobs".into(),
            said: said.into(),
            intake: false,
            pool: true,
        }]
    );
    let again: Vec<Event> = h
        .events()
        .into_iter()
        .filter(|e| matches!(e, Event::ReadAgain { pool: true, .. }))
        .collect();
    assert_eq!(again.len(), 1, "{again:?}");
    let Event::ReadAgain { rounds, remote, .. } = &again[0] else {
        unreachable!()
    };
    assert_eq!(remote, "forge");
    assert!(*rounds >= 3, "{rounds}");
    assert!(
        !h.events().iter().any(|e| matches!(
            e,
            Event::Lapsed { .. } | Event::ReadRefused { pool: false, .. }
        )),
        "nothing lapsed, and no read of the remote refused"
    );
    let run = pool_run(&h);
    assert_eq!(run.state, RunState::Running);
    assert_eq!(run.claim.unwrap().by, claim);
}

/// A branch whose records are on its remote, its gate trusting `lead`, the
/// signer its leading host signs as, which may claim no system, and
/// `codebuild`, the `kms` signer of the CodeBuild pool `codebuild` that
/// runs `linux-x64`, testing two combinations side by side.
fn on_a_fleet() -> Harness {
    let p = policy(false, "fail", 2).replace(
        "trust = [\"open\", \"sandbox\", \"container\"]\n",
        "trust = [\"lead\", \"codebuild\"]\nremote = \"origin\"\nsystems = [\"linux-x64\"]\n",
    ) + "\n[signers.lead]\nssh = \"lead ssh-ed25519 AAAA\"\nsystems = []\n\
         \n[signers.codebuild]\nkms = { key = \"arn:aws:kms:us-east-1:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab\", public = \"MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEg44RRacS1htcK8s6X92E7lTYn8f6s5LC4GIxCQv+9fg1UWxDvETlpCR7yLWZ337t0Q/nT0nGEvEP5rLDBHhtTg==\" }\n\
         systems = [\"linux-x64\"]\n\
         \n[pools.codebuild]\ncodebuild = { region = \"us-east-1\", sign = \"sluice-sign\" }\nsigner = \"codebuild\"\n\
         runners = { linux-x64 = \"sluice-run-linux-x64\" }\n\
         \n[systems]\nlinux-x64 = { os = \"linux\", arch = \"x86_64\" }\n";
    let h = Harness::new(&p);
    h.w().signs_as("lead");
    h.w().platform("linux", "x86_64");
    h
}

/// Host `name`, signing as `lead`.
fn leading<'a>(h: &'a Harness, name: &str) -> Sluice<'a> {
    h.w().be_host(name);
    h.sluice().as_peer(Peer {
        name: name.into(),
        repository: "this-repository".into(),
        lease: Duration::from_secs(300),
        alive: Duration::from_secs(120),
        signer: Some("lead".into()),
        systems: None,
        expire_after: sluice_core::EXPIRE_AFTER,
    })
}

/// satisfies: 71, 79, 87, 107; record 0119; the codebuild-pool spec's "A
/// lead with room beside a pool": a gate trusting a pool's signer and the
/// lead's, which may claim no system, has every run of its combinations
/// claimed by the pool; the lead, of the run's system and with room, leads
/// the branch and claims none, holding no part of its bound for them
/// (boomdrive-design's ruling of 2026-11-16)
#[test]
fn a_gate_trusting_only_a_pools_signer_runs_every_run_on_its_pool() {
    let h = on_a_fleet();
    // The lead, and a second host of the same signer, live with room.
    leading(&h, "a").step(BRANCH).unwrap();
    leading(&h, "b").step(BRANCH).unwrap();
    let beats = h.sluice().seams().state.beats().unwrap();
    assert!(beats.contains_key("b"), "b is live: {:?}", beats.keys());
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    h.submit("unit/b");
    for host in ["a", "b", "a", "b"] {
        leading(&h, host).step(BRANCH).unwrap();
    }
    assert_eq!(h.doc().lead.as_deref(), Some("a"), "the host leads");
    let c = candidates(&h);
    assert_eq!(c.len(), 2, "two combinations open: {c:?}");
    let mut claims = Vec::new();
    for combination in &c {
        assert_eq!(combination.runs.len(), 1, "{combination:?}");
        let run = &combination.runs[0];
        assert_eq!(run.system.as_deref(), Some("linux-x64"));
        let claim = run.claim.as_ref().expect("claimed").by.clone();
        assert!(claim.starts_with("pool/codebuild/"), "{claim}");
        claims.push(claim);
    }
    let launches = h.w().launches();
    assert_eq!(launches.len(), 2);
    for (l, claim) in launches.iter().zip(&claims) {
        assert_eq!(l.kind.0, "codebuild");
        assert_eq!(l.runner, "sluice-run-linux-x64");
        assert_eq!(&l.claim, claim);
    }
    assert!(h.w().started().is_empty(), "neither host started one");
    // A host that leads nothing holds a part only to look for a run it may
    // claim, and gives it back at once (rule core.49): none stays held.
    assert_eq!(h.w().parts_held(), 0, "no part of a bound held for them");
}
