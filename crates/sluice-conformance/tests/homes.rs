//! A branch's queue's home, written in its records, and a move between
//! homes: written in the current home, waiting for the combinations under
//! test, carrying every live submission with its number and state, and
//! finished by the next round wherever the last one stopped (record 0122;
//! 58, 59, 62; C.6; design D3 to D7).

use sluice_conformance::{policy, Harness, Rig, BRANCH};
use sluice_core::records::{CandidateState, Event, HomeState, State, HERE};
use sluice_core::{Peer, Step};
use sluice_seams::Rev;

/// The bolt policy with its records on `origin`, trusting the signer
/// `box` this host signs as, as peers.rs's harness has it.
fn signed() -> String {
    policy(false, "fail", 4).replace(
        "trust = [\"open\", \"sandbox\", \"container\"]\n",
        "trust = [\"box\"]\nremote = \"origin\"\n",
    ) + "\n[signers.box]\nssh = \"box ssh-ed25519 AAAA\"\n"
}

/// The bolt policy, its records in the host's own files.
fn unsigned() -> String {
    policy(false, "fail", 4)
}

/// The revision of the branch's combination whose newest submission is
/// `id`: once the entries ahead land as tested, it is named on the revision
/// they landed as with the entries still ahead of it (rule core.17).
fn combination(h: &Harness, id: u64) -> Rev {
    h.doc()
        .candidates
        .iter()
        .find(|c| c.submissions.last() == Some(&id))
        .unwrap_or_else(|| panic!("no combination ending in #{id}"))
        .revision
        .clone()
}

/// One round on this host, which signs as `box`.
fn step_box(h: &Harness) -> Step {
    h.sluice()
        .as_peer(Peer {
            signer: Some("box".into()),
            ..Peer::default()
        })
        .step(BRANCH)
        .expect("a step runs")
}

/// Complete each run of `id`'s combination and step, as a host signing
/// as `box`, until it lands.
fn land(h: &Harness, id: u64) {
    for _ in 0..12 {
        if h.sub(id).state == State::Landed {
            return;
        }
        if h.w().live_runs() > 0 {
            h.w().complete(&combination(h, id));
        }
        step_box(h);
    }
    assert_eq!(h.sub(id).state, State::Landed);
}

fn moves_logged(h: &Harness) -> Vec<Event> {
    h.events()
        .into_iter()
        .filter(|e| matches!(e, Event::Moving { .. } | Event::Stayed { .. }))
        .collect()
}

/// A bolt open in this host's own files, runs held: `p`, the signed
/// policy, and `a` are submitted and tested, `p` passes and lands, and the
/// next round reads the signed policy at the head while `[p, a]` is under
/// test. With `back`, `a` brings the unsigned policy back. Returns `p`
/// and `a`.
fn moving(back: bool) -> (Harness, u64, u64) {
    let h = Harness::new(&unsigned());
    h.w().hold_runs(true);
    h.work(
        "unit/p",
        "agent-p",
        &[(".config/sluice.toml", Some(&signed()))],
    );
    let p = h.submit("unit/p");
    let back_to = unsigned();
    let a_change: Vec<(&str, Option<&str>)> = match back {
        true => vec![
            (".config/sluice.toml", Some(&back_to)),
            ("a/lib", Some("a2")),
        ],
        false => vec![("a/lib", Some("a2"))],
    };
    let at = h.w().work(&h.sub(p).head, &a_change);
    h.w().session("unit/a", "agent-a", &at);
    let a = h.submit("unit/a");
    assert_eq!(h.step(), Step::Stepped);
    let home = h
        .w()
        .home_in(HERE, BRANCH)
        .expect("its first write wrote it");
    assert_eq!(home.state, HomeState::Open);
    h.w().complete(&combination(&h, p));
    h.step();
    h.step();
    assert_eq!(h.sub(p).state, State::Landed);
    (h, p, a)
}

/// satisfies: record 0122, 58 (design D3); the queue-home spec's "A move
/// waits for a combination under test": a landing brings a policy naming
/// signers and `remote = "origin"` while `[p, a]` is under test in this
/// host's own files; the round logs `moving`, a submission made meanwhile
/// is taken in and has no combination opened for it, and nothing is
/// carried while `[p, a]` is under test.
#[test]
fn a_policy_naming_another_home_writes_a_move_and_opens_no_combination() {
    let (h, p, a) = moving(false);
    let home = h.w().home_in(HERE, BRANCH).unwrap();
    assert_eq!(home.state, HomeState::Moving);
    assert_eq!(home.moving_to(), Some("origin"));
    assert_eq!(
        moves_logged(&h),
        [Event::Moving {
            to: "origin".into(),
            at: Some(h.tip()),
        }]
    );
    let under_test = h
        .doc()
        .candidates
        .iter()
        .find(|c| c.submissions.contains(&a))
        .map(|c| c.state);
    assert_eq!(
        under_test,
        Some(CandidateState::Testing),
        "#{p} landed first"
    );

    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let b = h.submit("unit/b");
    for _ in 0..3 {
        h.step();
    }
    assert!(
        h.tested().iter().all(|ids| !ids.contains(&b)),
        "{:?}",
        h.tested()
    );
    assert!(h
        .doc()
        .candidates
        .iter()
        .all(|c| !c.submissions.contains(&b)));
    assert!(!h.sub(b).state.is_out(), "taken in");
    assert_eq!(
        h.w().carries(),
        0,
        "nothing carried while [p, a] is under test"
    );
    assert!(h.w().home_in("origin", BRANCH).is_none());
    assert_eq!(
        h.w().home_in(HERE, BRANCH).unwrap().state,
        HomeState::Moving
    );
}

/// satisfies: record 0122 (design D3); the queue-home spec's "The policy
/// names the open home again": `a` brings back a policy naming no signers
/// before the move carries anything, `[p, a]` under test all along; once
/// it lands, the next round logs `stayed`, the queue stays open in this
/// host's own files, and combinations form again.
#[test]
fn a_policy_naming_the_open_home_again_ends_the_move() {
    let (h, _, a) = moving(true);
    assert_eq!(
        h.w().home_in(HERE, BRANCH).unwrap().state,
        HomeState::Moving
    );
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let b = h.submit("unit/b");
    h.step();
    assert!(h.tested().iter().all(|ids| !ids.contains(&b)));
    // [p, a]'s run passes under the gate the branch carries now, which
    // trusts box: its replays no box proved run again (108).
    h.w().signs_as("box");
    land(&h, a);
    h.step();
    assert!(matches!(
        moves_logged(&h).last(),
        Some(Event::Stayed { .. })
    ));
    let home = h.w().home_in(HERE, BRANCH).unwrap();
    assert_eq!((home.state, home.moving), (HomeState::Open, None));
    assert!(h.w().home_in("origin", BRANCH).is_none());
    assert_eq!(h.w().carries(), 0);
    h.step();
    assert!(
        h.tested().iter().any(|ids| ids.contains(&b)),
        "combinations form again: {:?}",
        h.tested()
    );
}
