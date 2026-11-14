//! The records and the binding of seams: B.6 and C.6.

use std::path::Path;

use sluice_conformance::{shared, Harness, Rig, BRANCH};
use sluice_core::line::Line;
use sluice_core::records::State;
use sluice_seams::Snapshot;

fn examples() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/state/bolt/concurrent-units")
}

/// satisfies: 53, 26
#[test]
fn the_walkthrough_records_are_read_as_written() {
    let dir = examples();
    let mut files = std::collections::BTreeMap::new();
    for name in ["12", "13"] {
        let path = format!("submissions/{name}.json");
        files.insert(path.clone(), std::fs::read(dir.join(&path)).unwrap());
    }
    files.insert(
        "log.jsonl".into(),
        std::fs::read(dir.join("log.jsonl")).unwrap(),
    );
    let snap = Snapshot { version: 57, files };
    let line = Line::load("bolt/concurrent-units", snap.clone()).unwrap();
    assert_eq!(line.subs[&12].state, State::Landed);
    assert_eq!(line.subs[&13].state, State::Failed);
    assert_eq!(line.subs[&13].failed[0].task, "tests:t-batch-cards");
    assert_eq!(Line::read_log(&snap).unwrap().len(), 10);
    let version = std::fs::read_to_string(dir.join("version")).unwrap();
    assert_eq!(version.trim(), "57");
}

/// satisfies: 53, 26, 49
#[test]
fn every_record_written_is_the_real_format_and_names_its_schema() {
    let h = Harness::bolt();
    shared::every_record_written_is_the_real_format_and_names_its_schema(&h);
    // The version is a file of its own, as local files keep it.
    let version: u64 = String::from_utf8(h.w().files(BRANCH)["version"].clone())
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(version, h.version());
    // The fakes finish every run in the step that starts it, so #1 + #2
    // is recorded too.
    assert_eq!(h.results().len(), 3 + 3 + 3);
}

/// satisfies: 52, 27
#[test]
fn the_core_gives_the_profiles_a_policy_names_no_meaning_of_its_own() {
    // The same queue under a policy naming other profiles behaves the
    // same: the profiles are the binary's to bind (52).
    let mut outcomes = Vec::new();
    for (runner, method) in [("fake", "fake"), ("another", "elsewhere")] {
        let policy = sluice_conformance::policy(false, "fail", 4)
            .replace("runner = \"fake\"", &format!("runner = \"{runner}\""))
            .replace(
                "[checkout]\n",
                &format!("[checkout]\nmethod = \"{method}\"\n"),
            );
        let h = Harness::new(&policy);
        h.work("unit/a", "agent-a", &[("a/lib", Some("a2"))]);
        let one = h.submit("unit/a");
        h.settle();
        outcomes.push((h.sub(one).state, h.tested()));
    }
    assert_eq!(outcomes[0], outcomes[1]);
}
