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

/// satisfies: record 0122, 59 (design D3); the queue-home spec's "A move
/// waits for a combination under test": while `[a]` is under test the move
/// carries nothing, rounds after rounds; once it has landed, the next round
/// carries the submission made meanwhile to origin and closes this host's
/// own home.
#[test]
fn a_move_waits_for_the_combination_under_test() {
    let (h, _, a) = moving(false);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let b = h.submit("unit/b");
    for _ in 0..3 {
        step_box(&h);
        assert_eq!(h.w().carries(), 0);
    }
    h.w().signs_as("box");
    land(&h, a);
    assert_eq!(
        h.w().carries(),
        0,
        "the round that landed it carried nothing"
    );
    step_box(&h);
    assert_eq!(h.w().carries(), 1);
    let own = h.w().home_in(HERE, BRANCH).unwrap();
    assert_eq!(own.closed_to(), Some("origin"));
    assert!(h
        .w()
        .file_in("origin", BRANCH, &format!("submissions/{b}.json"))
        .is_some());
    assert_eq!(h.sub(b).state, State::Queued);
}

/// A bolt open in this host's own files holding #52 queued and #53 blamed,
/// with no combination under test, and a policy naming signers and
/// `remote = "origin"` put on the branch by hand.
fn blamed_and_queued() -> Harness {
    let h = Harness::new(&unsigned());
    h.w().fails_with("b:test", "b/lib", "bad");
    h.w().numbers_from(53);
    h.work("unit/b", "agent-b", &[("b/lib", Some("bad"))]);
    assert_eq!(h.submit("unit/b"), 53);
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(h.sub(53).state, State::Failed);
    h.w().numbers_from(52);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    assert_eq!(h.submit("unit/a"), 52);
    h.w()
        .hand_commit(BRANCH, &[(".config/sluice.toml", Some(&signed()))]);
    h
}

/// satisfies: record 0122, 62, 63 (design D3, D5); the queue-home spec's
/// "A host's queue moves to origin": #52 queued and #53 blamed are carried
/// to origin under those numbers and in those states, origin's next number
/// is past 53, this host's own home is closed naming origin, and the round
/// logs `carried` naming both.
#[test]
fn a_move_carries_every_live_submission_with_its_number_and_state() {
    let h = blamed_and_queued();
    step_box(&h);
    assert_eq!(h.w().carries(), 1);
    assert_eq!(h.sub(52).state, State::Queued);
    assert_eq!(h.sub(53).state, State::Failed);
    for id in [52, 53] {
        assert!(h
            .w()
            .file_in("origin", BRANCH, &format!("submissions/{id}.json"))
            .is_some());
        assert!(h
            .w()
            .file_in(HERE, BRANCH, &format!("submissions/{id}.json"))
            .is_none());
    }
    let s = h.sluice();
    let next = s.seams().state.allocate_id(BRANCH).unwrap();
    assert!(next > 53, "origin's next number is past 53: {next}");
    let own = h.w().home_in(HERE, BRANCH).unwrap();
    assert_eq!(own.state, HomeState::Closed);
    assert_eq!(own.closed_to(), Some("origin"));
    let origin = h.w().home_in("origin", BRANCH).unwrap();
    assert_eq!(origin.state, HomeState::Open);
    assert_eq!(origin.came_from.unwrap().home, HERE);
    assert!(h.events().contains(&Event::Carried {
        to: "origin".into(),
        numbers: [(52, 52), (53, 53)].into(),
    }));
}

/// satisfies: record 0122, 59 (design D3); the queue-home spec's "Stopped
/// after the new home was written": a round that wrote the submissions and
/// the home on origin and stopped before closing this host's own home is
/// followed by one that closes it naming origin and writes nothing more on
/// origin, each submission queued there once.
#[test]
fn a_move_stopped_after_the_new_home_was_written_is_closed_by_the_next_round() {
    let h = blamed_and_queued();
    h.w().stop_next_carry();
    assert!(h
        .sluice()
        .as_peer(Peer {
            signer: Some("box".into()),
            ..Peer::default()
        })
        .step(BRANCH)
        .is_err());
    assert_eq!(
        h.w().home_in(HERE, BRANCH).unwrap().state,
        HomeState::Moving
    );
    assert_eq!(
        h.w().home_in("origin", BRANCH).unwrap().state,
        HomeState::Open
    );
    let written = h.w().file_in("origin", BRANCH, "version");
    step_box(&h);
    assert_eq!(
        h.w().home_in(HERE, BRANCH).unwrap().closed_to(),
        Some("origin")
    );
    assert_eq!(
        h.w().file_in("origin", BRANCH, "version"),
        written,
        "nothing more written on origin"
    );
    let live: Vec<u64> =
        sluice_core::line::Line::load(BRANCH, h.sluice().seams().state.read(BRANCH).unwrap())
            .unwrap()
            .subs
            .keys()
            .copied()
            .collect();
    assert_eq!(live, [52, 53], "each once");
    let carried = h
        .events()
        .into_iter()
        .filter(|e| matches!(e, Event::Carried { .. }))
        .count();
    assert_eq!(carried, 1);
}

/// satisfies: record 0122, 62 (design D3): a submission made while its
/// branch's move is written is taken into the moving home, this host's own
/// files, and carried with the rest once the move carries.
#[test]
fn a_submission_made_while_moving_is_carried() {
    let (h, _, a) = moving(false);
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let b = h.submit("unit/b");
    let in_own = format!("submissions/{b}.json");
    assert!(
        h.w().file_in(HERE, BRANCH, &in_own).is_some(),
        "taken into the moving home"
    );
    assert!(h.w().home_in("origin", BRANCH).is_none());
    h.w().signs_as("box");
    land(&h, a);
    step_box(&h);
    assert!(h.w().file_in(HERE, BRANCH, &in_own).is_none());
    assert!(h.w().file_in("origin", BRANCH, &in_own).is_some());
    assert_eq!(h.sub(b).state, State::Queued);
    let carried = h.events().into_iter().find_map(|e| match e {
        Event::Carried { numbers, .. } => Some(numbers),
        _ => None,
    });
    assert_eq!(carried, Some([(b, b)].into()));
}

/// satisfies: record 0122, 62 (design D3); the queue-home spec's "A submit
/// racing the carry": a submit reads the branch's home in this host's own
/// files as moving, and rounds land what was under test and carry the queue,
/// closing that home, between the submit's numbering and its write; the
/// submit's write is not made there but on origin, where its pointer names,
/// and the submission is queued on origin.
#[test]
fn a_submit_racing_the_carry_is_written_in_the_new_home() {
    let (h, _, a) = moving(false);
    h.w().signs_as("box");
    h.work("unit/c", "agent-c", &[("c/lib", Some("c2"))]);
    let world = h.w().clone();
    h.w().after_allocating(move |w, _| {
        let f = sluice_fakes::Fakes::of(w.clone());
        let s = f.sluice().as_peer(Peer {
            signer: Some("box".into()),
            ..Peer::default()
        });
        for _ in 0..12 {
            if w.carries() > 0 {
                return;
            }
            if w.live_runs() > 0 {
                w.complete_all();
            }
            s.step(BRANCH).expect("a round runs");
        }
    });
    assert_eq!(
        world.home_in(HERE, BRANCH).unwrap().state,
        HomeState::Moving
    );
    let c = h.submit("unit/c");
    assert_eq!(world.carries(), 1);
    assert!(
        world
            .file_in(HERE, BRANCH, &format!(".finished/{a}.json"))
            .is_some(),
        "#{a} landed first"
    );
    assert_eq!(
        world.home_in(HERE, BRANCH).unwrap().closed_to(),
        Some("origin")
    );
    let file = format!("submissions/{c}.json");
    assert!(
        world.file_in(HERE, BRANCH, &file).is_none(),
        "not written in the closed home"
    );
    assert!(world.file_in("origin", BRANCH, &file).is_some());
    assert_eq!(h.sub(c).state, State::Queued);
}

/// Instance `name`, on its own host, signing as `box`.
fn on<'a>(h: &'a Harness, name: &str) -> sluice_core::Sluice<'a> {
    h.w().be_host(name);
    h.sluice().as_peer(Peer {
        name: name.into(),
        signer: Some("box".into()),
        ..Peer::default()
    })
}

/// satisfies: record 0122, 26, C.6 (design D4); the queue-home spec's "Two
/// hosts' submissions come home": `bolt/x` open on origin with #7 made on
/// host a and #8 on host b; a landing brings a policy naming no signers and
/// nothing is under test. a's round closes origin's home toward the hosts'
/// own files carrying #7 into a's own; b's next round carries #8 into b's
/// own before it reads the branch, and origin's home names each submission
/// and the host that took it.
#[test]
fn a_round_carries_its_hosts_submissions_home_before_it_reads_the_branch() {
    let h = Harness::new(&signed());
    h.w().signs_as("box");
    h.w().numbers_from(7);
    for (host, worktree) in [("a", "unit/a"), ("b", "unit/b")] {
        h.w().be_host(host);
        let file = if host == "a" { "a/lib" } else { "b/lib" };
        h.work(worktree, &format!("agent-{host}"), &[(file, Some("2"))]);
        h.w().from_host(worktree, host);
        h.submit(worktree);
    }
    let origin = h.w().home_in("origin", BRANCH).expect("open on origin");
    assert_eq!(origin.state, HomeState::Open);
    h.w()
        .hand_commit(BRANCH, &[(".config/sluice.toml", Some(&unsigned()))]);

    on(&h, "a").step(BRANCH).unwrap();
    h.w().be_host("a");
    assert_eq!(
        h.w().home_in("origin", BRANCH).unwrap().closed_to(),
        Some(HERE)
    );
    assert!(h.w().file_in(HERE, BRANCH, "submissions/7.json").is_some());
    assert_eq!(h.sub(7).state, State::Queued);
    assert!(
        h.w()
            .file_in("origin", BRANCH, "submissions/8.json")
            .is_some(),
        "b's, left for b"
    );

    on(&h, "b").step(BRANCH).unwrap();
    h.w().be_host("b");
    assert!(h.w().file_in(HERE, BRANCH, "submissions/8.json").is_some());
    assert!(h
        .w()
        .file_in("origin", BRANCH, "submissions/8.json")
        .is_none());
    assert_eq!(h.sub(8).state, State::Queued);
    let first = h.events().into_iter().next();
    assert_eq!(
        first,
        Some(Event::Carried {
            to: HERE.into(),
            numbers: [(8, 8)].into(),
        }),
        "carried before the round read the branch"
    );
    let carried = h
        .w()
        .home_in("origin", BRANCH)
        .unwrap()
        .closed
        .unwrap()
        .carried;
    let by: Vec<(u64, u64, Option<String>)> = carried
        .into_iter()
        .map(|(n, t)| (n, t.number, t.host))
        .collect();
    assert_eq!(by, [(7, 7, Some("a".into())), (8, 8, Some("b".into()))]);
}
