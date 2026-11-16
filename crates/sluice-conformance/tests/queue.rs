//! Submission and order: A.1 and A.2, and the driver's operations.

use sluice_conformance::{shared, Harness, Rig, BRANCH};
use sluice_core::records::{Event, State};
use sluice_core::{AmendRefusal, Requester, WithdrawRefusal, Withdrew};

/// satisfies: 1, 3, 30, 47
#[test]
fn a_submission_names_its_after_priority_and_submitter_and_does_nothing_more() {
    shared::a_submission_names_its_after_priority_and_submitter_and_does_nothing_more(
        &Harness::bolt(),
    );
}

/// satisfies: 2
#[test]
fn a_place_holding_more_than_its_commits_is_refused_and_named() {
    shared::a_place_holding_more_than_its_commits_is_refused_and_named(&Harness::bolt());
}

/// satisfies: 67, 54, 5
#[test]
fn a_resubmission_withdraws_the_earlier_submission_which_stays_readable() {
    shared::a_resubmission_withdraws_the_earlier_submission_which_stays_readable(&Harness::bolt());
}

/// satisfies: 122, 5, 6
#[test]
fn work_another_landing_took_is_landed_as_the_commit_that_landed_it() {
    shared::work_another_landing_took_is_landed_as_the_commit_that_landed_it(&Harness::bolt());
}

/// satisfies: 122, 5, 6, 10: a combination under test whose work another
/// queue's landing took is dropped and its checkout released; its
/// submission is landed as that landing, and the one behind it is formed
/// again on the branch as moved.
#[test]
fn a_combination_whose_work_another_landing_took_is_dropped() {
    let h = Harness::bolt();
    h.w().hold_runs(true);
    let head = h.work("unit/a", "agent-a", &[("a/one", Some("1"))]);
    h.work("unit/b", "agent-b", &[("a/two", Some("2"))]);
    let one = h.submit("unit/a");
    let two = h.submit("unit/b");
    h.step();
    assert_eq!(h.tested(), vec![vec![one], vec![one, two]]);
    let landing = match h.sluice().seams().branch.form(&h.tip(), &head).unwrap() {
        sluice_seams::Formed::Revision(rev) => rev,
        other => panic!("{other:?}"),
    };
    h.branch_at(BRANCH, &landing);
    h.step();
    let sub = h.sub(one);
    assert_eq!(sub.state, State::Landed);
    assert_eq!(sub.landed_as, Some(landing.clone()));
    let doc = h.doc();
    assert!(doc
        .candidates
        .iter()
        .all(|c| c.on == landing && c.submissions == vec![two]));
    assert_eq!(h.sub(two).state, State::Testing);
    h.w().hold_runs(false);
    h.w().complete_all();
    h.settle();
    assert_eq!(h.sub(two).state, State::Landed);
    assert!(!h.sub(two).landed_elsewhere);
}

/// satisfies: 6
#[test]
fn priority_never_displaces_a_combination_under_test() {
    let h = Harness::bolt();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/one", Some("1"))]);
    h.work("unit/b", "agent-b", &[("a/two", Some("2"))]);
    let one = h.submit("unit/a");
    let two = h.submit("unit/b");
    h.step();
    assert_eq!(h.tested(), vec![vec![one], vec![one, two]]);

    // Submitted later with a higher priority: it goes behind what is
    // under test, never ahead of it.
    h.work("unit/c", "agent-c", &[("a/three", Some("3"))]);
    let three = h.try_submit("unit/c", &[], 9).unwrap().id;
    h.step();
    assert_eq!(
        h.tested(),
        vec![vec![one], vec![one, two], vec![one, two, three]]
    );

    // Among submissions not under test, priority orders.
    h.work("unit/d", "agent-d", &[("a/four", Some("4"))]);
    h.work("unit/e", "agent-e", &[("a/five", Some("5"))]);
    let four = h.submit("unit/d");
    let five = h.try_submit("unit/e", &[], 1).unwrap().id;
    h.w().hold_runs(false);
    h.w().complete_all();
    h.settle();
    let landed: Vec<u64> = h
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Landed { ids, .. } => Some(ids),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(landed, vec![one, two, three, five, four]);
}

/// satisfies: 7
#[test]
fn a_submission_waits_for_its_after_and_holds_no_one() {
    shared::a_submission_waits_for_its_after_and_holds_no_one(&Harness::bolt());
}

/// satisfies: 8, 10
#[test]
fn every_submission_is_tested_on_top_of_every_one_ahead() {
    shared::every_submission_is_tested_on_top_of_every_one_ahead(&Harness::bolt());
}

/// satisfies: 47b, 3, 6
#[test]
fn amend_changes_order_only_for_a_submission_not_under_test() {
    let h = Harness::new(&sluice_conformance::policy(false, "fail", 1));
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/one", Some("1"))]);
    h.work("unit/b", "agent-b", &[("a/two", Some("2"))]);
    h.work("unit/c", "agent-c", &[("a/three", Some("3"))]);
    let (one, two, three) = (h.submit("unit/a"), h.submit("unit/b"), h.submit("unit/c"));
    h.step();
    assert_eq!(h.tested(), vec![vec![one]]);

    let s = h.sluice();
    assert_eq!(
        s.amend(one, &Requester::Planner, None, Some(5)).unwrap(),
        Err(AmendRefusal::UnderTest)
    );
    // A session may not change another's order.
    assert_eq!(
        s.amend(three, &Requester::Session("agent-b".into()), None, Some(5))
            .unwrap(),
        Err(AmendRefusal::NotYours)
    );
    assert_eq!(
        s.amend(three, &Requester::Planner, None, Some(5)).unwrap(),
        Ok(())
    );
    assert_eq!(
        s.amend(
            two,
            &Requester::Session("agent-b".into()),
            Some(vec![three]),
            None
        )
        .unwrap(),
        Ok(())
    );
    h.w().hold_runs(false);
    h.w().complete_all();
    h.settle();
    let landed: Vec<u64> = h
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Landed { ids, .. } => Some(ids),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(landed, vec![one, three, two]);
    assert!(h
        .events()
        .iter()
        .any(|e| matches!(e, Event::Amended { id, .. } if *id == three)));
}

/// satisfies: 48, 5, 21, 25
#[test]
fn withdrawal_by_submitter_planner_or_operator_only() {
    let h = Harness::bolt();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/one", Some("1"))]);
    h.work("unit/b", "agent-b", &[("a/two", Some("2"))]);
    let (one, two) = (h.submit("unit/a"), h.submit("unit/b"));
    h.step();
    let s = h.sluice();
    assert_eq!(
        s.withdraw(one, &Requester::Session("agent-b".into()))
            .unwrap(),
        Err(WithdrawRefusal::NotYours)
    );
    assert_eq!(
        s.withdraw(one, &Requester::Session("agent-a".into()))
            .unwrap(),
        Ok(Withdrew::Now)
    );
    // Asked again, it was withdrawn already; still not another's to ask.
    assert_eq!(
        s.withdraw(one, &Requester::Session("agent-a".into()))
            .unwrap(),
        Ok(Withdrew::Already)
    );
    assert_eq!(
        s.withdraw(one, &Requester::Session("agent-b".into()))
            .unwrap(),
        Err(WithdrawRefusal::NotYours)
    );
    // Withdrawn while under test: its place is released and #2 is formed
    // again without it.
    h.step();
    assert!(h.tested().contains(&vec![two]));
    assert_eq!(h.w().live_places().len(), 1);
    h.w().hold_runs(false);
    h.w().complete_all();
    h.settle();
    assert_eq!(h.sub(two).state, State::Landed);
    assert_eq!(
        s.withdraw(two, &Requester::Operator).unwrap(),
        Err(WithdrawRefusal::Final(State::Landed))
    );
    assert_eq!(h.sub(one).withdrawn.unwrap().by, "agent-a");
}

/// satisfies: 49, 26
#[test]
fn state_and_status_read_the_records_with_no_step_running() {
    shared::state_and_status_read_the_records_with_no_step_running(&Harness::bolt());
}

/// satisfies: 71; the run-bound spec's "The bound is the host's, across
/// every repository": another repository's run on the host leaves one run
/// of a bound of two, whatever `main`'s window allows
#[test]
fn another_repositorys_run_leaves_less_of_the_hosts_bound() {
    let h = Harness::bolt();
    h.w().host_limit(2);
    h.w().hold_runs(true);
    h.w().other_runs(1);
    for n in 1..=3 {
        h.work(
            &format!("unit/{n}"),
            "agent",
            &[(&format!("a/{n}"), Some("ok"))],
        );
        h.submit(&format!("unit/{n}"));
    }
    h.step();
    assert_eq!(h.w().started().len(), 1);
    assert_eq!(h.w().live_runs(), 1);
}

/// The run states of `main`'s candidates' runs, with whether each has a
/// place and a claim.
fn run_states(h: &Harness) -> Vec<(sluice_core::records::RunState, bool, bool)> {
    let s = h.sluice();
    let line =
        sluice_core::line::Line::load(BRANCH, s.seams().state.read(BRANCH).unwrap()).unwrap();
    line.doc
        .candidates
        .iter()
        .flat_map(|c| &c.runs)
        .map(|r| (r.state, r.place.is_some(), r.claim.is_some()))
        .collect()
}

/// satisfies: 71, 16; record 0122: the run-bound spec's "Two rounds race
/// for one free run": a start the check seam refuses at the host's bound
/// runs nothing, releases its place, records no run and blames no one; the
/// run ends with no outcome, and a later round starts its follow-up
#[test]
fn a_start_refused_at_the_bound_waits_for_a_later_round() {
    use sluice_core::records::RunState;
    let h = Harness::bolt();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    // Another repository takes the host's last run after this round counts.
    h.w().refuse_next_start();
    h.step();
    assert!(h.w().started().is_empty(), "nothing of it ran");
    assert!(h.w().live_places().is_empty(), "its place was released");
    assert_eq!(run_states(&h), [(RunState::Closed, false, false)]);
    assert!(h.w().results_log().is_empty(), "no run is recorded");
    assert!(!h
        .events()
        .iter()
        .any(|e| matches!(e, Event::Blamed { .. } | Event::Ran { .. })));
    assert_eq!(h.sub(one).state, State::Testing);

    h.step();
    assert_eq!(h.w().started().len(), 1);
    assert_eq!(
        run_states(&h),
        [
            (RunState::Closed, false, false),
            (RunState::Running, true, false)
        ]
    );
    h.w().hold_runs(false);
    h.w().complete_all();
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: 71; record 0122: a start refused at the bound starts no
/// further run that round: the run decided behind it ends with no outcome,
/// no place made for it, and its follow-up starts in a later round
#[test]
fn a_refused_start_starts_no_further_run_that_round() {
    use sluice_core::records::RunState;
    let h = Harness::bolt();
    h.w().host_limit(2);
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    h.submit("unit/a");
    h.submit("unit/b");
    h.w().refuse_next_start();
    h.step();
    assert!(h.w().started().is_empty());
    assert_eq!(h.w().made_places().len(), 1, "only the refused run's place");
    assert_eq!(
        run_states(&h),
        [
            (RunState::Closed, false, false),
            (RunState::Closed, false, false)
        ]
    );
    h.step();
    assert_eq!(h.w().started().len(), 2);
}

/// 124: a branch sluice lands on, submitted by name from no worktree, as
/// its policy holds it, and the branches it is refused for, writing
/// nothing (record 0090; rule core.50).
mod by_name {
    use sluice_conformance::{policy, Harness, Rig, BRANCH};
    use sluice_core::records::State;
    use sluice_core::SubmitRefusal;
    use sluice_seams::{Rev, Submitter};

    /// Another bolt, a branch sluice lands on by the policy's `bolt/*`.
    const NAMED: &str = "bolt/w";

    /// `NAMED` at a commit on the target's revision applying `changes`;
    /// its revision.
    fn named_at(h: &Harness, changes: &[(&str, Option<&str>)]) -> Rev {
        let at = h.w().work(&h.tip(), changes);
        h.branch_at(NAMED, &at);
        at
    }

    /// The target's records as they stand, to show a refusal wrote none.
    fn records(h: &Harness) -> (u64, std::collections::BTreeMap<String, Vec<u8>>) {
        (h.version(), h.files())
    }

    /// satisfies: 124, 1, 47, 51, core.50
    #[test]
    fn a_branch_by_name_is_submitted_at_its_revision() {
        let h = Harness::bolt();
        let at = named_at(&h, &[("a/lib", Some("a2"))]);
        // A worktree beside it holding changes beyond its commits: nothing
        // of any worktree is read for a branch named.
        h.work("unit/a", "agent-a", &[("b/lib", Some("b2"))]);
        h.dirty("unit/a", &["untracked"]);
        let tip = h.tip();
        let accepted = h.try_submit_by_name(NAMED).expect("accepted");
        assert_eq!(accepted.ahead, Vec::<u64>::new());
        assert_eq!(accepted.withdrew, Vec::<u64>::new());
        assert_eq!(accepted.warnings, Vec::<String>::new());
        assert_eq!(accepted.copy, None);
        let sub = h.sub(accepted.id);
        assert_eq!(sub.head, at);
        assert_eq!(sub.state, State::Queued);
        assert_eq!(sub.attempt, 1);
        assert_eq!(
            sub.from,
            Submitter {
                agent: NAMED.into(),
                worktree: NAMED.into(),
                path: None,
                head: Some(NAMED.into()),
                host: Some(h.host()),
                session: None,
            }
        );
        assert!(sub.from.by_name());
        assert_eq!(h.tip(), tip, "the target is where it was");
        assert_eq!(h.moves(), 0);

        // The agent the session's harness names, where it names one.
        h.w().harness_names(Some("planner"));
        let again = h.try_submit_by_name(NAMED).expect("accepted");
        assert_eq!(h.sub(again.id).from.agent, "planner");
        assert_eq!(again.withdrew, vec![accepted.id]);

        h.settle();
        assert_eq!(h.sub(again.id).state, State::Landed);
        assert_eq!(h.file(&h.tip(), "a/lib").as_deref(), Some("a2"));
        assert_eq!(h.w().tip(NAMED), at, "the branch named is where it was");
    }

    /// satisfies: 124, 1, core.4, core.50: kept on a remote, the remote's
    /// revision, with nothing said of this host's copy and nothing moved.
    #[test]
    fn a_branch_by_name_on_a_remote_is_submitted_at_the_remotes_revision() {
        let h = Harness::new(&policy(false, "fail", 4).replace(
            "trust = [\"open\", \"sandbox\", \"container\"]\n",
            "trust = [\"open\", \"sandbox\", \"container\"]\nremote = \"origin\"\n",
        ));
        h.w().on_remote(BRANCH);
        let copy = named_at(&h, &[("a/lib", Some("a2"))]);
        h.w().on_remote(NAMED);
        h.w().set_copy(NAMED, Some(&copy));
        let remote = h.w().work(&copy, &[("b/lib", Some("b2"))]);
        h.branch_at(NAMED, &remote);
        let accepted = h.try_submit_by_name(NAMED).expect("accepted");
        assert_eq!(h.sub(accepted.id).head, remote);
        assert_eq!(accepted.copy, None, "nothing said of this host's copy");
        assert_eq!(h.w().copy_of(NAMED), Some(copy), "this host's copy unmoved");
        assert_eq!(h.w().tip(NAMED), remote, "the remote's branch unmoved");
        assert_eq!(h.moves(), 0);
    }

    /// satisfies: 124, 2, core.50
    #[test]
    fn a_branch_sluice_lands_on_nowhere_is_refused_by_name() {
        let h = Harness::bolt();
        let at = h.w().work(&h.tip(), &[("a/lib", Some("a2"))]);
        h.branch_at("unit/a", &at);
        let before = records(&h);
        assert_eq!(
            h.try_submit_by_name("unit/a"),
            Err(SubmitRefusal::NotLandedOn)
        );
        assert_eq!(records(&h), before, "nothing written");
    }

    /// satisfies: 124, core.50
    #[test]
    fn a_branch_is_not_submitted_to_itself() {
        let h = Harness::bolt();
        let before = records(&h);
        assert_eq!(h.try_submit_by_name(BRANCH), Err(SubmitRefusal::ToItself));
        assert_eq!(records(&h), before, "nothing written");
    }

    /// satisfies: 124, core.50
    #[test]
    fn no_branch_to_submit_by_name() {
        let h = Harness::bolt();
        let before = records(&h);
        assert_eq!(
            h.try_submit_by_name("bolt/gone"),
            Err(SubmitRefusal::NoHead("bolt/gone".into()))
        );
        assert_eq!(records(&h), before, "nothing written");
    }

    /// satisfies: 67, 54, 5, 124: a retry by name after a fix lands on the
    /// branch withdraws the blamed submission, and counts its attempt.
    #[test]
    fn a_retry_by_name_withdraws_the_blamed_one() {
        let h = Harness::bolt();
        h.fails_with("a:test", "a/lib", "broken");
        let broken = named_at(&h, &[("a/lib", Some("broken"))]);
        let first = h.submit_by_name(NAMED);
        h.settle();
        assert_eq!(h.sub(first).state, State::Failed);

        let fixed = h.w().work(&broken, &[("a/lib", Some("fixed"))]);
        h.branch_at(NAMED, &fixed);
        let again = h.try_submit_by_name(NAMED).expect("accepted");
        assert_eq!(again.withdrew, vec![first]);
        let old = h.sub(first);
        assert_eq!(old.state, State::Withdrawn);
        assert_eq!(old.withdrawn.map(|w| w.by).as_deref(), Some("resubmission"));
        let new = h.sub(again.id);
        assert_eq!(new.head, fixed);
        assert_eq!(new.attempt, 2);
        h.settle();
        assert_eq!(h.sub(again.id).state, State::Landed);
    }

    /// satisfies: 67, 54, 124: a submission by name and one from a
    /// worktree on its branch are one work, each withdrawing the other's.
    #[test]
    fn a_worktree_on_the_branch_withdraws_a_submission_by_name() {
        let h = Harness::bolt();
        let at = named_at(&h, &[("a/lib", Some("a2"))]);
        let by_name = h.submit_by_name(NAMED);
        // A worktree that has checked out the branch named.
        h.w().session(NAMED, "agent-w", &at);
        let from_worktree = h.try_submit(NAMED, &[], 0).expect("accepted");
        assert_eq!(from_worktree.withdrew, vec![by_name]);
        let old = h.sub(by_name);
        assert_eq!(old.state, State::Withdrawn);
        assert_eq!(old.withdrawn.map(|w| w.by).as_deref(), Some("resubmission"));
        assert_eq!(h.sub(from_worktree.id).attempt, 2);
        assert!(!h.sub(from_worktree.id).from.by_name());

        // And by name again, after it.
        let again = h.try_submit_by_name(NAMED).expect("accepted");
        assert_eq!(again.withdrew, vec![from_worktree.id]);
        assert_eq!(h.sub(from_worktree.id).state, State::Withdrawn);
        assert_eq!(h.sub(again.id).attempt, 3);
    }
}
