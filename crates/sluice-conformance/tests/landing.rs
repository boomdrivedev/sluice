//! Landing, conflicts, crashes and notifications: A.3, A.5, B.4, B.5.

use sluice_conformance::{policy, shared, Crash, Harness, Rig, BRANCH};
use sluice_core::records::{Event, State};
use sluice_core::Step;

/// satisfies: 24, 46, 59, S14
#[test]
fn a_crash_between_the_landing_intent_and_the_move_is_finished_by_the_next_step() {
    for crash in [Crash::BeforeMove, Crash::AfterMove] {
        let h = Harness::bolt();
        let base = h.tip();
        h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
        let one = h.submit("unit/a");
        h.step();
        h.w().crash(crash);
        assert!(h.sluice().step(BRANCH).is_err());
        // The intent is in the records; the submission is landing.
        assert_eq!(h.sub(one).state, State::Landing);

        h.settle();
        let s = h.sub(one);
        assert_eq!(s.state, State::Landed, "{crash:?}");
        assert_eq!(h.tip(), s.landed_as.unwrap());
        // Exactly one move, from the revision the candidate was formed on.
        let moves = h.w().moves();
        assert_eq!(moves.len(), 1, "{crash:?}");
        assert_eq!(moves[0].from, base);
        assert_eq!(h.w().delivered_to("agent-a").len(), 1);
    }
}

/// satisfies: 122, 24
#[test]
fn a_submissions_work_is_on_its_branch_once_the_branch_holds_its_landing() {
    let h = Harness::bolt();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    let s = h.sluice();
    assert_eq!(s.landed_as(one).unwrap(), None, "queued");
    h.step();
    // The landing is recorded, and the branch has not moved.
    h.w().crash(Crash::BeforeMove);
    assert!(h.sluice().step(BRANCH).is_err());
    assert_eq!(h.sub(one).state, State::Landing);
    assert_eq!(s.landed_as(one).unwrap(), None, "not moved");

    // The branch moved and the write confirming it failed: the record
    // still says landing, and the work is on the branch all the same,
    // past a commit added on top.
    h.w().crash_next_write();
    assert!(h.sluice().step(BRANCH).is_err());
    assert_eq!(h.sub(one).state, State::Landing);
    let landing = h.tip();
    assert_eq!(s.landed_as(one).unwrap(), Some(landing.clone()));
    h.w().hand_commit(BRANCH, &[("c/lib", Some("c2"))]);
    assert_eq!(s.landed_as(one).unwrap(), Some(landing.clone()));

    // The next round records it landed, as the same commit.
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.sub(one).landed_as, Some(landing.clone()));
    assert_eq!(s.landed_as(one).unwrap(), Some(landing));
}

/// satisfies: 51, 50
#[test]
fn a_notification_repeated_after_a_crash_carries_the_same_id_and_outcome() {
    let h = Harness::bolt();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    h.w().crash_after_next_notify();
    assert!(h.sluice().step(BRANCH).is_err());
    h.settle();
    let delivered = h.w().delivered_to("agent-a");
    assert_eq!(delivered.len(), 2);
    assert_eq!(delivered[0], delivered[1]);
    assert_eq!(delivered[0].id, format!("{one}:landed"));
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.handed(&format!("{one}:landed")), ["notice"]);
}

/// satisfies: 65, 12
#[test]
fn a_conflict_names_every_submission_it_is_contested_with() {
    shared::a_conflict_names_every_submission_it_is_contested_with(&Harness::bolt());
}

/// satisfies: 14, 22, 23, 55
#[test]
fn deepest_first_the_longest_passes_and_all_land_on_its_verdict() {
    shared::deepest_first_the_longest_passes_and_all_land_on_its_verdict(&Harness::new(&policy(
        true, "fail", 4,
    )));
}

/// satisfies: 62, 14
#[test]
fn one_step_at_a_time_and_a_refused_write_does_nothing() {
    let h = Harness::bolt();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");

    h.w().hold_lock(BRANCH, true);
    assert_eq!(h.step(), Step::Busy);
    h.w().hold_lock(BRANCH, false);

    h.w().interfere_next_write();
    assert_eq!(h.step(), Step::Contended);
    assert!(h.w().made_places().is_empty());
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: 22, 24
#[test]
fn the_line_moves_only_in_chain_order_from_its_formed_revision() {
    let h = Harness::bolt();
    h.w().hold_runs(true);
    h.work("unit/1", "agent-1", &[("a/one", Some("1"))]);
    h.work("unit/2", "agent-2", &[("a/two", Some("2"))]);
    let (one, two) = (h.submit("unit/1"), h.submit("unit/2"));
    h.step();
    // The second passes first: it may not land past the first.
    let c2 = h.sub(two).tested.unwrap().combination;
    h.w().complete(&c2);
    h.step();
    h.step();
    assert!(h.w().moves().is_empty());
    assert_eq!(h.sub(two).state, State::Passed);
    h.w().hold_runs(false);
    h.w().complete_all();
    h.settle();
    let landed: Vec<Vec<u64>> = h
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Landed { ids, .. } => Some(ids),
            _ => None,
        })
        .collect();
    assert_eq!(landed, vec![vec![one, two]]);
    let m = h.w().moves();
    assert_eq!(m.len(), 1);
}

/// satisfies: 24, 59
#[test]
fn a_landing_moved_over_by_hand_before_it_was_recorded_still_counts() {
    let h = Harness::bolt();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    h.w().crash(Crash::AfterMove);
    assert!(h.sluice().step(BRANCH).is_err());
    let by_hand = h.w().hand_commit(BRANCH, &[("c/lib", Some("by hand"))]);
    h.settle();
    let s = h.sub(one);
    assert_eq!(s.state, State::Landed);
    assert!(h.w().is_ancestor(&s.landed_as.unwrap(), &by_hand));
    assert!(!h
        .events()
        .iter()
        .any(|e| matches!(e, Event::Refused { .. })));
}

/// satisfies: 121, 22, 45
#[test]
fn each_landing_is_the_revision_the_branch_must_then_contain() {
    let h = Harness::bolt();
    assert_eq!(h.doc().landed, None);
    for (n, file) in ["a/one", "b/two"].into_iter().enumerate() {
        let unit = format!("unit/{n}");
        h.work(&unit, "agent", &[(file, Some("x"))]);
        let id = h.submit(&unit);
        h.settle();
        assert_eq!(h.sub(id).state, State::Landed);
        assert_eq!(h.doc().landed, Some(h.tip()), "after landing {file}");
    }
    assert_eq!(h.w().moves().len(), 2);
}

/// What GitHub's push protection says of a landing it refuses.
const DECLINED: &str = "remote: error: GH013: Repository rule violations found for refs/heads/bolt/x.\nremote: - Push cannot contain secrets";

/// The `forge_refused` lines of the branch's log, by their submissions and
/// words.
fn forge_refused(h: &Harness) -> Vec<(Vec<u64>, String)> {
    h.events()
        .into_iter()
        .filter_map(|e| match e {
            Event::ForgeRefused { ids, words } => Some((ids, words)),
            _ => None,
        })
        .collect()
}

/// satisfies: 152, 24, 45; rule core.5; record 0124: a landing the forge
/// refuses with the branch where it expected it is pushed once and held,
/// with the forge's words, its submission refused
#[test]
fn a_landing_the_forge_refused_is_pushed_once_and_held() {
    let h = Harness::bolt();
    let base = h.tip();
    h.w().forge_refuses(BRANCH, Some(DECLINED));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    for _ in 0..6 {
        h.step();
    }
    assert_eq!(h.w().refused_moves(), [BRANCH], "pushed once");
    assert_eq!(h.tip(), base, "the branch stays where it was");
    assert_eq!(h.sub(one).state, State::Refused);
    let landing = h.doc().landing.expect("the landing is held");
    let refused = landing.refused.expect("with the forge's refusal");
    assert_eq!(refused.words, DECLINED);
    assert_eq!(forge_refused(&h), [(vec![one], DECLINED.to_string())]);
    assert!(h.w().moves().is_empty());
    assert!(h
        .w()
        .delivered()
        .iter()
        .all(|d| d.id != format!("{one}:failed")));
    assert!(
        !h.sluice().in_play(BRANCH).unwrap(),
        "nothing of it is in play"
    );
}

/// satisfies: 152, 7; record 0124: while a landing the forge refused is
/// held, nothing is formed on its branch and the submissions behind it wait
#[test]
fn the_submissions_behind_a_held_landing_wait() {
    let h = Harness::bolt();
    h.w().forge_refuses(BRANCH, Some(DECLINED));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(h.sub(one).state, State::Refused);
    let (forms, started) = (h.w().forms(), h.w().started().len());
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let two = h.submit("unit/b");
    for _ in 0..4 {
        assert_eq!(h.step(), Step::Stepped);
    }
    assert_eq!(h.sub(two).state, State::Waiting);
    assert_eq!(h.w().forms(), forms, "nothing formed");
    assert_eq!(h.w().started().len(), started, "nothing started");
    assert_eq!(h.w().refused_moves().len(), 1, "nothing pushed again");
    assert_eq!(h.sub(one).state, State::Refused);
}

/// satisfies: 152, 67, 5; record 0124: a submission from the place of a
/// submission whose landing the forge refused withdraws it, which clears
/// the held landing, and the new submission lands
#[test]
fn a_submission_from_its_place_clears_a_held_landing() {
    let h = Harness::bolt();
    h.w().forge_refuses(BRANCH, Some(DECLINED));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(h.sub(one).state, State::Refused);
    // The submitter replaces its work, the secret taken out.
    h.w().forge_refuses(BRANCH, None);
    let head = h.work_on("unit/a", &h.tip(), &[("a/lib", Some("a3"))]);
    let two = h.submit("unit/a");
    assert_eq!(h.sub(one).state, State::Withdrawn);
    h.step();
    assert!(h.doc().landing.is_none(), "the held landing is cleared");
    h.settle();
    let s = h.sub(two);
    assert_eq!(s.state, State::Landed);
    assert_eq!(s.head, head);
}

/// satisfies: 152, 24, 5; record 0124: once one of a held landing's
/// submissions is withdrawn, the landing is dropped, and the rest go back
/// to the queue and are formed again
#[test]
fn a_held_landing_is_dropped_once_one_of_its_submissions_leaves() {
    use sluice_core::Requester;
    let h = Harness::bolt();
    h.w().forge_refuses(BRANCH, Some(DECLINED));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let one = h.submit("unit/a");
    let two = h.submit("unit/b");
    for _ in 0..4 {
        h.step();
    }
    let held = h.doc().landing.expect("a landing held");
    assert_eq!(held.ids, [one, two]);
    assert_eq!(h.sub(two).state, State::Refused);
    h.sluice()
        .withdraw(one, &Requester::Operator)
        .unwrap()
        .unwrap();
    h.w().forge_refuses(BRANCH, None);
    h.step();
    assert!(h.doc().landing.is_none(), "the landing is dropped");
    assert_ne!(h.sub(two).state, State::Refused, "back in the queue");
    h.settle();
    assert_eq!(h.sub(one).state, State::Withdrawn);
    assert_eq!(h.sub(two).state, State::Landed);
}

/// satisfies: 152, 50, 51; rule core.52; record 0124: a submission whose
/// landing the forge refused is owed to its work under `<id>:refused`,
/// with the forge's words, deciding no outcome
#[test]
fn a_held_landing_is_owed_to_its_submitter() {
    use sluice_seams::Decided;
    let h = Harness::bolt();
    h.w().forge_refuses(BRANCH, Some(DECLINED));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    for _ in 0..4 {
        h.step();
    }
    let key = format!("{one}:refused");
    let s = h.sluice();
    let line =
        sluice_core::line::Line::load(BRANCH, s.seams().state.read(BRANCH).unwrap()).unwrap();
    let owed: Vec<&weir_delivery::Delivery> = line
        .deliveries()
        .iter()
        .filter_map(|r| match r {
            weir_delivery::Record::Delivery(d) if d.key().as_str() == key => Some(d),
            _ => None,
        })
        .collect();
    assert_eq!(owed.len(), 1, "recorded once, under {key}");
    assert!(
        owed[0].message().as_str().contains(DECLINED),
        "{}",
        owed[0].message().as_str()
    );
    assert_eq!(
        owed[0]
            .readers()
            .iter()
            .map(|r| r.as_str())
            .collect::<Vec<_>>(),
        ["unit/a"],
        "owed to its work"
    );
    let told = h.delivered_to("agent-a");
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].id, key);
    assert_eq!(
        told[0].decided,
        Decided::Refused {
            words: DECLINED.to_string()
        }
    );
    let sub = h.sub(one);
    assert_eq!(sub.state, State::Refused);
    assert_eq!(sub.decided(), None, "no outcome decided");
}
