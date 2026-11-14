//! The scenarios that need no fault, written once over any [`Rig`]: each
//! test that runs one cites the clauses it exercises, here on the fakes
//! (`tests/`) and in the `sluice` crate on the real profiles.

use sluice_core::line::Line;
use sluice_core::policy::POLICY_PATH;
use sluice_core::preview::{PreviewRefusal, PreviewState};
use sluice_core::records::{
    parse_lines, to_document, BranchDoc, Event, LogLine, ResultKind, State, Submission,
    BRANCH_FILE, BRANCH_SCHEMA, SUBMISSION_SCHEMA,
};
use sluice_core::session::Recorded;
use sluice_core::{Forecast, Lookup, Requester, Sluice, SubmitRefusal};
use sluice_seams::{
    Changes, Decided, LastResult, Outcome, Progress, ReleasePolicy, Released, Rev, RunHandle,
    RunOutcome, SessionPlace, SessionRunKind, Written,
};

use crate::{Rig, BRANCH};

/// S1: two heads pass and land in order, each told only its own landing.
pub fn s1_two_heads_pass_and_land_in_order(h: &impl Rig) {
    let base = h.tip();
    let a = h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let b = h.work("unit/b", "agent-b", &[("a/notes", Some("from b"))]);
    let one = h.submit("unit/a");
    let two = h.submit("unit/b");

    // Submitting moves nothing and runs nothing (1).
    assert_eq!(h.tip(), base);
    assert!(h.started().is_empty());

    h.settle();

    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.sub(two).state, State::Landed);
    // Each tested exactly as it would land: the branch, then A, then B (10).
    assert_eq!(h.tested(), vec![vec![one], vec![one, two]]);
    // One commit per submission, in order, the head as second parent (23).
    let history = h.first_parents();
    let (k2, k1) = (&history[0], &history[1]);
    assert_eq!(history[2], base);
    assert_eq!(h.parents(k1), vec![base.clone(), a]);
    assert_eq!(h.parents(k2), vec![k1.clone(), b]);
    assert!(h
        .message(k1)
        .starts_with(&format!("chore(land): #{one} unit/a\n")));
    // Each session learns its own landing, and only its own (50).
    let ta = h.delivered_to("agent-a");
    assert_eq!(ta.len(), 1);
    assert_eq!(
        ta[0].decided,
        Decided::Landed {
            landed_as: k1.clone(),
            beside: false,
            covered_by: None,
        }
    );
    assert_eq!(h.delivered_to("agent-b")[0].submission, two);
    // Every place made was released (33, 34).
    assert_eq!(h.made_places().len(), 2);
    assert!(h.live_places().is_empty());
}

/// S2: a conflict is told at forming, before any check runs on it.
pub fn s2_a_conflict_is_told_before_any_check_runs(h: &impl Rig) {
    h.work("unit/a", "agent-a", &[("a/lib", Some("from a"))]);
    h.work("unit/b", "agent-b", &[("a/lib", Some("from b"))]);
    let one = h.submit("unit/a");
    let two = h.submit("unit/b");

    h.step();
    // Delivered at forming, before any run on a combination holding it.
    let delivered = h.delivered_to("agent-b");
    assert_eq!(delivered.len(), 1);
    match &delivered[0].decided {
        Decided::Conflict { paths, with } => {
            assert_eq!(paths, &vec!["a/lib".to_string()]);
            assert_eq!(with.len(), 1);
            assert_eq!(with[0].id, one);
            assert_eq!(with[0].from.worktree, "unit/a");
        }
        other => panic!("delivered {other:?}"),
    }
    assert!(h.tested().iter().all(|c| !c.contains(&two)));
    assert_eq!(h.sub(two).state, State::Conflict);

    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert!(h
        .delivered_to("agent-a")
        .iter()
        .all(|n| n.decided.word() == "landed"));
}

/// S3: the second of three fails; only its session hears, and the third
/// lands.
pub fn s3_the_second_fails_and_only_its_session_hears(h: &impl Rig) {
    h.fails_with("a:test", "a/bad", "yes");
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.work("unit/b", "agent-b", &[("a/bad", Some("yes"))]);
    h.work("unit/c", "agent-c", &[("c/lib", Some("c2"))]);
    let one = h.submit("unit/a");
    let two = h.submit("unit/b");
    let three = h.submit("unit/c");

    h.settle();

    assert_eq!(h.sub(one).state, State::Landed);
    let failed = h.sub(two);
    assert_eq!(failed.state, State::Failed);
    assert_eq!(failed.failed[0].task, "a:test");
    let delivered = h.delivered_to("agent-b");
    assert_eq!(delivered.len(), 1);
    match &delivered[0].decided {
        Decided::Failed {
            failed: tasks,
            submissions,
            logs,
            ..
        } => {
            assert_eq!(tasks, &vec!["a:test".to_string()]);
            assert_eq!(submissions, &vec![one, two]);
            // 20: told where the failed task's output is kept, readable
            // with no sluice running, and so is its record.
            assert_eq!(logs.len(), 1);
            assert_eq!(logs[0].task, "a:test");
            assert_eq!(failed.failed[0].log.as_deref(), Some(logs[0].text.as_str()));
            assert!(!h.kept_log(&logs[0].text).unwrap_or_default().is_empty());
        }
        other => panic!("delivered {other:?}"),
    }
    // No other session hears of the failure (20).
    for agent in ["agent-a", "agent-c"] {
        assert!(h
            .delivered_to(agent)
            .iter()
            .all(|n| n.decided.word() == "landed"));
    }
    assert_eq!(h.sub(three).state, State::Landed);
}

/// S4: the first fails; the second is formed again without it and lands.
pub fn s4_the_first_fails_and_the_second_is_formed_again_without_it(h: &impl Rig) {
    let base = h.tip();
    h.fails_with("a:test", "a/lib", "broken");
    h.work("unit/a", "agent-a", &[("a/lib", Some("broken"))]);
    h.work("unit/b", "agent-b", &[("a/more", Some("b"))]);
    let one = h.submit("unit/a");
    let two = h.submit("unit/b");

    h.settle();

    assert_eq!(h.sub(one).state, State::Failed);
    assert_eq!(h.delivered_to("agent-a")[0].decided.word(), "failed");
    let landed = h.sub(two);
    assert_eq!(landed.state, State::Landed);
    // Tested again as the branch and B alone (10, 21).
    assert_eq!(landed.tested.as_ref().unwrap().submissions, vec![two]);
    assert!(h.tested().contains(&vec![two]));
    let tip = h.tip();
    assert_eq!(h.file(&tip, "a/lib").as_deref(), Some("a1"));
    assert_eq!(h.file(&tip, "a/more").as_deref(), Some("b"));
    // B's is the branch's only landing.
    assert_eq!(h.parents(&tip)[0], base);
}

/// S9: deepest first; on its failure the skipped candidates are tested
/// before any blame. The rig's policy batches.
pub fn s9_deepest_first_finds_the_culprit_by_testing_what_it_skipped(h: &impl Rig) {
    h.fails_with("a:test", "a/two", "bad");
    h.work("unit/1", "agent-1", &[("a/one", Some("1"))]);
    h.work("unit/2", "agent-2", &[("a/two", Some("bad"))]);
    h.work("unit/3", "agent-3", &[("a/three", Some("3"))]);
    let (one, two, three) = (h.submit("unit/1"), h.submit("unit/2"), h.submit("unit/3"));

    h.step();
    // Only the deepest is tested.
    assert_eq!(h.tested(), vec![vec![one, two, three]]);
    h.settle();

    // It failed; the skipped ones were tested before any blame (14, 18).
    let tested = h.tested();
    assert!(tested.contains(&vec![one]));
    assert!(tested.contains(&vec![one, two]));
    let blamed: Vec<u64> = h
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Blamed { id, .. } => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(blamed, vec![two]);
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.sub(two).state, State::Failed);
    assert_eq!(h.sub(three).state, State::Landed);
}

/// S11: a landing names its verdict, and sluice releases its own place,
/// never the session's.
pub fn s11_a_landing_names_its_verdict_and_leaves_the_session_place(h: &impl Rig) {
    let head = h.work("unit/touches", "crw-1-unit-1", &[("a/lib", Some("a2"))]);
    let id = h.submit("unit/touches");
    h.settle();

    let s = h.sub(id);
    let landed = s.landed_as.clone().unwrap();
    assert_eq!(h.tip(), landed);
    assert_eq!(h.parents(&landed)[1], head);
    let tested = s.tested.unwrap().combination;
    assert_eq!(
        h.message(&landed),
        format!(
            "chore(land): #{id} unit/touches\n\nSluice-Submission: {id}\n\
             Sluice-From: crw-1-unit-1 (unit/touches)\nSluice-Tested: {tested}\n\
             Sluice-Result: passed (3 run, 0 replayed)\n"
        )
    );
    // The branch had no verdict, so every required check ran (70).
    // The landed tree is the tested tree (I1).
    assert_eq!(h.tree(&landed), h.tree(&tested));
    // sluice released its own places and never the session's (25).
    assert!(!h.made_places().is_empty());
    assert!(h.live_places().is_empty());
    assert!(!h.made_places().contains(&h.place("unit/touches")));
}

/// S12: a submission is judged by the gate on the branch, not by the one
/// it carries.
pub fn s12_a_submission_cannot_loosen_the_gate_it_is_judged_by(h: &impl Rig) {
    h.fails_with("a:test", "a/lib", "broken");
    let loosened = h.policy_text(&crate::policy(false, "fail", 4).replace("[\"#gate\"]", "[]"));
    h.work(
        "unit/a",
        "agent-a",
        &[
            ("a/lib", Some("broken")),
            (".config/sluice.toml", Some(&loosened)),
        ],
    );
    let id = h.submit("unit/a");
    h.settle();
    // Judged by the policy on the branch, so it fails and does not land.
    assert_eq!(h.sub(id).state, State::Failed);
    assert_eq!(h.requests()[0].required, vec!["#gate".to_string()]);
}

/// S59, 125: three heads submitted as one act on a branch that batches
/// are formed and tested once, as one combination, and land in one move
/// in the order given; thereafter each is its own submission: the middle
/// one conflicting with the first is set aside alone, naming it (12, 65),
/// the middle one failing is blamed alone (17, 18), one withdrawn leaves
/// alone (5), and each session is told its own outcome (50). The rig's
/// policy batches.
pub fn s59_heads_submitted_as_one_act_are_formed_together_and_each_its_own(h: &impl Rig) {
    let base = h.tip();
    h.work("unit/1", "agent-1", &[("a/one", Some("1"))]);
    h.work("unit/2", "agent-2", &[("b/two", Some("2"))]);
    h.work("unit/3", "agent-3", &[("c/three", Some("3"))]);
    let ids = h.submit_group(&["unit/1", "unit/2", "unit/3"]);
    // Numbered in the order given, none after another of the act (6).
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
    for id in &ids {
        assert!(h.sub(*id).after.is_empty());
    }
    assert!(h.started().is_empty(), "submitting runs nothing (1)");

    h.step();
    // One combination of all three is formed, and none of fewer.
    assert_eq!(h.tested(), vec![ids.clone()]);
    h.settle();
    assert_eq!(h.tested(), vec![ids.clone()], "tested once");
    assert_eq!(h.moves(), 1, "landed in one move");
    let history = h.first_parents();
    assert_eq!(history[3], base);
    for (k, id) in ids.iter().enumerate() {
        let s = h.sub(*id);
        assert_eq!(s.state, State::Landed);
        // In the order given: the first's landing is nearest the base.
        assert_eq!(s.landed_as.as_ref(), Some(&history[2 - k]));
        let delivered = h.delivered_to(&format!("agent-{}", k + 1));
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].submission, *id);
        assert_eq!(delivered[0].decided.word(), "landed");
    }

    // The middle one conflicts with the first as the combinations form.
    let tested = h.tested().len();
    h.work("unit/4", "agent-4", &[("a/lib", Some("four"))]);
    h.work("unit/5", "agent-5", &[("a/lib", Some("five"))]);
    h.work("unit/6", "agent-6", &[("c/lib", Some("six"))]);
    let ids = h.submit_group(&["unit/4", "unit/5", "unit/6"]);
    h.settle();
    let middle = h.sub(ids[1]);
    assert_eq!(middle.state, State::Conflict);
    let conflict = middle.conflict.unwrap();
    assert_eq!(conflict.paths, vec!["a/lib".to_string()]);
    let with: Vec<(u64, String)> = conflict
        .with
        .iter()
        .map(|c| (c.id, c.from.worktree.clone()))
        .collect();
    assert_eq!(with, vec![(ids[0], h.place("unit/4"))]);
    match &h.delivered_to("agent-5")[..] {
        [n] => {
            assert!(matches!(&n.decided, Decided::Conflict { with, .. } if with[0].id == ids[0]))
        }
        other => panic!("delivered {other:?}"),
    }
    assert!(h.tested()[tested..].iter().all(|c| !c.contains(&ids[1])));
    for id in [ids[0], ids[2]] {
        assert_eq!(h.sub(id).state, State::Landed);
    }

    // The middle one fails, and it alone is blamed.
    h.fails_with("b:test", "b/bad", "yes");
    let tested = h.tested().len();
    h.work("unit/7", "agent-7", &[("a/seven", Some("7"))]);
    h.work("unit/8", "agent-8", &[("b/bad", Some("yes"))]);
    h.work("unit/9", "agent-9", &[("c/nine", Some("9"))]);
    let ids = h.submit_group(&["unit/7", "unit/8", "unit/9"]);
    h.settle();
    let blamed: Vec<u64> = h
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Blamed { id, .. } => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(blamed, vec![ids[1]]);
    assert_eq!(h.sub(ids[1]).state, State::Failed);
    assert_eq!(h.delivered_to("agent-8")[0].decided.word(), "failed");
    // The third is tested again without it, and both others land.
    assert!(h.tested()[tested..]
        .iter()
        .any(|c| c.contains(&ids[2]) && !c.contains(&ids[1])));
    for (id, agent) in [(ids[0], "agent-7"), (ids[2], "agent-9")] {
        assert_eq!(h.sub(id).state, State::Landed);
        assert!(h
            .delivered_to(agent)
            .iter()
            .all(|n| n.decided.word() == "landed"));
    }

    // The first withdrawn leaves alone; the other lands without it.
    let tested = h.tested().len();
    h.work("unit/10", "agent-10", &[("a/ten", Some("10"))]);
    h.work("unit/11", "agent-11", &[("b/eleven", Some("11"))]);
    let ids = h.submit_group(&["unit/10", "unit/11"]);
    h.withdraw(ids[0], Requester::Session("agent-10".into()));
    h.settle();
    assert_eq!(h.sub(ids[0]).state, State::Withdrawn);
    assert_eq!(h.sub(ids[1]).state, State::Landed);
    assert!(h.tested()[tested..].iter().all(|c| !c.contains(&ids[0])));
}

/// 64, 65, 47a: a branch no place holds, a bolt's, is forecast against
/// the branch it lands on: the file, the submission it meets, the
/// revision it was formed on and those ahead of it, and nothing written.
/// Its own submission is left out, as submitting it again would withdraw
/// it (67).
pub fn a_branch_is_forecast_with_no_place_of_it(h: &impl Rig) {
    h.work("unit/a", "agent-a", &[("a/lib", Some("from a"))]);
    let one = h.submit("unit/a");
    let tip = h.tip();
    let first = h.work("bolt/y", "conductor", &[("b/lib", Some("from y"))]);
    h.branch_at("bolt/y", &first);
    h.submit("bolt/y");
    let head = h.work_on("bolt/y", &first, &[("a/lib", Some("from y"))]);
    h.branch_at("bolt/y", &head);

    let version = h.version();
    let f = h
        .sluice()
        .forecast_branch("bolt/y", BRANCH, &[])
        .unwrap()
        .unwrap();
    assert_eq!(f.head, head);
    assert_eq!(f.onto, tip);
    assert_eq!(f.ahead, vec![one]);
    match f.forecast {
        Forecast::Conflict { paths, with } => {
            assert_eq!(paths, vec!["a/lib".to_string()]);
            assert_eq!(with.len(), 1);
            assert_eq!(with[0].id, one);
        }
        other => panic!("forecast {other:?}"),
    }
    assert_eq!(h.version(), version);
    assert!(h.started().is_empty());

    // A branch that is not there is refused, naming it.
    assert_eq!(
        h.sluice()
            .forecast_branch("bolt/gone", BRANCH, &[])
            .unwrap(),
        Err(SubmitRefusal::NoHead("bolt/gone".into()))
    );
}

/// S16: a forecast names the file and the work it would conflict with,
/// and writes nothing.
pub fn s16_a_forecast_names_the_file_and_the_work_and_writes_nothing(h: &impl Rig) {
    h.work("unit/a", "agent-a", &[("a/lib", Some("from a"))]);
    let one = h.submit("unit/a");
    h.work("unit/b", "agent-b", &[("a/lib", Some("from b"))]);

    let version = h.version();
    let f = h
        .sluice()
        .forecast(&h.place("unit/b"), BRANCH, &[])
        .unwrap()
        .unwrap()
        .forecast;
    match f {
        Forecast::Conflict { paths, with } => {
            assert_eq!(paths, vec!["a/lib".to_string()]);
            assert_eq!(with[0].id, one);
            assert_eq!(with[0].from.agent, "agent-a");
        }
        other => panic!("forecast {other:?}"),
    }
    // A forecast creates no entry and changes nothing (64).
    assert_eq!(h.version(), version);
    assert!(h.started().is_empty());

    // The session settles it on the first's work; both form and land.
    let a = h.sub(one).head;
    h.work_on("unit/b", &a, &[("a/lib", Some("from a and b"))]);
    assert_eq!(
        h.sluice()
            .forecast(&h.place("unit/b"), BRANCH, &[])
            .unwrap()
            .unwrap()
            .forecast,
        Forecast::Clear
    );
    let two = h.submit("unit/b");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.sub(two).state, State::Landed);
    assert_eq!(h.file(&h.tip(), "a/lib").as_deref(), Some("from a and b"));
}

/// 8, 10: every submission is tested on top of every one ahead of it.
pub fn every_submission_is_tested_on_top_of_every_one_ahead(h: &impl Rig) {
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    h.work("unit/c", "agent-c", &[("a/more", Some("c"))]);
    let (one, two, three) = (h.submit("unit/a"), h.submit("unit/b"), h.submit("unit/c"));
    let stepped = h.step();
    assert_eq!(
        h.tested(),
        vec![vec![one], vec![one, two], vec![one, two, three]],
        "{stepped:?}: {:#?}",
        h.events()
    );
    h.settle();
}

/// 49, 26: state and status read the records, with no step running.
pub fn state_and_status_read_the_records_with_no_step_running(h: &impl Rig) {
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.settle();
    let status = h.sluice().status(BRANCH).unwrap();
    assert_eq!(status.at, Some(h.tip()));
    assert_eq!(status.parallel, Some(4));
    // Landed, it is among the finished records, read by its number (130, 131).
    assert!(status.submissions.is_empty());
    assert_eq!(status.finished, 1);
    assert_eq!(h.sluice().history(BRANCH).unwrap().len(), 1);
    let s = h.sluice().state(one).unwrap().unwrap();
    assert_eq!(s.state, State::Landed);
    assert_eq!(s.result.as_deref(), Some("passed"));
    assert!(s.tested.is_some());
}

/// 67, 54, 5: a resubmission withdraws the earlier one, which stays
/// readable.
pub fn a_resubmission_withdraws_the_earlier_submission_which_stays_readable(h: &impl Rig) {
    h.fails_with("a:test", "a/lib", "broken");
    h.work("unit/a", "agent-a", &[("a/lib", Some("broken"))]);
    let first = h.submit("unit/a");
    h.settle();
    // Blamed, it stays in the records until withdrawn (5).
    assert_eq!(h.sub(first).state, State::Failed);

    h.work("unit/a", "agent-a", &[("a/lib", Some("fixed"))]);
    let again = h.try_submit("unit/a", &[], 0).unwrap();
    assert_eq!(again.withdrew, vec![first]);
    let old = h.sub(first);
    assert_eq!(old.state, State::Withdrawn);
    assert_eq!(old.result.as_deref(), Some("failed"));
    // The count of earlier submissions is the record's; never bounded (54).
    assert_eq!(h.sub(again.id).attempt, 2);
    h.work("unit/a", "agent-a", &[("a/lib", Some("fixed again"))]);
    let third = h.submit("unit/a");
    assert_eq!(h.sub(third).attempt, 3);
    assert_eq!(h.sub(again.id).state, State::Withdrawn);
    h.settle();
    assert_eq!(h.sub(third).state, State::Landed);
}

/// 122, 5, 6: a submission whose work the branch took by a landing not its
/// own, as another queue of the branch lands it, and then moved on past, is
/// landed as the commit that landed it, logged as found so and delivered as
/// any landing; nothing is formed, run or landed of it, and it is never
/// withdrawn.
pub fn work_another_landing_took_is_landed_as_the_commit_that_landed_it(h: &impl Rig) {
    let head = h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let id = h.submit("unit/a");
    let landing = match h.sluice().seams().branch.form(&h.tip(), &head).unwrap() {
        sluice_seams::Formed::Revision(rev) => rev,
        other => panic!("{other:?}"),
    };
    h.branch_at(BRANCH, &landing);
    h.elsewhere(&[("later", Some("later"))]);
    let (moves, started) = (h.moves(), h.started().len());
    h.settle();
    let sub = h.sub(id);
    assert_eq!(sub.state, State::Landed, "{sub:?}");
    assert_eq!(sub.landed_as.as_ref(), Some(&landing));
    assert!(sub.landed_elsewhere);
    assert_eq!(h.started().len(), started, "nothing is run of it");
    assert_eq!(h.moves(), moves, "nothing is landed of it");
    assert!(h.events().iter().any(
        |e| matches!(e, Event::FoundLanded { ids, to } if *ids == vec![id] && *to == landing)
    ));
    assert!(!h
        .events()
        .iter()
        .any(|e| matches!(e, Event::Withdrawn { id: w, .. } if *w == id)));
    let told = h.delivered_to("agent-a");
    assert!(
        told.iter().any(|n| n.submission == id
            && matches!(&n.decided, Decided::Landed { landed_as, .. } if *landed_as == landing)),
        "{told:?}"
    );
}

/// 7: a submission waits for its `after`, and holds no one behind it.
pub fn a_submission_waits_for_its_after_and_holds_no_one(h: &impl Rig) {
    h.fails_with("a:test", "a/lib", "broken");
    h.work("unit/a", "agent-a", &[("a/lib", Some("broken"))]);
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Failed);

    // After #1, which is neither landed nor ahead of it in the chain.
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let two = h.try_submit("unit/b", &[one], 0).unwrap().id;
    h.work("unit/c", "agent-c", &[("c/lib", Some("c2"))]);
    let three = h.submit("unit/c");
    h.settle();
    assert_eq!(h.sub(two).state, State::Waiting);
    // The one behind it is tested and lands without it.
    assert!(h.tested().contains(&vec![three]));
    assert_eq!(h.sub(three).state, State::Landed);
}

/// 1, 3, 30, 47: a submission names its after, priority and submitter,
/// and does nothing more.
pub fn a_submission_names_its_after_priority_and_submitter_and_does_nothing_more(h: &impl Rig) {
    let base = h.tip();
    let head = h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let two = h.try_submit("unit/b", &[one], 3).unwrap();
    assert_eq!(two.ahead, vec![one]);

    let s = h.sub(two.id);
    assert_eq!((s.after.clone(), s.priority), (vec![one], 3));
    assert_eq!(
        (s.from.agent.as_str(), s.from.worktree.as_str()),
        ("agent-b", "unit/b")
    );
    assert_eq!(h.sub(one).head, head);
    assert_eq!(s.state, State::Queued);
    // Nothing formed, run, committed or moved (1; rule core.4).
    assert_eq!(h.tip(), base);
    assert_eq!(h.moves(), 0);
    assert!(h.started().is_empty());
    assert!(h.tested().is_empty());

    // Every run sluice starts is the gate's required checks, through the
    // check seam: none of its own (30).
    h.settle();
    assert!(h
        .requests()
        .iter()
        .all(|r| r.required == vec!["#gate".to_string()]));
}

/// 2: a place holding more than its commits is refused, and what it holds
/// named.
pub fn a_place_holding_more_than_its_commits_is_refused_and_named(h: &impl Rig) {
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.dirty("unit/a", &["a/scratch.txt", "notes.md"]);
    // Each path named, as the driver shows it (a driver may add what
    // state each is in).
    match h.try_submit("unit/a", &[], 0) {
        Err(SubmitRefusal::Dirty(held)) => {
            assert_eq!(held.len(), 2, "{held:?}");
            assert!(held[0].ends_with("a/scratch.txt"), "{held:?}");
            assert!(held[1].ends_with("notes.md"), "{held:?}");
        }
        other => panic!("submitted: {other:?}"),
    }
    assert_eq!(h.version(), 0);
}

/// 16, 37, 38: every run is recorded before its verdict, a fail exactly
/// as a pass.
pub fn every_run_is_recorded_before_its_verdict_a_fail_exactly_as_a_pass(h: &impl Rig) {
    h.fails_with("b:test", "b/lib", "broken");
    h.work("unit/a", "agent-a", &[("b/lib", Some("broken"))]);
    h.submit("unit/a");
    h.settle();

    let events = h.events();
    let result_at = events
        .iter()
        .position(|e| matches!(e, Event::Result { .. }))
        .unwrap();
    let ran: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e, Event::Ran { .. }))
        .map(|(n, _)| n)
        .collect();
    assert_eq!(ran.len(), 3);
    assert!(ran.iter().all(|n| *n < result_at));
    // The fail is recorded with its key, outcome and executor.
    let host = h.host();
    assert!(events.iter().any(|e| matches!(e,
        Event::Ran { task, outcome: Outcome::Fail, host: h2, .. }
            if task == "b:test" && *h2 == host)));
    assert!(h
        .results()
        .iter()
        .any(|l| l.task == "b:test" && l.outcome == Outcome::Fail));
}

/// 84, 16: every observed result records what its executor enforced.
pub fn every_observed_result_records_what_its_executor_enforced(h: &impl Rig) {
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    h.settle();
    let rs = h.results();
    assert!(!rs.is_empty());
    for r in rs {
        match r.kind {
            ResultKind::Ran => {
                let e = r.executor.expect("a run records its executor");
                assert_eq!(e.kind, "open");
                assert!(!e.confined, "an open run is never confined");
            }
            ResultKind::Replayed => assert!(r.executor.is_none()),
        }
    }
}

/// 55, 70, 56: a landed change to the gate runs every check it now
/// requires. The rig's policy requires `a:test` alone.
pub fn a_landed_change_to_the_gate_runs_every_check_it_now_requires(h: &impl Rig) {
    // b:test fails on the branch as it is.
    let wide = crate::policy(false, "fail", 4).replace("[\"#gate\"]", "[\"a:test\", \"b:test\"]");
    h.fails_with("b:test", "b/lib", "b1");
    h.work(
        "unit/gate",
        "agent-g",
        &[(".config/sluice.toml", Some(&h.policy_text(&wide)))],
    );
    let one = h.submit("unit/gate");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);

    // The next candidate touches neither project, yet b:test runs.
    h.work("unit/c", "agent-c", &[("c/lib", Some("c2"))]);
    let two = h.submit("unit/c");
    h.settle();
    assert_eq!(h.requests()[1].since, None);
    assert_eq!(h.sub(two).state, State::Failed);
    assert_eq!(h.sub(two).failed[0].task, "b:test");
}

/// 14, 22, 23, 55: deepest first, the longest passes and all land on its
/// verdict. The rig's policy batches.
pub fn deepest_first_the_longest_passes_and_all_land_on_its_verdict(h: &impl Rig) {
    h.work("unit/1", "agent-1", &[("a/one", Some("1"))]);
    h.work("unit/2", "agent-2", &[("a/two", Some("2"))]);
    h.work("unit/3", "agent-3", &[("a/three", Some("3"))]);
    let ids = [h.submit("unit/1"), h.submit("unit/2"), h.submit("unit/3")];
    h.settle();

    assert_eq!(h.tested(), vec![ids.to_vec()]);
    assert_eq!(h.moves(), 1);
    let deepest = h.sub(ids[2]).tested.unwrap().combination;
    for id in ids {
        let s = h.sub(id);
        assert_eq!(s.state, State::Landed);
        let message = h.message(&s.landed_as.unwrap());
        assert!(
            message.contains(&format!("Sluice-Tested: {deepest}\n")),
            "#{id}: {message}"
        );
    }
}

/// 65, 12: a conflict names every submission it is contested with.
pub fn a_conflict_names_every_submission_it_is_contested_with(h: &impl Rig) {
    h.work("unit/1", "agent-1", &[("a/f", Some("one"))]);
    h.work("unit/2", "agent-2", &[("a/g", Some("two"))]);
    h.work("unit/3", "agent-3", &[("a/h", Some("three"))]);
    h.work(
        "unit/4",
        "agent-4",
        &[("a/f", Some("four")), ("a/g", Some("four"))],
    );
    let (one, two, three, four) = (
        h.submit("unit/1"),
        h.submit("unit/2"),
        h.submit("unit/3"),
        h.submit("unit/4"),
    );
    h.settle();
    match &h.delivered_to("agent-4")[0].decided {
        Decided::Conflict { paths, with } => {
            assert_eq!(paths, &vec!["a/f".to_string(), "a/g".to_string()]);
            let ids: Vec<u64> = with.iter().map(|c| c.id).collect();
            // Each it conflicts with alone; not #3.
            assert_eq!(ids, vec![one, two]);
            assert_eq!(with[1].from.worktree, "unit/2");
        }
        other => panic!("delivered {other:?}"),
    }
    assert_eq!(h.sub(four).conflict.unwrap().with.len(), 2);
    // The chain continued without it (12).
    for id in [one, two, three] {
        assert_eq!(h.sub(id).state, State::Landed);
    }
}

/// 53, 26, 49: every record written is the real format and names its
/// schema.
pub fn every_record_written_is_the_real_format_and_names_its_schema(h: &impl Rig) {
    h.fails_with("a:test", "a/lib", "broken");
    h.work("unit/a", "agent-a", &[("a/lib", Some("broken"))]);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let (one, two) = (h.submit("unit/a"), h.submit("unit/b"));
    h.settle();

    let files = h.files();
    assert!(h.version() > 0);
    let doc: BranchDoc = serde_json::from_slice(&files["branch.json"]).unwrap();
    assert_eq!(doc.schema, BRANCH_SCHEMA);
    for id in [one, two] {
        // Live, or among the finished records in the same form (130, 53).
        let bytes = match files.get(&format!("submissions/{id}.json")) {
            Some(b) => b.clone(),
            None => h
                .sluice()
                .seams()
                .state
                .read_finished(BRANCH, &format!("{id}.json"))
                .unwrap()
                .expect("a finished submission"),
        };
        let s: Submission = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(s.schema, SUBMISSION_SCHEMA);
        // Readable as plain JSON by any reader.
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v["state"].is_string());
    }
    let log: Vec<LogLine> = parse_lines(&files["log.jsonl"]).unwrap();
    assert!(!log.is_empty());
    // Every check of #1, and of #2 alone once #1 is blamed (21); #1 + #2
    // too, unless #1's failure was read first and its run ended unfinished.
    // A runner's cache the checkouts share (35) may hand #2 alone a replay
    // of what #1 + #2 ran and never recorded; no recorded pass proves it,
    // so #2's checks run again with no replay (108), and a check it had
    // replayed is recorded once more, as run, for the same combination.
    let results = h.results();
    let checks: std::collections::BTreeSet<_> =
        results.iter().map(|r| (&r.task, &r.combination)).collect();
    assert!(
        checks.len() == 3 + 3 || checks.len() == 3 + 3 + 3,
        "{:#?}",
        results
            .iter()
            .map(|r| (&r.task, r.outcome, &r.kind))
            .collect::<Vec<_>>()
    );
}

/// S26: a session runs the checks in a clean place, edits nothing, and
/// submits. The candidate's required checks are covered under their keys
/// by the session's run, recorded as an open run; the gate trusts open
/// runs and the cache, so the candidate replays them and lands without
/// running them again (69, 92, 93; session-runs-1).
pub fn s26_a_clean_session_run_is_replayed_by_its_submission(h: &impl Rig) {
    let head = h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let (report, recorded) = h.session_run("unit/a");
    assert!(!report.tasks.is_empty(), "the session ran its checks");
    assert!(report.tasks.iter().all(|t| t.outcome == Outcome::Pass));
    assert_eq!(recorded, Recorded::Written(report.tasks.len()));
    let session: Vec<_> = h
        .results()
        .into_iter()
        .filter(|r| r.combination == head)
        .collect();
    assert_eq!(session.len(), report.tasks.len());
    for r in &session {
        assert_eq!(r.kind, ResultKind::Ran);
        assert!(r.worktree.is_some());
        let e = r.executor.as_ref().expect("a run records its executor");
        assert!(!e.confined, "a session run is never confined");
    }
    let before = h.results().len();

    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    // The candidate's checks under the session's keys were replays, and
    // recorded as replays, never as runs (16, 38).
    let candidate: Vec<_> = h.results().into_iter().skip(before).collect();
    for r in &session {
        assert!(
            candidate
                .iter()
                .any(|c| c.task == r.task && c.hash == r.hash && c.kind == ResultKind::Replayed),
            "{} was not replayed: {candidate:?}",
            r.task
        );
    }
    assert!(candidate
        .iter()
        .filter(|c| session.iter().any(|r| r.task == c.task && r.hash == c.hash))
        .all(|c| c.kind == ResultKind::Replayed));
}

/// S28: a session runs its checks with a file beyond its commits. The run
/// gives its outcome and records nothing; the candidate formed later runs
/// the checks itself (92; session-runs-1).
pub fn s28_a_dirty_session_run_records_nothing(h: &impl Rig) {
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.dirty("unit/a", &["a/scratch"]);
    let before = h.results();
    let (report, recorded) = h.session_run("unit/a");
    assert!(!report.tasks.is_empty());
    assert_eq!(recorded, Recorded::NotClean);
    assert_eq!(h.results(), before, "a dirty run records nothing");
}

/// S27: a session runs its checks twice; its place stays equipped between
/// the runs and through its landing, which leaves it alone; its release,
/// under the policy read from the branch now, ends it, and a second
/// release finds nothing (25, 94, 95; core.16; session-runs-1).
pub fn s27_a_sessions_place_stays_until_it_is_released(h: &impl Rig) {
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let place = h.place("unit/a");
    let s = h.sluice();
    h.session_run("unit/a");
    let adopted = s
        .seams()
        .checkout
        .adopted(&place)
        .expect("the place seam reads")
        .expect("equipped after its first run");
    h.session_run("unit/a");
    assert_eq!(
        s.seams().checkout.adopted(&place).unwrap().as_ref(),
        Some(&adopted),
        "the same place between runs"
    );
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert!(
        s.seams().checkout.adopted(&place).unwrap().is_some(),
        "a landing released the session's place"
    );
    let policy = s
        .policy_at(&h.tip())
        .expect("the policy reads")
        .expect("a policy");
    let release = ReleasePolicy {
        branch: "bolt/x".into(),
        tests: policy.tests,
        limits: policy.limits,
    };
    assert_eq!(
        s.seams()
            .checkout
            .release_session(&adopted.name, Some(&release))
            .unwrap(),
        Released::Policy
    );
    assert!(s.seams().checkout.adopted(&place).unwrap().is_none());
    assert_eq!(
        s.seams()
            .checkout
            .release_session(&adopted.name, Some(&release))
            .unwrap(),
        Released::Nothing
    );
}

/// The session place listed for `worktree`, if any.
fn held(h: &impl Rig, worktree: &str) -> Option<SessionPlace> {
    let place = h.place(worktree);
    h.sluice()
        .session_places()
        .expect("the place seam lists")
        .into_iter()
        .find(|p| p.worktree == std::path::Path::new(&place))
}

/// `worktree`'s last result, which it must have.
fn last_result(h: &impl Rig, worktree: &str) -> LastResult {
    held(h, worktree)
        .and_then(|p| p.last_result)
        .unwrap_or_else(|| panic!("{worktree} has no last result"))
}

fn expect_last(
    last: &LastResult,
    head: &Rev,
    kind: SessionRunKind,
    outcome: RunOutcome,
    recorded: bool,
) {
    assert_eq!(
        (&last.head, last.kind, last.outcome, last.recorded),
        (head, kind, outcome, recorded),
        "{last:?}"
    );
}

/// Session-runs-1 clauses 92, 97, rule core.18: every session run leaves
/// on its place the head and outcome of its last run, and says recorded
/// only of a clean run of the checks whose results are under their keys;
/// a command's run and an unclean run never; the next run's start drops
/// it, its end replaces it, and the release removes it.
pub fn a_session_runs_last_result_says_whether_it_was_recorded(h: &impl Rig) {
    let head = h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    assert_eq!(held(h, "unit/a"), None);
    let (_, recorded) = h.session_run("unit/a");
    assert!(matches!(recorded, Recorded::Written(_)), "{recorded:?}");
    expect_last(
        &last_result(h, "unit/a"),
        &head,
        SessionRunKind::Checks,
        RunOutcome::Pass,
        true,
    );

    // A rerun whose every result is under its key already is recorded.
    let (_, recorded) = h.session_run("unit/a");
    assert_eq!(recorded, Recorded::Written(0));
    expect_last(
        &last_result(h, "unit/a"),
        &head,
        SessionRunKind::Checks,
        RunOutcome::Pass,
        true,
    );

    // A clean run that fails is recorded, and replaces the pass.
    h.fails_with("a:test", "a/lib", "a3");
    let failing = h.work_on("unit/a", &head, &[("a/lib", Some("a3"))]);
    h.session_run("unit/a");
    expect_last(
        &last_result(h, "unit/a"),
        &failing,
        SessionRunKind::Checks,
        RunOutcome::Fail,
        true,
    );

    // An unclean run that passes is not.
    let passing = h.work_on("unit/a", &failing, &[("a/lib", Some("a4"))]);
    h.dirty("unit/a", &["a/scratch"]);
    let before = h.results();
    let (_, recorded) = h.session_run("unit/a");
    assert_eq!(recorded, Recorded::NotClean);
    assert_eq!(h.results(), before, "an unclean run records nothing");
    expect_last(
        &last_result(h, "unit/a"),
        &passing,
        SessionRunKind::Checks,
        RunOutcome::Pass,
        false,
    );

    // Nor is a command that exits 0.
    let (_, recorded) = h.session_exec("unit/a", Some("true"));
    assert_eq!(recorded, Recorded::Nothing);
    expect_last(
        &last_result(h, "unit/a"),
        &passing,
        SessionRunKind::Command,
        RunOutcome::Pass,
        false,
    );

    // A run that starts and never ends, its starter killed, shows none:
    // the one before is not the last run.
    let s = h.sluice();
    let plan = s
        .session_plan(BRANCH)
        .expect("the plan reads")
        .expect("the branch is managed");
    s.seams()
        .checkout
        .adopt(
            &h.place("unit/a"),
            BRANCH,
            &plan.policy.checkout,
            "2026-10-16T09:30:00Z",
        )
        .expect("the place is equipped");
    let listed = held(h, "unit/a").expect("held");
    assert_eq!(listed.last_result, None, "{listed:?}");
    assert_eq!(listed.last_run.as_deref(), Some("2026-10-16T09:30:00Z"));

    // The release removes it.
    let name = held(h, "unit/a").expect("held").name;
    s.seams()
        .checkout
        .release_session(&name, None)
        .expect("the release runs");
    assert_eq!(held(h, "unit/a"), None);
    // A result kept after the release is kept nowhere.
    s.seams()
        .checkout
        .ran(&name, &last_result_of(&passing))
        .expect("nothing to keep it on");
    assert_eq!(held(h, "unit/a"), None);
}

fn last_result_of(head: &Rev) -> LastResult {
    LastResult {
        at: "2026-10-16T09:00:00Z".into(),
        head: head.clone(),
        kind: SessionRunKind::Checks,
        outcome: RunOutcome::Fail,
        recorded: false,
        tasks: Vec::new(),
        reused: None,
    }
}

/// Session-runs-1 clause 92, rule core.18: a worktree whose head or files
/// change while its run of the checks runs records nothing under keys,
/// and its result names the head the run started on, not recorded.
pub fn a_worktree_that_changes_while_its_run_runs_records_nothing(h: &impl Rig) {
    for (worktree, change) in [("unit/a", "commit"), ("unit/b", "edit")] {
        let head = h.work(worktree, "agent", &[("a/lib", Some("a2"))]);
        let s = h.sluice();
        let place = h.place(worktree);
        let plan = s
            .session_plan(BRANCH)
            .expect("the plan reads")
            .expect("the branch is managed");
        let session = s.seams().driver.session(&place).expect("a session");
        let part = session_part(&s);
        let started = s
            .start_session(&plan, &place, &session.head, None, part)
            .expect("the run starts");
        match change {
            "commit" => {
                h.work_on(worktree, &head, &[("a/lib", Some("a3"))]);
            }
            _ => h.dirty(worktree, &["a/scratch"]),
        }
        let until = std::time::Instant::now() + std::time::Duration::from_secs(300);
        let report = loop {
            match s.seams().tests.progress(&started.run).expect("progress") {
                sluice_seams::Progress::Finished(r) => break r,
                sluice_seams::Progress::Running if std::time::Instant::now() < until => {
                    std::thread::sleep(std::time::Duration::from_millis(50))
                }
                p => panic!("the run did not finish: {p:?}"),
            }
        };
        let before = h.results();
        let finished = s
            .finish_session(&plan, &session, &started, false, &report)
            .expect("the run finishes");
        match (&finished.recorded, change) {
            (Recorded::Changed { moved_to, paths }, "commit") => {
                assert!(
                    moved_to.is_some() && paths.is_empty(),
                    "{moved_to:?} {paths:?}"
                )
            }
            (Recorded::Changed { moved_to, paths }, _) => assert!(
                moved_to.is_none() && paths.len() == 1 && paths[0].ends_with("a/scratch"),
                "{moved_to:?} {paths:?}"
            ),
            (other, _) => panic!("{change}: {other:?}"),
        }
        assert_eq!(h.results(), before, "{change}: recorded under keys");
        let last = last_result(h, worktree);
        assert_eq!(
            (&last.head, last.recorded),
            (&head, false),
            "{change}: {last:?}"
        );
    }
}

/// Session-runs-1 clause 97, rule core.18: a worktree's last result
/// decides nothing. A clean run's recorded pass is replayed by its
/// submission as it would be whatever the place says of a later run.
pub fn a_last_result_decides_nothing(h: &impl Rig) {
    let head = h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let (report, _) = h.session_run("unit/a");
    let name = held(h, "unit/a").expect("held").name;
    h.sluice()
        .seams()
        .checkout
        .ran(&name, &last_result_of(&head))
        .expect("the place keeps it");
    let before = h.results().len();
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    let candidate: Vec<_> = h.results().into_iter().skip(before).collect();
    for t in &report.tasks {
        assert!(
            candidate
                .iter()
                .any(|c| c.task == t.task && c.hash == t.hash && c.kind == ResultKind::Replayed),
            "{} was not replayed: {candidate:?}",
            t.task
        );
    }
}

/// Session-runs-1 clause 97, 123: a session's place whose worktree no
/// longer exists is no session's, and the next round on the host runs the
/// release its runs stored, once, frees it and logs it `gone`, with its
/// release run, where the branch's releases are; no round runs its release
/// or names it again. A place whose worktree exists stays, however long
/// since its last run, and is listed with the time of that run (25, 95;
/// core.16).
pub fn a_removed_worktrees_place_is_freed_by_the_next_round(h: &impl Rig) {
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    h.session_run("unit/a");
    h.session_run("unit/b");
    let s = h.sluice();
    let held = s.session_places().expect("the place seam lists");
    assert_eq!(held.len(), 2, "{held:?}");
    assert!(
        held.iter().all(|p| !p.gone && p.last_run.is_some()),
        "{held:?}"
    );
    assert!(held.iter().all(|p| p.branch == BRANCH), "{held:?}");

    let (gone, kept) = removed(h, "unit/a");
    let named = || named_gone(h, &gone);
    assert_eq!(named(), 1, "{:?}", h.events());
    let ran = h.events().into_iter().find_map(|e| match e {
        Event::Gone {
            place, ran, failed, ..
        } if place == gone.name => Some((ran, failed)),
        _ => None,
    });
    assert_eq!(ran, Some((true, None)), "{:?}", h.events());
    // Its stored release ran once; no release was built for it.
    assert_eq!(h.gone_releases_of(&gone.name), 1);
    assert_eq!(h.releases_of(&gone.name), 0);

    // Rounds after leave a place whose worktree exists alone, and never
    // run or name the one freed again.
    h.step();
    h.step();
    assert_eq!(s.session_places().expect("lists"), vec![kept]);
    assert_eq!(named(), 1);
    assert_eq!(h.gone_releases_of(&gone.name), 1);
}

/// 123: a removed worktree whose stored release fails is freed all the
/// same and logged `gone` with what its release printed; its release is
/// never run again, and no round counts it as a failed release.
pub fn a_removed_worktrees_failing_release_is_freed_and_never_run_again(h: &impl Rig) {
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    h.session_run("unit/a");
    h.session_run("unit/b");
    h.release_fails(true);
    let (gone, kept) = removed(h, "unit/a");
    assert_eq!(named_gone(h, &gone), 1, "{:?}", h.events());
    let failed = h.events().into_iter().find_map(|e| match e {
        Event::Gone {
            place, ran, failed, ..
        } if place == gone.name => Some((ran, failed)),
        _ => None,
    });
    let Some((true, Some(failed))) = failed else {
        panic!("{:?}", h.events());
    };
    assert!(failed.contains(RELEASE_FAILS), "{failed}");
    for _ in 0..3 {
        h.step();
    }
    assert_eq!(h.gone_releases_of(&gone.name), 1);
    assert_eq!(h.sluice().session_places().expect("lists"), vec![kept]);
    assert!(
        !h.events()
            .iter()
            .any(|e| matches!(e, Event::ReleaseFailed { place, .. } if *place == gone.name)),
        "{:?}",
        h.events()
    );
}

/// The worktree `worktree`, whose session ran, is removed by hand, and
/// rounds step until its place is freed: that place as listed while gone,
/// and the place left, the other session's.
fn removed(h: &impl Rig, worktree: &str) -> (SessionPlace, SessionPlace) {
    let s = h.sluice();
    h.remove_worktree(worktree);
    let listed = s.session_places().expect("the place seam lists");
    let gone: Vec<_> = listed.iter().filter(|p| p.gone).cloned().collect();
    assert_eq!(gone.len(), 1, "{listed:?}");
    let kept = listed
        .iter()
        .find(|p| !p.gone)
        .cloned()
        .expect("the other session's place is listed");
    // A rig whose lock a starting child holds for a moment leaves it to
    // the round after; on the fakes the first round frees it.
    for _ in 0..50 {
        h.step();
        if s.session_places().expect("lists").iter().all(|p| !p.gone) {
            break;
        }
    }
    let left = s.session_places().expect("the place seam lists");
    assert_eq!(left, vec![kept.clone()], "only the other place is left");
    (gone[0].clone(), kept)
}

/// How many lines of the log name the session's place `gone`: as gone,
/// with its worktree, or as released or failed.
fn named_gone(h: &impl Rig, gone: &SessionPlace) -> usize {
    let worktree = gone.worktree.to_string_lossy().into_owned();
    h.events()
        .into_iter()
        .filter(|e| match e {
            Event::Gone {
                place, worktree: w, ..
            } => *place == gone.name && w.as_deref() == Some(worktree.as_str()),
            Event::Released { place, .. } | Event::ReleaseFailed { place, .. } => {
                *place == gone.name
            }
            _ => false,
        })
        .count()
}

/// A preview of `rev` through the rig's branch, its command started and
/// read until it has ended; the preview as listed then.
fn preview_up(h: &impl Rig, rev: &sluice_seams::Rev) -> sluice_seams::PreviewPlace {
    let s = h.sluice();
    let plan = s
        .preview_plan(BRANCH, rev)
        .expect("the branch reads")
        .expect("a landed revision with a preview table");
    let (place, _) = s.make_preview(&plan).expect("the preview is made");
    let part = session_part(&s);
    s.start_preview(&plan, place, part)
        .expect("its command starts");
    for _ in 0..600 {
        let p = s.preview_of(rev).unwrap().expect("listed");
        match s.preview_state(&p).unwrap() {
            PreviewState::Starting => std::thread::sleep(std::time::Duration::from_millis(50)),
            PreviewState::Up => return p,
            other => panic!("the preview's command ended {other:?}"),
        }
    }
    panic!("the preview's command never ended")
}

/// S30, S35: a landed revision is previewed at addresses filled in with
/// its names and ports, known before its command runs; a head the branch
/// has not landed is refused and nothing is made (previews-1 clauses 98,
/// 99).
pub fn s30_a_landed_revision_is_previewed_at_its_addresses(h: &impl Rig) {
    let s = h.sluice();
    let head = h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    assert_eq!(
        s.preview_plan(BRANCH, &head).unwrap().unwrap_err(),
        PreviewRefusal::NotLanded
    );
    assert!(s.seams().checkout.previews().unwrap().is_empty());
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    let landed = h.tip();
    let plan = s.preview_plan(BRANCH, &landed).unwrap().unwrap();
    let (_, made) = s.make_preview(&plan).unwrap();
    assert_eq!(made.name, Sluice::preview_name(&landed));
    assert_eq!(
        made.addresses["web"],
        format!("https://{}.localhost", made.names["SLUICE_NAME_WEB"])
    );
    assert_eq!(
        made.addresses["api"],
        format!("http://localhost:{}/docs", made.ports["SLUICE_PORT_API"])
    );
    assert_eq!(s.preview_state(&made).unwrap(), PreviewState::Waiting);
    s.release_preview(&made.name).unwrap();
    let up = preview_up(h, &landed);
    assert_eq!(up.revision, landed);
    // 103: nothing of it is a result.
    let results = String::from_utf8(s.seams().state.results(BRANCH).unwrap()).unwrap();
    assert!(!results.contains(&up.name) && !results.contains("preview"));
    assert!(s.release_preview(&up.name).unwrap());
}

/// S31: the next landing's preview is made beside the last, sharing no
/// port and no name, and the last is released at its caller's asking
/// (previews-1 clauses 99, 101).
pub fn s31_the_next_landings_preview_replaces_the_last(h: &impl Rig) {
    let s = h.sluice();
    let r1 = h.tip();
    let first = preview_up(h, &r1);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let two = h.submit("unit/b");
    h.settle();
    assert_eq!(h.sub(two).state, State::Landed);
    let r2 = h.tip();
    let second = preview_up(h, &r2);
    assert!(first
        .ports
        .values()
        .all(|p| !second.ports.values().any(|q| q == p)));
    assert!(first
        .names
        .values()
        .all(|n| !second.names.values().any(|m| m == n)));
    assert!(s.release_preview(&first.name).unwrap());
    let left: Vec<String> = s
        .seams()
        .checkout
        .previews()
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(left, vec![second.name.clone()]);
    assert!(s.release_preview(&second.name).unwrap());
    assert!(!s.release_preview(&second.name).unwrap());
}

/// S32: a caller that asks again gets the preview it asked for the first
/// time, and finds every preview the host holds by listing them
/// (previews-1 clauses 100, 101).
pub fn s32_asking_again_gives_back_the_same_preview(h: &impl Rig) {
    let s = h.sluice();
    let rev = h.tip();
    let up = preview_up(h, &rev);
    let plan = s.preview_plan(BRANCH, &rev).unwrap().unwrap();
    let (_, again) = s.make_preview(&plan).unwrap();
    assert_eq!(again, up);
    let listed = s.previews().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].place, up);
    assert_eq!(listed[0].state, PreviewState::Up);
    // A round leaves a preview that is up alone (rule core.16).
    h.step();
    assert_eq!(s.preview_of(&rev).unwrap(), Some(up.clone()));
    assert!(s.release_preview(&up.name).unwrap());
}

/// What a failing release command prints, in the scenarios of 123.
pub const RELEASE_FAILS: &str = "usage: scripts/preview up";

/// Step until `done`, a round at a time, as a waiting submit does: at once
/// on the fakes, and between a rig's runs on the real profiles.
fn step_until(h: &impl Rig, what: &str, done: impl Fn() -> bool) {
    for _ in 0..3000 {
        if done() {
            return;
        }
        h.step();
        if !done() {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    panic!("{what} never came");
}

/// A submission from `unit/a` landed while every release fails; its
/// checkout's name.
fn landed_with_its_release_failing(h: &impl Rig) -> String {
    h.release_fails(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    step_until(h, "#1's landing", || h.sub(one).state == State::Landed);
    h.made_places()[0].clone()
}

/// Each failed try of `place`'s release the log holds: its tries and
/// whether it held the place.
fn failed_tries(h: &impl Rig, place: &str) -> Vec<(u32, bool)> {
    h.events()
        .into_iter()
        .filter_map(|e| match e {
            Event::ReleaseFailed {
                place: p,
                failed,
                tries,
                held,
            } if p == place => {
                assert!(failed.contains(RELEASE_FAILS), "{failed}");
                Some((tries, held))
            }
            _ => None,
        })
        .collect()
}

/// 123, 122: a combination's checkout whose release command fails stays,
/// and the branch's state names it with what failed and one try; the log
/// holds `release_failed` naming it, and its submission landed all the
/// same.
pub fn a_failed_release_is_held_with_what_failed(h: &impl Rig) {
    let place = landed_with_its_release_failing(h);
    assert!(h.live_places().contains(&place), "{place} was removed");
    let held = h.sluice().status(BRANCH).unwrap().failed_releases;
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0].place, place);
    assert!(held[0].failed.contains(RELEASE_FAILS), "{held:?}");
    assert_eq!((held[0].tries, held[0].stopped), (1, false));
    assert_eq!(failed_tries(h, &place), [(1, false)]);
}

/// 123: with no `release_retries`, a release that fails on every try is
/// run four times in all, one a round, held on the fourth, and run no
/// more by the rounds after.
pub fn a_failed_release_is_tried_a_bounded_number_of_times(h: &impl Rig) {
    let place = landed_with_its_release_failing(h);
    step_until(h, "the hold", || h.releases_of(&place) >= 4);
    for _ in 0..3 {
        h.step();
    }
    assert_eq!(h.releases_of(&place), 4);
    assert_eq!(
        failed_tries(h, &place),
        [(1, false), (2, false), (3, false), (4, true)]
    );
    let held = h.sluice().status(BRANCH).unwrap().failed_releases;
    assert_eq!((held[0].tries, held[0].stopped), (4, true), "{held:?}");
    assert!(h.live_places().contains(&place));
}

/// 123, 56: with `release_retries = 1`, a release that fails twice holds
/// its checkout after the second failure, with what failed.
pub fn a_failed_release_is_held_after_the_retries_its_branch_names(h: &impl Rig) {
    let place = landed_with_its_release_failing(h);
    step_until(h, "the hold", || h.releases_of(&place) >= 2);
    h.step();
    assert_eq!(h.releases_of(&place), 2);
    assert_eq!(failed_tries(h, &place), [(1, false), (2, true)]);
}

/// 123: a release that fails once and succeeds on the next round leaves
/// no checkout and no failure in the branch's state, and the log holds
/// `released` naming it.
pub fn a_release_that_succeeds_on_a_retry_is_listed_no_more(h: &impl Rig) {
    let place = landed_with_its_release_failing(h);
    h.release_fails(false);
    h.step();
    assert!(!h.live_places().contains(&place), "{place} is left");
    assert!(h
        .sluice()
        .status(BRANCH)
        .unwrap()
        .failed_releases
        .is_empty());
    let events = h.events();
    let failed = events
        .iter()
        .position(|e| matches!(e, Event::ReleaseFailed { place: p, .. } if *p == place));
    let released = events
        .iter()
        .position(|e| matches!(e, Event::Released { place: p, .. } if *p == place));
    assert!(failed < released && failed.is_some(), "{events:?}");
}

/// 123, 122: a held checkout never holds its branch: the round that tries
/// its release again starts a waiting submission's combination in a new
/// checkout, and that submission lands when it passes.
pub fn a_held_checkout_never_holds_its_branch(h: &impl Rig) {
    let first = landed_with_its_release_failing(h);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let two = h.submit("unit/b");
    h.step();
    assert_eq!(h.releases_of(&first), 2, "the round tried it again");
    let made = h.made_places();
    assert_eq!(made.len(), 2, "the round started #{two}: {made:?}");
    assert_ne!(made[1], first);
    step_until(h, "#2's landing", || h.sub(two).state == State::Landed);
    assert!(h.live_places().contains(&first));
    assert_eq!(h.tested().last(), Some(&vec![two]));
}

/// The runs going on on the host for the queue, by the check seam's count:
/// a run ended is no longer among them (71).
fn running(h: &impl Rig) -> usize {
    let s = h.sluice();
    s.seams().tests.running().expect("the host's count").queue
}

/// Until every run of a combination under test has finished, with no
/// round reading any: at once on the fakes, as its checks end on the real
/// profiles.
fn finished(h: &impl Rig) {
    let runs: Vec<RunHandle> = h
        .doc()
        .candidates
        .iter()
        .flat_map(|c| c.runs.iter().filter_map(|r| r.run.clone()))
        .collect();
    assert!(!runs.is_empty(), "no run under test");
    let s = h.sluice();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(300);
    for run in &runs {
        loop {
            match s.seams().tests.progress(run).expect("progress") {
                Progress::Finished(_) => break,
                Progress::Lost => panic!("the run {} was lost", run.0),
                Progress::Running => {
                    assert!(std::time::Instant::now() < until, "{} never ended", run.0);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }
    }
}

/// Nothing of `combination`'s runs was read: no check of it recorded and
/// no result for it (25, 81; rule core.7).
fn read_for_nothing(h: &impl Rig, combination: &Rev) {
    let recorded: Vec<_> = h
        .results()
        .into_iter()
        .filter(|r| r.combination == *combination)
        .collect();
    assert!(recorded.is_empty(), "checks of it recorded: {recorded:?}");
    let events = h.events();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::Result { combination: c, .. } if c == combination)),
        "a result for it: {events:?}"
    );
}

/// `id`'s combination under test, as the records name it.
fn under_test(h: &impl Rig, id: u64) -> Rev {
    h.sub(id).tested.expect("under test").combination
}

/// The obsolete-combinations spec's "Withdrawn while under test" and "Its
/// run still going" (25, 32, 81; rules core.10, core.16, core.23): the
/// round after a withdrawal drops the combination, ends its run and
/// releases its checkout, and starts the combination of the submission
/// behind it; the records hold no combination with the withdrawn one, and
/// nothing of the run is recorded.
pub fn a_withdrawn_combinations_run_is_ended_and_its_checkout_released(h: &impl Rig) {
    h.hangs("a:test");
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    let first = under_test(h, one);
    let place = h.made_places()[0].clone();
    assert_eq!(running(h), 1);
    h.work("unit/b", "agent-b", &[("a/notes", Some("from b"))]);
    let two = h.submit("unit/b");
    h.withdraw(one, Requester::Session("agent-a".into()));

    h.step();
    assert!(!h.live_places().contains(&place), "{place} is left");
    assert_eq!(h.releases_of(&place), 1);
    assert!(h
        .doc()
        .candidates
        .iter()
        .all(|c| !c.submissions.contains(&one)));
    assert_eq!(h.tested(), [vec![one], vec![two]]);
    assert_eq!(running(h), 1, "#1's run ended, #2's goes on");
    read_for_nothing(h, &first);

    let second = under_test(h, two);
    h.withdraw(two, Requester::Session("agent-b".into()));
    h.step();
    assert_eq!(running(h), 0);
    assert!(h.live_places().is_empty(), "{:?}", h.live_places());
    assert!(h.doc().candidates.is_empty());
    read_for_nothing(h, &second);
    assert_eq!(h.moves(), 0);
}

/// The obsolete-combinations spec's "Its run had finished" (25, 81; rule
/// core.7): a run that passed every check, its submission withdrawn
/// before any round read it, is read by no round: no check of it is
/// recorded, no result written, nothing landed and nothing told.
pub fn a_withdrawn_combinations_passed_run_is_read_for_nothing(h: &impl Rig) {
    let base = h.tip();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    let combination = under_test(h, one);
    let place = h.made_places()[0].clone();
    finished(h);
    h.withdraw(one, Requester::Session("agent-a".into()));

    h.step();
    read_for_nothing(h, &combination);
    assert!(!h.live_places().contains(&place), "{place} is left");
    assert!(h.doc().candidates.is_empty());
    assert_eq!(h.sub(one).state, State::Withdrawn);
    assert_eq!(h.sub(one).result, None);
    assert_eq!((h.moves(), h.tip()), (0, base));
    assert!(h.delivered_to("agent-a").is_empty());
}

/// The obsolete-combinations spec's "Its run had failed" (17, 20, 25, 81;
/// rule core.8): runs that failed a check, their combinations holding a
/// submission withdrawn before any round read them, blame no one and tell
/// no session; the submission behind it is tested without it and lands.
pub fn a_withdrawn_combinations_failed_run_blames_no_one(h: &impl Rig) {
    h.fails_with("a:test", "a/bad", "yes");
    h.work("unit/a", "agent-a", &[("a/bad", Some("yes"))]);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let one = h.submit("unit/a");
    let two = h.submit("unit/b");
    h.step();
    assert_eq!(h.tested(), [vec![one], vec![one, two]]);
    let (first, both) = (under_test(h, one), under_test(h, two));
    finished(h);
    h.withdraw(one, Requester::Session("agent-a".into()));

    h.step();
    read_for_nothing(h, &first);
    read_for_nothing(h, &both);
    assert!(!h.events().iter().any(|e| matches!(e, Event::Blamed { .. })));
    for id in [one, two] {
        let s = h.sub(id);
        assert!(s.failed.is_empty(), "#{id} was blamed: {s:?}");
        assert_ne!(s.state, State::Failed);
    }
    assert!(h.delivered_to("agent-a").is_empty());
    assert!(h.delivered_to("agent-b").is_empty());

    h.settle();
    assert_eq!(h.sub(two).state, State::Landed);
    assert_eq!(h.tested().last(), Some(&vec![two]));
    assert!(h
        .delivered_to("agent-b")
        .iter()
        .all(|n| n.decided.word() == "landed"));
}

/// The obsolete-combinations spec's "Its release fails" (123): the
/// checkout of a withdrawn submission's combination whose release fails is
/// held, named with what failed, and tried again the next round as any
/// failed release is, until it is released.
pub fn a_withdrawn_combinations_failed_release_holds_its_checkout(h: &impl Rig) {
    h.hangs("a:test");
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    let place = h.made_places()[0].clone();
    h.release_fails(true);
    h.withdraw(one, Requester::Session("agent-a".into()));

    h.step();
    assert!(h.doc().candidates.is_empty());
    assert!(h.live_places().contains(&place), "{place} was removed");
    let held = h.sluice().status(BRANCH).unwrap().failed_releases;
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0].place, place);
    assert!(held[0].failed.contains(RELEASE_FAILS), "{held:?}");
    assert_eq!((held[0].tries, held[0].stopped), (1, false));

    h.step();
    assert_eq!(h.releases_of(&place), 2);
    assert_eq!(failed_tries(h, &place), [(1, false), (2, false)]);
    h.release_fails(false);
    h.step();
    assert!(!h.live_places().contains(&place), "{place} is left");
    assert_eq!(running(h), 0);
}

/// 129, 124: what a reader that decides nothing reads of the branch is the
/// branch as the host holds it: in the host's own refs, as a read reads
/// it; on a remote, its revision as last fetched, with nothing fetched,
/// until a read fetches what another host landed there.
pub fn a_branch_is_held_as_the_host_last_read_it(h: &impl Rig) {
    let s = h.sluice();
    let b = s.seams().branch;
    let tip = b.read(BRANCH).unwrap();
    assert_eq!(tip, Some(h.tip()));
    assert_eq!(b.held(BRANCH).unwrap(), tip);
    let moved = h.elsewhere(&[("c/lib", Some("landed elsewhere"))]);
    if h.on_remote() {
        assert_eq!(b.held(BRANCH).unwrap(), tip, "held as last fetched");
        assert_eq!(b.held(BRANCH).unwrap(), tip, "held fetched nothing");
    } else {
        assert_eq!(b.held(BRANCH).unwrap(), Some(moved.clone()));
    }
    assert_eq!(b.read(BRANCH).unwrap(), Some(moved.clone()));
    assert_eq!(b.held(BRANCH).unwrap(), Some(moved));
}

/// 129: a branch's records as the host holds them after a tick's ask are
/// the records a read reads, before and after a round writes them.
pub fn records_held_after_a_refresh_are_the_records_read(h: &impl Rig) {
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    let s = h.sluice();
    let state = s.seams().state;
    state.refresh().unwrap();
    assert_eq!(
        state.read_held(BRANCH).unwrap(),
        state.read(BRANCH).unwrap()
    );
    h.settle();
    state.refresh().unwrap();
    let held = state.read_held(BRANCH).unwrap();
    assert_eq!(held, state.read(BRANCH).unwrap());
    assert!(held.version > 1, "a round wrote the records");
}

/// A `[[checkout.cache]]` table, which this sluice refuses wherever a
/// policy holds it (S64; ADR 0049).
const CACHE: &str = "\n[[checkout.cache]]\nkey = [\"Cargo.lock\"]\npaths = [\"target\"]\n";

/// The branch's records as a sluice that read `at`'s policy left them: a
/// round stood on `at` last, and none has run since (129).
fn last_saw(h: &impl Rig, at: &Rev) {
    let s = h.sluice();
    let lock = s.seams().state.lock(BRANCH).unwrap().expect("the lock");
    let snapshot = s.seams().state.read(BRANCH).unwrap();
    let version = snapshot.version;
    let mut doc = Line::load(BRANCH, snapshot).unwrap().doc;
    doc.base = Some(at.clone());
    let mut changes = Changes::default();
    changes.put.insert(BRANCH_FILE.into(), to_document(&doc));
    let written = s.seams().state.write(BRANCH, version, changes).unwrap();
    assert!(matches!(written, Written::Written(_)), "{written:?}");
    drop(lock);
}

/// A quiet branch mended outside sluice: one submission landed, a commit
/// on the branch adding a `[[checkout.cache]]` table, `r1`, the branch's
/// records last seeing it, and a commit taking the table out again, `r2`,
/// which this host holds and no round has read (124, 129).
struct Mended {
    landed: u64,
    r1: Rev,
    r2: Rev,
}

fn mended(h: &impl Rig) -> Mended {
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let landed = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(landed).state, State::Landed);
    let clean = h.file(&h.tip(), POLICY_PATH).expect("the branch's policy");
    let r1 = h.elsewhere(&[(POLICY_PATH, Some(&format!("{clean}{CACHE}")))]);
    last_saw(h, &r1);
    let r2 = h.elsewhere(&[(POLICY_PATH, Some(&clean))]);
    if !h.on_remote() {
        assert_eq!(
            h.sluice().seams().branch.held(BRANCH).unwrap(),
            Some(r2.clone())
        );
    }
    Mended { landed, r1, r2 }
}

/// The branch's records as they stand, to tell that no round ran after.
struct Before {
    files: std::collections::BTreeMap<String, Vec<u8>>,
    moved: usize,
}

fn moved(h: &impl Rig) -> usize {
    h.events()
        .iter()
        .filter(|e| matches!(e, Event::Moved { .. }))
        .count()
}

fn before(h: &impl Rig) -> Before {
    Before {
        files: h.files(),
        moved: moved(h),
    }
}

/// That no round ran since `before`: the records are as they were, their
/// `base` still `at`, and no `moved` is logged (129).
fn no_round_since(h: &impl Rig, before: &Before, at: &Rev) {
    assert!(
        h.files() == before.files,
        "the branch's records were written"
    );
    assert_eq!(h.doc().base.as_ref(), Some(at));
    assert_eq!(moved(h), before.moved, "a round moved the branch's base");
}

/// branch-refusal's "A quiet branch mended outside sluice" (26, 56, 124,
/// 129; rule core.15): status and the overview read the branch's policy
/// at the revision this host holds, so the branch is read whole, its gate
/// the mended revision's, with no round run.
pub fn a_quiet_branch_mended_outside_sluice_is_read_whole(h: &impl Rig) {
    let m = mended(h);
    let was = before(h);
    let s = h.sluice();
    let status = s.status(BRANCH).expect("status reads the branch whole");
    assert_eq!(status.at.as_ref(), Some(&m.r1));
    assert_eq!(status.parallel, Some(4));
    assert_eq!(status.finished, 1);
    assert!(status.submissions.is_empty());
    let view = s
        .overview(None)
        .unwrap()
        .into_iter()
        .find(|v| v.branch == BRANCH)
        .expect("the overview shows the branch");
    assert_eq!(view.unread, None);
    assert_eq!(view.at.as_ref(), Some(&m.r1));
    assert_eq!(view.parallel, Some(4));
    if !h.on_remote() {
        assert_eq!(view.revision.as_ref(), Some(&m.r2));
    }
    no_round_since(h, &was, &m.r1);
}

/// branch-refusal's "Status of a submission on the mended branch" (26,
/// 56, 124): `sluice status <n>` finds the submission and reads its
/// branch's status, which says nothing of the refusal the mend ended.
pub fn status_of_a_submission_on_a_mended_branch_answers_for_it(h: &impl Rig) {
    let m = mended(h);
    let s = h.sluice();
    match s.lookup(m.landed).unwrap() {
        Lookup::Found(sub) => {
            assert_eq!((sub.id, sub.branch.as_str()), (m.landed, BRANCH));
            assert_eq!(sub.state, State::Landed);
        }
        other => panic!("{} not found: {other:?}", m.landed),
    }
    let status = s.status(BRANCH).expect("its branch's status reads");
    assert!(status.warnings.is_empty(), "{:?}", status.warnings);
    assert_eq!(status.refused, None);
}

/// branch-refusal's "The refusal names the revision this host holds" (26,
/// 56, 124; record 0069): the records last saw a revision whose policy
/// reads, the branch moved outside sluice to one adding a
/// `[[checkout.cache]]` table, and status and the overview refuse it
/// naming the revision this host holds, never the records'.
pub fn a_refusal_names_the_revision_this_host_holds(h: &impl Rig) {
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let landed = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(landed).state, State::Landed);
    let r1 = h.doc().base.expect("a round stood on the branch");
    let clean = h.file(&h.tip(), POLICY_PATH).expect("the branch's policy");
    let r2 = h.elsewhere(&[(POLICY_PATH, Some(&format!("{clean}{CACHE}")))]);
    let was = before(h);
    let s = h.sluice();
    let refused = format!(
        "{:#}",
        s.status(BRANCH).expect_err("status refuses the branch")
    );
    let view = s
        .overview(None)
        .unwrap()
        .into_iter()
        .find(|v| v.branch == BRANCH)
        .expect("the overview shows the branch");
    let unread = view.unread.expect("the overview refuses the branch");
    for words in [&refused, &unread] {
        assert!(
            words.contains(r2.as_str()) && words.contains("[[checkout.cache]]"),
            "{words}"
        );
        assert!(!words.contains(r1.as_str()), "{words}");
    }
    no_round_since(h, &was, &r1);
}

/// branch-refusal's "The state is left as it was" (26, 129; record 0046):
/// status of a mended branch still names the revision its records last
/// saw as `at`, and a status, an overview and a view of it fetch nothing
/// and leave its records, their version and its log as they were.
pub fn a_read_of_a_mended_branch_leaves_its_state_as_it_was(h: &impl Rig) {
    let m = mended(h);
    let was = before(h);
    let (version, log) = (h.version(), h.log());
    let s = h.sluice();
    assert_eq!(s.status(BRANCH).unwrap().at.as_ref(), Some(&m.r1));
    s.overview(None).unwrap();
    s.branch_view_of(BRANCH).unwrap();
    assert_eq!(h.version(), version);
    assert!(h.log() == log, "the log was written");
    no_round_since(h, &was, &m.r1);
}

/// The cases of this file the fakes run here, the real profiles in the
/// `sluice` crate's conformance tests.
/// A session's part of the host's bound, as `sluice exec` holds it before
/// its run starts (71, 150; rule core.49).
pub fn session_part(s: &sluice_core::Sluice<'_>) -> sluice_seams::Part {
    s.seams()
        .tests
        .hold(sluice_seams::RunKind::Session)
        .expect("the host's room answers")
        .expect("room on the host for a session's run")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{policy, Harness};

    /// The bolt policy, its branch kept on `origin`.
    fn on_origin() -> Harness {
        let h = Harness::new(&policy(false, "fail", 4).replace(
            "trust = [\"open\", \"sandbox\", \"container\"]\n",
            "trust = [\"open\", \"sandbox\", \"container\"]\nremote = \"origin\"\n",
        ));
        h.w().on_remote(BRANCH);
        h
    }

    /// satisfies: 129, 124
    #[test]
    fn a_branch_in_the_hosts_refs_is_held_as_read() {
        a_branch_is_held_as_the_host_last_read_it(&Harness::bolt());
    }

    /// satisfies: 129, 124, C.7
    #[test]
    fn a_branch_on_a_remote_is_held_as_last_fetched() {
        a_branch_is_held_as_the_host_last_read_it(&on_origin());
    }

    /// satisfies: 129
    #[test]
    fn reading_a_branch_as_held_fetches_nothing() {
        let h = on_origin();
        let s = h.sluice();
        let b = s.seams().branch;
        let fetched = h.w().fetches();
        h.elsewhere(&[("c/lib", Some("landed elsewhere"))]);
        b.held(BRANCH).unwrap();
        b.held(BRANCH).unwrap();
        assert_eq!(h.w().fetches(), fetched);
        b.read(BRANCH).unwrap();
        assert_eq!(h.w().fetches(), fetched + 1);
    }

    /// satisfies: 129, 26
    #[test]
    fn records_held_after_a_refresh_are_the_records_read_on_the_fakes() {
        records_held_after_a_refresh_are_the_records_read(&Harness::bolt());
    }
}
