//! `sluice equip` over the fakes (147, the command): the policy it reads,
//! its refusals and exits, both forms of its answer, and what the check
//! seam was asked.

mod support;

use clap::Parser;
use sluice::{run, Cli, Pause, CONFLICT, FAILED, LANDED};
use sluice_fakes::Fakes;
use sluice_seams::{EquipStep, Setting, Tests, Through};

const POLICY: &str = "[tests]\ntoolchain = \"devenv print-dev-env\"\n\n\
[branches.main]\nrequired = [\"#gate\"]\n";

/// A pause that fails the test: an equip never waits.
struct Never;

impl Pause for Never {
    fn pause(&self) -> bool {
        panic!("sluice equip waited");
    }
}

fn setup() -> Fakes {
    let f = Fakes::new();
    f.world.init(
        "main",
        &[(".config/sluice.toml", POLICY), ("app/a.rs", "a")],
    );
    f
}

fn sluice(f: &Fakes, args: &[&str]) -> (i32, String) {
    let cli = Cli::try_parse_from(std::iter::once("sluice").chain(args.iter().copied())).unwrap();
    let mut out = Vec::new();
    let host = support::FakeHost::new(f.world.clone(), 0);
    let code = run(cli, f.seams(), &host, &(), &(), &f.clock, &Never, &mut out).unwrap();
    (code, String::from_utf8(out).unwrap())
}

fn step(name: &str, passed: bool, words: &str) -> EquipStep {
    EquipStep {
        step: name.into(),
        passed,
        words: words.into(),
    }
}

/// The steps of a moon repository whose setup fails.
fn setup_fails() -> Vec<EquipStep> {
    vec![
        step("toolchain", true, ""),
        step("proto install", true, ""),
        step("moon --version", true, "moon 2.6.1"),
        step("moon setup", false, "exited 1: error: no such toolchain"),
    ]
}

/// The toolchain the one equip asked was given, through the setup.
fn asked_toolchain(f: &Fakes) -> Setting {
    let equips = f.tests.equips();
    assert_eq!(equips.len(), 1, "{equips:?}");
    assert_eq!(equips[0].through, Through::Running);
    equips[0].tests["toolchain"].clone()
}

fn json(out: &str) -> serde_json::Value {
    serde_json::from_str(out).unwrap_or_else(|e| panic!("{e}: {out}"))
}

/// 147, the command: every step passed under `--json` is one document
/// naming each step passed, and exits 0.
#[test]
fn every_step_passed_exits_0() {
    let f = setup();
    f.tests.equip_answers(&[
        step("toolchain", true, ""),
        step("moon --version", true, "moon 2.6.1"),
        step("moon setup", true, ""),
    ]);
    let (code, out) = sluice(&f, &["equip", "main", "--json"]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(out.lines().count(), 1, "{out}");
    let doc = json(&out);
    assert_eq!(doc["branch"], "main");
    assert_eq!(doc["revision"], f.world.tip("main").as_str());
    let steps = doc["steps"].as_array().unwrap();
    let names: Vec<&str> = steps.iter().map(|s| s["step"].as_str().unwrap()).collect();
    assert_eq!(names, ["toolchain", "moon --version", "moon setup"]);
    assert!(steps.iter().all(|s| s["passed"] == true), "{doc}");
    assert_eq!(steps[1]["words"], "moon 2.6.1");
}

/// 147, the command: a step that failed is the last of the document,
/// with its words, and the command exits 1.
#[test]
fn a_failed_step_exits_1() {
    let f = setup();
    f.tests.equip_answers(&setup_fails());
    let (code, out) = sluice(&f, &["equip", "main", "--json"]);
    assert_eq!(code, FAILED, "{out}");
    let doc = json(&out);
    assert_eq!(
        doc["steps"],
        serde_json::json!([
            {"step": "toolchain", "passed": true, "words": ""},
            {"step": "proto install", "passed": true, "words": ""},
            {"step": "moon --version", "passed": true, "words": "moon 2.6.1"},
            {"step": "moon setup", "passed": false, "words": "exited 1: error: no such toolchain"},
        ])
    );
}

/// 147, the command: for a person, what is equipped, each step and its
/// outcome, and a failed step's words indented beneath it.
#[test]
fn a_failed_step_is_printed_with_its_words() {
    let f = setup();
    f.tests.equip_answers(&setup_fails());
    let (code, out) = sluice(&f, &["equip", "main"]);
    assert_eq!(code, FAILED, "{out}");
    assert_eq!(
        out,
        format!(
            "equipping . for main at {}\n\
             toolchain: passed\n\
             proto install: passed\n\
             moon --version: passed\n\
             moon setup: failed\n  \
             exited 1: error: no such toolchain\n",
            f.world.tip("main").short()
        )
    );
}

/// 147, 56, the command: `--at` reads the policy at the revision named,
/// not at the branch's, and the document names that revision in full.
#[test]
fn equip_reads_the_policy_at_the_revision_named() {
    let f = setup();
    let earlier = f.world.tip("main");
    f.world.hand_commit(
        "main",
        &[(
            ".config/sluice.toml",
            Some("[tests]\ntoolchain = \"nix print-dev-env\"\n\n[branches.main]\n"),
        )],
    );
    let (code, out) = sluice(&f, &["equip", "main", "--at", earlier.as_str(), "--json"]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(json(&out)["revision"], earlier.as_str());
    assert_eq!(
        asked_toolchain(&f),
        Setting::Text("devenv print-dev-env".into())
    );
}

/// 147, the command: under `--at`, a revision whose policy names no
/// `[branches]` table is equipped by its `[tests]` all the same.
#[test]
fn equip_at_a_revision_equips_whatever_its_branches_name() {
    let f = setup();
    let base = f.world.work(
        &f.world.tip("main"),
        &[(
            ".config/sluice.toml",
            Some("[tests]\ntoolchain = \"nix print-dev-env\"\n"),
        )],
    );
    let (code, out) = sluice(&f, &["equip", "main", "--at", base.as_str()]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(
        asked_toolchain(&f),
        Setting::Text("nix print-dev-env".into())
    );
}

/// 147, 56, the command: with no `--at`, the policy at the branch's
/// current revision.
#[test]
fn equip_reads_the_branchs_policy() {
    let f = setup();
    let tip = f.world.hand_commit(
        "main",
        &[(
            ".config/sluice.toml",
            Some("[tests]\ntoolchain = \"nix print-dev-env\"\n\n[branches.main]\n"),
        )],
    );
    let (code, out) = sluice(&f, &["equip", "main", "--json"]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(json(&out)["revision"], tip.as_str());
    assert_eq!(
        asked_toolchain(&f),
        Setting::Text("nix print-dev-env".into())
    );
}

/// 147, the command: a branch it cannot read is refused with no steps,
/// nothing asked, exiting 2.
#[test]
fn equip_refuses_a_branch_it_cannot_read() {
    let f = setup();
    let (code, out) = sluice(&f, &["equip", "nosuch", "--json"]);
    assert_eq!(code, CONFLICT, "{out}");
    let doc = json(&out);
    assert_eq!(doc["error"]["kind"], "refused", "{doc}");
    assert!(
        doc["error"]["message"].as_str().unwrap().contains("nosuch"),
        "{doc}"
    );
    assert!(doc.get("steps").is_none(), "{doc}");
    assert!(f.tests.equips().is_empty());
}

/// 147, the command: with no `--at`, a branch whose policy names no
/// `[branches]` table for it is refused, naming the policy, the revision
/// and the branch.
#[test]
fn equip_refuses_a_branch_its_policy_does_not_manage() {
    let f = setup();
    let tip = f.world.hand_commit(
        "main",
        &[(
            ".config/sluice.toml",
            Some("[tests]\ntoolchain = \"nix print-dev-env\"\n\n[branches.release]\n"),
        )],
    );
    let (code, out) = sluice(&f, &["equip", "main"]);
    assert_eq!(code, CONFLICT, "{out}");
    assert_eq!(
        out,
        format!(
            "refused: no [branches] table in .config/sluice.toml at {} names main\n",
            tip.short()
        )
    );
    assert!(f.tests.equips().is_empty());
}

/// 147, the command: a revision with no policy is refused, naming the
/// policy and the revision, nothing asked.
#[test]
fn equip_refuses_a_revision_with_no_policy() {
    let f = setup();
    let bare = f
        .world
        .work(&f.world.tip("main"), &[(".config/sluice.toml", None)]);
    let (code, out) = sluice(&f, &["equip", "main", "--at", bare.as_str(), "--json"]);
    assert_eq!(code, CONFLICT, "{out}");
    let doc = json(&out);
    assert_eq!(doc["error"]["kind"], "refused", "{doc}");
    assert_eq!(
        doc["error"]["message"],
        format!("no .config/sluice.toml at {}", bare.short())
    );
    assert!(doc.get("steps").is_none(), "{doc}");
    assert!(f.tests.equips().is_empty());
}

/// 147, 71, the command: with the host's bound full, an equip runs at
/// once and the count of running checks is unchanged by it; it makes and
/// adopts no place.
#[test]
fn equip_takes_no_part_of_the_bound() {
    let f = setup();
    f.world.host_limit(1);
    f.world.session_run("/w/other", true);
    f.tests.equip_answers(&[step("toolchain", true, "")]);
    let before = f.tests.running().unwrap();
    let (code, out) = sluice(&f, &["equip", "main"]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(f.tests.equips().len(), 1);
    assert_eq!(f.tests.running().unwrap(), before);
    assert!(f.world.started().is_empty());
    assert!(f.world.made_places().is_empty());
    assert!(f.world.adopted_places().is_empty());
}
