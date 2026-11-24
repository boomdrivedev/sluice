//! A key the tree does not bear out holds the branch: on a branch whose
//! records are on its remote, a check whose key names a file the
//! combination's tree does not hold blames no submission. The branch holds
//! with the check and the file named, its entries stay queued, and they
//! are formed again at the first revision of the branch that changes the
//! check's inputs (the decision
//! `a-key-the-tree-does-not-bear-out-holds-the-branch`; C.6, 17, 20, 114;
//! rule core.7).

use std::time::Duration;

use sluice_conformance::{policy, Harness, Rig, BRANCH};
use sluice_core::records::{Event, KeyHold, State};
use sluice_core::{Peer, Requester, Sluice};

/// The file `a:test`'s key names, which the branch's tree does not hold.
const FILE: &str = "a/settings";

/// The bolt policy, its gate the `gate` tag, on `origin`, its runs
/// counted through the signer `box`.
fn signed() -> String {
    policy(false, "fail", 4).replace(
        "trust = [\"open\", \"sandbox\", \"container\"]\n",
        "trust = [\"box\"]\nremote = \"origin\"\n",
    ) + "\n[signers.box]\nssh = \"box ssh-ed25519 AAAA\"\n"
}

/// The three projects on a signed branch, this host signing as `box`, and
/// `a:test`'s key naming [`FILE`].
fn rig() -> Harness {
    let h = Harness::new(&signed());
    h.w().signs_as("box");
    h.w().lacks("a:test", Some(FILE));
    h
}

/// This host, `fake-host`, as the signer `box`.
fn signer(h: &Harness) -> Sluice<'_> {
    h.sluice().as_peer(Peer {
        name: "fake-host".into(),
        repository: "this-repository".into(),
        lease: Duration::from_secs(300),
        alive: Duration::from_secs(120),
        signer: Some("box".into()),
        systems: None,
        expire_after: sluice_core::EXPIRE_AFTER,
    })
}

/// Step as the signer until a step changes nothing.
fn settle(h: &Harness) {
    for _ in 0..64 {
        let v = h.w().version(BRANCH);
        let delivered = h.w().delivered().len();
        let places = h.w().made_places().len();
        signer(h).step(BRANCH).expect("a step runs");
        if v == h.w().version(BRANCH)
            && delivered == h.w().delivered().len()
            && places == h.w().made_places().len()
        {
            return;
        }
    }
    panic!("{BRANCH} never settled");
}

/// Each `key_held` line's check, file and entries, in order.
fn held_lines(h: &Harness) -> Vec<(String, String, Vec<u64>)> {
    h.events()
        .into_iter()
        .filter_map(|e| match e {
            Event::KeyHeld {
                check, file, ids, ..
            } => Some((check, file, ids)),
            _ => None,
        })
        .collect()
}

/// Each `key_hold_ended` line's check and revision, in order.
fn ended_lines(h: &Harness) -> Vec<(String, Option<sluice_seams::Rev>)> {
    h.events()
        .into_iter()
        .filter_map(|e| match e {
            Event::KeyHoldEnded { check, revision } => Some((check, revision)),
            _ => None,
        })
        .collect()
}

/// #1, changing `a/lib`, submitted and held on `a:test`'s key.
fn held(h: &Harness) -> u64 {
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    settle(h);
    one
}

/// satisfies: the decision `a-key-the-tree-does-not-bear-out-holds-the-branch`;
/// C.6, 17, 20; rule core.7; the key-hold spec's "A file the tree does not
/// hold"
#[test]
fn a_key_naming_a_file_the_tree_lacks_blames_no_one_and_holds_the_branch() {
    let h = rig();
    let one = held(&h);
    let sub = h.sub(one);
    assert_eq!(sub.state, State::Waiting, "{sub:?}");
    assert!(sub.failed.is_empty(), "{sub:?}");
    assert!(
        h.results().iter().all(|r| r.task != "a:test"),
        "nothing is recorded under a:test's key"
    );
    let hold = h.doc().key_held.expect("the branch holds");
    assert_eq!(
        (hold.check.as_str(), hold.file.as_str(), hold.ids.as_slice()),
        ("a:test", FILE, [one].as_slice())
    );
    assert_eq!(hold.why, format!("a:test: {FILE} is not in the tree"));
    assert_eq!(
        held_lines(&h),
        [("a:test".to_string(), FILE.to_string(), vec![one])]
    );
    let status = h.sluice().status(BRANCH).unwrap();
    assert_eq!(status.key_held.map(|k| k.ids), Some(vec![one]));
}

/// satisfies: the decision `a-key-the-tree-does-not-bear-out-holds-the-branch`;
/// 114; the key-hold spec's "Rounds while it holds"
#[test]
fn a_held_entry_is_not_run_again_while_the_branch_holds() {
    let h = rig();
    let one = held(&h);
    let runs = h.requests().len();
    for _ in 0..5 {
        signer(&h).step(BRANCH).unwrap();
    }
    assert_eq!(h.requests().len(), runs, "nothing is run again");
    assert_eq!(h.sub(one).state, State::Waiting);
    assert_eq!(held_lines(&h).len(), 1, "the hold is logged once");
}

/// satisfies: the decision `a-key-the-tree-does-not-bear-out-holds-the-branch`;
/// 140; the key-hold spec's "A move that does not change its inputs"
#[test]
fn a_move_that_does_not_change_the_checks_inputs_keeps_the_hold() {
    let h = rig();
    let one = held(&h);
    let runs = h.requests().len();
    h.elsewhere(&[("c/lib", Some("c2"))]);
    settle(&h);
    assert_eq!(h.requests().len(), runs, "nothing is formed again");
    assert_eq!(h.sub(one).state, State::Waiting);
    assert!(h.doc().key_held.is_some());
    assert!(ended_lines(&h).is_empty());
}

/// satisfies: the decision `a-key-the-tree-does-not-bear-out-holds-the-branch`;
/// 10, 140; the key-hold spec's "A revision that changes its inputs"
#[test]
fn a_revision_that_changes_the_checks_inputs_takes_the_entries_again() {
    let h = rig();
    let one = held(&h);
    let fixed = h.elsewhere(&[(FILE, Some("1"))]);
    settle(&h);
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(ended_lines(&h), [("a:test".to_string(), Some(fixed))]);
    assert_eq!(h.doc().key_held, None);
}

/// satisfies: the decision `a-key-the-tree-does-not-bear-out-holds-the-branch`;
/// 6, 10; rule core.6; the key-hold spec's "Work that changes its inputs"
#[test]
fn an_entry_that_changes_the_checks_inputs_lands_and_ends_the_hold() {
    let h = rig();
    let one = held(&h);
    h.work("unit/b", "agent-b", &[(FILE, Some("1"))]);
    let two = h.submit("unit/b");
    settle(&h);
    assert_eq!(h.sub(two).state, State::Landed);
    assert_eq!(h.sub(one).state, State::Landed);
    let landed: Vec<Vec<u64>> = h
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Landed { ids, .. } => Some(ids),
            _ => None,
        })
        .collect();
    assert_eq!(
        landed,
        [vec![two], vec![one]],
        "the work that ends the hold lands first"
    );
    assert_eq!(ended_lines(&h).len(), 1);
}

/// satisfies: the decision `a-key-the-tree-does-not-bear-out-holds-the-branch`;
/// the key-hold spec's "Work refused alike"
#[test]
fn an_entry_refused_alike_joins_the_hold() {
    let h = rig();
    let one = held(&h);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let two = h.submit("unit/b");
    settle(&h);
    assert_eq!(h.sub(two).state, State::Waiting);
    assert!(h.sub(two).failed.is_empty());
    let hold: KeyHold = h.doc().key_held.expect("the branch holds");
    assert_eq!(hold.ids, [one, two]);
    assert_eq!(held_lines(&h).len(), 1);
}

/// satisfies: the decision `a-key-the-tree-does-not-bear-out-holds-the-branch`;
/// 5, 67; the key-hold spec's "Its entries are their submitters'"
#[test]
fn a_held_entry_is_withdrawn_and_the_hold_ends_with_its_last() {
    let h = rig();
    let one = held(&h);
    signer(&h)
        .withdraw(one, &Requester::Operator)
        .unwrap()
        .unwrap();
    settle(&h);
    assert_eq!(h.sub(one).state, State::Withdrawn);
    assert_eq!(h.doc().key_held, None);
    assert_eq!(ended_lines(&h), [("a:test".to_string(), None)]);
}

/// satisfies: C.6, 20; the pass-proof spec's "A forged key": a key the
/// tree does not bear out for any cause but a file it does not hold is
/// blamed as before, and holds nothing
#[test]
fn a_forged_key_holds_nothing() {
    let h = rig();
    h.w().lacks("a:test", None);
    h.w().forges("a:test");
    let one = held(&h);
    assert_eq!(h.sub(one).state, State::Failed);
    assert_eq!(h.doc().key_held, None);
    assert!(held_lines(&h).is_empty());
}
