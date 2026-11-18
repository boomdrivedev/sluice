//! A required check its runner skipped for a task it depends on that ran
//! and failed: failed by that task, whose run is recorded and weighed
//! under its own key, and never read as left without a result (149, 15,
//! 16, 19, 114, 115; record 0084; rules core.7, core.34).

use std::time::Duration;

use sluice_conformance::{policy, Harness, Rig, BRANCH};
use sluice_core::records::{Event, ResultLine, State};
use sluice_core::{Peer, Sluice, SubmitHead, SubmitRequest};
use sluice_seams::{Decided, Outcome};

/// The check `main` requires: a task that runs nothing of its own.
const CHECK: &str = "root:integration";

/// The task it depends on, which no gate of `bolt/x` requires.
const DEP: &str = "api:integration";

/// What a check whose dependency exited as `[tests] not_run` names says.
const COULD_NOT: &str = "its dependency api:integration could not run: exited 77";

/// The bolt policy requiring `root:integration` alone, a key with a pass
/// and a fail read as `flaky` says.
fn integration(flaky: &str) -> String {
    policy(false, flaky, 4).replace(
        "required = [\"#gate\"]",
        &format!("required = [\"{CHECK}\"]"),
    )
}

/// The three projects under `policy`, with `root:integration`, a check of
/// `r/` that depends on `api:integration`, the check of `a/`.
fn rig(policy: &str) -> Harness {
    let h = Harness::new(policy);
    h.w().task(DEP, "a/");
    h.w().task(CHECK, "r/");
    h.f.tests.depends_on(CHECK, DEP);
    h
}

fn results(h: &Harness) -> Vec<ResultLine> {
    sluice_core::records::parse_lines(h.w().results_log().as_bytes()).unwrap()
}

/// The checks each lost run left not run, with why, in order.
fn not_run_events(h: &Harness) -> Vec<Vec<(String, String)>> {
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

/// A submission from `unit/a` of `a/lib` at `content`.
fn submit_a(h: &Harness, content: &str) -> u64 {
    h.work("unit/a", "agent-a", &[("a/lib", Some(content))]);
    h.submit("unit/a")
}

/// 19; design D4: `api:integration`'s key at `a/lib` = `content` given a
/// pass a trusted run recorded, as a branch whose gate requires the task
/// records its passes: `bolt/y`, its policy written as `policy` with
/// `api:integration` required, lands work that holds it.
fn passed_on_another_branch(h: &Harness, policy: &str, content: &str) {
    let other = "bolt/y";
    let y = policy.replace(
        &format!("required = [\"{CHECK}\"]"),
        &format!("required = [\"{DEP}\"]"),
    );
    let base = h.w().init(
        other,
        &[
            (".config/sluice.toml", y.as_str()),
            ("a/lib", "y1"),
            ("b/lib", "b1"),
            ("c/lib", "c1"),
        ],
    );
    let head = h.w().work(&base, &[("a/lib", Some(content))]);
    h.w().session("unit/y", "agent-y", &head);
    h.sluice()
        .submit(&SubmitRequest {
            head: SubmitHead::Worktree(h.place("unit/y")),
            branch: other.into(),
            after: Vec::new(),
            priority: 0,
            subject: None,
        })
        .expect("submit runs")
        .expect("accepted");
    for _ in 0..16 {
        h.sluice().step(other).expect("a step of bolt/y runs");
    }
    assert!(
        results(h)
            .iter()
            .any(|r| r.task == DEP && r.branch == other && r.outcome == Outcome::Pass),
        "bolt/y recorded a pass of {DEP}"
    );
}

/// satisfies: 149, 15, 16, 17, 20; the failed-dependency spec's "A check
/// that runs nothing of its own"
#[test]
fn a_check_skipped_for_its_dependency_is_failed_by_it() {
    let h = rig(&integration("fail"));
    h.fails_with(DEP, "a/lib", "broken");
    let one = submit_a(&h, "broken");
    h.settle();
    let sub = h.sub(one);
    assert_eq!(sub.state, State::Failed);
    assert_eq!(sub.result.as_deref(), Some("failed"));
    assert_eq!(sub.failed.len(), 2, "{:?}", sub.failed);
    let (dep, check) = (&sub.failed[0], &sub.failed[1]);
    assert_eq!(dep.task, DEP);
    assert!(!dep.hash.is_empty(), "the task that ran has its key");
    assert!(dep.failed_by.is_empty() && dep.not_run.is_none());
    let log = dep.log.as_deref().expect("its log is kept");
    assert!(
        h.kept_log(log)
            .unwrap()
            .starts_with("api:integration failed"),
        "its own log"
    );
    assert_eq!(check.task, CHECK);
    assert_eq!(check.failed_by, [DEP]);
    assert!(check.hash.is_empty(), "no run gave it a key");
    assert!(check.log.is_none() && check.not_run.is_none() && check.flaky.is_none());
    match &h.delivered_to("agent-a")[0].decided {
        Decided::Failed { failed, .. } => assert_eq!(failed, &[DEP, CHECK]),
        other => panic!("delivered {other:?}"),
    }
}

/// satisfies: 149, 16; rule core.7; the failed-dependency spec's "The
/// fail in the results"
#[test]
fn the_dependencys_fail_is_recorded_under_its_key() {
    let h = rig(&integration("fail"));
    h.fails_with(DEP, "a/lib", "broken");
    let one = submit_a(&h, "broken");
    h.settle();
    let sub = h.sub(one);
    let combination = sub.tested.expect("tested").combination;
    let lines: Vec<ResultLine> = results(&h)
        .into_iter()
        .filter(|r| r.combination == combination)
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    let l = &lines[0];
    assert_eq!((l.task.as_str(), l.outcome), (DEP, Outcome::Fail));
    assert_eq!(l.hash, sub.failed[0].hash);
    assert_eq!(l.host.as_deref(), Some("fake-host"));
    assert_eq!(l.executor.as_ref().map(|e| e.kind.as_str()), Some("open"));
    assert!(results(&h).iter().all(|r| r.task != CHECK));
    assert!(h.events().iter().any(|e| matches!(
        e,
        Event::Ran { task, outcome: Outcome::Fail, .. } if task == DEP
    )));
}

/// satisfies: 149, 16; design D4; the failed-dependency spec's "A pass of
/// the dependency"
#[test]
fn a_pass_of_the_dependency_is_not_recorded() {
    let h = rig(&integration("fail"));
    h.fails_with(DEP, "a/lib", "broken");
    let one = submit_a(&h, "broken");
    h.settle();
    assert_eq!(h.sub(one).state, State::Failed);
    let two = submit_a(&h, "fixed");
    h.settle();
    assert_eq!(h.sub(two).state, State::Landed);
    let lines = results(&h);
    assert!(lines
        .iter()
        .any(|r| r.task == CHECK && r.outcome == Outcome::Pass));
    assert!(lines
        .iter()
        .filter(|r| r.task == DEP)
        .all(|r| r.outcome == Outcome::Fail));
    assert_eq!(lines.iter().filter(|r| r.task == DEP).count(), 1);
}

/// satisfies: 149, 19, 69; rule core.34; the failed-dependency spec's "A
/// flaky dependency read as failed"
#[test]
fn a_flaky_dependency_read_as_failed_fails_the_check() {
    let p = integration("fail");
    let h = rig(&p);
    h.w().flaky(DEP, &[Outcome::Pass, Outcome::Fail]);
    passed_on_another_branch(&h, &p, "a2");
    let one = submit_a(&h, "a2");
    h.settle();
    let sub = h.sub(one);
    assert_eq!(sub.state, State::Failed);
    let dep = &sub.failed[0];
    assert_eq!(dep.task, DEP);
    let k = dep.flaky.as_ref().expect("read as flaky");
    assert_eq!((k.here, k.other.outcome), (Outcome::Fail, Outcome::Pass));
    assert_eq!(k.other.branch, "bolt/y", "naming the pass");
    assert_eq!(sub.failed[1].task, CHECK);
    assert_eq!(sub.failed[1].failed_by, [DEP]);
}

/// satisfies: 149, 19, 114, 115; the failed-dependency spec's "A flaky
/// dependency read as passed"
#[test]
fn a_flaky_dependency_read_as_passed_leaves_the_check_unjudged() {
    let p = integration("pass");
    let h = rig(&p);
    h.w().flaky(DEP, &[Outcome::Pass, Outcome::Fail]);
    passed_on_another_branch(&h, &p, "a2");
    let one = submit_a(&h, "a2");
    for _ in 0..8 {
        if !not_run_events(&h).is_empty() {
            break;
        }
        h.step();
    }
    assert_eq!(
        not_run_events(&h),
        [vec![(
            CHECK.to_string(),
            "its dependency api:integration failed, and its key reads passed, \
             as flaky = \"pass\" reads a key with a pass and a fail"
                .to_string()
        )]]
    );
    assert_ne!(h.sub(one).state, State::Failed);
    assert!(h.delivered_to("agent-a").is_empty(), "nobody is blamed");

    // Tested again, its dependency passes and the check runs.
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    assert!(results(&h)
        .iter()
        .any(|r| r.task == CHECK && r.outcome == Outcome::Pass));
}

/// satisfies: 149, 19, 83; rule core.34; the failed-dependency spec's "A
/// pass under other limits"
#[test]
fn a_dependencys_pass_under_other_limits_is_not_weighed() {
    let p = integration("pass");
    let h = rig(&(p.clone() + "\n[executor]\ndocker = true\n"));
    h.w().flaky(DEP, &[Outcome::Pass, Outcome::Fail]);
    passed_on_another_branch(&h, &p, "a2");
    let one = submit_a(&h, "a2");
    h.settle();
    let sub = h.sub(one);
    assert_eq!(sub.state, State::Failed, "{:?}", not_run_events(&h));
    assert_eq!(sub.failed[0].task, DEP);
    assert!(sub.failed[0].flaky.is_none(), "the pass is not weighed");
    assert_eq!(sub.failed[1].failed_by, [DEP]);
}

/// satisfies: 149, 38, 19; the failed-dependency spec's "Never under the
/// skipped check's key"
#[test]
fn nothing_is_recorded_under_the_skipped_checks_key() {
    let h = rig(&integration("fail"));
    h.fails_with(DEP, "a/lib", "broken");
    let one = submit_a(&h, "a2");
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
    let before: Vec<ResultLine> = results(&h)
        .into_iter()
        .filter(|r| r.task == CHECK)
        .collect();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].outcome, Outcome::Pass);

    let two = submit_a(&h, "broken");
    h.settle();
    let sub = h.sub(two);
    assert_eq!(sub.state, State::Failed);
    let check = sub.failed.iter().find(|f| f.task == CHECK).unwrap();
    assert!(check.hash.is_empty() && check.flaky.is_none());
    assert_eq!(check.failed_by, [DEP]);
    let after: Vec<ResultLine> = results(&h)
        .into_iter()
        .filter(|r| r.task == CHECK)
        .collect();
    assert_eq!(after, before, "nothing more under its key");
}

/// satisfies: 149, 114, 18; the failed-dependency spec's "The
/// dependency's exit is a not-run exit"
#[test]
fn a_check_whose_dependency_could_not_run_is_tested_again() {
    let h = rig(&integration("fail"));
    h.w().cannot_run(DEP, Some("exited 77"));
    let one = submit_a(&h, "a2");
    for _ in 0..8 {
        if !not_run_events(&h).is_empty() {
            break;
        }
        h.step();
    }
    assert_eq!(
        not_run_events(&h),
        [vec![(CHECK.to_string(), COULD_NOT.to_string())]]
    );
    assert!(results(&h).iter().all(|r| r.task != CHECK && r.task != DEP));
    assert_ne!(h.sub(one).state, State::Failed);
    assert!(h.w().delivered().is_empty(), "nobody is blamed");

    h.w().cannot_run(DEP, None);
    h.settle();
    assert_eq!(h.sub(one).state, State::Landed);
}

/// satisfies: 149, 115, 17, 20; the failed-dependency spec's "Three runs
/// in a row"
#[test]
fn three_runs_in_a_row_blame_the_newest() {
    let h = rig(&integration("fail"));
    h.w().cannot_run(DEP, Some("exited 77"));
    let one = submit_a(&h, "a2");
    h.settle();
    assert_eq!(h.sub(one).state, State::Failed);
    assert_eq!(h.w().started().len(), 3);
    assert_eq!(not_run_events(&h).len(), 2);
    let failed = &h.sub(one).failed;
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert_eq!(failed[0].task, CHECK);
    assert!(failed[0].hash.is_empty() && failed[0].failed_by.is_empty());
    assert_eq!(failed[0].not_run.as_deref(), Some(COULD_NOT));
    match &h.delivered_to("agent-a")[0].decided {
        Decided::Failed { failed, .. } => assert_eq!(failed, &[CHECK]),
        other => panic!("delivered {other:?}"),
    }
    assert!(results(&h).iter().all(|r| r.task != DEP));
}

/// satisfies: 149, 114; the failed-dependency spec's "One failed and one
/// could not run"
#[test]
fn a_failed_dependency_outweighs_one_that_could_not_run() {
    let h = rig(&integration("fail"));
    h.w().task("ui:integration", "b/");
    h.f.tests.depends_on(CHECK, "ui:integration");
    h.fails_with(DEP, "a/lib", "broken");
    h.w().cannot_run("ui:integration", Some("exited 77"));
    let one = submit_a(&h, "broken");
    h.settle();
    let sub = h.sub(one);
    assert_eq!(sub.state, State::Failed);
    assert_eq!(h.w().started().len(), 1, "a fail is a verdict");
    let names: Vec<&str> = sub.failed.iter().map(|f| f.task.as_str()).collect();
    assert_eq!(names, [DEP, CHECK]);
    assert_eq!(sub.failed[1].failed_by, [DEP]);
    assert!(sub.failed.iter().all(|f| f.not_run.is_none()));
    assert!(results(&h).iter().all(|r| r.task != "ui:integration"));
}

/// The rig's sluice, signing as `box`.
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

/// satisfies: 149, 16, 78, 79, C.6; rules core.9, core.22; the
/// failed-dependency spec's "A signed branch"
#[test]
fn a_signed_branch_counts_the_dependency_through_its_signer() {
    let p = integration("fail").replace(
        "trust = [\"open\", \"sandbox\", \"container\"]\n",
        "trust = [\"box\"]\nremote = \"origin\"\n",
    ) + "\n[signers.box]\nssh = \"box ssh-ed25519 AAAA\"\n";
    let h = rig(&p);
    h.w().signs_as("box");
    h.fails_with(DEP, "a/lib", "broken");
    let one = submit_a(&h, "broken");
    for _ in 0..8 {
        signer(&h).step(BRANCH).unwrap();
    }
    let sub = h.sub(one);
    assert_eq!(sub.state, State::Failed);
    assert_eq!(sub.failed[0].task, DEP);
    assert_eq!(sub.failed[1].failed_by, [DEP]);
    let signed: Vec<ResultLine> = h.results().into_iter().filter(|r| r.task == DEP).collect();
    assert_eq!(signed.len(), 1, "{signed:?}");
    assert_eq!(signed[0].outcome, Outcome::Fail);
    assert_eq!(signed[0].hash, sub.failed[0].hash);
    assert_eq!(signed[0].signer.as_deref(), Some("box"));
}

/// satisfies: 149, 114, 139; records 0072 and 0084; rule core.35; the
/// run-outcome spec's "A test failed by a task the run was refused"
#[test]
fn a_refusal_leaves_a_check_failed_by_a_dependency_unjudged() {
    const REFUSED: &str = "sluice's sandbox refused the run file-write-create \
                           /Users/me/.cache/x (by python3)";
    let h = rig(&integration("fail"));
    h.fails_with(DEP, "a/lib", "broken");
    h.w().refuses(Some(REFUSED));
    let one = submit_a(&h, "broken");
    for _ in 0..8 {
        if !not_run_events(&h).is_empty() {
            break;
        }
        h.step();
    }
    assert_eq!(
        not_run_events(&h),
        [vec![(CHECK.to_string(), REFUSED.to_string())]]
    );
    assert!(results(&h).iter().all(|r| r.task != CHECK && r.task != DEP));
    assert!(!h.events().iter().any(|e| matches!(
        e,
        Event::Ran { task, .. } if task == DEP
    )));
    assert_ne!(h.sub(one).state, State::Failed);
    assert!(h.w().delivered().is_empty(), "nobody is blamed");
}
