//! A pass the run kept no proof of: on a branch whose records are on its
//! remote, a check that passed in a run that kept nothing proving it ran
//! has no outcome. Nothing is recorded under its key and nobody is blamed,
//! it runs again within 115's bound, and the branch's log keeps its cause
//! once per cause and host, with its end (114, 115, 16; record 0105; the
//! change `a-pass-nothing-proves-blames-no-one`; rules core.9, core.28,
//! core.35).

use std::time::Duration;

use sluice_conformance::{policy, Harness, Rig, BRANCH};
use sluice_core::records::{Event, LostRun, ResultKind, ResultLine, State};
use sluice_core::{Peer, Sluice};
use sluice_seams::{Decided, NotRun, Outcome};

/// Where the fakes say they looked for the run's record of a pass.
const STORE: &str = "/the/store";

/// What the fakes say of a pass whose record is not in [`STORE`].
const WHY: &str = "it passed, but nothing the run kept proves it ran: \
                   its record is not in /the/store";

/// The bolt policy requiring `a:test` and `b:test`, on `origin`, its runs
/// counted through the signer `box`, as
/// `a_signed_branch_counts_the_dependency_through_its_signer` in
/// `failed_dependency.rs` has it.
fn signed() -> String {
    policy(false, "fail", 4)
        .replace(
            "required = [\"#gate\"]",
            "required = [\"a:test\", \"b:test\"]",
        )
        .replace(
            "trust = [\"open\", \"sandbox\", \"container\"]\n",
            "trust = [\"box\"]\nremote = \"origin\"\n",
        )
        + "\n[signers.box]\nssh = \"box ssh-ed25519 AAAA\"\n"
}

/// The three projects on a signed branch, this host signing as `box`.
fn rig() -> Harness {
    let h = Harness::new(&signed());
    h.w().signs_as("box");
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

fn step(h: &Harness) {
    signer(h).step(BRANCH).expect("a step runs");
}

/// Step as the signer until a step changes nothing.
fn settle(h: &Harness) {
    for _ in 0..64 {
        let v = h.w().version(BRANCH);
        let delivered = h.w().delivered().len();
        let places = h.w().made_places().len();
        step(h);
        if v == h.w().version(BRANCH)
            && delivered == h.w().delivered().len()
            && places == h.w().made_places().len()
        {
            return;
        }
    }
    panic!("{BRANCH} never settled");
}

/// The checks each lost run left not run, with why, in order.
fn lost(h: &Harness) -> Vec<Vec<(String, String)>> {
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

/// Each `proof-missing` line's host and why, in order.
fn missing(h: &Harness) -> Vec<(String, String)> {
    h.events()
        .into_iter()
        .filter_map(|e| match e {
            Event::ProofMissing { host, why } => Some((host, why)),
            _ => None,
        })
        .collect()
}

fn results_of(h: &Harness, task: &str) -> Vec<ResultLine> {
    h.results().into_iter().filter(|r| r.task == task).collect()
}

/// The key the run gave `task`, as the executor observed it.
fn key_of(h: &Harness, task: &str) -> String {
    h.w()
        .observed()
        .into_iter()
        .find(|(t, _, _)| t == task)
        .map(|(_, key, _)| key)
        .unwrap_or_else(|| panic!("{task} ran"))
}

/// `tasks`, each could not run for [`WHY`].
fn unkept(tasks: &[&str]) -> Vec<(String, String)> {
    tasks
        .iter()
        .map(|t| (t.to_string(), WHY.to_string()))
        .collect()
}

/// satisfies: 114, 16, 115; record 0105; the pass-proof spec's "Every
/// pass of a run has no proof" and "The next run keeps its proof", and
/// the run-outcome spec's "A pass with no proof"
#[test]
fn a_pass_nothing_proves_has_no_outcome_and_runs_again() {
    let h = rig();
    h.w().keeps_no_proof(Some(STORE));
    h.work(
        "unit/a",
        "agent-a",
        &[("a/lib", Some("a2")), ("b/lib", Some("b2"))],
    );
    let one = h.submit("unit/a");
    for _ in 0..8 {
        if !lost(&h).is_empty() {
            break;
        }
        step(&h);
    }
    assert_eq!(lost(&h), [unkept(&["a:test", "b:test"])]);
    assert!(results_of(&h, "a:test").is_empty(), "{:?}", h.results());
    assert!(results_of(&h, "b:test").is_empty(), "{:?}", h.results());
    assert_ne!(h.sub(one).state, State::Failed);
    assert!(h.w().delivered().is_empty(), "nobody is blamed");
    assert_eq!(missing(&h), [("fake-host".to_string(), WHY.to_string())]);
    let doc = h.doc();
    let runs: Vec<&LostRun> = doc
        .candidates
        .iter()
        .flat_map(|c| &c.runs)
        .filter_map(|r| r.lost.as_ref())
        .collect();
    assert_eq!(
        runs,
        [&LostRun {
            host: "fake-host".into(),
            system: None,
            not_run: ["a:test", "b:test"]
                .iter()
                .map(|t| NotRun {
                    task: t.to_string(),
                    reason: WHY.into(),
                    key: Some(key_of(&h, t)),
                })
                .collect(),
        }]
    );
    // Record 0122 (design D7): a follow-up holds the checks with no
    // outcome, why its run had none going with it.
    let follow = doc.candidates[0].runs.last().expect("a follow-up");
    assert_eq!(follow.follows, Some(0));
    assert_eq!(follow.checks, ["a:test", "b:test"]);
    assert!(follow.lost.is_some());

    h.w().keeps_no_proof(None);
    settle(&h);
    assert_eq!(h.sub(one).state, State::Landed);
    for task in ["a:test", "b:test"] {
        let ran: Vec<ResultLine> = results_of(&h, task)
            .into_iter()
            .filter(|r| r.kind == ResultKind::Ran)
            .collect();
        assert_eq!(ran.len(), 1, "{ran:?}");
        assert_eq!(ran[0].outcome, Outcome::Pass);
        assert_eq!(ran[0].host.as_deref(), Some("fake-host"));
        assert_eq!(ran[0].signer.as_deref(), Some("box"));
    }
    let kept: Vec<(String, String, u64)> = h
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::ProofKept {
                host, why, runs, ..
            } => Some((host, why, runs)),
            _ => None,
        })
        .collect();
    assert_eq!(kept, [("fake-host".to_string(), WHY.to_string(), 1)]);
    let doc = h.doc();
    assert!(doc.proof_missing.is_empty(), "{:?}", doc.proof_missing);
    assert!(doc
        .candidates
        .iter()
        .flat_map(|c| &c.runs)
        .all(|r| r.lost.is_none()));
}

/// satisfies: 115, 114, 17, 20; record 0105; the pass-proof spec's
/// "Three runs in a row"
#[test]
fn three_runs_whose_passes_nothing_proves_blame_with_why() {
    let h = rig();
    h.w().keeps_no_proof(Some(STORE));
    h.work(
        "unit/a",
        "agent-a",
        &[("a/lib", Some("a2")), ("b/lib", Some("b2"))],
    );
    let one = h.submit("unit/a");
    settle(&h);
    assert_eq!(h.sub(one).state, State::Failed);
    assert_eq!(h.w().started().len(), 3, "no fourth run");
    assert_eq!(lost(&h).len(), 2);
    // Record 0122 (design D7): a run and two follow-ups, three in a row.
    let opened: Vec<usize> = h
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Opened { run, .. } => Some(run),
            _ => None,
        })
        .collect();
    assert_eq!(opened, [0, 1, 2]);
    let failed: Vec<(String, Option<String>, bool)> = h
        .sub(one)
        .failed
        .iter()
        .map(|f| (f.task.clone(), f.not_run.clone(), f.hash.is_empty()))
        .collect();
    assert_eq!(
        failed,
        [
            ("a:test".to_string(), Some(WHY.to_string()), true),
            ("b:test".to_string(), Some(WHY.to_string()), true),
        ]
    );
    match &h.delivered_to("agent-a")[0].decided {
        Decided::Failed { failed, .. } => {
            assert_eq!(failed, &vec!["a:test".to_string(), "b:test".to_string()])
        }
        other => panic!("delivered {other:?}"),
    }
    assert!(results_of(&h, "a:test").is_empty());
    assert!(results_of(&h, "b:test").is_empty());
    assert_eq!(missing(&h).len(), 1, "one cause, however many runs");
}

/// satisfies: 114, 17; record 0105; the pass-proof spec's "A test that
/// failed on its own beside it"
#[test]
fn a_check_that_failed_beside_a_pass_nothing_proves_fails_as_before() {
    let h = rig();
    h.fails_with("a:test", "a/lib", "broken");
    h.w().keeps_no_proof(Some(STORE));
    h.work(
        "unit/a",
        "agent-a",
        &[("a/lib", Some("broken")), ("b/lib", Some("b2"))],
    );
    let one = h.submit("unit/a");
    settle(&h);
    assert_eq!(h.sub(one).state, State::Failed);
    assert_eq!(h.w().started().len(), 1, "a fail of its own is a verdict");
    let failed = &h.sub(one).failed;
    let a = failed.iter().find(|f| f.task == "a:test").unwrap();
    assert!(!a.hash.is_empty() && a.not_run.is_none(), "{a:?}");
    let fail = results_of(&h, "a:test");
    assert_eq!(fail.len(), 1, "{fail:?}");
    assert_eq!(fail[0].outcome, Outcome::Fail);
    assert_eq!(fail[0].hash, a.hash);
    let b = failed.iter().find(|f| f.task == "b:test").unwrap();
    assert_eq!(b.not_run.as_deref(), Some(WHY));
    assert!(results_of(&h, "b:test").is_empty());
}

/// satisfies: C.6, 15; record 0105; the pass-proof spec's "A key the
/// tree does not bear out"
#[test]
fn a_key_the_tree_does_not_bear_out_is_still_failed() {
    let h = rig();
    h.w().forges("a:test");
    h.work(
        "unit/a",
        "agent-a",
        &[("a/lib", Some("a2")), ("b/lib", Some("b2"))],
    );
    let one = h.submit("unit/a");
    settle(&h);
    assert_eq!(h.sub(one).state, State::Failed);
    let failed = &h.sub(one).failed;
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert_eq!(failed[0].task, "a:test");
    assert!(!failed[0].hash.is_empty(), "under its key");
    assert!(failed.iter().all(|f| f.not_run.is_none()), "{failed:?}");
    assert!(lost(&h).is_empty());
    assert!(missing(&h).is_empty());
}

/// satisfies: 114; record 0105; rule core.28; the pass-proof spec's "The
/// lost line" (design D3 of the change
/// `a-signed-run-without-a-sandbox-proves-its-passes`)
#[test]
fn a_lost_line_names_each_unkept_key() {
    let h = rig();
    h.w().keeps_no_proof(Some(STORE));
    h.work(
        "unit/a",
        "agent-a",
        &[("a/lib", Some("a2")), ("b/lib", Some("b2"))],
    );
    h.submit("unit/a");
    let line = || {
        h.events().into_iter().find_map(|e| match e {
            Event::Lost { not_run, .. } if !not_run.is_empty() => Some(not_run),
            _ => None,
        })
    };
    for _ in 0..8 {
        if line().is_some() {
            break;
        }
        step(&h);
    }
    let not_run = line().expect("a lost line");
    let named: Vec<(String, Option<String>)> = not_run
        .iter()
        .map(|n| (n.task.clone(), n.key.clone()))
        .collect();
    assert_eq!(
        named,
        [
            ("a:test".to_string(), Some(key_of(&h, "a:test"))),
            ("b:test".to_string(), Some(key_of(&h, "b:test"))),
        ]
    );
    let json = serde_json::to_value(&not_run).unwrap();
    assert_eq!(json[0]["key"], key_of(&h, "a:test"));
    assert_eq!(json[0]["reason"], WHY);
    assert_eq!(missing(&h), [("fake-host".to_string(), WHY.to_string())]);
}

/// satisfies: C.6, 20; the pass-proof spec's "A forged key" (design D5
/// of the change `a-signed-run-without-a-sandbox-proves-its-passes`)
#[test]
fn a_key_the_tree_does_not_bear_out_is_blamed_with_why() {
    let h = rig();
    h.w().forges("a:test");
    h.work(
        "unit/a",
        "agent-a",
        &[("a/lib", Some("a2")), ("b/lib", Some("b2"))],
    );
    let one = h.submit("unit/a");
    settle(&h);
    assert_eq!(h.sub(one).state, State::Failed);
    let key = key_of(&h, "a:test");
    let failed: Vec<(String, String, Option<String>)> = h
        .sub(one)
        .failed
        .iter()
        .map(|f| (f.task.clone(), f.hash.clone(), f.unsigned.clone()))
        .collect();
    assert_eq!(
        failed,
        [(
            "a:test".to_string(),
            key,
            Some("a:test: the manifest under its key does not hash to it".to_string())
        )]
    );
}

/// satisfies: 78; record 0105; the pass-proof spec's "Its runs keep no
/// manifest"
#[test]
fn a_branch_in_one_hosts_files_reads_no_proof() {
    let h = Harness::bolt();
    h.w().keeps_no_proof(Some(STORE));
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    let one = h.submit("unit/a");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert!(missing(&h).is_empty());
    assert!(!h.events().iter().any(|e| matches!(e, Event::Lost { .. })));
    assert!(h.doc().proof_missing.is_empty());
}

/// satisfies: 114; record 0105; rule core.28; the pass-proof spec's "Two
/// combinations on one host" and "Three runs touched"
#[test]
fn the_cause_is_logged_once_across_runs_and_combinations() {
    let h = rig();
    h.w().keeps_no_proof(Some(STORE));
    let until = |n: usize| {
        for _ in 0..16 {
            if lost(&h).len() >= n {
                break;
            }
            step(&h);
        }
        assert_eq!(lost(&h).len(), n);
    };
    // #1's combination starts alone, then #1 and #2's beside its second
    // run: three runs in all, none at the bound.
    h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
    h.submit("unit/a");
    for _ in 0..8 {
        if h.events()
            .iter()
            .any(|e| matches!(e, Event::Testing { .. }))
        {
            break;
        }
        step(&h);
    }
    h.work("unit/b", "agent-b", &[("b/lib", Some("b2"))]);
    h.submit("unit/b");
    until(3);
    let combinations: std::collections::BTreeSet<_> = h
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Lost { combination, .. } => Some(combination),
            _ => None,
        })
        .collect();
    assert_eq!(combinations.len(), 2, "two combinations");
    assert_eq!(missing(&h), [("fake-host".to_string(), WHY.to_string())]);
    let standing: Vec<(String, String, u64)> = h
        .doc()
        .proof_missing
        .into_iter()
        .map(|p| (p.host, p.why, p.runs))
        .collect();
    assert_eq!(standing, [("fake-host".to_string(), WHY.to_string(), 3)]);
}
