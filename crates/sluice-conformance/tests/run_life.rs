//! A run's life, written in its branch's home before anything acts on it
//! (record 0122; the run-life spec): its five states, each act's proof,
//! the limit from its own start, follow-up runs, a change of lead, and a
//! pool's run through the same states.
//!
//! Two harnesses: [`signed`], a branch whose records are on its remote and
//! whose policy names `[signers.box]`, as `peers.rs` has it, and [`local`],
//! a branch in the host's own files.

use std::time::Duration;

use sluice_conformance::{policy, Crash, Harness, Rig, BRANCH};
use sluice_core::records::{Candidate, RunState, State, SystemRun};
use sluice_core::{Peer, Sluice};

/// A branch whose records are on its remote, trusting the signer `box`.
#[allow(dead_code)]
fn signed() -> Harness {
    signed_with("")
}

/// [`signed`], its policy ending in `more`.
#[allow(dead_code)]
fn signed_with(more: &str) -> Harness {
    let p = policy(false, "fail", 4).replace(
        "trust = [\"open\", \"sandbox\", \"container\"]\n",
        "trust = [\"box\"]\nremote = \"origin\"\n",
    ) + "\n[signers.box]\nssh = \"box ssh-ed25519 AAAA\"\n"
        + more;
    let h = Harness::new(&p);
    h.w().signs_as("box");
    h
}

/// A branch in the host's own files.
fn local() -> Harness {
    Harness::bolt()
}

/// Instance `name` of the rig, on its own host, with a lease of five
/// minutes and a sign of life bound of two.
#[allow(dead_code)]
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

/// The branch's candidates, as its records hold them.
fn candidates(h: &Harness) -> Vec<Candidate> {
    h.doc().candidates
}

/// The runs of the candidate holding exactly `ids`.
fn runs_of(h: &Harness, ids: &[u64]) -> Vec<SystemRun> {
    candidates(h)
        .into_iter()
        .find(|c| c.submissions == ids)
        .map(|c| c.runs)
        .unwrap_or_default()
}

/// satisfies: record 0122; rule core.14; the run-life spec's "Nothing acts
/// on a run before the state naming the act is written", a host that stops
/// after starting the checks
#[test]
fn a_run_started_before_its_running_was_written_is_not_started_again() {
    let h = local();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.w().crash(Crash::AfterStart);
    assert!(h.sluice().step(BRANCH).is_err(), "the step stopped");
    let run = &runs_of(&h, &[one])[0];
    assert_eq!(run.state, RunState::Claimed);
    let place = run.place.clone().expect("its checkout named at the claim");
    assert_eq!(h.w().started().len(), 1, "its checks were started");
    let began = h.w().run_starts().pop();

    // The next round, minutes on, finds the supervisor's run.
    h.w().advance(chrono::Duration::minutes(3));
    h.step();
    let run = &runs_of(&h, &[one])[0];
    assert_eq!(run.state, RunState::Running);
    assert_eq!(
        run.running, began,
        "running from the run's own start, not the round's"
    );
    assert!(run.run.is_some());
    assert_eq!(h.w().started().len(), 1, "nothing started again");
    assert_eq!(h.w().made_places(), std::slice::from_ref(&place));
    assert_eq!(h.releases_of(&place), 0, "nothing released");

    h.w().complete_all();
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.w().started().len(), 1);
}

/// satisfies: record 0122; rule core.16; the run-life spec's "Nothing acts
/// on a run before the state naming the act is written", a host that stops
/// after making the checkout
#[test]
fn a_checkout_made_before_the_start_is_used_once() {
    let h = local();
    h.w().hold_runs(true);
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.w().crash(Crash::AfterCheckout);
    assert!(h.sluice().step(BRANCH).is_err(), "the step stopped");
    let run = &runs_of(&h, &[one])[0];
    assert_eq!(run.state, RunState::Claimed);
    let place = run.place.clone().expect("its checkout named at the claim");
    assert_eq!(h.w().made_places(), std::slice::from_ref(&place));
    assert!(h.w().started().is_empty(), "no checks started");

    h.step();
    assert_eq!(h.w().started().len(), 1, "started once, in the checkout");
    assert_eq!(
        h.w().made_places(),
        std::slice::from_ref(&place),
        "no second checkout"
    );
    assert_eq!(h.releases_of(&place), 0);
    assert_eq!(runs_of(&h, &[one])[0].state, RunState::Running);
    h.w().complete_all();
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert_eq!(h.releases_of(&place), 1);
    assert_eq!(h.w().made_places(), [place]);
    assert_eq!(h.w().started().len(), 1);
}
