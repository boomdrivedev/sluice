//! Reading a branch: a round forms only on a revision holding the
//! branch's last landing, and holds a branch read without it (121), over
//! the fakes.

use sluice_conformance::{Harness, Rig, BRANCH};
use sluice_core::branches::{AcceptRefusal, Acceptance};
use sluice_core::line::Line;
use sluice_core::records::{to_document, BranchHold, Event, State, BRANCH_FILE};
use sluice_core::Step;
use sluice_seams::{Changes, Rev, Written};

/// A bolt that has landed one submission: the harness, the revision it
/// was made at, and the landing.
fn landed_once() -> (Harness, Rev, Rev) {
    let h = Harness::bolt();
    let made = h.tip();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    let landing = h.tip();
    (h, made, landing)
}

/// The `held` lines of the branch's log, by the revision read and the
/// landing it lacked.
fn held_lines(h: &Harness) -> Vec<(Rev, Rev)> {
    h.events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Held { read, lacks } => Some((read, lacks)),
            _ => None,
        })
        .collect()
}

/// What a round did that a hold rules out: merges formed, runs started
/// and moves of the branch.
fn work_done(h: &Harness) -> (usize, usize, usize) {
    (h.w().forms(), h.started().len(), h.moves())
}

/// satisfies: 121, 22
#[test]
fn a_revision_behind_the_last_landing_holds_the_branch() {
    let (h, made, landing) = landed_once();
    h.branch_at(BRANCH, &made);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let two = h.submit("unit/b");
    let before = work_done(&h);
    assert_eq!(h.step(), Step::Held);
    assert_eq!(work_done(&h), before, "nothing formed, started or landed");
    assert_eq!(h.sub(two).state, State::Queued);
    assert_eq!(h.tip(), made, "the branch is left where it is read");
    let held = h.doc().held.expect("the records hold the hold");
    assert_eq!((&held.read, &held.lacks), (&made, &landing));
    assert_eq!(held_lines(&h), vec![(made, landing)]);
    // `base` stays the revision last formed on: no `moved` line.
    assert!(!h.events().iter().any(|e| matches!(e, Event::Moved { .. })));
}

/// satisfies: 121
#[test]
fn a_revision_beside_the_last_landing_holds_the_branch() {
    let (h, made, landing) = landed_once();
    let beside = h.w().work(&made, &[("c/lib", Some("beside"))]);
    h.branch_at(BRANCH, &beside);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let two = h.submit("unit/b");
    let before = work_done(&h);
    assert_eq!(h.step(), Step::Held);
    assert_eq!(work_done(&h), before);
    assert_eq!(h.sub(two).state, State::Queued);
    assert_eq!(held_lines(&h), vec![(beside, landing)]);
}

/// satisfies: 121, 22, 45, S8
#[test]
fn a_commit_on_top_of_the_last_landing_is_formed_on() {
    let (h, _, landing) = landed_once();
    let on_top = h.w().hand_commit(BRANCH, &[("c/lib", Some("by hand"))]);
    h.work_on("unit/a", &on_top, &[("a/more", Some("a3"))]);
    let two = h.submit("unit/a");
    h.settle();
    let s = h.sub(two);
    assert_eq!(s.state, State::Landed);
    assert_eq!(s.tested.unwrap().on, on_top);
    assert_eq!(h.w().parents(&h.tip())[0], on_top);
    assert!(held_lines(&h).is_empty());
    assert_eq!(h.doc().held, None);
    assert_ne!(h.doc().landed, Some(landing));
    assert_eq!(h.doc().landed, Some(h.tip()));
}

/// satisfies: 121, 26
#[test]
fn rounds_held_at_one_revision_write_one_line() {
    let (h, made, landing) = landed_once();
    h.branch_at(BRANCH, &made);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    h.submit("unit/b");
    for _ in 0..3 {
        assert_eq!(h.step(), Step::Held);
    }
    assert_eq!(held_lines(&h), vec![(made.clone(), landing.clone())]);
    // Read at another revision lacking it, the hold names that one.
    let beside = h.w().work(&made, &[("c/lib", Some("beside"))]);
    h.branch_at(BRANCH, &beside);
    assert_eq!(h.step(), Step::Held);
    assert_eq!(
        held_lines(&h),
        vec![(made, landing.clone()), (beside.clone(), landing.clone())]
    );
    let round = h.sluice().round(BRANCH).unwrap();
    assert_eq!(round.step, Step::Held);
    let held = round.held.expect("the round says the hold");
    assert_eq!(
        held,
        BranchHold {
            read: beside,
            lacks: landing,
            since: held.since.clone(),
        }
    );
}

/// satisfies: 121, 17, 10
#[test]
fn a_run_under_test_finishes_and_lands_once_the_landing_is_back() {
    let (h, made, landing) = landed_once();
    h.w().hold_runs(true);
    h.work_on("unit/a", &landing, &[("a/more", Some("a3"))]);
    let two = h.submit("unit/a");
    h.step();
    assert_eq!(h.sub(two).state, State::Testing);
    let started = h.started().len();
    let combination = h.sub(two).tested.unwrap().combination;

    h.branch_at(BRANCH, &made);
    assert_eq!(h.step(), Step::Held);
    // The run goes on: its result is not read while the branch is held.
    h.w().complete(&combination);
    assert_eq!(h.step(), Step::Held);
    assert_eq!(h.sub(two).state, State::Testing);
    assert_eq!(h.w().live_places().len(), 1, "its place is kept");

    h.branch_at(BRANCH, &landing);
    h.settle();
    let s = h.sub(two);
    assert_eq!(s.state, State::Landed);
    assert_eq!(s.tested.unwrap().on, landing);
    assert_eq!(h.started().len(), started, "no new run");
    assert_eq!(h.doc().held, None);
    assert!(!h.events().iter().any(|e| matches!(e, Event::Moved { .. })));
}

/// The branch's records as an earlier sluice left them: no revision a
/// read must contain, and `base` as given when it is.
fn written_before(h: &Harness, base: Option<&Rev>) {
    let s = h.sluice();
    let lock = s.seams().state.lock(BRANCH).unwrap().expect("the lock");
    let snapshot = s.seams().state.read(BRANCH).unwrap();
    let version = snapshot.version;
    let mut doc = Line::load(BRANCH, snapshot).unwrap().doc;
    doc.landed = None;
    if let Some(b) = base {
        doc.base = Some(b.clone());
    }
    let mut changes = Changes::default();
    changes.put.insert(BRANCH_FILE.into(), to_document(&doc));
    let written = s.seams().state.write(BRANCH, version, changes).unwrap();
    assert!(matches!(written, Written::Written(_)));
    drop(lock);
    assert_eq!(h.doc().landed, None);
}

/// satisfies: 121, 26
#[test]
fn an_older_branchs_records_hold_it_by_their_last_landing() {
    let (h, made, landing) = landed_once();
    written_before(&h, None);
    h.branch_at(BRANCH, &made);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let two = h.submit("unit/b");
    assert_eq!(h.step(), Step::Held);
    assert_eq!(h.sub(two).state, State::Queued);
    assert_eq!(held_lines(&h), vec![(made, landing.clone())]);
    assert_eq!(h.doc().landed, Some(landing), "taken and written");
}

/// satisfies: 121, 104, 105
#[test]
fn an_older_branch_removed_and_made_again_stands_on_its_new_revision() {
    let (h, made, landing) = landed_once();
    // An earlier sluice removed it, made it again where the landing is
    // not, and formed on that revision.
    let again = h.w().work(&made, &[("c/lib", Some("made again"))]);
    h.branch_at(BRANCH, &again);
    written_before(&h, Some(&again));
    h.work_on("unit/a", &again, &[("a/more", Some("a3"))]);
    let two = h.submit("unit/a");
    assert_eq!(h.step(), Step::Stepped);
    assert_eq!(h.doc().landed, Some(again.clone()), "taken from base");
    assert_ne!(h.doc().landed, Some(landing));
    h.settle();
    let s = h.sub(two);
    assert_eq!(s.state, State::Landed);
    assert_eq!(s.tested.unwrap().on, again);
    assert!(held_lines(&h).is_empty());
}

/// A bolt landed once and then read at the revision it was made at,
/// held there, with a submission queued.
fn held_at_made() -> (Harness, Rev, Rev, u64) {
    let (h, made, landing) = landed_once();
    h.branch_at(BRANCH, &made);
    h.work_on("unit/a", &made, &[("b/lib", Some("b2"))]);
    let two = h.submit("unit/a");
    assert_eq!(h.step(), Step::Held);
    (h, made, landing, two)
}

/// satisfies: 121, C.5, S8
#[test]
fn the_operator_accepts_the_revision_a_branch_is_held_at() {
    let (h, made, landing, two) = held_at_made();
    let accepted = h.sluice().accept_branch(BRANCH, made.short()).unwrap();
    assert_eq!(
        accepted,
        Ok(Acceptance {
            revision: made.clone(),
            lacked: landing.clone(),
        })
    );
    assert_eq!(h.doc().held, None);
    assert_eq!(h.doc().landed, Some(made.clone()));
    assert_eq!(h.tip(), made, "it moves no branch");
    assert!(h.events().contains(&Event::Accepted {
        revision: made.clone(),
        lacked: landing.clone(),
    }));
    // The next round finds the branch moved from what it formed on, and
    // forms the queue on the accepted revision (S8).
    assert_eq!(h.step(), Step::Stepped);
    assert!(h.events().contains(&Event::Moved {
        from: landing,
        to: made.clone(),
    }));
    h.settle();
    let s = h.sub(two);
    assert_eq!(s.state, State::Landed);
    assert_eq!(s.tested.unwrap().on, made);
    assert_eq!(h.w().parents(&h.tip())[0], made);
}

/// satisfies: 121, C.5
#[test]
fn an_acceptance_names_the_revision_the_branch_is_held_and_read_at() {
    let (h, made, landing, _) = held_at_made();
    let s = h.sluice();
    let version = h.version();
    let refused = |given: &str| s.accept_branch(BRANCH, given).unwrap().unwrap_err();
    let held_at = |read: &Rev| AcceptRefusal::NotHeldAt {
        held: made.clone(),
        read: read.clone(),
    };
    assert_eq!(refused(landing.as_str()), held_at(&made));
    assert_eq!(refused(&made.as_str()[..6]), held_at(&made), "too short");
    // Read at another revision since the hold was recorded: neither is
    // both the one held at and the one read.
    let since = h.w().work(&made, &[("c/lib", Some("since"))]);
    h.branch_at(BRANCH, &since);
    assert_eq!(refused(made.as_str()), held_at(&since));
    assert_eq!(refused(since.as_str()), held_at(&since));
    assert_eq!(h.version(), version, "nothing recorded");

    assert_eq!(
        s.accept_branch("bolt/none", made.as_str()).unwrap(),
        Err(AcceptRefusal::Absent)
    );
    h.w().hold_lock(BRANCH, true);
    assert_eq!(
        s.accept_branch(BRANCH, since.as_str()).unwrap(),
        Err(AcceptRefusal::Busy)
    );
    h.w().hold_lock(BRANCH, false);
}

/// satisfies: 121, C.5
#[test]
fn a_branch_that_is_not_held_has_nothing_to_accept() {
    let (h, _, landing) = landed_once();
    let version = h.version();
    assert_eq!(
        h.sluice().accept_branch(BRANCH, landing.as_str()).unwrap(),
        Err(AcceptRefusal::NotHeld(landing))
    );
    assert_eq!(h.version(), version, "nothing recorded");
}

/// satisfies: 129; design D16: a status read held, once the host
/// process's reading is current, reads every branch as the host holds it
/// and asks no remote: no fetch of the branch and no read of its records
/// afresh. One that is not held reads them afresh, and a round never reads
/// held.
#[test]
fn a_held_status_asks_no_remote() {
    let h = Harness::bolt();
    h.w().on_remote(BRANCH);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    let (fetches, reads) = (h.w().fetches(), h.w().state_reads());
    let status = h.sluice().holding().status(BRANCH).unwrap();
    assert_eq!(status.submissions.len(), 1);
    assert_eq!(status.submissions[0].id, one);
    assert_eq!(
        (h.w().fetches(), h.w().state_reads()),
        (fetches, reads),
        "a held status asked a remote"
    );
    h.sluice().status(BRANCH).unwrap();
    assert!(h.w().fetches() + h.w().state_reads() > fetches + reads);
    assert!(h.sluice().holding().round(BRANCH).is_err());
}
