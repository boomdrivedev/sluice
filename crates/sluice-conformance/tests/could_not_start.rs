//! A run whose tests could not be started: recorded with why, its
//! checkout released, nothing under any key and nobody blamed, open again
//! for any host, and counted toward the runs in a row after which its
//! combination is not passed (16, 114, 115; design D1). A run whose
//! checkout could not be made never began, and counts toward nothing; a
//! claimed run's checkout is made at its combination, fetched first where
//! the host lacks it (60, 82, C.6; record 0118).

use std::time::Duration;

use sluice_conformance::{policy, Harness, Rig, BRANCH};
use sluice_core::records::{Event, RunState, State, SystemRun};
use sluice_core::{Peer, Sluice, Started};
use sluice_seams::{Decided, State as _};

/// What a toolchain that fails to enter says.
const TOOLCHAIN: &str =
    "[tests] toolchain nix develop exited exit status: 1: error: flake.lock is not up to date";

/// What a `[tests] not_run` exit says of a check that could not run.
const NOT_RUN: &str = "exited 77, which [tests] not_run names: it could not run here";

/// A start's reason for `words`.
fn could_not_start(words: &str) -> String {
    format!("could not start: {words}")
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

/// The runs of the branch's candidates.
fn runs(h: &Harness) -> Vec<SystemRun> {
    h.doc()
        .candidates
        .iter()
        .flat_map(|c| c.runs.clone())
        .collect()
}

/// How many runs in a row of the branch's one candidate left checks not
/// run.
fn in_a_row(h: &Harness) -> u32 {
    h.doc().candidates[0].not_run_runs
}

/// The gate's one entry, not run for `why`.
fn gate_not_run(why: &str) -> Vec<(String, String)> {
    vec![("#gate".to_string(), why.to_string())]
}

/// satisfies: 16, 114; the run-outcome spec's "The toolchain fails to
/// enter"
#[test]
fn a_run_whose_toolchain_fails_to_enter_is_recorded_with_its_words() {
    let h = Harness::bolt();
    h.w().start_fails(Some(TOOLCHAIN));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    assert_eq!(not_run(&h), [gate_not_run(&could_not_start(TOOLCHAIN))]);
    assert_eq!(h.w().made_places().len(), 1, "its checkout was made");
    assert!(h.w().live_places().is_empty(), "and released");
    assert!(h.w().results_log().is_empty(), "nothing under any key");
    assert!(h.w().delivered().is_empty(), "nobody is blamed");
    assert_ne!(h.sub(one).state, State::Failed);
    let r = &runs(&h)[0];
    assert_eq!(
        (r.state, r.host.clone(), r.place.clone()),
        (RunState::Open, None, None)
    );
    assert!(h.doc().release.is_empty(), "nothing left to release");

    h.w().start_fails(None);
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: 16, 60, 114, 18; the run-outcome spec's "The checkout
/// cannot be made" (record 0118)
#[test]
fn a_run_whose_checkout_cannot_be_made_is_recorded_with_why() {
    let h = Harness::bolt();
    let words = "no room left on the device";
    h.w().checkout_fails(Some(words));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    assert_eq!(not_run(&h), [gate_not_run(&could_not_start(words))]);
    assert!(h.w().made_places().is_empty(), "no checkout was made");
    assert!(h.w().requests().is_empty(), "nothing started");
    assert!(h.w().results_log().is_empty());
    assert!(h.w().delivered().is_empty(), "nobody is blamed");
    assert_ne!(h.sub(one).state, State::Failed);
    let r = &runs(&h)[0];
    assert_eq!(
        (r.state, r.host.clone(), r.place.clone()),
        (RunState::Open, None, None)
    );
    let lost = r.lost.as_ref().expect("its record says why");
    assert_eq!(lost.not_run[0].reason, could_not_start(words));
    assert_eq!(in_a_row(&h), 0, "it counts toward nothing");

    h.w().checkout_fails(None);
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: 60, 114, 115; the run-outcome spec's "Checkouts that cannot
/// be made, run after run" (record 0118)
#[test]
fn checkouts_that_cannot_be_made_run_after_run_blame_no_one() {
    let h = Harness::bolt();
    let words = "fatal: invalid reference: 05b3ce00";
    h.w().checkout_fails(Some(words));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    for _ in 0..5 {
        h.step();
    }
    assert_eq!(not_run(&h).len(), 5, "each run that never began says why");
    assert_eq!(in_a_row(&h), 0, "none counts toward the runs in a row");
    assert!(h.w().delivered().is_empty(), "nobody is blamed");
    assert_ne!(h.sub(one).state, State::Failed);
    assert_eq!(runs(&h)[0].state, RunState::Open);

    h.w().checkout_fails(None);
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.w().requests().len(), 1, "its tests ran once");
}

/// satisfies: 84, 114, 139; the run-outcome spec's "The executor turns
/// the run down at its start"
#[test]
fn a_run_its_executor_turns_down_at_its_start_is_recorded_with_its_words() {
    let h = Harness::new(&(policy(false, "fail", 4) + "\n[executor]\nmemory = \"4G\"\n"));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    let refused = could_not_start("the open executor cannot hold a memory limit");
    assert_eq!(not_run(&h), [gate_not_run(&refused)]);
    assert!(h.w().live_places().is_empty());
    assert!(h.w().results_log().is_empty());
    assert!(h.w().delivered().is_empty());
    assert_ne!(h.sub(one).state, State::Failed);
}

/// The bolt policy with its records on `origin`, trusting the signer
/// `box`.
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
/// Instance `name` on its own host, signing as `signer` or as no one.
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

/// The bolt policy with its records on `origin`, its runs taken by the
/// signer `box` or launched on the forge's pool.
fn with_pool() -> Harness {
    let p = policy(false, "fail", 4).replace(
        "trust = [\"open\", \"sandbox\", \"container\"]\n",
        "trust = [\"box\", \"forge\"]\nremote = \"origin\"\nsystems = [\"linux-x64\"]\n",
    ) + "\n[signers.box]\nssh = \"box ssh-ed25519 AAAA\"\nsystems = [\"linux-x64\"]\n\
         \n[signers.forge]\ngithub = { workflow = \".github/workflows/sluice.yml\", ref = \"refs/heads/main\" }\n\
         \n[pools.forge]\ngithub = { workflow = \".github/workflows/sluice.yml\" }\nsigner = \"forge\"\n\
         runners = { linux-x64 = \"ubuntu-24.04\" }\n\
         \n[systems]\nlinux-x64 = { os = \"linux\", arch = \"x86_64\" }\n";
    let h = Harness::new(&p);
    h.w().signs_as("box");
    h
}

/// A run of `bolt/x` launched on the forge's pool by a lead that signs as
/// no one the gate trusts, no other host live: the pool's claim, and the
/// candidate it holds.
fn launched(h: &Harness) -> (String, String, sluice_seams::Rev) {
    h.w().hold_runs(true);
    h.w().be_host("lead");
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    keeper(h).step(BRANCH).unwrap();
    assert_eq!(h.w().launches().len(), 1);
    let claim = claims(h).pop().expect("the pool's claim");
    assert!(claim.starts_with("pool/forge/"), "{claim}");
    let c = h.doc().candidates[0].clone();
    (claim, c.id, c.revision)
}

/// satisfies: 82, 88, C.6; the run-claims spec's "A pool's job on a
/// machine that lacks the combination" (record 0118)
#[test]
fn a_pools_job_makes_its_place_at_a_combination_only_the_remote_holds() {
    let h = with_pool();
    let (claim, id, combination) = launched(&h);
    h.w().be_host("pool-machine");
    h.w().lack(&combination, true);
    let mut releases = Vec::new();
    let started = h
        .sluice()
        .start_claimed(BRANCH, &claim, &mut releases)
        .unwrap();
    assert!(matches!(started, Some(Started::Running(_))), "{started:?}");
    assert_eq!(
        h.w().candidate_fetches(),
        [("pool-machine".to_string(), BRANCH.to_string(), id)],
        "the combination is fetched by its own ref"
    );
    let asked = h.w().requests();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].revision, combination, "its place is at it");
    let r = &runs(&h)[0];
    assert_eq!(
        (r.state, r.host.as_deref()),
        (RunState::Testing, Some(claim.as_str()))
    );
    assert!(not_run(&h).is_empty());
}

/// satisfies: 60, 82, 114; the run-outcome spec's "The checkout cannot be
/// made" (record 0118)
#[test]
fn a_claimed_run_whose_combination_the_remote_no_longer_holds_never_began() {
    let h = with_pool();
    let (claim, id, combination) = launched(&h);
    h.f.state.unpublish_candidate(BRANCH, &id).unwrap();
    h.w().be_host("pool-machine");
    h.w().lack(&combination, true);
    let mut releases = Vec::new();
    let started = h
        .sluice()
        .start_claimed(BRANCH, &claim, &mut releases)
        .unwrap();
    let why = match started {
        Some(Started::Unmade(why)) => why,
        other => panic!("started {other:?}"),
    };
    assert!(why.contains("invalid reference"), "{why}");
    assert!(h.w().requests().is_empty(), "nothing started");
    assert_eq!(not_run(&h), [gate_not_run(&why)]);
    assert_eq!(in_a_row(&h), 0, "it counts toward nothing");
    assert!(h.w().delivered().is_empty(), "nobody is blamed");
    let r = &runs(&h)[0];
    assert_eq!((r.state, r.host.clone()), (RunState::Open, None));
}

/// satisfies: 82; the run-claims spec's "A host that holds the
/// combination" (record 0118)
#[test]
fn a_host_that_holds_the_combination_fetches_nothing_more() {
    let h = peers();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    keeper(&h).step(BRANCH).unwrap();
    on(&h, "a", Some("box")).step(BRANCH).unwrap();
    assert_eq!(claims(&h), ["a"]);
    assert_eq!(h.w().requests().len(), 1);
    assert!(h.w().candidate_fetches().is_empty());
}

/// Each claim in the branch's log, by host.
fn claims(h: &Harness) -> Vec<String> {
    h.events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Claimed { host, .. } => Some(host),
            _ => None,
        })
        .collect()
}

/// satisfies: 75, 77, 114; the run-outcome spec's "Another host takes the
/// run"
#[test]
fn a_run_one_signing_host_could_not_start_is_taken_by_another() {
    let h = peers();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    // A lead with no room of its own opens the run (design D8).
    keeper(&h).step(BRANCH).unwrap();
    h.w().start_fails(Some(TOOLCHAIN));
    on(&h, "a", Some("box")).step(BRANCH).unwrap();
    assert_eq!(claims(&h), ["a"]);
    assert_eq!(not_run(&h), [gate_not_run(&could_not_start(TOOLCHAIN))]);
    let r = &runs(&h)[0];
    assert_eq!(
        (r.state, r.host.clone()),
        (RunState::Open, None),
        "no host holds it"
    );
    assert!(h.w().live_places().is_empty());

    h.w().start_fails(None);
    on(&h, "b", Some("box")).step(BRANCH).unwrap();
    assert_eq!(claims(&h), ["a", "b"]);
    let r = &runs(&h)[0];
    assert_eq!((r.state, r.host.as_deref()), (RunState::Testing, Some("b")));
    assert_eq!(h.w().requests().len(), 1);
}

/// satisfies: 71, 114; the run-outcome spec's "The host's last run is
/// taken"
#[test]
fn a_run_refused_at_the_bound_is_recorded_as_nothing_and_counts_for_nothing() {
    let h = Harness::bolt();
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.w().refuse_next_start();
    h.step();
    let r = &runs(&h)[0];
    assert_eq!((r.state, r.host.clone()), (RunState::Open, None));
    assert!(not_run(&h).is_empty(), "no record of a run with no result");
    assert_eq!(in_a_row(&h), 0, "it counts toward nothing");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: 115, 17, 20, 50; the run-outcome spec's "Three starts that
/// fail"
#[test]
fn three_starts_that_fail_leave_the_combination_not_passed_with_why() {
    let h = Harness::bolt();
    h.w().start_fails(Some(TOOLCHAIN));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.settle();
    let why = could_not_start(TOOLCHAIN);
    let sub = h.sub(one);
    assert_eq!(sub.state, State::Failed);
    assert_eq!(not_run(&h).len(), 2, "two runs opened again");
    assert_eq!(h.w().made_places().len(), 3, "no fourth checkout");
    assert!(h.w().live_places().is_empty());
    assert_eq!(sub.failed.len(), 1, "{:?}", sub.failed);
    assert_eq!(sub.failed[0].task, "#gate");
    assert!(sub.failed[0].hash.is_empty(), "no key: it has no result");
    assert_eq!(sub.failed[0].not_run.as_deref(), Some(why.as_str()));
    assert!(h.w().results_log().is_empty());
    let told = h.delivered_to("agent-a");
    assert_eq!(told.len(), 1);
    match &told[0].decided {
        Decided::Failed { failed, .. } => assert_eq!(failed, &vec!["#gate".to_string()]),
        other => panic!("delivered {other:?}"),
    }
    let report = told[0].report.as_ref().expect("the notice's report");
    assert!(
        report
            .rows
            .iter()
            .any(|r| r.not_run.as_deref() == Some(why.as_str())),
        "{:?}",
        report.rows
    );
}

/// satisfies: 115, 114; the run-outcome spec's "Causes mixed"
#[test]
fn starts_that_fail_and_checks_that_could_not_run_count_together() {
    let h = Harness::bolt();
    h.w().start_fails(Some(TOOLCHAIN));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.step();
    assert_eq!(in_a_row(&h), 1);

    // The second run starts, and leaves its check not run.
    h.w().start_fails(None);
    h.w().cannot_run("a:test", Some(NOT_RUN));
    h.step();
    assert_eq!(h.w().requests().len(), 1);

    // Its report is read, and the third run's start fails.
    h.w().start_fails(Some(TOOLCHAIN));
    h.step();
    assert_eq!(
        not_run(&h),
        [
            gate_not_run(&could_not_start(TOOLCHAIN)),
            vec![("a:test".to_string(), NOT_RUN.to_string())],
        ]
    );
    h.settle();
    let sub = h.sub(one);
    assert_eq!(sub.state, State::Failed);
    let named: Vec<(String, Option<String>)> = sub
        .failed
        .iter()
        .map(|f| (f.task.clone(), f.not_run.clone()))
        .collect();
    assert_eq!(
        named,
        [("#gate".to_string(), Some(could_not_start(TOOLCHAIN)))],
        "named with the reason of the run that ended it"
    );
    assert_eq!(h.w().made_places().len(), 3);
}
