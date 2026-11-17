//! The statement's scenarios (section 11), for a bolt's branch on one host.

use std::cell::RefCell;
use std::rc::Rc;

use sluice_conformance::{policy, shared, Crash, Harness, Rig, BRANCH};
use sluice_core::records::{Event, State};
use sluice_core::{Forecast, Step};
use sluice_fakes::Fakes;
use sluice_seams::{Decided, Outcome};

/// satisfies: S1, 1, 10, 11, 22, 23, 33, 34, 37, 42, 43, 44, 45, 47, 50
#[test]
fn s1_two_heads_pass_and_land_in_order() {
    shared::s1_two_heads_pass_and_land_in_order(&Harness::bolt());
}

/// satisfies: S2, 12, 43, 65, 20, 50
#[test]
fn s2_a_conflict_is_told_before_any_check_runs() {
    shared::s2_a_conflict_is_told_before_any_check_runs(&Harness::bolt());
}

/// satisfies: S3, 16, 17, 18, 20, 22
#[test]
fn s3_the_second_fails_and_only_its_session_hears() {
    shared::s3_the_second_fails_and_only_its_session_hears(&Harness::bolt());
}

/// satisfies: S4, 10, 11, 17, 21, 22
#[test]
fn s4_the_first_fails_and_the_second_is_formed_again_without_it() {
    shared::s4_the_first_fails_and_the_second_is_formed_again_without_it(&Harness::bolt());
}

/// satisfies: S6, 8, 10
/// satisfies: S6, 8, 10
#[test]
fn s6_unrelated_work_is_tested_in_one_chain_and_not_again() {
    let h = Harness::bolt();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let one = h.submit("unit/a");
    let two = h.submit("unit/b");
    h.step();
    // The second is tested on top of the first (8, 10).
    assert_eq!(h.tested(), vec![vec![one], vec![one, two]]);

    // The second finishing first lands nothing: it lands after the first.
    let b_combination = h.sub(two).tested.unwrap().combination;
    h.w().complete(&b_combination);
    h.step();
    h.step();
    assert!(h.w().moves().is_empty());

    h.w().hold_runs(false);
    h.w().complete_all();
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.sub(two).state, State::Landed);
    // Not tested again when the first landed.
    assert_eq!(h.tested(), vec![vec![one], vec![one, two]]);
    let tree = h.w().tree(&h.tip());
    assert_eq!(
        (tree["a/lib"].as_str(), tree["b/lib"].as_str()),
        ("a2", "b2")
    );
}

/// satisfies: S7, 19, 69
#[test]
fn s7_a_flaky_key_reads_as_the_bolt_policy_says() {
    for (flaky, lands) in [("pass", true), ("fail", false)] {
        let h = Harness::new(&policy(false, flaky, 4));
        // The first run of the key fails, the second passes: the key
        // holds a pass and a fail from trusted executors.
        h.w().flaky("a:test", &[Outcome::Fail, Outcome::Pass]);
        h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
        let first = h.submit("unit/a");
        h.settle();
        assert_eq!(h.sub(first).state, State::Failed);

        let again = h.submit("unit/a");
        h.settle();
        let observed: Vec<_> = h
            .w()
            .observed()
            .into_iter()
            .filter(|(task, ..)| task == "a:test")
            .collect();
        assert_eq!(observed.len(), 2);
        assert_eq!(observed[0].1, observed[1].1, "one key");
        let state = h.sub(again).state;
        assert_eq!(state == State::Landed, lands, "flaky = {flaky}: {state:?}");
    }
}

/// satisfies: S8, 24, 42, 45, 46, 11
#[test]
fn s8_a_hand_move_refuses_the_landing_and_forms_again() {
    let h = Harness::bolt();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    // The verdict is read and the landing written; the step stops
    // before the move, and someone moves the branch by hand.
    h.w().crash(Crash::BeforeMove);
    assert!(h.sluice().step(BRANCH).is_err());
    let expected = h.tip();
    let by_hand = h.w().hand_commit(BRANCH, &[("c/lib", Some("by hand"))]);

    h.settle();

    let refused = h.events().into_iter().find_map(|e| match e {
        Event::Refused {
            expected, found, ..
        } => Some((expected, found)),
        _ => None,
    });
    assert_eq!(refused, Some((expected.clone(), Some(by_hand.clone()))));
    // Formed again on the new revision, tested, and landed on it.
    let s = h.sub(one);
    assert_eq!(s.state, State::Landed);
    assert_eq!(s.tested.unwrap().on, by_hand);
    assert_eq!(h.w().parents(&h.tip())[0], by_hand);
    // No move from the stale revision ever succeeded (45, 46).
    assert!(h.w().moves().iter().all(|m| m.from != expected));
}

/// satisfies: S9, 14, 17, 18, 23
#[test]
fn s9_deepest_first_finds_the_culprit_by_testing_what_it_skipped() {
    shared::s9_deepest_first_finds_the_culprit_by_testing_what_it_skipped(&Harness::new(&policy(
        true, "fail", 4,
    )));
}

/// satisfies: S59, 125, 6, 12, 17, 18, 5, 50
#[test]
fn s59_heads_submitted_as_one_act_are_formed_together_and_each_its_own() {
    shared::s59_heads_submitted_as_one_act_are_formed_together_and_each_its_own(&Harness::new(
        &policy(true, "fail", 4),
    ));
}

/// satisfies: 125, 62, 26
#[test]
fn a_round_between_an_acts_numbering_and_its_write_sees_none_of_it() {
    let h = Harness::new(&policy(true, "fail", 4));
    for (w, p) in [
        ("unit/1", "a/one"),
        ("unit/2", "b/two"),
        ("unit/3", "c/three"),
    ] {
        h.work(w, &format!("agent-{w}"), &[(p, Some("x"))]);
    }
    // Another process runs a round each time the act takes a number.
    let seen = Rc::new(RefCell::new(Vec::new()));
    let saw = seen.clone();
    h.w().after_allocating(move |world, _| {
        let other = Fakes::of(world.clone());
        assert_eq!(other.sluice().step(BRANCH).unwrap(), Step::Stepped);
        saw.borrow_mut()
            .push(other.sluice().status(BRANCH).unwrap().submissions.len());
    });
    let ids = h.submit_group(&["unit/1", "unit/2", "unit/3"]);
    assert_eq!(*seen.borrow(), vec![0, 0, 0], "none of the act");
    assert!(h.tested().is_empty(), "nothing formed on any of it");
    let queued: Vec<u64> = h
        .sluice()
        .status(BRANCH)
        .unwrap()
        .submissions
        .iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(queued, ids, "and after it, all three");
    h.step();
    assert_eq!(h.tested(), vec![ids]);
}

/// satisfies: S10, 26, 49, 59
#[test]
fn s10_stopped_and_started_again_it_runs_nothing_already_judged() {
    let h = Harness::bolt();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.work("unit/b", "agent-b", &[("a/more", Some("b"))]);
    let one = h.submit("unit/a");
    let two = h.submit("unit/b");
    h.step();
    let c1 = h.sub(one).tested.unwrap().combination;
    h.w().complete(&c1);
    h.step();
    // The first has a verdict; the second is still under test.
    let observed = h.w().observed().len();

    // sluice stops: every state is readable with no step running (26).
    let files = h.w().files(BRANCH);
    let doc: serde_json::Value =
        serde_json::from_slice(&files[&format!("submissions/{two}.json")]).unwrap();
    assert_eq!(doc["state"], "testing");

    // Started again: a new sluice, from the records alone.
    h.w().hold_runs(false);
    h.w().complete_all();
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.sub(two).state, State::Landed);
    // The judged candidate was not run again; only the second ran.
    assert_eq!(h.w().observed().len(), observed + 1);
    assert_eq!(h.w().started().len(), 2);
}

/// satisfies: S11, 23, 25, 34
#[test]
fn s11_a_landing_names_its_verdict_and_leaves_the_session_place() {
    shared::s11_a_landing_names_its_verdict_and_leaves_the_session_place(&Harness::bolt());
}

/// satisfies: S12, 56, 27, 15
#[test]
fn s12_a_submission_cannot_loosen_the_gate_it_is_judged_by() {
    shared::s12_a_submission_cannot_loosen_the_gate_it_is_judged_by(&Harness::bolt());
}

/// satisfies: S16, 64, 65, 47a
#[test]
fn s16_a_forecast_names_the_file_and_the_work_and_writes_nothing() {
    shared::s16_a_forecast_names_the_file_and_the_work_and_writes_nothing(&Harness::bolt());
}

/// satisfies: 64, 65, 47a, 67
#[test]
fn a_branch_is_forecast_with_no_place_of_it() {
    shared::a_branch_is_forecast_with_no_place_of_it(&Harness::bolt());
}

/// satisfies: 64, 10
/// A forecast takes the candidates a round formed on the branch's
/// revision, and forms only the head on them.
#[test]
fn a_forecast_takes_the_candidates_a_round_formed() {
    let h = Harness::bolt();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("from a"))]);
    let one = h.submit("unit/a");
    h.work("unit/b", "agent-b", &[("b/lib", Some("from b"))]);
    let two = h.submit("unit/b");
    h.step();
    assert_eq!(h.tested(), vec![vec![one], vec![one, two]]);
    h.work("unit/c", "agent-c", &[("c/lib", Some("from c"))]);

    let before = h.w().forms();
    let f = h
        .sluice()
        .forecast(&h.place("unit/c"), BRANCH, &[])
        .unwrap()
        .unwrap();
    assert_eq!(f.forecast, Forecast::Clear);
    assert_eq!(f.ahead, vec![one, two]);
    assert_eq!(h.w().forms() - before, 1);
}

/// satisfies: S26, 69, 92, 93, 16, 38
#[test]
fn s26_a_clean_session_run_is_replayed_by_its_submission() {
    shared::s26_a_clean_session_run_is_replayed_by_its_submission(&Harness::bolt());
}

/// satisfies: 92, 97, core.18
#[test]
fn a_session_runs_last_result_says_whether_it_was_recorded() {
    shared::a_session_runs_last_result_says_whether_it_was_recorded(&Harness::bolt());
}

/// satisfies: 92, 97, core.18
#[test]
fn a_worktree_that_changes_while_its_run_runs_records_nothing() {
    shared::a_worktree_that_changes_while_its_run_runs_records_nothing(&Harness::bolt());
}

/// satisfies: 37, 93, 97, core.18
#[test]
fn a_last_result_decides_nothing() {
    shared::a_last_result_decides_nothing(&Harness::bolt());
}

/// satisfies: 66, 92, 97, core.7
#[test]
fn a_timed_out_lost_or_stopped_run_leaves_its_result_unrecorded() {
    use sluice_seams::{RunOutcome, SessionRunKind};
    let h = Harness::bolt();
    let head = h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let last = |h: &Harness| {
        h.sluice().session_places().unwrap()[0]
            .last_result
            .clone()
            .expect("a last result")
    };

    // A clean run's results are in the log before its result is kept.
    h.session_run("unit/a");
    assert!(!h.results().is_empty());
    let len = h.w().results_log().len();
    assert_eq!(h.w().results_when_ran("session/unit/a"), Some(len));

    h.w().hangs("a:test");
    let changed = h.work_on("unit/a", &head, &[("a/lib", Some("a3"))]);
    let (report, recorded) = h.session_run("unit/a");
    assert!(report.timed_out);
    assert_eq!(recorded, sluice_core::session::Recorded::Nothing);
    let l = last(&h);
    assert_eq!(
        (l.outcome, l.recorded, &l.head),
        (RunOutcome::TimedOut, false, &changed)
    );

    let s = h.sluice();
    for outcome in [RunOutcome::Lost, RunOutcome::Stopped] {
        s.session_ended("session/unit/a", &changed, true, outcome)
            .unwrap();
        let l = last(&h);
        assert_eq!(
            (l.kind, l.outcome, l.recorded),
            (SessionRunKind::Command, outcome, false)
        );
    }
}

/// satisfies: S28, 92
#[test]
fn s28_a_dirty_session_run_records_nothing() {
    shared::s28_a_dirty_session_run_records_nothing(&Harness::bolt());
}

/// satisfies: S25, 71, 89, 91
#[test]
fn s25_sessions_runs_wait_and_a_submissions_run_starts_first() {
    // Four sessions on a host whose bound is two, two kept for
    // submissions' runs by none: two run, two wait.
    let h = Harness::bolt();
    h.w().host_limit(2);
    let s = h.sluice();
    let mut running = 0;
    for n in 0..4 {
        if s.session_capacity(0).unwrap() > 0 {
            h.w().session_run(&format!("/w/{n}"), true);
            running += 1;
        }
    }
    assert_eq!(running, 2);
    // A submission arriving now waits only until the bound has room, and
    // takes that room before the third session.
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    h.step();
    assert!(h.w().started().is_empty());
    h.w().kill_session("/w/0");
    h.step();
    assert_eq!(
        h.w().started().len(),
        1,
        "the submission's run took the room"
    );
    assert_eq!(s.session_capacity(0).unwrap(), 0, "the third session waits");

    // With one run of four reserved, three sessions run and a submission
    // still starts at once.
    let h = Harness::bolt();
    h.w().host_limit(4);
    let s = h.sluice();
    for n in 0..4 {
        if s.session_capacity(1).unwrap() > 0 {
            h.w().session_run(&format!("/w/{n}"), true);
        }
    }
    assert_eq!(s.session_capacity(1).unwrap(), 0);
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    h.step();
    assert_eq!(h.w().started().len(), 1);
}

/// satisfies: S27, 25, 94, 95
#[test]
fn s27_a_sessions_place_stays_until_it_is_released() {
    shared::s27_a_sessions_place_stays_until_it_is_released(&Harness::bolt());
}

/// satisfies: 25, 123; session-runs-1 95, 97
#[test]
fn a_removed_worktrees_place_is_freed_by_the_next_round() {
    shared::a_removed_worktrees_place_is_freed_by_the_next_round(&Harness::bolt());
}

/// satisfies: 123
#[test]
fn a_removed_worktrees_failing_release_is_freed_and_never_run_again() {
    shared::a_removed_worktrees_failing_release_is_freed_and_never_run_again(&Harness::bolt());
}

/// satisfies: 25, 32, 81, 5, 67; the obsolete-combinations spec's
/// "Withdrawn while under test" and "Its run still going"
#[test]
fn a_withdrawn_combinations_run_is_ended_and_its_checkout_released() {
    shared::a_withdrawn_combinations_run_is_ended_and_its_checkout_released(&Harness::bolt());
}

/// satisfies: 25, 81, 16; the obsolete-combinations spec's "Its run had
/// finished"
#[test]
fn a_withdrawn_combinations_passed_run_is_read_for_nothing() {
    shared::a_withdrawn_combinations_passed_run_is_read_for_nothing(&Harness::bolt());
}

/// satisfies: 17, 20, 25, 81; the obsolete-combinations spec's "Its run
/// had failed"
#[test]
fn a_withdrawn_combinations_failed_run_blames_no_one() {
    shared::a_withdrawn_combinations_failed_run_blames_no_one(&Harness::bolt());
}

/// satisfies: 123, 25; the obsolete-combinations spec's "Its release
/// fails"
#[test]
fn a_withdrawn_combinations_failed_release_holds_its_checkout() {
    shared::a_withdrawn_combinations_failed_release_holds_its_checkout(&Harness::bolt());
}

/// satisfies: S57, 121, 22, 45
#[test]
fn s57_a_branch_read_from_an_older_copy_is_held_until_restored_or_accepted() {
    for accept in [false, true] {
        let h = Harness::bolt();
        let mut landings = Vec::new();
        for (unit, file) in [("unit/1", "a/one"), ("unit/2", "b/two")] {
            h.work(unit, unit, &[(file, Some("x"))]);
            let id = h.submit(unit);
            h.settle();
            assert_eq!(h.sub(id).state, State::Landed);
            landings.push(h.tip());
        }
        let (first, second) = (landings[0].clone(), landings[1].clone());
        // The round now reads the branch from another copy, the remote's,
        // which a refused landing left at the first landing.
        h.branch_at(BRANCH, &first);
        h.work_on("unit/1", &first, &[("c/three", Some("x"))]);
        let three = h.submit("unit/1");
        let (forms, moves) = (h.w().forms(), h.moves());
        h.settle();
        assert_eq!((h.w().forms(), h.moves()), (forms, moves), "{accept}");
        assert_eq!(h.sub(three).state, State::Queued);
        let held: Vec<(sluice_seams::Rev, sluice_seams::Rev)> = h
            .events()
            .into_iter()
            .filter_map(|e| match e {
                Event::Held { read, lacks } => Some((read, lacks)),
                _ => None,
            })
            .collect();
        assert_eq!(held, vec![(first.clone(), second.clone())]);

        // The operator accepts the copy's revision, naming it, or puts
        // the second landing back on it; the queue is formed on it.
        let on = if accept {
            h.sluice()
                .accept_branch(BRANCH, first.as_str())
                .unwrap()
                .unwrap();
            first
        } else {
            h.branch_at(BRANCH, &second);
            second
        };
        h.settle();
        let s = h.sub(three);
        assert_eq!(s.state, State::Landed, "{accept}");
        assert_eq!(s.tested.unwrap().on, on);
        assert_eq!(h.w().parents(&h.tip())[0], on);
        assert_eq!(h.doc().held, None);
    }
}

/// satisfies: S64, 124, 122
#[test]
fn s64_a_levelling_git_holds_the_lock_on_is_tried_again_by_the_next_round() {
    let h = Harness::bolt();
    h.w().on_remote(BRANCH);
    let made = h.tip();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    // git holds the lock on this host's bolt/x while the landing is pushed,
    // and lets it go before the next round.
    let lock = "git holds the lock on refs/heads/bolt/x";
    h.w().fail_levels(1, lock);
    for _ in 0..8 {
        if h.sub(one).state == State::Landed {
            break;
        }
        h.step();
    }
    assert_eq!(h.sub(one).state, State::Landed);
    let landing = h.tip();
    assert_eq!(h.w().copy_of(BRANCH), Some(made.clone()));
    let behind = h
        .sluice()
        .status(BRANCH)
        .unwrap()
        .copy
        .expect("the copy behind");
    assert_eq!(
        (behind.at, behind.lacks, behind.why.as_str()),
        (Some(made), landing.clone(), lock)
    );

    h.step();
    assert_eq!(h.w().copy_of(BRANCH), Some(landing));
    assert_eq!(h.sluice().status(BRANCH).unwrap().copy, None);
}

/// satisfies: S58, 124, 1
#[test]
fn s58_a_bolts_landing_worktree_submits_the_bolt_as_its_remote_holds_it() {
    let h = Harness::new(&policy(false, "fail", 4).replace(
        "trust = [\"open\", \"sandbox\", \"container\"]\n",
        "trust = [\"open\", \"sandbox\", \"container\"]\nremote = \"origin\"\n",
    ));
    h.w().on_remote(BRANCH);
    let made = h.tip();
    for (unit, file) in [("unit/1", "a/one"), ("unit/2", "b/two")] {
        h.work(unit, unit, &[(file, Some("x"))]);
        let id = h.submit(unit);
        h.settle();
        assert_eq!(h.sub(id).state, State::Landed);
    }
    let remote = h.tip();
    // This host's bolt/x stands two landings behind, and a worktree has it
    // checked out.
    h.w().set_copy(BRANCH, Some(&made));
    h.w().session(BRANCH, "crew-landing", &made);
    h.w().init(
        "main",
        &[(
            ".config/sluice.toml",
            "[branches.main]\nrequired = [\"#gate\"]\n",
        )],
    );
    let moves = h.moves();
    let accepted = h
        .sluice()
        .submit(&sluice_core::SubmitRequest {
            head: sluice_core::SubmitHead::Worktree(BRANCH.into()),
            branch: "main".into(),
            after: Vec::new(),
            priority: 0,
            subject: None,
        })
        .unwrap()
        .expect("accepted");
    let sub = h.sluice().state(accepted.id).unwrap().unwrap();
    assert_eq!(sub.head, remote);
    assert_eq!(
        (h.tip(), h.moves()),
        (remote.clone(), moves),
        "nothing of the bolt written"
    );
    assert_eq!(h.w().copy_of(BRANCH), Some(made.clone()));
    let copy = accepted.copy.expect("the copy said");
    assert_eq!(
        (copy.copy, copy.at, copy.how),
        (made, remote.clone(), sluice_core::CopyHow::Behind)
    );

    // The bolt's next round levels this host's copy.
    h.step();
    assert_eq!(h.w().copy_of(BRANCH), Some(remote));
}

/// satisfies: S73, 124, 132, 148; rule core.47: a timer that sets this
/// host's copy of the branch itself every five minutes for two hours is
/// one command, logged once and counted 25 times; status and the overview
/// give it from the branch's state, a part's close that takes the log's
/// lines away changes nothing of it, and a day after its last refusal
/// neither gives it and the next write logs its end with its count.
#[test]
fn s73_a_timers_refused_writes_are_one_command_counted_and_shown_until_a_day_after_the_last() {
    use chrono::Duration;
    use sluice_core::Peer;
    use sluice_seams::{Changes, Rev};

    let h = Harness::bolt();
    let full = |n: u64| Rev::new(format!("{n:040x}"));
    let command =
        format!("git -C /home/me/app update-ref refs/heads/{BRANCH} <revision> <revision>");
    let host = Peer::default().name;
    let s = h.sluice();
    let state = s.seams().state;
    for run in 0..25 {
        let (new, old) = (full(2 * run + 2), full(2 * run + 1));
        let lock = state.lock(BRANCH).unwrap().expect("the lock is free");
        s.record_protected(
            BRANCH,
            &old,
            Some(&new),
            &format!("git -C /home/me/app update-ref refs/heads/{BRANCH} {new} {old}"),
        )
        .unwrap();
        drop(lock);
        if run < 24 {
            h.w().advance(Duration::minutes(5));
        }
    }
    let protected = |h: &Harness| -> Vec<Event> {
        h.events()
            .into_iter()
            .filter(|e| matches!(e, Event::Protected { .. } | Event::ProtectedEnded { .. }))
            .collect()
    };
    assert_eq!(protected(&h).len(), 1, "one line for the timer");
    let shown = |h: &Harness| {
        let status = h.sluice().status(BRANCH).unwrap().refused_writes;
        let overview = h
            .sluice()
            .overview(None)
            .unwrap()
            .into_iter()
            .find(|b| b.branch == BRANCH)
            .expect("the branch has records")
            .refused_writes;
        assert_eq!(status, overview, "status and the overview give the same");
        status
    };
    let cause = shown(&h);
    assert_eq!(cause.len(), 1);
    let cause = cause[0].clone();
    assert_eq!(
        (cause.host.as_str(), cause.command.as_str(), cause.times),
        (host.as_str(), command.as_str(), 25)
    );
    assert_eq!(
        (&cause.from, cause.to.as_ref()),
        (&full(49), Some(&full(50))),
        "the last refused write's revisions"
    );

    // A part's close takes the log's lines away; the count is the state's.
    {
        let _lock = state.lock(BRANCH).unwrap().expect("the lock is free");
        let log = h
            .w()
            .files(BRANCH)
            .remove("log.jsonl")
            .expect("the branch has a log");
        let mut close = Changes::default();
        close.put.insert("log/1.jsonl".into(), log);
        close.remove.insert("log.jsonl".into());
        state.write(BRANCH, h.version(), close).unwrap();
    }
    assert!(h.events().is_empty());
    assert_eq!(shown(&h), std::slice::from_ref(&cause));

    h.w().advance(Duration::days(1));
    assert!(
        shown(&h).is_empty(),
        "a day after the last, neither gives it"
    );
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    assert_eq!(
        protected(&h),
        [Event::ProtectedEnded {
            host,
            reference: format!("refs/heads/{BRANCH}"),
            command,
            times: 25,
            first: cause.first,
            last: cause.last,
        }]
    );
}

/// satisfies: S74, 15, 16, 20, 114, 149; record 0084
#[test]
fn s74_a_check_that_runs_nothing_is_failed_by_the_task_it_depends_on() {
    let h = Harness::new(&policy(false, "fail", 4).replace(
        "required = [\"#gate\"]",
        "required = [\"root:integration\"]",
    ));
    h.w().task("api:integration", "a/");
    h.w().task("root:integration", "r/");
    h.f.tests.depends_on("root:integration", "api:integration");
    h.fails_with("api:integration", "a/lib", "two tests fail");
    h.work("unit/a", "agent-a", &[("a/lib", Some("two tests fail"))]);
    let one = h.submit("unit/a");
    h.settle();
    let sub = h.sub(one);
    assert_eq!(sub.state, State::Failed);
    // Its session is told the task failed, with what it printed, and the
    // check it failed.
    match &h.delivered_to("agent-a")[0].decided {
        Decided::Failed { failed, logs, .. } => {
            assert_eq!(failed, &["api:integration", "root:integration"]);
            let logged: Vec<&str> = logs.iter().map(|l| l.task.as_str()).collect();
            assert_eq!(logged, ["api:integration"]);
        }
        other => panic!("delivered {other:?}"),
    }
    // A tool that reads the record finds a check that ran and failed.
    let dep = sub
        .failed
        .iter()
        .find(|f| f.task == "api:integration")
        .expect("the task that failed");
    assert!(!dep.hash.is_empty() && dep.log.is_some(), "{dep:?}");
    let check = sub
        .failed
        .iter()
        .find(|f| f.task == "root:integration")
        .expect("the check it failed");
    assert_eq!(check.failed_by, ["api:integration"]);
    assert!(check.hash.is_empty() && check.not_run.is_none());
    let lines = h.results();
    assert!(lines
        .iter()
        .any(|r| r.task == "api:integration" && r.hash == dep.hash && r.outcome == Outcome::Fail));
    assert!(lines.iter().all(|r| r.task != "root:integration"));

    // On another candidate the task exits as `[tests] not_run` names.
    h.w().cannot_run(
        "api:integration",
        Some("exited 77, which [tests] not_run names: it could not run here"),
    );
    h.work("unit/b", "agent-b", &[("a/lib", Some("a3"))]);
    let two = h.submit("unit/b");
    let lost = |h: &Harness| {
        h.events().into_iter().find_map(|e| match e {
            Event::Lost { not_run, .. } if !not_run.is_empty() => Some(not_run),
            _ => None,
        })
    };
    for _ in 0..8 {
        if lost(&h).is_some() {
            break;
        }
        h.step();
    }
    let not_run = lost(&h).expect("the run left its check not run");
    assert_eq!(not_run.len(), 1);
    assert_eq!(not_run[0].task, "root:integration");
    assert!(not_run[0]
        .reason
        .starts_with("its dependency api:integration could not run: exited 77"));
    assert_ne!(h.sub(two).state, State::Failed);
    assert!(h.delivered_to("agent-b").is_empty(), "nobody is blamed");
    assert!(h.results().iter().all(|r| r.task != "root:integration"));
    h.w().cannot_run("api:integration", None);
    h.settle();
    assert_eq!(h.sub(two).state, State::Landed, "tested again");
}
