//! A branch whose records are on its remote, kept by one instance at a
//! time and taken by another after the lease (72 to 74; S17).

use std::time::Duration;

use sluice_conformance::{policy, Crash, Harness, Rig, BRANCH};
use sluice_core::records::{Event, State};
use sluice_core::{Peer, Sluice, Step};
use sluice_seams::RunKind;

/// The bolt policy with its records on `origin`, trusting the signer
/// `box` this host signs as.
fn peers() -> Harness {
    peers_with("")
}

/// [`peers`], its policy ending in `more`.
fn peers_with(more: &str) -> Harness {
    let p = policy(false, "fail", 4).replace(
        "trust = [\"open\", \"sandbox\", \"container\"]\n",
        "trust = [\"box\"]\nremote = \"origin\"\n",
    ) + "\n[signers.box]\nssh = \"box ssh-ed25519 AAAA\"\n"
        + more;
    let h = Harness::new(&p);
    h.w().signs_as("box");
    h
}

/// Instance `name` of the rig, on its own host, with a lease of five
/// minutes.
fn on<'a>(h: &'a Harness, name: &str) -> Sluice<'a> {
    h.w().be_host(name);
    h.sluice().as_peer(Peer {
        name: name.into(),
        lease: Duration::from_secs(300),
        alive: Duration::from_secs(120),
        signer: Some("box".into()),
        systems: None,
        expire_after: sluice_core::EXPIRE_AFTER,
    })
}

fn took(h: &Harness) -> Vec<(String, Option<String>)> {
    h.events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Took { by, from } => Some((by, from)),
            _ => None,
        })
        .collect()
}

/// satisfies: 72; the peer-lead spec's "A host keeps a branch only by
/// writing its state"
#[test]
fn the_first_instance_takes_the_branch_and_another_is_led() {
    let h = peers();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    assert_eq!(on(&h, "a").step(BRANCH).unwrap(), Step::Stepped);
    assert_eq!(took(&h), [("a".to_string(), None)]);
    assert_eq!(on(&h, "b").step(BRANCH).unwrap(), Step::Led);
    for _ in 0..4 {
        on(&h, "a").step(BRANCH).unwrap();
    }
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: 72, 73, 58; the peer-lead spec's "A lead that went quiet"
/// and "The former lead stops after a takeover"
#[test]
fn a_quiet_lead_is_taken_after_the_lease_by_the_takers_own_observation() {
    let h = peers();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    on(&h, "a").step(BRANCH).unwrap();
    // a goes quiet with work to do; b sees the same version stand.
    assert_eq!(on(&h, "b").step(BRANCH).unwrap(), Step::Led);
    h.w().advance(chrono::Duration::minutes(4));
    assert_eq!(on(&h, "b").step(BRANCH).unwrap(), Step::Led);
    h.w().advance(chrono::Duration::minutes(2));
    assert_eq!(on(&h, "b").step(BRANCH).unwrap(), Step::Stepped);
    assert_eq!(
        took(&h).last().cloned(),
        Some(("b".to_string(), Some("a".to_string())))
    );
    // The former lead writes nothing more for the branch.
    assert_eq!(on(&h, "a").step(BRANCH).unwrap(), Step::Led);
    for _ in 0..4 {
        on(&h, "b").step(BRANCH).unwrap();
    }
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.w().moves().len(), 1);
}

/// satisfies: 73; a version written meanwhile starts the lease again
#[test]
fn a_lead_that_writes_keeps_the_branch() {
    let h = peers();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    on(&h, "a").step(BRANCH).unwrap();
    assert_eq!(on(&h, "b").step(BRANCH).unwrap(), Step::Led);
    h.w().advance(chrono::Duration::minutes(4));
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    h.submit("unit/b");
    on(&h, "a").step(BRANCH).unwrap();
    h.w().advance(chrono::Duration::minutes(2));
    assert_eq!(on(&h, "b").step(BRANCH).unwrap(), Step::Led);
}

/// A branch with nothing to do is not taken, however long its lead is
/// quiet.
#[test]
fn a_quiet_branch_with_nothing_to_do_is_not_taken() {
    let h = peers();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    for _ in 0..4 {
        on(&h, "a").step(BRANCH).unwrap();
    }
    assert_eq!(h.sub(one).state, State::Landed);
    on(&h, "b").step(BRANCH).unwrap();
    h.w().advance(chrono::Duration::minutes(30));
    assert_eq!(on(&h, "b").step(BRANCH).unwrap(), Step::Led);
    assert_eq!(took(&h).len(), 1);
}

/// satisfies: 72, 73, 24, 59; S17
#[test]
fn s17_a_landing_left_by_a_quiet_lead_lands_once_by_the_next() {
    let h = peers();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    on(&h, "a").step(BRANCH).unwrap();
    h.w().crash(Crash::BeforeMove);
    assert!(on(&h, "a").step(BRANCH).is_err());
    assert_eq!(h.sub(one).state, State::Landing);
    on(&h, "b").step(BRANCH).unwrap();
    h.w().advance(chrono::Duration::minutes(6));
    on(&h, "b").step(BRANCH).unwrap();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(on(&h, "a").step(BRANCH).unwrap(), Step::Led);
    assert_eq!(h.w().moves().len(), 1);
    assert_eq!(h.w().delivered_to("agent-a").len(), 1);
}

/// Instance `name`, signing as `signer` or as no one.
fn as_signer<'a>(h: &'a Harness, name: &str, signer: Option<&str>) -> Sluice<'a> {
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

/// The runs of the branch's first candidate: `(host, state)`.
fn runs(h: &Harness) -> Vec<(Option<String>, sluice_core::records::RunState)> {
    let s = h.sluice();
    let line =
        sluice_core::line::Line::load(BRANCH, s.seams().state.read(BRANCH).unwrap()).unwrap();
    line.doc
        .candidates
        .iter()
        .flat_map(|c| c.runs.iter().map(|r| (r.host.clone(), r.state)))
        .collect()
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

/// Design D8: the instance that keeps the branch, `lead`, signing as
/// `box`, which the branch accepts, with no room of its own, so other
/// hosts take the runs.
fn keeper<'a>(h: &'a Harness) -> Sluice<'a> {
    let s = as_signer(h, "lead", Some("box"));
    h.w().no_room();
    s
}

/// A lead with no room of its own, so other hosts take the runs; a
/// submission it has opened a run for.
fn opened(h: &Harness) -> u64 {
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    keeper(h).step(BRANCH).unwrap();
    assert_eq!(runs(h), [(None, sluice_core::records::RunState::Open)]);
    one
}

/// satisfies: 75; the run-claims spec's "Two hosts reach for one run"
#[test]
fn a_run_is_taken_by_one_claim() {
    let h = peers();
    opened(&h);
    as_signer(&h, "b", Some("box")).step(BRANCH).unwrap();
    as_signer(&h, "c", Some("box")).step(BRANCH).unwrap();
    assert_eq!(claims(&h), ["b"]);
    assert_eq!(h.w().requests().len(), 1);
}

/// satisfies: 71, 75; the run-claims spec's "A full host claims nothing"
#[test]
fn a_full_host_claims_nothing() {
    let h = peers();
    opened(&h);
    h.w().host_limit(0);
    as_signer(&h, "b", Some("box")).step(BRANCH).unwrap();
    assert!(claims(&h).is_empty());
}

/// satisfies: 71, 75: a claim whose start the host refuses at its bound
/// runs nothing and is given up, open to another host
#[test]
fn a_claim_refused_at_the_bound_is_open_to_another_host() {
    use sluice_core::records::RunState;
    let h = peers();
    opened(&h);
    h.w().refuse_next_start();
    as_signer(&h, "b", Some("box")).step(BRANCH).unwrap();
    assert_eq!(claims(&h), ["b"]);
    assert!(h.w().requests().is_empty(), "nothing of it ran");
    assert_eq!(runs(&h), [(None, RunState::Open)]);
    as_signer(&h, "c", Some("box")).step(BRANCH).unwrap();
    assert_eq!(claims(&h), ["b", "c"]);
    assert_eq!(runs(&h), [(Some("c".into()), RunState::Testing)]);
    assert_eq!(h.w().requests().len(), 1);
}

/// satisfies: 79, 107: a host whose signer the gate does not trust takes
/// no run of it
#[test]
fn a_host_the_gate_does_not_trust_claims_nothing() {
    let h = peers();
    opened(&h);
    as_signer(&h, "b", Some("sessions")).step(BRANCH).unwrap();
    as_signer(&h, "c", None).step(BRANCH).unwrap();
    assert!(claims(&h).is_empty());
}

/// satisfies: 76; the run-claims spec's "The first combination first"
#[test]
fn runs_are_claimed_earliest_first() {
    let h = peers();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    h.submit("unit/b");
    keeper(&h).step(BRANCH).unwrap();
    h.w().host_limit(1);
    as_signer(&h, "b", Some("box")).step(BRANCH).unwrap();
    let started = h.w().requests();
    assert_eq!(started.len(), 1);
    let s = h.sluice();
    let line =
        sluice_core::line::Line::load(BRANCH, s.seams().state.read(BRANCH).unwrap()).unwrap();
    let first = line
        .doc
        .candidates
        .iter()
        .find(|c| c.submissions.len() == 1)
        .unwrap();
    assert_eq!(started[0].revision, first.revision);
}

/// satisfies: 77, 60; the run-claims spec's "A host stops mid-run" and
/// "A late report"; S20
#[test]
fn s20_a_lapsed_claim_runs_again_elsewhere_and_its_late_report_is_not_written() {
    let h = peers();
    let one = opened(&h);
    as_signer(&h, "b", Some("box")).step(BRANCH).unwrap();
    assert_eq!(runs(&h)[0].0.as_deref(), Some("b"));
    // b stops; the lead watches its sign of life stand still.
    keeper(&h).step(BRANCH).unwrap();
    h.w().advance(chrono::Duration::minutes(3));
    keeper(&h).step(BRANCH).unwrap();
    assert!(h
        .events()
        .iter()
        .any(|e| matches!(e, Event::Lapsed { host, .. } if host == "b")));
    assert_eq!(runs(&h)[0], (None, sluice_core::records::RunState::Open));
    // c claims it again and passes it.
    h.w().hold_runs(false);
    as_signer(&h, "c", Some("box")).step(BRANCH).unwrap();
    as_signer(&h, "c", Some("box")).step(BRANCH).unwrap();
    // b returns: its run finished meanwhile; nothing of it is written, and
    // its place goes before it claims again (81).
    h.w().complete_all();
    let places_before = h.w().live_places().len();
    as_signer(&h, "b", Some("box")).step(BRANCH).unwrap();
    assert!(h.w().live_places().len() < places_before);
    for _ in 0..4 {
        keeper(&h).step(BRANCH).unwrap();
        as_signer(&h, "c", Some("box")).step(BRANCH).unwrap();
    }
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: 81, 123, 72, 73; the run-claims spec's "Back after a
/// lapse", the host back as the lead: its claim lapsed, it asks whether it
/// keeps the branch as intake does (137), with no round, and takes the
/// branch once the quiet lead's lease is out; that round releases the
/// place its lapsed claim left before it claims the run again.
#[test]
fn a_place_left_by_a_lapsed_claim_is_released_by_its_host_as_the_lead() {
    use sluice_core::records::RunState;
    let h = peers();
    opened(&h);
    as_signer(&h, "b", Some("box")).step(BRANCH).unwrap();
    let left = h.w().live_places();
    assert_eq!(left.len(), 1, "{left:?}");
    keeper(&h).step(BRANCH).unwrap();
    h.w().advance(chrono::Duration::minutes(3));
    keeper(&h).step(BRANCH).unwrap();
    assert_eq!(runs(&h), [(None, RunState::Open)]);
    assert!(h.w().live_places().contains(&left[0]));

    assert!(!as_signer(&h, "b", Some("box")).keeps(BRANCH).unwrap());
    h.w().advance(chrono::Duration::minutes(6));
    assert_eq!(
        as_signer(&h, "b", Some("box")).step(BRANCH).unwrap(),
        Step::Stepped
    );
    assert_eq!(
        took(&h).last().cloned(),
        Some(("b".to_string(), Some("lead".to_string())))
    );
    assert!(
        !h.w().live_places().contains(&left[0]),
        "{} is left",
        left[0]
    );
    assert!(h
        .events()
        .iter()
        .any(|e| matches!(e, Event::Released { place, .. } if *place == left[0])));
    assert_eq!(runs(&h), [(Some("b".into()), RunState::Testing)]);
}

/// satisfies: 74, 77; the run-claims spec's "A long run keeps its claim"
#[test]
fn a_long_run_keeps_its_claim_while_its_host_shows_it_is_alive() {
    let h = peers();
    opened(&h);
    as_signer(&h, "b", Some("box")).step(BRANCH).unwrap();
    for _ in 0..10 {
        h.w().advance(chrono::Duration::minutes(1));
        keeper(&h).step(BRANCH).unwrap();
        as_signer(&h, "b", Some("box")).step(BRANCH).unwrap();
    }
    assert!(!h.events().iter().any(|e| matches!(e, Event::Lapsed { .. })));
    assert_eq!(runs(&h)[0].0.as_deref(), Some("b"));
}

/// The peers policy with two systems, `main`'s required checks needing
/// both.
fn two_systems() -> Harness {
    let p = policy(false, "fail", 4).replace(
        "trust = [\"open\", \"sandbox\", \"container\"]\n",
        "trust = [\"box\"]\nremote = \"origin\"\nsystems = [\"linux-x64\", \"macos-arm64\"]\n",
    ) + "\n[signers.box]\nssh = \"box ssh-ed25519 AAAA\"\n\n[systems]\n\
         linux-x64 = { os = \"linux\", arch = \"x86_64\" }\n\
         macos-arm64 = { os = \"macos\", arch = \"aarch64\" }\n";
    let h = Harness::new(&p);
    h.w().signs_as("box");
    h
}

fn with_systems<'a>(h: &'a Harness, name: &str, systems: &[&str]) -> Sluice<'a> {
    h.w().be_host(name);
    h.sluice().as_peer(Peer {
        name: name.into(),
        lease: Duration::from_secs(300),
        alive: Duration::from_secs(120),
        signer: Some("box".into()),
        systems: Some(systems.iter().map(|s| s.to_string()).collect()),
        expire_after: sluice_core::EXPIRE_AFTER,
    })
}

/// satisfies: 87; the standing-executor spec's "A box for one system"
#[test]
fn a_host_claims_only_runs_of_its_systems() {
    let h = two_systems();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    keeper(&h).step(BRANCH).unwrap();
    with_systems(&h, "box", &["linux-x64"])
        .step(BRANCH)
        .unwrap();
    let held: Vec<(Option<String>, Option<String>)> = {
        let s = h.sluice();
        let line =
            sluice_core::line::Line::load(BRANCH, s.seams().state.read(BRANCH).unwrap()).unwrap();
        line.doc.candidates[0]
            .runs
            .iter()
            .map(|r| (r.system.clone(), r.host.clone()))
            .collect()
    };
    assert_eq!(
        held,
        [
            (Some("linux-x64".into()), Some("box".into())),
            (Some("macos-arm64".into()), None)
        ]
    );
}

/// satisfies: 87; the standing-executor spec's "A system the host is not"
#[test]
fn a_host_naming_a_system_it_is_not_is_refused() {
    let h = two_systems();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    let err = with_systems(&h, "box", &["macos-arm64"])
        .check_host_systems()
        .unwrap_err();
    assert!(err.to_string().contains("macos-arm64"), "{err}");
    with_systems(&h, "box", &["linux-x64"])
        .check_host_systems()
        .unwrap();
}

/// satisfies: 77, 107; the standing-executor spec's "A host gone quiet"
#[test]
fn the_overview_shows_hosts_their_room_and_a_lapse() {
    let h = peers();
    opened(&h);
    h.w().host_limit(3);
    as_signer(&h, "b", Some("box")).step(BRANCH).unwrap();
    let lead = keeper(&h);
    let hosts = lead.hosts().unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].host, "b");
    assert_eq!(hosts[0].signer.as_deref(), Some("box"));
    assert_eq!(
        (hosts[0].held, hosts[0].room, hosts[0].lapsed),
        (0, 3, false)
    );
    // The lead watches b's beat stand still past the bound.
    lead.step(BRANCH).unwrap();
    h.w().advance(chrono::Duration::minutes(3));
    let hosts = keeper(&h).hosts().unwrap();
    assert!(hosts[0].lapsed);
    assert_eq!(hosts[0].room, 0);
}

/// satisfies: C.4, C.7; the overview spec's "A branch on origin"
#[test]
fn the_overview_names_the_remote_a_branch_lands_on() {
    let h = peers();
    opened(&h);
    let view = keeper(&h).branch_view_of(BRANCH).unwrap();
    assert_eq!(view.remote.as_deref(), Some("origin"));
}

/// satisfies: 71, 75; the run-claims spec's "A host full with another
/// repository's runs claims nothing"
#[test]
fn a_host_full_with_another_repositorys_runs_claims_nothing() {
    let h = peers();
    opened(&h);
    h.w().host_limit(2);
    h.w().be_host("b");
    h.w().other_runs(2);
    as_signer(&h, "b", Some("box")).step(BRANCH).unwrap();
    assert!(claims(&h).is_empty());
}

/// satisfies: 71, 107; the run-claims spec's "Another repository's runs
/// leave less room"
#[test]
fn another_repositorys_runs_leave_less_room_in_a_hosts_sign_of_life() {
    let h = peers();
    opened(&h);
    h.w().host_limit(4);
    as_signer(&h, "b", Some("box")).step(BRANCH).unwrap();
    assert_eq!(h.w().live_runs(), 1, "b runs main's combination");
    h.w().be_host("b");
    h.w().other_runs(1);
    as_signer(&h, "b", Some("box")).step(BRANCH).unwrap();
    let hosts = keeper(&h).hosts().unwrap();
    assert_eq!(hosts[0].host, "b");
    assert_eq!((hosts[0].held, hosts[0].room), (2, 2));
}

/// Design D8, rule core.22: a passing verdict written into the state with
/// no signed results behind it lands nothing; the combination is tested
/// again.
#[test]
fn a_verdict_the_state_alone_holds_lands_nothing() {
    let h = peers();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    let lead = on(&h, "a");
    lead.step(BRANCH).unwrap();
    // A host that can write the state forges a pass.
    let state = lead.seams().state;
    let snap = state.read(BRANCH).unwrap();
    let mut line = sluice_core::line::Line::load(BRANCH, snap).unwrap();
    for c in &mut line.doc.candidates {
        c.state = sluice_core::records::CandidateState::Passed;
        for r in &mut c.runs {
            r.state = sluice_core::records::RunState::Passed;
            r.place = None;
            r.run = None;
        }
    }
    let at = line.version;
    {
        let _g = state.lock(BRANCH).unwrap().unwrap();
        assert!(matches!(
            state.write(BRANCH, at, line.changes()).unwrap(),
            sluice_seams::Written::Written(_)
        ));
    }
    on(&h, "a").step(BRANCH).unwrap();
    assert!(h
        .events()
        .iter()
        .any(|e| matches!(e, Event::Unsigned { .. })));
    assert_ne!(h.sub(one).state, State::Landed);
    assert!(h.w().moves().is_empty());
}

/// satisfies: 87, 78; rule core.22: a host that signs as a signer the
/// gate trusts, with a key the branch's policy does not list, takes no
/// run; a host whose key it lists takes it, and the submission lands.
#[test]
fn a_host_whose_key_the_branch_does_not_list_takes_no_run() {
    let h = peers();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    keeper(&h).step(BRANCH).unwrap();
    h.w().be_host("a");
    h.w().key_unlisted(true);
    on(&h, "a").step(BRANCH).unwrap();
    on(&h, "a").step(BRANCH).unwrap();
    assert_eq!(runs(&h), [(None, sluice_core::records::RunState::Open)]);
    assert!(claims(&h).is_empty());
    assert!(h.w().requests().is_empty());
    for _ in 0..4 {
        on(&h, "b").step(BRANCH).unwrap();
        on(&h, "a").step(BRANCH).unwrap();
        keeper(&h).step(BRANCH).unwrap();
    }
    assert_eq!(claims(&h), ["b"]);
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: 87, 66: a run no host whose key the branch lists takes,
/// its lead turning it down for what its host lacks (record 0074), stays
/// open, and the candidate's time limit reads it not passed.
#[test]
fn a_run_no_listed_host_takes_is_not_passed_at_the_time_limit() {
    let h = peers_with("\n[executor]\ndocker = true\n");
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.w().be_host("lead");
    h.w().engine_unmet(Some("no container engine answers"));
    keeper(&h).step(BRANCH).unwrap();
    h.w().be_host("a");
    h.w().key_unlisted(true);
    on(&h, "a").step(BRANCH).unwrap();
    h.w().advance(chrono::Duration::minutes(19));
    on(&h, "a").step(BRANCH).unwrap();
    keeper(&h).step(BRANCH).unwrap();
    assert_eq!(runs(&h), [(None, sluice_core::records::RunState::Open)]);
    h.w().advance(chrono::Duration::minutes(2));
    on(&h, "a").step(BRANCH).unwrap();
    keeper(&h).step(BRANCH).unwrap();
    assert_eq!(h.sub(one).state, State::Failed);
    assert!(h.w().requests().is_empty());
}

/// satisfies: 87, 28; S18: a run whose result its host signed with a key
/// the branch's policy no longer lists, read when it reports, is missing,
/// never failed: it is open again, and a host whose key the branch lists
/// settles it.
#[test]
fn s18_a_result_signed_with_a_key_the_branch_does_not_list_is_missing() {
    let h = peers();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    on(&h, "a").step(BRANCH).unwrap();
    assert_eq!(h.w().live_runs(), 1, "a runs the combination");
    // The branch's policy moves on, and lists a's key no more.
    h.w().key_unlisted(true);
    h.w().hold_runs(false);
    h.w().complete_all();
    on(&h, "a").step(BRANCH).unwrap();
    assert!(h
        .events()
        .iter()
        .any(|e| matches!(e, Event::Uncounted { host, .. } if host == "a")));
    assert_eq!(runs(&h), [(None, sluice_core::records::RunState::Open)]);
    assert_ne!(h.sub(one).state, State::Failed);
    assert!(h.sub(one).failed.is_empty());
    // Design D8: a, which the branch no longer accepts, leads no more; b,
    // with no room as it first sees the state, takes the branch once the
    // state has stood for the lease.
    h.w().refuse_next_hold();
    on(&h, "b").step(BRANCH).unwrap();
    h.w().advance(chrono::Duration::minutes(6));
    for _ in 0..4 {
        on(&h, "b").step(BRANCH).unwrap();
        on(&h, "a").step(BRANCH).unwrap();
    }
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: 108, 78; S42: a key whose only pass was signed by a signer
/// the gate does not trust replays for nothing, and the check runs.
#[test]
fn s42_a_replay_no_trusted_signer_proved_runs_again() {
    let p = policy(false, "fail", 4).replace(
        "trust = [\"open\", \"sandbox\", \"container\"]\n",
        "trust = [\"box\"]\nremote = \"origin\"\n",
    ) + "\n[branches.\"bolt/y\"]\nrequired = [\"#gate\"]\nremote = \"origin\"\ntrust = [\"sessions\"]\n\
         \n[signers.box]\nssh = \"box ssh-ed25519 AAAA\"\n\
         \n[signers.sessions]\nssh = \"s ssh-ed25519 BBBB\"\n";
    let h = Harness::new(&p);
    let other = "bolt/y";
    h.w().init(
        other,
        &[
            (".config/sluice.toml", &p),
            ("a/lib", "a1"),
            ("b/lib", "b1"),
            ("c/lib", "c1"),
        ],
    );
    // The work passes on bolt/y, signed by the sessions' signer.
    h.w().signs_as("sessions");
    let head = h.w().work(&h.w().tip(other), &[("a/lib", Some("a2"))]);
    h.w().session("unit/y", "agent-y", &head);
    let sessions = Sluice::new(h.sluice().seams(), &h.f.clock).as_peer(Peer {
        name: "s".into(),
        lease: Duration::from_secs(300),
        alive: Duration::from_secs(120),
        signer: Some("sessions".into()),
        systems: None,
        expire_after: sluice_core::EXPIRE_AFTER,
    });
    let y = sessions
        .submit(&sluice_core::SubmitRequest {
            head: sluice_core::SubmitHead::Worktree("unit/y".into()),
            branch: other.into(),
            after: vec![],
            priority: 0,
            subject: None,
        })
        .unwrap()
        .unwrap()
        .id;
    for _ in 0..6 {
        sessions.step(other).unwrap();
    }
    assert_eq!(sessions.state(y).unwrap().unwrap().state, State::Landed);
    let ran = h.w().observed().len();

    // The same work on bolt/x, whose gate trusts only box: the runner
    // replays every key, no box pass proves them, and they run again.
    h.w().signs_as("box");
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    for _ in 0..8 {
        on(&h, "a").step(BRANCH).unwrap();
    }
    assert_eq!(h.sub(one).state, State::Landed);
    assert!(h
        .events()
        .iter()
        .any(|e| matches!(e, Event::Unproven { .. })));
    assert!(h.w().requests().iter().any(|r| !r.replay));
    assert!(h.w().observed().len() > ran);
}

/// Two systems, a box that signs for `linux-x64` and a forge pool with a
/// runner for each, both trusted.
fn with_pool() -> Harness {
    let p = policy(false, "fail", 4).replace(
        "trust = [\"open\", \"sandbox\", \"container\"]\n",
        "trust = [\"box\", \"forge\"]\nremote = \"origin\"\nsystems = [\"linux-x64\", \"macos-arm64\"]\n",
    ) + "\n[signers.box]\nssh = \"box ssh-ed25519 AAAA\"\nsystems = [\"linux-x64\"]\n\
         \n[signers.forge]\ngithub = { workflow = \".github/workflows/sluice.yml\", ref = \"refs/heads/main\" }\n\
         \n[pools.forge]\ngithub = { workflow = \".github/workflows/sluice.yml\" }\nsigner = \"forge\"\n\
         runners = { linux-x64 = \"ubuntu-24.04\", macos-arm64 = \"macos-15\" }\n\
         \n[systems]\nlinux-x64 = { os = \"linux\", arch = \"x86_64\" }\n\
         macos-arm64 = { os = \"macos\", arch = \"aarch64\" }\n";
    let h = Harness::new(&p);
    h.w().signs_as("box");
    h
}

fn signer<'a>(h: &'a Harness, name: &str, signer: &str) -> Sluice<'a> {
    h.w().be_host(name);
    h.w().signs_as(signer);
    h.sluice().as_peer(Peer {
        name: name.into(),
        lease: Duration::from_secs(300),
        alive: Duration::from_secs(120),
        signer: Some(signer.into()),
        systems: None,
        expire_after: sluice_core::EXPIRE_AFTER,
    })
}

/// satisfies: 107, 88; S41; the run-claims spec's "The box has room" and
/// "The box is full"
#[test]
fn s41_a_box_with_room_comes_before_the_pool() {
    let h = with_pool();
    h.w().hold_runs(true);
    keeper(&h).step(BRANCH).unwrap();
    signer(&h, "box", "box").step(BRANCH).unwrap();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    assert_eq!(h.w().wakes(), [BRANCH]);
    keeper(&h).step(BRANCH).unwrap();
    let launched: Vec<(Option<String>, String)> = h
        .w()
        .launches()
        .iter()
        .map(|l| (l.system.clone(), l.runner.clone()))
        .collect();
    // The box takes linux-x64; only macos-arm64, which no box runs, goes
    // to the pool.
    assert_eq!(launched, [(Some("macos-arm64".into()), "macos-15".into())]);
    signer(&h, "box", "box").step(BRANCH).unwrap();
    assert!(claims(&h).contains(&"box".to_string()));

    // A full box: the pool takes linux-x64 too.
    let h = with_pool();
    h.w().hold_runs(true);
    keeper(&h).step(BRANCH).unwrap();
    h.w().host_limit(0);
    signer(&h, "box", "box").step(BRANCH).unwrap();
    h.w().host_limit(4);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    keeper(&h).step(BRANCH).unwrap();
    assert_eq!(h.w().launches().len(), 2);
}

/// satisfies: 88, 111, 77; S19: every host of the submitter is off; the
/// pool's jobs run each system and the work lands; a job its pool lost
/// lapses and the run is started again.
#[test]
fn s19_a_pools_jobs_land_the_work_with_no_box() {
    let h = with_pool();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    let lead = || keeper(&h);
    lead().step(BRANCH).unwrap();
    let first: Vec<String> = h.w().launches().iter().map(|l| l.claim.clone()).collect();
    assert_eq!(first.len(), 2);
    // The macOS job is lost before it reports.
    h.w().end_job(&first[1]);
    lead().step(BRANCH).unwrap();
    assert!(h
        .events()
        .iter()
        .any(|e| matches!(e, Event::Lapsed { host, .. } if *host == first[1])));
    assert_eq!(h.w().launches().len(), 3);
    // Each job the pool is given runs under its claim and reports; a
    // macOS job that replays keys only Linux proved runs them again (108).
    h.w().hold_runs(false);
    for _ in 0..6 {
        let jobs: Vec<String> = h
            .w()
            .launches()
            .iter()
            .map(|l| l.claim.clone())
            .filter(|c| *c != first[1])
            .collect();
        for c in &jobs {
            let job = signer(&h, c, "forge");
            for _ in 0..2 {
                job.step(BRANCH).unwrap();
            }
        }
        lead().step(BRANCH).unwrap();
        if h.sub(one).state == State::Landed {
            break;
        }
    }
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: 71, 88; records 0119, 0123; the run-bound spec's "A
/// repository on a pool": with the host's one run another repository's,
/// this repository's combination is launched on its pool with no part of
/// the host's bound held, and counts in no repository's runs: the other
/// repository's next run waits only for its own to end.
#[test]
fn a_pools_launch_holds_no_part() {
    let h = with_pool();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    let lead = as_signer(&h, "lead", Some("box"));
    h.w().host_limit(1);
    h.w().other_runs(1);
    lead.step(BRANCH).unwrap();
    assert_eq!(
        h.w().launches().len(),
        2,
        "each system launched on the pool"
    );
    assert!(h.w().holds().is_empty(), "no part was held for a launch");
    assert_eq!(h.w().parts_held(), 0);
    let tests = &h.sluice().seams().tests;
    assert_eq!(
        tests.running().unwrap().queue,
        1,
        "only the other repository's run counts"
    );
    assert!(tests.hold(RunKind::Queue).unwrap().is_none());
    // The other repository's run ends: its next takes the host's run.
    h.w().other_runs(0);
    assert!(tests.hold(RunKind::Queue).unwrap().is_some());
}

/// Each claim in the branch's log, with the forge's words when its
/// dispatch went unanswered.
fn claimed(h: &Harness) -> Vec<(String, Option<String>)> {
    h.events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Claimed {
                host, unanswered, ..
            } => Some((host, unanswered)),
            _ => None,
        })
        .collect()
}

/// satisfies: 134, 77, 88; the remote-retry spec's "A run's dispatch
/// answered 502" and "The dispatch was not taken": a run whose dispatch
/// went unanswered is claimed after one launch, never launched again under
/// that claim, its claim's event holding the forge's words; when no job of
/// the claim runs, the next round lapses it and launches the run once
/// more, under a new claim.
#[test]
fn a_run_whose_dispatch_went_unanswered_is_claimed_once() {
    let h = with_pool();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    let lead = || keeper(&h);
    h.w().fail_next_launch(true);
    lead().step(BRANCH).unwrap();
    let first: Vec<String> = h.w().launches().iter().map(|l| l.claim.clone()).collect();
    assert_eq!(first.len(), 2, "each run launched once");
    let claims = claimed(&h);
    assert_eq!(claims.len(), 2, "{claims:?}");
    let (host, words) = &claims[0];
    assert_eq!(host, &first[0]);
    let words = words.as_deref().expect("the unanswered words");
    assert!(words.contains("its answer was not read: 502"), "{words}");
    assert_eq!(claims[1], (first[1].clone(), None));
    assert!(runs(&h)
        .iter()
        .any(|(host, state)| host.as_deref() == Some(first[0].as_str())
            && *state == sluice_core::records::RunState::Starting));

    // A round while the job may run launches nothing more.
    lead().step(BRANCH).unwrap();
    assert_eq!(h.w().launches().len(), 2);

    // GitHub never took it: no job of the claim runs.
    h.w().end_job(&first[0]);
    lead().step(BRANCH).unwrap();
    assert!(h
        .events()
        .iter()
        .any(|e| matches!(e, Event::Lapsed { host, .. } if *host == first[0])));
    let launches = h.w().launches();
    assert_eq!(launches.len(), 3);
    let again = &launches[2].claim;
    assert!(!first.contains(again), "{again} is a new claim");
    assert_eq!(launches[2].system, launches[0].system);
    assert_eq!(claimed(&h).last(), Some(&(again.clone(), None)));
}

/// satisfies: 134; a launch the forge refused fails the round, and
/// nothing is claimed.
#[test]
fn a_refused_launch_fails_the_round_with_nothing_claimed() {
    let h = with_pool();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    h.w().fail_next_launch(false);
    let e = keeper(&h).step(BRANCH).unwrap_err();
    assert!(format!("{e:#}").contains("404 Not Found"), "{e:#}");
    assert!(claimed(&h).is_empty(), "{:?}", claimed(&h));
    assert!(h.w().launches().is_empty());
}

/// satisfies: 78, 79, 28, 29; S18: results signed by a signer the shared
/// branch does not name settle nothing there; a confinement claim counts
/// only from a signer that may make it.
#[test]
fn s18_a_signer_the_gate_does_not_trust_settles_nothing() {
    let p = policy(false, "fail", 4).replace(
        "trust = [\"open\", \"sandbox\", \"container\"]\n",
        "trust = [\"box\"]\nremote = \"origin\"\nconfined = true\n",
    ) + "\n[branches.\"bolt/y\"]\nrequired = [\"#gate\"]\nremote = \"origin\"\ntrust = [\"sessions\"]\n\
         \n[signers.box]\nssh = \"box ssh-ed25519 AAAA\"\nconfined = true\n\
         \n[signers.sessions]\nssh = \"s ssh-ed25519 BBBB\"\nconfined = false\n";
    let h = Harness::new(&p);
    h.w().init(
        "bolt/y",
        &[
            (".config/sluice.toml", &p),
            ("a/lib", "a1"),
            ("b/lib", "b1"),
            ("c/lib", "c1"),
        ],
    );
    // On bolt/x, whose gate trusts only box and only confined results, a
    // host signing as sessions settles nothing: the combination never
    // passes, and the branch is left as it was.
    h.w().signs_as("sessions");
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    let sessions = || {
        h.w().be_host("s");
        h.sluice().as_peer(Peer {
            name: "s".into(),
            lease: Duration::from_secs(300),
            alive: Duration::from_secs(120),
            signer: Some("sessions".into()),
            systems: None,
            expire_after: sluice_core::EXPIRE_AFTER,
        })
    };
    for _ in 0..4 {
        sessions().step(BRANCH).unwrap();
    }
    assert_ne!(h.sub(one).state, State::Landed);
    assert!(h.w().moves().is_empty());
    assert!(claims(&h).is_empty());

    // On bolt/y, which trusts sessions and asks for no confinement, the
    // same host's results land the work, read as they are: unconfined.
    let head = h.w().work(&h.w().tip("bolt/y"), &[("a/lib", Some("a2"))]);
    h.w().session("unit/y", "agent-y", &head);
    let s = sessions();
    let y = s
        .submit(&sluice_core::SubmitRequest {
            head: sluice_core::SubmitHead::Worktree("unit/y".into()),
            branch: "bolt/y".into(),
            after: vec![],
            priority: 0,
            subject: None,
        })
        .unwrap()
        .unwrap()
        .id;
    for _ in 0..6 {
        sessions().step("bolt/y").unwrap();
    }
    assert_eq!(sessions().state(y).unwrap().unwrap().state, State::Landed);
}

/// satisfies: 137, record 0093 as amended 2026-11-08, 58, 72 (design D8);
/// the peer-lead spec's "A session host submits to a bolt held on origin":
/// `bolt/x`'s home is origin, its policy trusts only `box`, host s signs as
/// no one and submits while no host leads it; s writes its submission and
/// no lead, its rounds are led, and a host signing as `box` with its key
/// listed takes the lead at its next round and tests the submission.
#[test]
fn a_host_whose_signature_the_branch_does_not_accept_never_leads() {
    let h = peers();
    h.w().be_host("s");
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    assert_eq!(
        h.w().home_in("origin", BRANCH).map(|h| h.at),
        Some("origin".into())
    );
    for _ in 0..2 {
        assert_eq!(as_signer(&h, "s", None).step(BRANCH).unwrap(), Step::Led);
    }
    assert_eq!(h.doc().lead, None);
    assert!(took(&h).is_empty());
    assert!(h.w().requests().is_empty());
    assert!(!as_signer(&h, "s", None).keeps(BRANCH).unwrap());
    assert_eq!(on(&h, "a").step(BRANCH).unwrap(), Step::Stepped);
    assert_eq!(took(&h), [("a".to_string(), None)]);
    assert_eq!(h.w().requests().len(), 1, "a tests the submission");
}

/// satisfies: 137, 87 (design D8); the peer-lead spec's "A host with an
/// unlisted key": host k signs as `box` with a key `bolt/x`'s policy does
/// not list, and `bolt/x`'s lead has been quiet past the lease; k does not
/// take it, and keeps nothing, so takes none of its pull requests in.
#[test]
fn a_host_with_an_unlisted_key_never_leads() {
    let h = peers();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    keeper(&h).step(BRANCH).unwrap();
    h.w().be_host("k");
    h.w().key_unlisted(true);
    assert_eq!(on(&h, "k").step(BRANCH).unwrap(), Step::Led);
    h.w().advance(chrono::Duration::minutes(10));
    assert_eq!(on(&h, "k").step(BRANCH).unwrap(), Step::Led);
    assert!(!on(&h, "k").keeps(BRANCH).unwrap());
    assert_eq!(took(&h), [("lead".to_string(), None)]);
    assert_eq!(h.doc().lead.as_deref(), Some("lead"));
}

/// satisfies: 73, 137 (design D8); the peer-lead spec's "A lead the branch
/// no longer accepts": `bolt/x`'s lead is host a; a's key is no longer
/// listed, and host b, which `bolt/x` accepts, has a submission waiting; a
/// writes nothing more as lead, and b takes the branch once it has seen
/// its state stand for the lease.
#[test]
fn a_lead_the_branch_no_longer_accepts_is_taken_after_the_lease() {
    let h = peers();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    on(&h, "a").step(BRANCH).unwrap();
    assert_eq!(h.doc().lead.as_deref(), Some("a"));
    h.w().be_host("a");
    h.w().key_unlisted(true);
    let version = h.version();
    assert_eq!(on(&h, "a").step(BRANCH).unwrap(), Step::Led);
    assert_eq!(h.version(), version, "a writes nothing as lead");
    assert_eq!(on(&h, "b").step(BRANCH).unwrap(), Step::Led);
    h.w().advance(chrono::Duration::minutes(6));
    assert_eq!(on(&h, "b").step(BRANCH).unwrap(), Step::Stepped);
    assert_eq!(
        took(&h).last().cloned(),
        Some(("b".to_string(), Some("a".to_string())))
    );
    assert_eq!(on(&h, "a").step(BRANCH).unwrap(), Step::Led);
}

/// satisfies: 72, 73, record 0122 (design D4, D8); the peer-lead spec's "A
/// bolt whose signers went": `bolt/x`'s home is origin and a landing brings
/// a policy naming no signers; host s, which signs as no one, steps it
/// while no host leads it, takes the lead, and writes its move home.
#[test]
fn any_host_leads_a_remote_home_whose_policy_names_no_signer() {
    let h = peers();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    let unsigned = sluice_conformance::policy(false, "fail", 4);
    h.w()
        .hand_commit(BRANCH, &[(".config/sluice.toml", Some(&unsigned))]);
    as_signer(&h, "s", None).step(BRANCH).unwrap();
    let log = h.w().file_in("origin", BRANCH, "log.jsonl").unwrap();
    assert!(log.contains("\"event\":\"took\",\"by\":\"s\""), "{log}");
    assert!(
        log.contains("\"event\":\"moving\",\"to\":\"here\""),
        "{log}"
    );
    assert_eq!(
        h.w().home_in("origin", BRANCH).unwrap().closed_to(),
        Some("here")
    );
}

fn lapses(h: &Harness) -> usize {
    h.events()
        .into_iter()
        .filter(|e| matches!(e, Event::Lapsed { .. }))
        .count()
}

/// satisfies: 72, 58, record 0122 (design D9); the peer-lead spec's "A run
/// longer than the lease": host a leads `bolt/x` and runs a combination
/// that takes 25 minutes against a five-minute lease, while host b, which
/// `bolt/x` accepts, steps every minute; a writes `kept` every half lease,
/// so b never takes `bolt/x`, the run's claim stands, and a lands the
/// combination once its run passes.
#[test]
fn a_lead_keeps_its_branch_through_a_run_longer_than_the_lease() {
    let h = peers();
    let p = h.file(&h.tip(), ".config/sluice.toml").unwrap();
    h.w().hand_commit(
        BRANCH,
        &[(
            ".config/sluice.toml",
            Some(&p.replace("timeout = \"20m\"", "timeout = \"40m\"")),
        )],
    );
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    on(&h, "a").step(BRANCH).unwrap();
    assert_eq!(h.w().live_runs(), 1, "a runs the combination");
    let mut kept = Vec::new();
    for _ in 0..25 {
        h.w().advance(chrono::Duration::minutes(1));
        on(&h, "a").step(BRANCH).unwrap();
        assert_eq!(on(&h, "b").step(BRANCH).unwrap(), Step::Led);
        kept.push(h.doc().kept);
    }
    kept.dedup();
    assert!(kept.len() >= 9, "kept every half lease: {kept:?}");
    assert_eq!(took(&h), [("a".to_string(), None)]);
    assert_eq!(lapses(&h), 0);
    h.w().complete_all();
    for _ in 0..4 {
        on(&h, "a").step(BRANCH).unwrap();
    }
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(took(&h), [("a".to_string(), None)]);
}

/// satisfies: 72, record 0122 (design D9); the peer-lead spec's "Nothing
/// live": host a leads `bolt/x` and no run of it is claimed, under test or
/// open; a's rounds write nothing that changes nothing, however long.
#[test]
fn a_lead_with_no_live_run_writes_nothing_to_keep_it() {
    let h = peers();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    for _ in 0..4 {
        on(&h, "a").step(BRANCH).unwrap();
    }
    assert_eq!(h.sub(one).state, State::Landed);
    on(&h, "a").step(BRANCH).unwrap();
    let version = h.version();
    for _ in 0..12 {
        h.w().advance(chrono::Duration::minutes(5));
        on(&h, "a").step(BRANCH).unwrap();
    }
    assert_eq!(h.version(), version, "nothing written to keep it");
}
