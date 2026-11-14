//! Places: A.7 and B.1.

use std::collections::BTreeSet;
use std::time::Duration;

use sluice_conformance::{policy, shared, Harness, Rig, BRANCH};
use sluice_core::records::{Event, State};
use sluice_core::release::Releasing;
use sluice_core::{Peer, Sluice};

/// satisfies: 25, 31, 32, 33, 34, 35
#[test]
fn every_place_holds_its_candidate_and_is_released_with_everything_in_it() {
    let h = Harness::bolt();
    h.w().fails_with("a:test", "a/two", "bad");
    h.w().hold_runs(true);
    h.work("unit/1", "agent-1", &[("a/one", Some("1"))]);
    h.work("unit/2", "agent-2", &[("a/two", Some("bad"))]);
    h.work("unit/3", "agent-3", &[("a/three", Some("3"))]);
    let ids = [h.submit("unit/1"), h.submit("unit/2"), h.submit("unit/3")];
    h.step();
    // Three places at once, none colliding (32).
    let live = h.w().live_places();
    assert_eq!(live.len(), 3);
    assert_eq!(live.iter().collect::<BTreeSet<_>>().len(), 3);
    // Each run's place holds exactly its candidate: the fake refuses a
    // run on a place holding anything else (33, 35).
    for r in h.w().requests() {
        assert!(r.place.name.starts_with("sluice/try/"));
    }

    h.w().hold_runs(false);
    h.w().complete_all();
    h.settle();
    assert_eq!(h.sub(ids[0]).state, State::Landed);
    assert_eq!(h.sub(ids[1]).state, State::Failed);
    assert_eq!(h.sub(ids[2]).state, State::Landed);
    // #3's first candidate held the culprit: it was dropped and its place
    // released with its run ended (21, 34). Nothing outlives its place.
    assert!(h.w().live_places().is_empty());
    assert_eq!(h.w().live_runs(), 0);
    let made: BTreeSet<_> = h.w().made_places().into_iter().collect();
    let released: BTreeSet<_> = h.w().released_places().into_iter().collect();
    assert_eq!(made, released);
    // Never a session's place (25; rule core.16): the fake asserts it.
    for session in ["unit/1", "unit/2", "unit/3"] {
        assert!(!released.contains(session));
    }
    // sluice chose no port and started nothing but the gate's checks
    // through the check seam (31): a run request carries a place and a
    // scratch directory, and nothing else of the host.
    let r = &h.w().requests()[0];
    assert!(r.place.tmp.to_string_lossy().contains(&r.place.name));
    let _ = BRANCH;
}

/// satisfies: 32, 25, 71
#[test]
fn places_of_two_lines_on_one_host_never_collide() {
    let h = Harness::bolt();
    let other = "bolt/y";
    h.w().init(
        other,
        &[
            (
                ".config/sluice.toml",
                &sluice_conformance::policy(false, "fail", 4),
            ),
            ("a/lib", "a1"),
        ],
    );
    h.w().hold_runs(true);
    h.work("unit/x", "agent-x", &[("a/lib", Some("x"))]);
    h.submit("unit/x");
    let head = h.w().work(&h.w().tip(other), &[("a/lib", Some("y"))]);
    h.w().session("unit/y", "agent-y", &head);
    let s = h.sluice();
    s.submit(&sluice_core::SubmitRequest {
        head: sluice_core::SubmitHead::Worktree("unit/y".into()),
        branch: other.into(),
        after: vec![],
        priority: 0,
        subject: None,
    })
    .unwrap()
    .unwrap();
    h.step();
    s.step(other).unwrap();
    s.step(other).unwrap();
    h.step();
    // Both runs live in their own places; neither branch ended the other's.
    assert_eq!(h.w().live_places().len(), 2);
    assert_eq!(h.w().live_runs(), 2);
    assert!(h.w().released_places().is_empty());
}

/// The checks each run that left checks not run named, with why, in
/// order.
fn not_run(h: &Harness) -> Vec<Vec<(String, String)>> {
    h.events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Lost { not_run, .. } if !not_run.is_empty() => {
                Some(not_run.into_iter().map(|n| (n.task, n.reason)).collect())
            }
            _ => None,
        })
        .collect()
}

/// satisfies: 59, 25, 34, 114: a place made for a run whose start failed
/// is released by the step, which records the run with no result
#[test]
fn a_place_made_by_a_step_that_stopped_before_its_run_started_is_released() {
    let h = Harness::bolt();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.w().crash_next_start();
    h.step();
    let made = h.w().made_places();
    assert_eq!(made.len(), 1);
    let crashed = format!(
        "could not start: crashed before starting a run in {}",
        made[0]
    );
    assert_eq!(
        not_run(&h),
        [vec![("#gate".to_string(), crashed)]],
        "recorded with no result, its gate's entry not run"
    );
    assert!(h.w().live_places().is_empty());
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.w().made_places().len(), 2);
    assert!(h.w().live_places().is_empty());
}

/// satisfies: 71, 150, S75; rule core.49; record 0087
///
/// A round holds the room for the run it decides to start from its count
/// to its start: through the landing the same round makes of work the
/// branch already holds (122), whose outcome it pushes before it starts
/// the run, an agent's ask for a session's room finds the host's one run
/// held, and the run starts in it.
#[test]
fn a_run_decided_holds_its_room_until_it_starts() {
    let h = Harness::bolt();
    h.w().host_limit(1);
    h.w().hold_runs(true);
    let landed = h.work("unit/a", "agent-a", &[("a/one", Some("1"))]);
    let one = h.submit("unit/a");
    h.work("unit/b", "agent-b", &[("a/two", Some("2"))]);
    let two = h.submit("unit/b");
    // #1's work reaches the branch by hand before any round.
    h.w().set_branch(BRANCH, &landed);
    h.w().probe_holds(true);
    h.step();
    assert_eq!(h.sub(one).state, State::Landed);
    assert!(h.sub(one).landed_elsewhere);
    assert_eq!(
        h.w().probed(),
        [false],
        "the agent's ask during the landing's outcome found no room"
    );
    assert_eq!(
        h.w().started().len(),
        1,
        "#2's run started in the room held"
    );
    assert_eq!(h.w().holds(), [sluice_seams::RunKind::Queue]);
    assert_eq!(h.w().parts_held(), 0, "the part went to the run");
    assert_eq!(h.sub(two).state, State::Testing);
}

/// satisfies: 71; rule core.49; record 0087
///
/// A run the host holds no room for is not marked starting: the round
/// that counts the host full makes no place and writes no start, and the
/// run starts once a later round holds its room.
#[test]
fn a_run_no_room_was_held_for_is_not_marked_starting() {
    let h = Harness::bolt();
    h.w().host_limit(1);
    h.w().other_runs(1);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    let starting = |h: &Harness| {
        h.events()
            .into_iter()
            .any(|e| matches!(e, Event::Testing { .. }))
            || h.sluice().status(BRANCH).unwrap().testing > 0
    };
    assert!(!starting(&h), "no run marked starting");
    assert!(h.w().made_places().is_empty());
    assert!(h.w().started().is_empty());
    assert_eq!(h.sub(one).state, State::Queued);
    // Another repository's run took the last room between the count and
    // the hold: still none.
    h.w().other_runs(0);
    h.w().refuse_next_hold();
    h.step();
    assert!(!starting(&h));
    assert!(h.w().started().is_empty());
    h.step();
    assert_eq!(h.w().started().len(), 1);
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
}

/// The bolt policy with `release_retries = 1`.
fn one_retry() -> Harness {
    Harness::new(&policy(false, "fail", 4).replace(
        "timeout = \"20m\"\n",
        "timeout = \"20m\"\nrelease_retries = 1\n",
    ))
}

/// satisfies: 123, 122
#[test]
fn a_failed_release_is_held_with_what_failed() {
    shared::a_failed_release_is_held_with_what_failed(&Harness::bolt());
}

/// satisfies: 123
#[test]
fn a_failed_release_is_tried_a_bounded_number_of_times() {
    shared::a_failed_release_is_tried_a_bounded_number_of_times(&Harness::bolt());
}

/// satisfies: 123, 56
#[test]
fn a_failed_release_is_held_after_the_retries_its_branch_names() {
    shared::a_failed_release_is_held_after_the_retries_its_branch_names(&one_retry());
}

/// satisfies: 123
#[test]
fn a_release_that_succeeds_on_a_retry_is_listed_no_more() {
    shared::a_release_that_succeeds_on_a_retry_is_listed_no_more(&Harness::bolt());
}

/// satisfies: 123, 122
#[test]
fn a_held_checkout_never_holds_its_branch() {
    shared::a_held_checkout_never_holds_its_branch(&Harness::bolt());
}

/// satisfies: 123, 59, 114: a checkout left by a run whose start failed,
/// whose release fails, is held as any other, and its run is opened again
/// and started in another checkout
#[test]
fn a_stale_checkout_whose_release_fails_has_its_run_opened_again() {
    let h = Harness::bolt();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.w().crash_next_start();
    h.release_fails(true);
    h.step();
    h.release_fails(false);
    let stale = h.w().live_places()[0].clone();
    h.w()
        .place_release_fails(&stale, Some(shared::RELEASE_FAILS));
    let held = h.sluice().status(BRANCH).unwrap().failed_releases;
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!((held[0].place.as_str(), held[0].tries), (stale.as_str(), 1));
    assert!(h.w().requests().is_empty(), "nothing started");
    h.step();
    let started: Vec<String> = h.w().requests().into_iter().map(|r| r.place.name).collect();
    assert_eq!(started.len(), 1);
    assert_ne!(started[0], stale, "started in another checkout");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    // Tried again by the rounds after, then held.
    assert!(h.releases_of(&stale) > 1);
    assert!(h.w().live_places().contains(&stale));
}

/// The bolt policy with its records on `origin`, trusting the signer
/// `box` every host signs as.
fn peers() -> Harness {
    let p = policy(false, "fail", 4).replace(
        "trust = [\"open\", \"sandbox\", \"container\"]\n",
        "trust = [\"box\"]\nremote = \"origin\"\n",
    ) + "\n[signers.box]\nssh = \"box ssh-ed25519 AAAA\"\n";
    let h = Harness::new(&p);
    h.w().signs_as("box");
    h
}

/// Design D8: the instance that keeps the branch, `lead`, signing as
/// `box`, which the branch accepts, with no room of its own, so other
/// hosts and pools take the runs.
fn keeper<'a>(h: &'a Harness) -> Sluice<'a> {
    let s = on(h, "lead", Some("box"));
    h.w().no_room();
    s
}
/// Host `name`, signing as `signer`.
fn on<'a>(h: &'a Harness, name: &str, signer: Option<&str>) -> Sluice<'a> {
    h.w().be_host(name);
    h.sluice().as_peer(Peer {
        name: name.into(),
        lease: Duration::from_secs(300),
        alive: Duration::from_secs(120),
        signer: signer.map(str::to_string),
        systems: None,
        expire_after: sluice_core::EXPIRE_AFTER,
    })
}

fn claims(h: &Harness) -> Vec<String> {
    h.events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Claimed { host, .. } => Some(host),
            _ => None,
        })
        .collect()
}

/// satisfies: 123, 81: a host back with a checkout from a lapsed claim
/// whose release fails names it with what failed and, in the same round,
/// claims the branch's open run in another checkout and reports its
/// result; the checkout is tried again and held, and never bars a claim
#[test]
fn a_host_whose_leftover_checkout_will_not_release_claims_all_the_same() {
    let h = peers();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    keeper(&h).step(BRANCH).unwrap();
    on(&h, "b", Some("box")).step(BRANCH).unwrap();
    let left = h.w().live_places()[0].clone();
    // b stops; its claim lapses, and c takes the run.
    keeper(&h).step(BRANCH).unwrap();
    h.w().advance(chrono::Duration::minutes(3));
    keeper(&h).step(BRANCH).unwrap();
    h.w().hold_runs(false);
    on(&h, "c", Some("box")).step(BRANCH).unwrap();
    assert_eq!(claims(&h), ["b", "c"]);
    for _ in 0..3 {
        on(&h, "c", Some("box")).step(BRANCH).unwrap();
        keeper(&h).step(BRANCH).unwrap();
    }
    assert_eq!(h.sub(one).state, State::Landed);

    // b returns, and its leftover checkout will not release.
    h.w()
        .place_release_fails(&left, Some(shared::RELEASE_FAILS));
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    let two = h.submit("unit/b");
    keeper(&h).step(BRANCH).unwrap();
    on(&h, "b", Some("box")).step(BRANCH).unwrap();
    let held = h.sluice().status(BRANCH).unwrap().failed_releases;
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0].place, left);
    assert!(held[0].failed.contains(shared::RELEASE_FAILS), "{held:?}");
    assert_eq!(held[0].host.as_deref(), Some("b"));
    assert_eq!(claims(&h)[2..], ["b"], "b claimed #{two}'s run at once");
    let claimed = h.w().live_places();
    assert!(claimed.len() == 2 && claimed.contains(&left), "{claimed:?}");

    for _ in 0..6 {
        keeper(&h).step(BRANCH).unwrap();
        on(&h, "b", Some("box")).step(BRANCH).unwrap();
    }
    assert_eq!(h.sub(two).state, State::Landed);
    // #1's run lapsed under b, so a result of b's is #2's.
    assert!(
        h.results().iter().any(|r| r.host.as_deref() == Some("b")),
        "b reported #{two}'s result: {:?}",
        h.results()
    );
    assert!(h.w().live_places().contains(&left));
    let held = h.sluice().status(BRANCH).unwrap().failed_releases;
    assert!(held[0].stopped, "held once its retries are spent: {held:?}");
    assert_eq!(h.releases_of(&left), 4);
}

/// A submission landed while every release fails, and rounds until its
/// checkout is held; the checkout's name.
fn held_checkout(h: &Harness) -> String {
    h.release_fails(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    for _ in 0..6 {
        h.step();
    }
    assert_eq!(h.sub(one).state, State::Landed);
    let place = h.made_places()[0].clone();
    let held = h.sluice().status(BRANCH).unwrap().failed_releases;
    assert!(
        held.iter().any(|f| f.place == place && f.stopped),
        "{held:?}"
    );
    place
}

/// satisfies: 123: the operator releases a held checkout by its name,
/// running its release once more whatever its tries: still failing, it
/// stays held with what failed; once it succeeds, it is gone, its
/// branch's state no longer lists it, and the log holds `released`
#[test]
fn a_held_checkout_is_released_by_its_name() {
    let h = Harness::bolt();
    let place = held_checkout(&h);
    let s = h.sluice();
    // The operator's release holds the branch's lock, as `sluice release`
    // takes it.
    let _lock = s.seams().state.wait_lock(BRANCH).unwrap();
    let again = s.release_held(BRANCH, &place, false).unwrap().unwrap();
    assert!(
        matches!(&again.how, Releasing::Held { failed, tries: 5 } if failed.contains(shared::RELEASE_FAILS)),
        "{again:?}"
    );
    assert_eq!(h.releases_of(&place), 5);
    assert!(h.w().live_places().contains(&place));
    let held = s.status(BRANCH).unwrap().failed_releases;
    assert_eq!((held[0].tries, held[0].stopped), (5, true), "{held:?}");

    h.release_fails(false);
    let done = s.release_held(BRANCH, &place, false).unwrap().unwrap();
    assert_eq!(done.how, Releasing::Released);
    assert!(!h.w().live_places().contains(&place));
    assert!(s.status(BRANCH).unwrap().failed_releases.is_empty());
    assert!(matches!(
        h.events().last(),
        Some(Event::Released { place: p, accepted: None, .. }) if *p == place
    ));
    // Nothing more of that name to release.
    assert_eq!(s.release_held(BRANCH, &place, false).unwrap(), None);
}

/// satisfies: 123: the operator accepts a held checkout's failure: it is
/// freed with no release run, its branch's state no longer lists it, and
/// the log holds `released` with the failure accepted
#[test]
fn a_held_checkouts_failure_is_accepted() {
    let h = Harness::bolt();
    let place = held_checkout(&h);
    let s = h.sluice();
    // The operator's release holds the branch's lock, as `sluice release`
    // takes it.
    let _lock = s.seams().state.wait_lock(BRANCH).unwrap();
    let accepted = s.release_held(BRANCH, &place, true).unwrap().unwrap();
    assert!(
        matches!(&accepted.how, Releasing::Accepted(Some(f)) if f.contains(shared::RELEASE_FAILS)),
        "{accepted:?}"
    );
    assert_eq!(h.releases_of(&place), 4, "its release ran again");
    assert_eq!(h.w().freed_places(), vec![place.clone()]);
    assert!(!h.w().live_places().contains(&place));
    assert!(s.status(BRANCH).unwrap().failed_releases.is_empty());
    assert!(matches!(
        h.events().last(),
        Some(Event::Released { place: p, accepted: Some(f), .. })
            if *p == place && f.contains(shared::RELEASE_FAILS)
    ));
    assert_eq!(
        s.release_held(BRANCH, "sluice/try/bolt/x/99", true)
            .unwrap(),
        None
    );
}

/// satisfies: 123, 26, 61: the overview lists a held checkout under its
/// branch, held, with what failed, from the branch's state; reading it
/// ten times runs no release, counts no try and writes nothing
#[test]
fn the_overview_shows_a_held_checkout_and_changes_nothing() {
    let h = Harness::bolt();
    let place = held_checkout(&h);
    let (version, files, releases) = (h.version(), h.files(), h.releases_of(&place));
    let s = h.sluice();
    let view = s.branch_view_of(BRANCH).unwrap();
    let held = view
        .held_checkouts
        .iter()
        .find(|f| f.place == place)
        .unwrap_or_else(|| panic!("{:?}", view.held_checkouts));
    assert!(held.stopped, "{held:?}");
    assert!(held.failed.contains(shared::RELEASE_FAILS), "{held:?}");
    for _ in 0..10 {
        let all = s.overview(None).unwrap();
        assert!(all.iter().any(|b| b.held_checkouts == view.held_checkouts));
    }
    assert_eq!(h.version(), version, "the branch's state was written");
    assert_eq!(h.files(), files, "its state or log changed");
    assert_eq!(h.releases_of(&place), releases, "a release was run");
}

/// How each `gone` line of the log names `place`: whether its release
/// ran, and what it printed when it failed.
fn gone_lines(h: &Harness, place: &str) -> Vec<(bool, Option<String>)> {
    h.events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Gone {
                place: p,
                worktree: None,
                ran,
                failed,
            } if p == place => Some((ran, failed)),
            _ => None,
        })
        .collect()
}

/// satisfies: 123: a combination's checkout deleted before its result is
/// read has the release its run stored run once when the round releases
/// it, and is freed and logged `gone`; its submission lands, and no round
/// runs its release again
#[test]
fn a_deleted_checkouts_stored_release_runs_once_and_it_is_freed() {
    let h = Harness::bolt();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    let place = h.w().live_places()[0].clone();
    h.w().delete_checkout(&place);
    h.w().complete(&h.w().requests()[0].revision);
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert!(!h.w().live_places().contains(&place));
    assert_eq!(h.gone_releases_of(&place), 1);
    assert_eq!(h.releases_of(&place), 0, "a release was asked of it");
    assert_eq!(gone_lines(&h, &place), [(true, None)]);
    for _ in 0..3 {
        h.step();
    }
    assert_eq!(h.gone_releases_of(&place), 1);
    assert!(h
        .sluice()
        .status(BRANCH)
        .unwrap()
        .failed_releases
        .is_empty());
}

/// satisfies: 123: a held checkout whose working copy is deleted since is
/// released once by the next round, from what sluice holds, and freed
/// whatever its release does; its branch's state no longer lists it, and
/// no round runs its release again
#[test]
fn a_held_checkout_deleted_since_is_released_once_and_freed() {
    let h = Harness::bolt();
    let place = held_checkout(&h);
    h.w().delete_checkout(&place);
    h.step();
    assert!(!h.w().live_places().contains(&place));
    assert_eq!(h.gone_releases_of(&place), 1);
    let gone = gone_lines(&h, &place);
    assert!(
        matches!(&gone[..], [(true, Some(f))] if f.contains(shared::RELEASE_FAILS)),
        "{gone:?}"
    );
    assert!(h
        .sluice()
        .status(BRANCH)
        .unwrap()
        .failed_releases
        .is_empty());
    for _ in 0..3 {
        h.step();
    }
    assert_eq!(h.gone_releases_of(&place), 1);
    assert_eq!(h.releases_of(&place), 4);
}
