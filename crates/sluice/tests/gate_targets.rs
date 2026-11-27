//! Each task of `crates/sluice/moon.yml` that builds sluice's binary
//! builds it in a target folder of its own (rule core.24; the change
//! `each-gate-task-builds-in-its-own-target`). cargo writes
//! `target/debug/sluice` again at each build, a build with nothing to do
//! among them, and a gate runs its tasks at once in one checkout. A test
//! that runs the binary while another task's cargo writes it finds no file
//! there (#183 on main, 2026-11-27: `protect`'s hook could not start
//! `sluice host` from `target/debug/sluice`, os error 2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use yaml_rust2::YamlLoader;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The target folder that each task building sluice's binary builds in,
/// by task: its `CARGO_TARGET_DIR`, or cargo's `target`.
fn targets() -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(root().join("crates/sluice/moon.yml")).unwrap();
    let docs = YamlLoader::load_from_str(&text).unwrap();
    let tasks = docs[0]["tasks"].as_hash().expect("its tasks");
    let mut out = BTreeMap::new();
    for (name, task) in tasks {
        let name = name.as_str().unwrap().to_string();
        let command = task["command"].as_str().unwrap_or_default();
        let builds =
            command.contains("cargo nextest run -p sluice ") || command.contains("e2e/check.sh");
        if !builds {
            continue;
        }
        let target = task["env"]["CARGO_TARGET_DIR"]
            .as_str()
            .unwrap_or("target")
            .to_string();
        out.insert(name, target);
    }
    out
}

#[test]
fn each_task_that_builds_sluice_has_a_target_folder_of_its_own() {
    let targets = targets();
    for task in ["test", "tools", "e2e"] {
        assert!(
            targets.contains_key(task),
            "{task} builds sluice: {targets:?}"
        );
    }
    let mut by_target: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (task, target) in &targets {
        by_target.entry(target).or_default().push(task);
    }
    let shared: Vec<_> = by_target.iter().filter(|(_, t)| t.len() > 1).collect();
    assert!(
        shared.is_empty(),
        "tasks that share a target folder: {shared:?}"
    );
}

/// The e2e check runs the sluice it built, from the target folder its task
/// names.
#[test]
fn the_e2e_check_runs_the_sluice_of_its_own_target_folder() {
    let script = std::fs::read_to_string(root().join("e2e/check.sh")).unwrap();
    assert!(!script.contains("$PWD/target/"), "{script}");
    assert!(script.contains("${CARGO_TARGET_DIR:?"), "{script}");
}
