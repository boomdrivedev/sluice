//! The CLI over the fakes: the walkthrough's output and exits.

mod support;

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use clap::Parser;
use sluice::{run, Cli, Pause, CONFLICT, FAILED, LANDED};
use sluice_core::policy::POLICY_PATH;
use sluice_fakes::Fakes;
use sluice_seams::{Changes, Outcome, Rev, State};
use support::soon;

const BRANCH: &str = "bolt/concurrent-units";

const POLICY: &str = "[branches.\"bolt/*\"]\nrequired = [\"#gate\"]\nparallel = 4\ntrust = [\"open\", \"sandbox\", \"container\"]\n";

struct Rounds(Cell<usize>);

impl Pause for Rounds {
    fn pause(&self) -> bool {
        self.0.set(self.0.get() + 1);
        assert!(self.0.get() < 50, "waited too long");
        true
    }
}

fn setup() -> Fakes {
    let f = Fakes::new();
    f.world.init(
        BRANCH,
        &[
            (".config/sluice.toml", POLICY),
            ("plugin/lib/plan.py", "plan"),
            ("tests/t_cards.py", "cards"),
        ],
    );
    f.world.task("tests:t-batch-cards", "tests/");
    f.world.task("plugin:test", "plugin/");
    f
}

fn sluice(f: &Fakes, args: &[&str]) -> (i32, String) {
    let cli = Cli::try_parse_from(std::iter::once("sluice").chain(args.iter().copied())).unwrap();
    let mut out = Vec::new();
    let code = settled(|| {
        run(
            cli,
            f.seams(),
            &support::FakeHost::new(f.world.clone(), 0),
            &(),
            &(),
            &f.clock,
            &Rounds(Cell::new(0)),
            &mut out,
        )
    })
    .unwrap();
    (code, String::from_utf8(out).unwrap())
}

/// `command`, a sluice run in this process, and then a moment for each
/// lock of the host's reader and driver it found free to be free again.
/// A lock a command lets go may still be held by a process another
/// thread of this binary starts meanwhile, until that process runs its
/// program, where the next command of a test, as another process on a
/// host, finds it free (129, 135).
fn settled<T>(command: impl FnOnce() -> T) -> T {
    let before = host_locks_free();
    let done = command();
    for (i, free) in before.into_iter().enumerate() {
        if free {
            soon(|| host_locks_free()[i]);
        }
    }
    done
}

/// Whether the repository's reader's lock, its lease and its driver's
/// lock are each free on the fakes' host: no process holds it alone. A
/// file not yet made is free. Asked shared, as the reader probes, so the
/// probe a process started meanwhile still holds reads as no lease.
fn host_locks_free() -> [bool; 3] {
    let free = |ext: &str| {
        let Ok(file) = std::fs::File::open(readers_file(REPOSITORY, ext)) else {
            return true;
        };
        !matches!(
            file.try_lock_shared(),
            Err(std::fs::TryLockError::WouldBlock)
        )
    };
    [free("lock"), free("lease"), free("driver")]
}

/// What a waiting command says while it waits, a submission's state or
/// its combination's progress (ADR 0042), left out where a test reads
/// the outcome it ends with.
fn outcome(out: &str) -> Vec<&str> {
    out.lines()
        .filter(|l| {
            !(l.starts_with('#') || l.starts_with("testing ") && !l.contains(" tasks to run, "))
        })
        .collect()
}

fn work(f: &Fakes, worktree: &str, agent: &str, changes: &[(&str, Option<&str>)]) {
    let head = f.world.work(&f.world.tip(BRANCH), changes);
    f.world.session(worktree, agent, &head);
}

#[test]
fn submit_waits_and_lands() {
    let f = setup();
    work(
        &f,
        "unit/touches",
        "crw-1-unit-1",
        &[("plugin/lib/plan.py", Some("touched"))],
    );
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/touches"]);
    let tip = f.world.tip(BRANCH);
    assert_eq!(code, LANDED);
    let lines: Vec<&str> = out.lines().collect();
    // ADR 0042: while it waits, its combination and how long it has run.
    assert!(
        lines[1].starts_with(&format!("testing {BRANCH} + #1 · "))
            && lines[1].ends_with(" of 1h00m"),
        "{out}"
    );
    assert_eq!(
        [&lines[..1], &lines[2..]].concat(),
        [
            format!("submission 1 to {BRANCH} · nothing queued ahead"),
            format!("testing {BRANCH} + #1 · 2 tasks to run, 0 replayed"),
            format!("passed · landed as {}", &tip.0[..7]),
        ]
    );
}

#[test]
fn submit_fails_and_names_the_combination() {
    let f = setup();
    f.world
        .fails_with("tests:t-batch-cards", "tests/t_cards.py", "broken");
    work(
        &f,
        "unit/touches",
        "crw-1-unit-1",
        &[("plugin/lib/plan.py", Some("touched"))],
    );
    // Behind #1, so tested on top of it.
    work(
        &f,
        "unit/batch-review",
        "crw-1-unit-2",
        &[
            ("tests/t_cards.py", Some("broken")),
            ("plugin/notes", Some("n")),
        ],
    );
    let (_, _) = sluice(
        &f,
        &["submit", BRANCH, "--worktree", "unit/touches", "--no-wait"],
    );
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/batch-review"]);
    assert_eq!(code, FAILED);
    let lines = outcome(&out);
    assert_eq!(
        lines[0],
        format!("submission 2 to {BRANCH} · 1 queued ahead (#1)")
    );
    assert!(lines[1].starts_with(&format!("testing {BRANCH} + #1 + #2 · ")));
    assert_eq!(lines[2], "failed: tests:t-batch-cards");
    assert!(lines[3].starts_with(&format!("  on {BRANCH} + #1 + #2 (")));
    assert_eq!(
        lines[4],
        "  #1 passed on its own, so the failure is from this submission"
    );
    // 20: where the failed task's output is kept, and what it holds.
    let at = lines[5]
        .strip_prefix("  log of tests:t-batch-cards: ")
        .unwrap_or_else(|| panic!("{}", lines[5]));
    assert!(f
        .world
        .kept_log(at)
        .unwrap()
        .contains("tests:t-batch-cards failed"));
    assert_eq!(
        lines[6],
        "the agent working in unit/batch-review fixes it there and submits again"
    );
}

/// Could-not-run-1 115: a check the host could not run is blamed on no
/// one, and its way on names the agent working in the worktree.
#[test]
fn a_check_the_host_could_not_run_is_submitted_again_by_the_agent_in_its_worktree() {
    let f = setup();
    f.world
        .cannot_run("tests:t-batch-cards", Some("no emulator on this host"));
    work(&f, "unit/a", "agent", &[("tests/t_cards.py", Some("x"))]);
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, FAILED, "{out}");
    assert_eq!(
        out.lines().last(),
        Some(
            "the host could not run tests:t-batch-cards: once it can, the agent working in \
             unit/a submits again"
        ),
        "{out}"
    );
}

/// 85, 86: a failure on a system the gate names says which system.
#[test]
fn a_failure_names_its_system() {
    let f = Fakes::new();
    f.world.init(
        BRANCH,
        &[
            (
                ".config/sluice.toml",
                &format!(
                    "[systems]\nlinux-x64 = {{ os = \"linux\", arch = \"x86_64\" }}\n\n{}\
                     systems = [\"linux-x64\"]\n",
                    POLICY.replace(
                        "required = [\"#gate\"]",
                        "required = [\"tests:t-batch-cards\"]"
                    )
                ),
            ),
            ("tests/t_cards.py", "cards"),
        ],
    );
    f.world.task("tests:t-batch-cards", "tests/");
    f.world
        .fails_with("tests:t-batch-cards", "tests/t_cards.py", "broken");
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("tests/t_cards.py", Some("broken"))],
    );
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, FAILED);
    assert!(
        out.contains("failed: tests:t-batch-cards (linux-x64)\n"),
        "{out}"
    );
    assert!(
        out.contains("  log of tests:t-batch-cards (linux-x64): "),
        "{out}"
    );
    let (_, out) = sluice(&f, &["status", BRANCH, "--all"]);
    assert!(out.contains("tests:t-batch-cards (linux-x64) on "), "{out}");
    let (_, out) = sluice(&f, &["status", BRANCH, "--all", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["submissions"][0]["failed"][0]["system"], "linux-x64");
}

/// 19, 20: a test read as failed only because its inputs failed before
/// says it is flaky: that it passed in this run, when and on which
/// combination the same inputs failed, how the branch reads that, and
/// where both runs' logs are kept; and that the same head lands once the
/// operator sets the fail aside, or a real fix changes what the test
/// reads, never by a change to its inputs only to have it tested again.
/// `--json` carries the same, with when and on which host each run was,
/// and so does the overview's combination while it is listed.
#[test]
fn a_flaky_failure_says_so() {
    let f = setup();
    f.world
        .flaky("tests:t-batch-cards", &[Outcome::Fail, Outcome::Pass]);
    work(
        &f,
        "unit/batch-review",
        "crw-1-unit-2",
        &[("tests/t_cards.py", Some("flaky"))],
    );
    let (code, first) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/batch-review"]);
    assert_eq!(code, FAILED, "{first}");
    let line = |out: &str, prefix: &str| -> String {
        out.lines()
            .find_map(|l| l.strip_prefix(prefix))
            .unwrap_or_else(|| panic!("no {prefix:?} in {out}"))
            .to_string()
    };
    let failed_on = line(&first, &format!("  on {BRANCH} + #1 ("));
    let failed_on = failed_on.trim_end_matches(')');
    let failed_log = line(&first, "  log of tests:t-batch-cards: ");

    // The same head again: the test runs, passes, and its key now holds
    // a pass and a fail, which the default policy reads as failed.
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/batch-review"]);
    assert_eq!(code, FAILED, "{out}");
    let (_, json) = sluice(&f, &["status", "2", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let flaky = &v["failed"][0]["flaky"];
    assert_eq!(flaky["here"], "pass", "{json}");
    // When and where each run was, from the lines recorded before the
    // verdict: this run's `ran` line, and the run it contradicts.
    let last_ran = |f: &Fakes| {
        let log = String::from_utf8(f.world.files(BRANCH)["log.jsonl"].clone()).unwrap();
        log.lines()
            .rev()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .find(|l| l["event"] == "ran" && l["task"] == "tests:t-batch-cards")
            .unwrap()
    };
    let ran = last_ran(&f);
    assert_eq!(ran["outcome"], "pass", "{ran}");
    assert_eq!(flaky["at"], ran["at"], "{json}");
    assert_eq!(flaky["host"], "fake-host", "{json}");
    assert_eq!(flaky["other"]["host"], "fake-host", "{json}");
    assert_eq!(flaky["other"]["outcome"], "fail", "{json}");
    assert_eq!(flaky["other"]["log"], failed_log.as_str(), "{json}");
    assert!(
        flaky["other"]["combination"]
            .as_str()
            .unwrap()
            .starts_with(failed_on),
        "{json}"
    );
    let (_, status) = sluice(&f, &["status", BRANCH, "--all"]);
    assert!(
        status.contains(&format!("tests:t-batch-cards (flaky) on {failed_on}")),
        "{status}"
    );
    let at = flaky["other"]["at"].as_str().unwrap();
    let this_log = v["failed"][0]["log"].as_str().unwrap();

    let lines = outcome(&out);
    assert_eq!(
        lines[0],
        format!("submission 2 to {BRANCH} · nothing queued ahead")
    );
    // The same combination as #1's: only this test of it is counted.
    assert_eq!(
        lines[1],
        format!("testing {BRANCH} + #2 · 1 tasks to run, 1 replayed")
    );
    assert_eq!(lines[2], "failed: tests:t-batch-cards (flaky)");
    assert!(lines[3].starts_with(&format!("  on {BRANCH} + #2 (")));
    assert_eq!(
        lines[4..],
        [
            format!(
                "  tests:t-batch-cards passed in this run and failed on the same inputs \
                 at {at} on {failed_on}"
            ),
            format!("  log of this run: {this_log}"),
            format!("  log of the run that failed: {failed_log}"),
            format!(
                "  if the host or a time limit caused that fail, the operator can set it aside: \
                 sluice set-aside tests:t-batch-cards --at {at} --reason \"…\""
            ),
            format!(
                "  {BRANCH} reads a test that passed and failed on the same inputs as failed \
                 (flaky = \"fail\")"
            ),
            "the same head lands once the operator sets the other result aside, or once a \
             real fix changes what tests:t-batch-cards reads"
                .to_string(),
        ],
        "{out}"
    );

    // A combination whose failure waits on one ahead of it to be blamed
    // (18) is listed in the overview, with when and where each run was:
    // on a branch whose window has room for both.
    let f = setup();
    f.world
        .flaky("tests:t-batch-cards", &[Outcome::Fail, Outcome::Pass]);
    work(
        &f,
        "unit/batch-review",
        "crw-1-unit-2",
        &[("tests/t_cards.py", Some("flaky"))],
    );
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/batch-review"]);
    assert_eq!(code, FAILED, "{out}");
    f.world.hold_runs(true);
    work(
        &f,
        "unit/ahead",
        "crw-1-unit-1",
        &[("plugin/ahead", Some("a"))],
    );
    sluice(
        &f,
        &["submit", BRANCH, "--worktree", "unit/ahead", "--no-wait"],
    );
    sluice(
        &f,
        &[
            "submit",
            BRANCH,
            "--worktree",
            "unit/batch-review",
            "--no-wait",
        ],
    );
    sluice(&f, &["step", BRANCH]);
    f.world.complete(&tested(&f, 3));
    sluice(&f, &["step", BRANCH]);
    let (_, ov) = sluice(&f, &["overview", "--json"]);
    let ov: serde_json::Value = serde_json::from_str(&ov).unwrap();
    let c = ov["branches"][0]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["submissions"] == serde_json::json!([2, 3]))
        .unwrap_or_else(|| panic!("{ov:#}"));
    let listed = &c["failed"][0]["flaky"];
    assert_eq!(c["state"], "failed", "{c:#}");
    assert_eq!(listed["here"], "pass", "{c:#}");
    assert_eq!(listed["at"], last_ran(&f)["at"], "{c:#}");
    assert_eq!(listed["host"], "fake-host", "{c:#}");
    assert_eq!(listed["other"]["host"], "fake-host", "{c:#}");
}

/// Time-limit-1 66: a submission whose run its time limit ended names the
/// test the limit cut off, with its log, says nothing was recorded for it
/// and gives the way on; the same head lands once the test ends in time,
/// with nothing under its key to read as flaky.
#[test]
fn a_timed_out_submission_names_the_test_the_limit_ended() {
    let f = setup();
    f.world.hangs("tests:t-batch-cards");
    work(
        &f,
        "unit/batch-review",
        "crw-1-unit-2",
        &[("tests/t_cards.py", Some("slow"))],
    );
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/batch-review"]);
    assert_eq!(code, FAILED, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert!(
        lines.contains(
            &"timed out: tests:t-batch-cards was running when the time limit ended the run"
        ),
        "{out}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("  log of tests:t-batch-cards: ")),
        "{out}"
    );
    assert_eq!(
        lines.last().copied(),
        Some(
            "nothing was recorded for tests:t-batch-cards: if the host was slow, the agent \
             working in unit/batch-review submits the same head again; if tests:t-batch-cards \
             never ends, it fixes it there and submits again"
        ),
        "{out}"
    );
    assert!(!out.contains("not run:"), "{out}");

    f.world.ends_in_time("tests:t-batch-cards");
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/batch-review"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("flaky"), "{out}");
}

/// Could-not-run-1 116: the operator sets aside the fail a host caused;
/// the same head then lands, and asking again for it is refused.
#[test]
fn a_fail_set_aside_lets_the_same_head_land() {
    let f = setup();
    f.world
        .flaky("tests:t-batch-cards", &[Outcome::Fail, Outcome::Pass]);
    work(
        &f,
        "unit/batch-review",
        "crw-1-unit-2",
        &[("tests/t_cards.py", Some("flaky"))],
    );
    let submit = ["submit", BRANCH, "--worktree", "unit/batch-review"];
    assert_eq!(sluice(&f, &submit).0, FAILED);
    assert_eq!(sluice(&f, &submit).0, FAILED, "the key reads flaky");
    let (_, json) = sluice(&f, &["status", "2", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let at = v["failed"][0]["flaky"]["other"]["at"]
        .as_str()
        .unwrap()
        .to_string();

    let aside = [
        "set-aside",
        "tests:t-batch-cards",
        "--at",
        &at,
        "--reason",
        "the engine was down",
    ];
    let (code, out) = sluice(&f, &aside);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with("set aside tests:t-batch-cards "), "{out}");
    assert!(
        out.contains(&format!("recorded at {at} on {BRANCH} ")),
        "{out}"
    );
    assert!(
        out.ends_with(
            ": the engine was down\nno gate reads it from now; its line stays in the results\n"
        ),
        "{out}"
    );
    let (code, out) = sluice(&f, &aside);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("already set aside"), "{out}");

    let (code, out) = sluice(&f, &submit);
    assert_eq!(code, LANDED, "{out}");
}

#[test]
fn a_conflict_exits_two_and_check_says_so_first() {
    let f = setup();
    work(
        &f,
        "unit/touches",
        "crw-1-unit-1",
        &[("plugin/lib/plan.py", Some("one"))],
    );
    sluice(
        &f,
        &["submit", BRANCH, "--worktree", "unit/touches", "--no-wait"],
    );
    work(
        &f,
        "unit/forecast",
        "crw-1-unit-3",
        &[("plugin/lib/plan.py", Some("two"))],
    );

    let (code, out) = sluice(
        &f,
        &["submit", "--check", BRANCH, "--worktree", "unit/forecast"],
    );
    assert_eq!(code, CONFLICT);
    assert_eq!(
        out,
        "would conflict: plugin/lib/plan.py with #1 (crw-1-unit-1, unit/touches)\n"
    );

    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/forecast"]);
    assert_eq!(code, CONFLICT);
    assert_eq!(
        out,
        format!(
            "submission 2 to {BRANCH} · 1 queued ahead (#1)\n\
             conflict: plugin/lib/plan.py\n  conflicts with #1 (crw-1-unit-1, unit/touches)\n\
             the agent working in unit/forecast merges or rebases onto {BRANCH} there and \
             submits again\n"
        )
    );
}

/// `--check --head` forecasts a branch no worktree holds, a bolt's, and
/// `--json` says what it was formed from: the head, the branch's revision
/// and the submissions ahead (64, 65, 47a).
#[test]
fn check_forecasts_a_branch_with_no_worktree() {
    let f = setup();
    work(
        &f,
        "unit/touches",
        "crw-1-unit-1",
        &[("plugin/lib/plan.py", Some("one"))],
    );
    sluice(
        &f,
        &["submit", BRANCH, "--worktree", "unit/touches", "--no-wait"],
    );
    let tip = f.world.tip(BRANCH);
    let bolt = f.world.work(&tip, &[("plugin/lib/plan.py", Some("two"))]);
    f.world.set_branch("bolt/y", &bolt);

    let (code, out) = sluice(&f, &["submit", BRANCH, "--check", "--head", "bolt/y"]);
    assert_eq!(code, CONFLICT);
    assert_eq!(
        out,
        "would conflict: plugin/lib/plan.py with #1 (crw-1-unit-1, unit/touches)\n"
    );

    let (code, out) = sluice(
        &f,
        &["submit", BRANCH, "--check", "--head", "bolt/y", "--json"],
    );
    assert_eq!(code, CONFLICT);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v["conflict"]["paths"],
        serde_json::json!(["plugin/lib/plan.py"])
    );
    assert_eq!(v["conflict"]["with"][0]["id"], 1);
    assert_eq!(v["head"], bolt.as_str());
    assert_eq!(v["onto"], tip.as_str());
    assert_eq!(v["ahead"], serde_json::json!([1]));

    let (code, out) = sluice(
        &f,
        &["submit", BRANCH, "--check", "--head", "bolt/none", "--json"],
    );
    assert_eq!(code, CONFLICT);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["error"]["message"], "no branch bolt/none to forecast");
}

/// 64, 129: a live reader of the fakes' repository, its tick `tick`
/// seconds, whose read just ended found `BRANCH` at `R`, a revision on the
/// host the branch is not at, with `#4` queued on it, `#1`'s work; while
/// the records hold `#1` queued on the branch's tip. The reader, its
/// document, `R`, and `#1`'s head.
fn reader_of_four(f: &Fakes, tick: u64) -> (sluice::reader::Lead, serde_json::Value, String, Rev) {
    let tip = f.world.tip(BRANCH);
    let one = f.world.work(&tip, &[("plugin/lib/plan.py", Some("one"))]);
    f.world.session("unit/touches", "crw-1-unit-1", &one);
    sluice(
        f,
        &["submit", BRANCH, "--worktree", "unit/touches", "--no-wait"],
    );
    let (_, out) = sluice(f, &["overview", "--json"]);
    let mut doc = json(&out);
    let r = f.world.work(&tip, &[("plugin/r", Some("r"))]);
    let b = &mut doc["branches"][0];
    b["revision"] = r.as_str().into();
    b["submissions"][0]["id"] = 4.into();
    doc["tick"] = tick.into();
    let lead = reader_of(REPOSITORY, &doc);
    (lead, doc, r.as_str().to_string(), one)
}

/// Set the reader's marks: its last read ended `ended` seconds before
/// now, and its next began `began` seconds before now.
fn mark_reader(ended: u64, began: u64) {
    let now = std::time::SystemTime::now();
    for (ext, ago) in [("json", ended), ("lock", began)] {
        std::fs::File::options()
            .write(true)
            .open(readers_file(REPOSITORY, ext))
            .unwrap()
            .set_modified(now - std::time::Duration::from_secs(ago))
            .unwrap();
    }
}

/// `unit/forecast`'s head, conflicting with `#1`'s work, and `--check
/// --json` of it, read.
fn check_forecast(f: &Fakes) -> serde_json::Value {
    work(
        f,
        "unit/forecast",
        "crw-1-unit-3",
        &[("plugin/lib/plan.py", Some("two"))],
    );
    let (code, out) = sluice(
        f,
        &[
            "submit",
            "--check",
            BRANCH,
            "--worktree",
            "unit/forecast",
            "--json",
        ],
    );
    assert_eq!(code, CONFLICT, "{out}");
    json(&out)
}

/// 64, 129: beside a live reader keeping its tick, a forecast forms on the
/// branch and the queue as the reader last read them, not on the records.
#[test]
fn a_forecast_takes_the_branch_from_a_current_reader() {
    let f = setup();
    let (_reader, _, r, _) = reader_of_four(&f, 3);
    let v = check_forecast(&f);
    assert_eq!(v["onto"], r.as_str(), "{v}");
    assert_eq!(v["ahead"], serde_json::json!([4]), "{v}");
    assert_eq!(v["conflict"]["with"][0]["id"], 4, "{v}");
    assert_eq!(
        v["conflict"]["with"][0]["from"]["worktree"], "unit/touches",
        "{v}"
    );
}

/// 64, 129: `--check --head` takes the branch it forecasts from the same
/// read where the reader read it, as it takes the branch it lands on.
#[test]
fn a_forecast_of_a_bolt_takes_both_from_the_reader() {
    let f = setup();
    let (_reader, mut doc, r, _) = reader_of_four(&f, 3);
    let tip = f.world.tip(BRANCH);
    let bolt = f.world.work(&tip, &[("plugin/lib/plan.py", Some("two"))]);
    let mut y = doc["branches"][0].clone();
    y["branch"] = "bolt/y".into();
    y["revision"] = bolt.as_str().into();
    y["submissions"] = serde_json::json!([]);
    y["candidates"] = serde_json::json!([]);
    y["depth"] = 0.into();
    doc["branches"].as_array_mut().unwrap().push(y);
    rewrite_readers_doc(&doc);
    let (code, out) = sluice(
        &f,
        &["submit", BRANCH, "--check", "--head", "bolt/y", "--json"],
    );
    assert_eq!(code, CONFLICT, "{out}");
    let v = json(&out);
    assert_eq!(v["head"], bolt.as_str(), "{v}");
    assert_eq!(v["onto"], r.as_str(), "{v}");
    assert_eq!(v["ahead"], serde_json::json!([4]), "{v}");
}

/// 64, 129: a reader whose last read ended five ticks ago, with no read
/// begun since, has stalled: the forecast reads the records as a round.
#[test]
fn a_stalled_reader_is_read_past() {
    let f = setup();
    let (_reader, _, _, _) = reader_of_four(&f, 1);
    mark_reader(5, 6);
    let v = check_forecast(&f);
    assert_eq!(v["onto"], f.world.tip(BRANCH).as_str(), "{v}");
    assert_eq!(v["ahead"], serde_json::json!([1]), "{v}");
}

/// 64, 129: a reader twenty seconds into a read it began a second after
/// its last ended is current: the forecast takes its last read.
#[test]
fn a_reader_in_a_slow_read_is_current() {
    let f = setup();
    let (_reader, _, r, _) = reader_of_four(&f, 1);
    mark_reader(21, 20);
    let v = check_forecast(&f);
    assert_eq!(v["onto"], r.as_str(), "{v}");
    assert_eq!(v["ahead"], serde_json::json!([4]), "{v}");
}

/// 64, 129: a read that names no revision of the branch, as a reader's
/// that could not ask its remote, is read past as a round reads.
#[test]
fn a_forecast_with_no_revision_reads_as_a_round() {
    let f = setup();
    let (_reader, mut doc, _, _) = reader_of_four(&f, 3);
    doc["branches"][0]
        .as_object_mut()
        .unwrap()
        .remove("revision");
    rewrite_readers_doc(&doc);
    let v = check_forecast(&f);
    assert_eq!(v["onto"], f.world.tip(BRANCH).as_str(), "{v}");
    assert_eq!(v["ahead"], serde_json::json!([1]), "{v}");
}

/// 64, 124, 129: where the host lacks `#4`'s head, the reader's read is
/// no read to form on: the forecast reads as a round does.
#[test]
fn a_forecast_lacking_a_head_reads_as_a_round() {
    let f = setup();
    let (_reader, _, _, one) = reader_of_four(&f, 3);
    f.world.lack(&one, true);
    let v = check_forecast(&f);
    assert_eq!(v["onto"], f.world.tip(BRANCH).as_str(), "{v}");
    assert_eq!(v["ahead"], serde_json::json!([1]), "{v}");
}

#[test]
fn a_dirty_place_is_refused_with_exit_two() {
    let f = setup();
    work(&f, "unit/a", "agent", &[("plugin/x", Some("x"))]);
    f.world.dirty("unit/a", &["plugin/scratch.py"]);
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, CONFLICT);
    assert_eq!(
        out,
        "refused: unit/a holds changes beyond its commits:\n  plugin/scratch.py\n\
         the agent working in unit/a commits or removes them and submits again\n"
    );
}

/// A refusal's way on names the agent working in the worktree under
/// `--json` as in text, and what moves it on is the same command.
#[test]
fn a_refusal_under_json_names_the_agent_and_keeps_its_next() {
    let f = setup();
    work(&f, "unit/a", "agent", &[("plugin/x", Some("x"))]);
    f.world.dirty("unit/a", &["plugin/scratch.py"]);
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--json"]);
    assert_eq!(code, CONFLICT, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v["error"]["message"],
        "unit/a holds changes beyond its commits:\n  plugin/scratch.py\n\
         the agent working in unit/a commits or removes them and submits again"
    );
    assert_eq!(
        v["error"]["next"],
        serde_json::json!([format!("sluice submit {BRANCH}")])
    );
}

/// ADR 0050: a branch requiring a check the task runner in the worktree
/// keeps out of CI refuses the submission with exit 2, naming the check,
/// the tag that requires it and its line of the policy, and the way on;
/// nothing is queued or published.
#[test]
fn a_submission_whose_gate_requires_a_check_kept_out_of_ci_is_refused_with_exit_two() {
    let f = setup();
    f.world.keeps_out("plugin:test");
    work(&f, "unit/a", "agent", &[("tests/t_cards.py", Some("x"))]);
    f.world.at_path("unit/a", "/work/unit-a");
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, CONFLICT);
    assert_eq!(
        out,
        format!(
            "refused: {BRANCH} requires plugin:test through #gate (.config/sluice.toml line 2), \
             which the task runner in this worktree keeps out of CI, so it is never run and \
             {BRANCH} cannot pass while it is required\n\
             the agent working in unit/a lets it run in CI there, or drops it from {BRANCH}'s \
             policy in the same submission that lets it run, and submits again\n"
        )
    );
    assert!(f.world.published().is_empty());
    let (_, status) = sluice(&f, &["status", BRANCH, "--json"]);
    let v: serde_json::Value = serde_json::from_str(&status).unwrap();
    assert_eq!(v["submissions"], serde_json::json!([]), "{status}");

    let (code, out) = sluice(
        &f,
        &[
            "submit",
            BRANCH,
            "--worktree",
            "unit/a",
            "--no-wait",
            "--json",
        ],
    );
    assert_eq!(code, CONFLICT, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["error"]["kind"], "refused");
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .starts_with(&format!("{BRANCH} requires plugin:test through #gate")));
    assert_eq!(
        v["error"]["next"],
        serde_json::json!([format!("sluice status {BRANCH}")])
    );
}

/// ADR 0050: when the task runner cannot be asked, the submission is
/// taken, with a warning saying why it was not checked; under `--json
/// --no-wait`, the warning is the document's.
#[test]
fn a_submission_the_runner_could_not_check_is_taken_with_a_warning() {
    let f = setup();
    f.world
        .kept_out_fails(Some("[tests] toolchain exited 1: no devenv"));
    work(&f, "unit/a", "agent", &[("tests/t_cards.py", Some("x"))]);
    f.world.at_path("unit/a", "/work/unit-a");
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        out,
        format!(
            "submission 1 to {BRANCH} · nothing queued ahead\n\
             warning: could not check which of {BRANCH}'s required checks the task runner in \
             this worktree keeps out of CI, so the submission was taken unchecked: \
             [tests] toolchain exited 1: no devenv\n"
        )
    );

    work(
        &f,
        "unit/b",
        "agent-b",
        &[("plugin/lib/plan.py", Some("y"))],
    );
    f.world.at_path("unit/b", "/work/unit-b");
    let (code, out) = sluice(
        &f,
        &[
            "submit",
            BRANCH,
            "--worktree",
            "unit/b",
            "--no-wait",
            "--json",
        ],
    );
    assert_eq!(code, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["id"], 2);
    let warnings = v["warnings"].as_array().expect("warnings");
    assert_eq!(warnings.len(), 1, "{out}");
    assert!(warnings[0].as_str().unwrap().contains("no devenv"), "{out}");
}

/// The message of the commit submission `id` landed as.
fn landing_message(f: &Fakes, id: u64) -> String {
    let (_, out) = sluice(f, &["status", &id.to_string(), "--json"]);
    let landed = json(&out)["landed_as"]
        .as_str()
        .expect("landed")
        .to_string();
    f.world.message(&sluice_seams::Rev::new(landed))
}

/// 23: a subject on a branch with no pull requests names its landing.
/// The submission is taken with no warning of its subject and lands, its
/// landing commit's first line the subject, `Sluice-From` still naming
/// the agent and the branch; under `--json --no-wait`, the document
/// carries no warning either.
#[test]
fn a_subject_on_a_branch_with_no_pull_requests_names_its_landing() {
    let f = setup();
    work(
        &f,
        "unit/review",
        "agent",
        &[("tests/t_cards.py", Some("x"))],
    );
    let subject = ["--subject", "Batch review cards"];
    let args = [
        &["submit", BRANCH, "--worktree", "unit/review"][..],
        &subject,
    ]
    .concat();
    let (code, out) = sluice(&f, &args);
    assert_eq!(code, LANDED, "{out}");
    assert!(!out.contains("warning:"), "{out}");
    let m = landing_message(&f, 1);
    assert_eq!(m.lines().next(), Some("chore(land): #1 Batch review cards"));
    assert!(m.contains("\nSluice-From: agent (unit/review)\n"), "{m}");

    work(
        &f,
        "unit/b",
        "agent-b",
        &[("plugin/lib/plan.py", Some("y"))],
    );
    let args = [
        &[
            "submit",
            BRANCH,
            "--worktree",
            "unit/b",
            "--no-wait",
            "--json",
        ][..],
        &subject,
    ]
    .concat();
    let (code, out) = sluice(&f, &args);
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    assert_eq!(v["id"], 2);
    assert_eq!(v.get("warnings"), None, "{out}");
}

/// 23: with no subject, a landing commit is named by its branch.
#[test]
fn with_no_subject_a_landing_is_named_by_its_branch() {
    let f = setup();
    work(
        &f,
        "unit/review",
        "agent",
        &[("tests/t_cards.py", Some("x"))],
    );
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/review"]);
    assert_eq!(code, LANDED, "{out}");
    let m = landing_message(&f, 1);
    assert_eq!(m.lines().next(), Some("chore(land): #1 unit/review"));
    assert!(m.contains("\nSluice-From: agent (unit/review)\n"), "{m}");
}

/// 125, 23: a group given a subject for each worktree queues its
/// submissions in order, each landing named by its own subject.
#[test]
fn a_subject_for_each_names_each_landing() {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    work(&f, "unit/b", "agent-b", &[("tests/b", Some("b"))]);
    let (code, out) = sluice(
        &f,
        &[
            "submit",
            BRANCH,
            "--no-wait",
            "--worktree",
            "unit/a",
            "--worktree",
            "unit/b",
            "--subject",
            "Batch review cards",
            "--subject",
            "Card filters",
        ],
    );
    assert_eq!(code, 0, "{out}");
    assert_eq!(queued(&f).len(), 2);
    for id in [1, 2] {
        assert_eq!(sluice(&f, &["wait", &id.to_string()]).0, LANDED);
    }
    for (id, subject, from) in [
        (1, "Batch review cards", "agent-a (unit/a)"),
        (2, "Card filters", "agent-b (unit/b)"),
    ] {
        let m = landing_message(&f, id);
        assert_eq!(
            m.lines().next(),
            Some(format!("chore(land): #{id} {subject}").as_str())
        );
        assert!(m.contains(&format!("\nSluice-From: {from}\n")), "{m}");
    }
}

/// Under `--json`, a refusal is one error document on stdout, naming
/// what moves it on; the exit is a refusal's still.
#[test]
fn a_refusal_under_json_is_an_error_document() {
    let f = setup();
    work(&f, "unit/a", "agent", &[("plugin/x", Some("x"))]);
    f.world.dirty("unit/a", &["plugin/scratch.py"]);
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--json"]);
    assert_eq!(code, CONFLICT, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["error"]["kind"], "refused");
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .starts_with("unit/a holds changes beyond its commits"));
    assert_eq!(
        v["error"]["next"],
        serde_json::json!([format!("sluice submit {BRANCH}")])
    );

    let (code, out) = sluice(
        &f,
        &[
            "submit",
            "bolt/none",
            "--worktree",
            "unit/a",
            "--check",
            "--json",
        ],
    );
    assert_eq!(code, CONFLICT, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["error"]["message"], "no branch bolt/none");
    assert_eq!(
        v["error"]["next"],
        serde_json::json!([
            "sluice branch create bolt/none --from main",
            "sluice overview"
        ])
    );
}

/// What is asked about and does not exist is `not_found` under `--json`.
#[test]
fn nothing_found_under_json_is_an_error_document() {
    let f = setup();
    let (code, out) = sluice(&f, &["status", "9999", "--json"]);
    assert_eq!(code, 1);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["error"]["kind"], "not_found");
    assert_eq!(v["error"]["message"], "no submission 9999");
    assert_eq!(v["error"]["next"], serde_json::json!(["sluice overview"]));

    work(&f, "unit/a", "agent", &[("plugin/x", Some("x"))]);
    let (code, out) = sluice(
        &f,
        &[
            "submit",
            BRANCH,
            "--worktree",
            "unit/a",
            "--status",
            "--json",
        ],
    );
    assert_eq!(code, 1);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["error"]["kind"], "not_found");

    let (code, out) = exec(&f, &[BRANCH, "--worktree", "nowhere", "--json"]);
    assert_eq!(code, CONFLICT);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["error"]["kind"], "refused");
}

fn home(f: &Fakes, worktree: &str) -> serde_json::Value {
    let (code, out) = sluice(f, &["--worktree", worktree, "--json"]);
    assert_eq!(code, 0, "{out}");
    serde_json::from_str(&out).unwrap()
}

/// With no command: the worktree's own submission, found by the path it
/// was submitted from, the branches' counts and what to run next.
#[test]
fn home_shows_the_worktrees_submission_by_its_path() {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    f.world.at_path("unit/a", "/w/a");
    let v = home(&f, "unit/a");
    assert_eq!(v["schema"], "sluice/home@1");
    assert_eq!(v["worktree"]["path"], "/w/a");
    assert_eq!(v["submission"], serde_json::Value::Null);

    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    let v = home(&f, "unit/a");
    assert_eq!(v["submission"]["id"], 1);
    assert_eq!(v["submission"]["state"], "queued");
    // No round has read the branch, and its gate is read where this host
    // holds it all the same (56, 129; rule core.15).
    assert_eq!(
        v["branches"],
        serde_json::json!([{"branch": BRANCH, "depth": 1, "testing": 0, "parallel": 4}])
    );
    assert_eq!(
        v["next"],
        serde_json::json!(["sluice status 1", "sluice withdraw 1 --as agent-a"])
    );

    // A submission with no path is no worktree's, whatever its branch.
    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/b", "--no-wait"]);
    assert_eq!(home(&f, "unit/b")["submission"], serde_json::Value::Null);
}

/// Once out of the queue, a submission is still the worktree's, found
/// through the branch's recent lines: landed, or failed with its log.
#[test]
fn home_finds_a_landed_or_failed_submission_through_recent() {
    let f = setup();
    f.world
        .fails_with("tests:t-batch-cards", "tests/t_cards.py", "broken");
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    f.world.at_path("unit/a", "/w/a");
    assert_eq!(
        sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]).0,
        LANDED
    );
    let v = home(&f, "unit/a");
    assert_eq!(v["submission"]["state"], "landed");
    assert_eq!(v["submission"]["landed_as"], f.world.tip(BRANCH).0);
    assert_eq!(v["next"][0], format!("sluice submit {BRANCH}"));

    work(
        &f,
        "unit/b",
        "agent-b",
        &[("tests/t_cards.py", Some("broken"))],
    );
    f.world.at_path("unit/b", "/w/b");
    assert_eq!(
        sluice(&f, &["submit", BRANCH, "--worktree", "unit/b"]).0,
        FAILED
    );
    let v = home(&f, "unit/b");
    assert_eq!(v["submission"]["state"], "failed");
    assert_eq!(v["submission"]["failed"][0]["task"], "tests:t-batch-cards");
    assert!(v["submission"]["failed"][0]["log"].is_string());
    assert_eq!(
        v["next"],
        serde_json::json!([format!("sluice submit {BRANCH}")])
    );

    let (code, out) = sluice(&f, &["--worktree", "unit/b"]);
    assert_eq!(code, 0);
    assert!(
        out.contains(&format!("#2 unit/b to {BRANCH} · failed\n")),
        "{out}"
    );
    assert!(out.contains("  failed tests:t-batch-cards: "), "{out}");
}

/// 26: the home view names its submission by its subject, or its branch
/// where its submit gave none; `--json` carries the subject, null where
/// there is none, its other fields as before.
#[test]
fn home_names_its_submission_with_its_subject() {
    let f = setup();
    work(&f, "unit/forecast", "agent-f", &[("plugin/a", Some("a"))]);
    f.world.at_path("unit/forecast", "/w/f");
    f.world.hold_runs(true);
    sluice(
        &f,
        &["submit", BRANCH, "--worktree", "unit/forecast", "--no-wait"],
    );
    sluice(&f, &["step", BRANCH]);
    let text = |f: &Fakes| sluice(f, &["--worktree", "unit/forecast"]).1;
    let out = text(&f);
    assert!(
        out.contains(&format!("\n#1 unit/forecast to {BRANCH} · testing\n")),
        "{out}"
    );
    let v = home(&f, "unit/forecast");
    assert_eq!(v["submission"]["subject"], serde_json::Value::Null);

    give_subject(&f, 1, "Forecast concurrent units");
    let out = text(&f);
    assert!(
        out.contains(&format!(
            "\n#1 Forecast concurrent units to {BRANCH} · testing\n"
        )),
        "{out}"
    );
    let v = home(&f, "unit/forecast");
    assert_eq!(
        v["submission"],
        serde_json::json!({
            "id": 1, "branch": BRANCH, "state": "testing", "failed": [],
            "landed_as": null, "subject": "Forecast concurrent units",
        })
    );
    assert_eq!(
        v["next"],
        serde_json::json!(["sluice status 1", "sluice withdraw 1 --as agent-f"])
    );
}

/// A pull request's submission names no worktree's path, but the branch it
/// was opened from: the worktree on that branch is shown it (ADR 0025).
#[test]
fn home_shows_a_pull_requests_submission_to_the_worktree_it_was_opened_from() {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    f.world.at_path("unit/a", "/w/a");
    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    f.world.at_path("unit/b", "/w/b");
    work(&f, "pull/9", "agent-a", &[("plugin/a", Some("a"))]);
    f.world.opened_from("pull/9", "unit/a");
    sluice(&f, &["submit", BRANCH, "--worktree", "pull/9", "--no-wait"]);

    let v = home(&f, "unit/a");
    assert_eq!(v["submission"]["id"], 1);
    assert_eq!(v["submission"]["state"], "queued");
    assert_eq!(v["next"][0], "sluice status 1");
    assert_eq!(home(&f, "unit/b")["submission"], serde_json::Value::Null);
}

/// Outside any worktree sluice can submit from, it says so and who
/// submits, shows the queues and points at the overview.
#[test]
fn home_outside_a_worktree_says_so() {
    let f = setup();
    let v = home(&f, "nowhere");
    assert_eq!(v["worktree"], serde_json::Value::Null);
    assert_eq!(v["next"], serde_json::json!(["sluice overview"]));
    let (_, out) = sluice(&f, &["--worktree", "nowhere"]);
    assert!(
        out.starts_with(
            "not in a worktree sluice can submit from: the agent working in a worktree submits \
             its work from there\n"
        ),
        "{out}"
    );
}

#[test]
fn status_lists_the_queue() {
    let f = setup();
    f.world
        .fails_with("tests:t-batch-cards", "tests/t_cards.py", "broken");
    work(
        &f,
        "unit/touches",
        "crw-1-unit-1",
        &[("plugin/lib/plan.py", Some("touched"))],
    );
    work(
        &f,
        "unit/batch-review",
        "crw-1-unit-2",
        &[("tests/t_cards.py", Some("broken"))],
    );
    sluice(
        &f,
        &["submit", BRANCH, "--worktree", "unit/touches", "--no-wait"],
    );
    sluice(
        &f,
        &[
            "submit",
            BRANCH,
            "--worktree",
            "unit/batch-review",
            "--no-wait",
        ],
    );
    sluice(&f, &["run", "--rounds", "6"]);
    f.world.hold_runs(true);
    work(
        &f,
        "unit/forecast",
        "crw-1-unit-3",
        &[("plugin/other.py", Some("x"))],
    );
    sluice(
        &f,
        &["submit", BRANCH, "--worktree", "unit/forecast", "--no-wait"],
    );
    sluice(&f, &["step", BRANCH]);

    // By default only the queue, with its count and what is left out.
    let (code, out) = sluice(&f, &["status", BRANCH]);
    assert_eq!(code, 0);
    let tip = f.world.tip(BRANCH);
    let header = format!(
        "{BRANCH} at {} · 1 in the queue · testing 1 at once (up to 4)",
        &tip.0[..7]
    );
    // ADR 0042: the combination under test, with how long it has run.
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], header);
    assert!(
        lines[1].starts_with(&format!("  testing {BRANCH} + #3 unit/forecast · ")),
        "{out}"
    );
    // D13 (51; record 0102): after the queue, what is owed, since nobody
    // confirmed #1's landing or #2's blame.
    assert_eq!(
        lines[2..],
        [
            format!("  #3 unit/forecast  testing   crw-1-unit-3  unit/forecast   {BRANCH} + #3"),
            format!("2 no longer in the queue: sluice status {BRANCH} --all"),
            "OWED".to_string(),
            "  unit/batch-review · never confirmed".to_string(),
            "    2:failed #2  owed until 2026-10-18T09:00:09Z · handed over by notice at \
             2026-10-11T09:00:15Z"
                .to_string(),
            "  unit/touches · never confirmed".to_string(),
            "    1:landed #1  owed until 2026-10-18T09:00:10Z · handed over by notice at \
             2026-10-11T09:00:16Z"
                .to_string(),
            "confirm one with: sluice confirm <key>, in its worktree, or sluice confirm <key> \
             --as operator"
                .to_string(),
        ],
        "{out}"
    );

    let (code, out) = sluice(&f, &["status", BRANCH, "--all"]);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], header);
    assert!(lines[1].starts_with(&format!("  testing {BRANCH} + #3 unit/forecast · ")));
    assert_eq!(
        lines[2],
        "  #1 unit/touches       landed    crw-1-unit-1  unit/touches"
    );
    assert!(lines[3].starts_with(
        "  #2 unit/batch-review  failed    crw-1-unit-2  unit/batch-review   tests:t-batch-cards on "
    ));
    assert_eq!(
        lines[4],
        format!(
            "  #3 unit/forecast      testing   crw-1-unit-3  unit/forecast       {BRANCH} + #3"
        )
    );

    // `--json` lists the queue, `--all` the history, the landed #1 read
    // from the finished records (131).
    let (_, json) = sluice(&f, &["status", BRANCH, "--json"]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["submissions"].as_array().unwrap().len(), 1);
    assert_eq!(v["depth"], 1);
    let (_, json) = sluice(&f, &["status", BRANCH, "--json", "--all"]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["submissions"].as_array().unwrap().len(), 3);
    assert_eq!(v["submissions"][0]["state"], "landed");
    assert_eq!(v["submissions"][1]["state"], "failed");
    assert!(f.world.files(BRANCH).contains_key(".finished/1.json"));
    assert!(!f.world.files(BRANCH).contains_key("submissions/1.json"));
}

/// `status <branch>` reads the branch at the revision its records last
/// saw, as the overview does, and asks the branch itself nothing: a
/// commit made since is shown by the next round (ADR 0046).
#[test]
fn status_reads_the_revision_the_records_saw() {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    sluice(&f, &["step", BRANCH]);
    let seen = f.world.tip(BRANCH);
    f.world.hand_commit(BRANCH, &[("plugin/b", Some("b"))]);
    let (_, out) = sluice(&f, &["status", BRANCH, "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["at"], seen.0);
}

/// 133, record 0061: while this host's rounds of the branch are refused,
/// status says what could not be read, from where, since when, for how
/// many rounds and what git said, on as many lines as git said it; once a
/// round reads the remote again it says nothing of it. A state read
/// answered from what was last fetched still says so (80).
#[test]
fn status_shows_this_hosts_refusal_while_it_stands() {
    let f = setup();
    sluice(&f, &["step", BRANCH]);
    let said = "remote: Repository not found.\n\
                fatal: repository 'https://github.com/acme/app.git/' not found";
    f.world.refuse_reads(BRANCH, Some(said));
    for _ in 0..3 {
        assert!(f.sluice().step(BRANCH).is_err());
    }
    let kept: serde_json::Value =
        serde_json::from_str(&f.world.refusal(BRANCH).expect("a refusal kept")).unwrap();
    let since = kept["since"].as_str().unwrap();
    let (code, out) = sluice(&f, &["status", BRANCH]);
    assert_eq!(code, 0);
    assert!(
        out.contains(&format!(
            "this host could not read refs/heads/{BRANCH} from origin since {since}, 3 rounds: \
             remote: Repository not found.\n  \
             fatal: repository 'https://github.com/acme/app.git/' not found\n"
        )),
        "{out}"
    );
    let (_, out) = sluice(&f, &["status", BRANCH, "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v["refused"],
        serde_json::json!({
            "remote": "origin",
            "asked": format!("refs/heads/{BRANCH}"),
            "said": said,
            "since": since,
            "rounds": 3,
        })
    );

    f.world.refuse_reads(BRANCH, None);
    f.sluice().step(BRANCH).unwrap();
    let (_, out) = sluice(&f, &["status", BRANCH]);
    assert!(!out.contains("could not read"), "{out}");
    let (_, out) = sluice(&f, &["status", BRANCH, "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v.get("refused").is_none(), "{v}");

    f.world
        .refuse_state_reads(BRANCH, Some("error: RPC failed; HTTP 503"));
    let (_, out) = sluice(&f, &["status", BRANCH]);
    assert!(
        out.contains("origin could not be reached: this is the state last fetched from it"),
        "{out}"
    );
    let (_, out) = sluice(&f, &["status", BRANCH, "--json"]);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&out).unwrap()["stale"],
        "origin"
    );
}

/// Clauses 130, 131: a landed submission leaves the live records in the
/// write that tells its outcome, is read by its number, still counts as
/// landed for an `after`, still counts toward its worktree's attempts,
/// and is refused a change as before.
#[test]
fn a_landed_submission_moves_to_the_finished_records() {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    assert_eq!(
        sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]).0,
        LANDED
    );
    let files = f.world.files(BRANCH);
    assert!(files.contains_key(".finished/1.json"), "{:?}", files.keys());
    assert!(!files.contains_key("submissions/1.json"));
    assert_eq!(
        std::str::from_utf8(&files["finished.jsonl"]).unwrap(),
        "{\"id\":1,\"state\":\"landed\",\"worktree\":\"unit/a\"}\n"
    );
    let (_, out) = sluice(&f, &["status", "1", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["state"], "landed");

    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    assert_eq!(
        sluice(
            &f,
            &["submit", BRANCH, "--worktree", "unit/b", "--after", "1"]
        )
        .0,
        LANDED
    );
    work(&f, "unit/a", "agent-a", &[("plugin/c", Some("c"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    let (_, out) = sluice(&f, &["status", "3", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["attempt"], 2);

    assert_eq!(
        sluice(&f, &["withdraw", "1", "--as", "agent-b"]),
        (
            2,
            "refused: #1 is not agent-b's to withdraw: ask --as agent-a, its agent, \
             or --as operator\n"
                .into()
        )
    );
    assert_eq!(
        sluice(&f, &["amend", "1", "--priority", "2"]),
        (
            2,
            "refused: #1 is landed, past changing: sluice status 1\n".into()
        )
    );
    let (_, out) = sluice(&f, &["status", BRANCH]);
    assert!(
        out.contains(&format!(
            "2 no longer in the queue: sluice status {BRANCH} --all"
        )),
        "{out}"
    );
}

/// A branch that does not exist and has no records is said to be none,
/// with the branches that have records, and exits 1.
#[test]
fn status_of_an_unknown_branch_says_so() {
    let f = setup();
    let (code, out) = sluice(&f, &["status", "nosuch"]);
    assert_eq!(code, FAILED);
    assert_eq!(out, "no branch nosuch, and no branch has records here\n");

    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    let (code, out) = sluice(&f, &["status", "nosuch", "--json"]);
    assert_eq!(code, FAILED);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["error"]["kind"], "not_found");
    assert_eq!(
        v["error"]["message"],
        format!("no branch nosuch; branches with records: {BRANCH}")
    );
    assert_eq!(
        v["error"]["next"],
        serde_json::json!([format!("sluice status {BRANCH}"), "sluice overview"])
    );

    let (code, out) = sluice(&f, &["status", BRANCH]);
    assert_eq!(code, 0);
    assert!(!out.contains("no longer in the queue"), "{out}");
}

/// 108: `cache` in `trust` is read as absent, and `sluice status` says so.
#[test]
fn status_warns_of_cache_in_trust() {
    let f = Fakes::new();
    f.world.init(
        BRANCH,
        &[
            (
                ".config/sluice.toml",
                &POLICY.replace("\"container\"]", "\"container\", \"cache\"]"),
            ),
            ("plugin/lib/plan.py", "plan"),
        ],
    );
    let (code, out) = sluice(&f, &["status", BRANCH]);
    assert_eq!(code, 0);
    assert!(
        out.contains(
            "warning: [branches.\"bolt/*\"] trust lists \"cache\", which no longer counts"
        ),
        "{out}"
    );
    let (_, out) = sluice(&f, &["status", BRANCH, "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["warnings"].as_array().unwrap().len(), 1);
    let (_, out) = sluice(&setup(), &["status", BRANCH, "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["warnings"], serde_json::json!([]));
}

/// 35, 71; ADR 0049: a policy that still declares a checkout cache is
/// refused when `sluice status` reads it, an error the binary prints and
/// exits non-zero on, naming what replaced it.
#[test]
fn status_refuses_a_checkout_cache() {
    let f = Fakes::new();
    f.world.init(
        BRANCH,
        &[
            (
                ".config/sluice.toml",
                &format!(
                    "{POLICY}\n[[checkout.cache]]\nkey = [\"Cargo.lock\"]\npaths = [\"target\"]\n"
                ),
            ),
            ("plugin/lib/plan.py", "plan"),
        ],
    );
    let cli = Cli::try_parse_from(["sluice", "status", BRANCH]).unwrap();
    let mut out = Vec::new();
    let host = support::FakeHost::new(f.world.clone(), 0);
    let e = run(
        cli,
        f.seams(),
        &host,
        &(),
        &(),
        &f.clock,
        &Rounds(Cell::new(0)),
        &mut out,
    )
    .unwrap_err();
    let e = format!("{e:#}");
    assert!(
        e.contains("[checkout] declares a cache")
            && e.contains("ADR 0049")
            && e.contains("[[caches]]"),
        "{e}"
    );
}

/// A landed #1, a blamed #2 and #3 under test, as `status_lists_the_queue`
/// leaves them.
fn three_submissions(f: &Fakes) {
    f.world
        .fails_with("tests:t-batch-cards", "tests/t_cards.py", "broken");
    work(
        f,
        "unit/touches",
        "crw-1-unit-1",
        &[("plugin/lib/plan.py", Some("touched"))],
    );
    work(
        f,
        "unit/batch-review",
        "crw-1-unit-2",
        &[("tests/t_cards.py", Some("broken"))],
    );
    sluice(
        f,
        &["submit", BRANCH, "--worktree", "unit/touches", "--no-wait"],
    );
    sluice(
        f,
        &[
            "submit",
            BRANCH,
            "--worktree",
            "unit/batch-review",
            "--no-wait",
        ],
    );
    sluice(f, &["run", "--rounds", "6"]);
    f.world.hold_runs(true);
    work(
        f,
        "unit/forecast",
        "crw-1-unit-3",
        &[("plugin/other.py", Some("x"))],
    );
    sluice(
        f,
        &["submit", BRANCH, "--worktree", "unit/forecast", "--no-wait"],
    );
    sluice(f, &["step", BRANCH]);
}

#[test]
fn overview_shows_every_branch_at_a_glance() {
    let f = setup();
    three_submissions(&f);

    let (code, json) = sluice(&f, &["overview", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["schema"], "sluice/overview@1");
    assert_eq!(v["host"], "fake-host");
    assert_eq!(v["repository"], "/fake/repo.git");
    assert!(v["at"].is_string());
    assert_eq!(v["sessions"], serde_json::json!([]));
    assert_eq!(v["previews"], serde_json::json!([]));
    // ADR 0035: the one combination under test counts against the bound.
    assert_eq!(v["room"]["queue"], 1);
    assert_eq!(v["room"]["sessions"], 0);
    let branches = v["branches"].as_array().unwrap();
    assert_eq!(branches.len(), 1);
    let b = &branches[0];
    assert_eq!(b["branch"], BRANCH);
    assert_eq!(b["at"], f.world.tip(BRANCH).0);
    assert_eq!(b["parallel"], 4);
    // C.4, C.7: a branch whose policy names no remote names none.
    assert!(b.get("remote").is_none(), "{json}");
    assert_eq!(b["depth"], 1);
    assert!(b["version"].as_u64().unwrap() > 0);
    // Only the submission still in play, as `status <id> --json` has it.
    let subs = b["submissions"].as_array().unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0]["id"], 3);
    assert_eq!(subs[0]["state"], "testing");
    assert_eq!(subs[0]["from"]["worktree"], "unit/forecast");
    let testing: Vec<&serde_json::Value> = b["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["state"] == "testing")
        .collect();
    assert_eq!(testing.len(), 1);
    assert_eq!(testing[0]["submissions"], serde_json::json!([3]));
    assert_eq!(testing[0]["targets"], serde_json::json!(["#gate"]));
    assert!(testing[0]["started"].is_string());
    // The log's own lines: #1 landed, #2 blamed.
    let recent = b["recent"].as_array().unwrap();
    let events: Vec<&str> = recent
        .iter()
        .map(|r| r["event"].as_str().unwrap())
        .collect();
    assert!(events.contains(&"landed"));
    assert!(events.contains(&"blamed"));
    let blamed = recent.iter().find(|r| r["event"] == "blamed").unwrap();
    assert_eq!(blamed["id"], 2);
    assert_eq!(blamed["schema"], "sluice/log@1");

    let (code, text) = sluice(&f, &["overview"]);
    assert_eq!(code, 0);
    let lines: Vec<&str> = text.lines().collect();
    let tip = f.world.tip(BRANCH);
    assert_eq!(
        lines[0],
        format!(
            "{BRANCH} at {} · 1 in the queue · testing 1 at once (up to 4)",
            &tip.0[..7]
        )
    );
    assert!(lines[1].starts_with(&format!("  testing {BRANCH} + #3 unit/forecast · ")));
}

/// 26, 49, 61: the overview names the worktree each submission its recent
/// lines name came from, the landed #1 read from the finished records.
#[test]
fn overview_names_where_recent_submissions_came_from() {
    let f = setup();
    three_submissions(&f);
    assert!(f.world.files(BRANCH).contains_key(".finished/1.json"));

    let (_, json) = sluice(&f, &["overview", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let b = &v["branches"][0];
    let mut named: Vec<u64> = b["recent"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|r| {
            r["ids"]
                .as_array()
                .cloned()
                .unwrap_or_else(|| vec![r["id"].clone()])
        })
        .map(|id| id.as_u64().unwrap())
        .collect();
    named.sort_unstable();
    assert_eq!(named, [1, 2], "{json}");
    assert_eq!(
        b["recent_from"],
        serde_json::json!({"1": "unit/touches", "2": "unit/batch-review"}),
        "{json}"
    );
}

/// 23, 26: the overview names the subject of each submission its recent
/// lines name whose submit gave one, once it has moved to the finished
/// records too, beside where each came from; none before anything lands.
#[test]
fn overview_names_the_subjects_of_recent_submissions() {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    for w in ["unit/a", "unit/b"] {
        sluice(&f, &["submit", BRANCH, "--worktree", w, "--no-wait"]);
    }
    give_subject(&f, 2, "Batch review cards");
    let branch = |f: &Fakes| json(&sluice(f, &["overview", "--json"]).1)["branches"][0].clone();
    let b = branch(&f);
    assert_eq!(b["recent"], serde_json::json!([]));
    assert_eq!(b["recent_subjects"], serde_json::json!({}));

    sluice(&f, &["run", "--rounds", "6"]);
    // Landed together, #2 moved to the finished records, #1 not yet.
    let files = f.world.files(BRANCH);
    assert!(files.contains_key(".finished/2.json"), "{:?}", files.keys());
    assert!(files.contains_key("submissions/1.json"));
    let b = branch(&f);
    assert_eq!(b["recent"][0]["ids"], serde_json::json!([1, 2]), "{b}");
    assert_eq!(
        b["recent_subjects"],
        serde_json::json!({"2": "Batch review cards"}),
        "{b}"
    );
    assert_eq!(
        b["recent_from"],
        serde_json::json!({"1": "unit/a", "2": "unit/b"}),
        "{b}"
    );
}

/// 26: the overview's text names each submission of a landing, a blame,
/// a conflict and a combination under test by its subject, or where it
/// came from where its submit gave none.
#[test]
fn overview_names_each_submission_with_its_subject() {
    let f = setup();
    f.world
        .fails_with("tests:t-batch-cards", "tests/t_cards.py", "broken");
    let submit = |w: &str| sluice(&f, &["submit", BRANCH, "--worktree", w, "--no-wait"]);
    let recent = |f: &Fakes| -> Vec<String> {
        sluice(f, &["overview"])
            .1
            .lines()
            .filter(|l| l.starts_with("  20"))
            .map(|l| {
                l.split_once("  ")
                    .unwrap()
                    .1
                    .split_once("  ")
                    .unwrap()
                    .1
                    .to_string()
            })
            .collect()
    };

    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    submit("unit/a");
    submit("unit/b");
    give_subject(&f, 2, "Batch review cards");
    sluice(&f, &["run", "--rounds", "6"]);
    let to = &f.world.tip(BRANCH).0[..7];
    assert_eq!(
        recent(&f),
        [format!("landed #1 unit/a, #2 Batch review cards as {to}")]
    );

    work(
        &f,
        "unit/c",
        "agent-c",
        &[("tests/t_cards.py", Some("broken"))],
    );
    submit("unit/c");
    give_subject(&f, 3, "Fix the forecast's dates");
    sluice(&f, &["run", "--rounds", "6"]);
    let blamed = recent(&f);
    assert!(
        blamed[0].starts_with("blamed #3 Fix the forecast's dates on "),
        "{blamed:?}"
    );
    sluice(&f, &["withdraw", "3", "--as", "agent-c"]);

    f.world.hold_runs(true);
    work(
        &f,
        "unit/d",
        "agent-d",
        &[("plugin/lib/plan.py", Some("one"))],
    );
    work(
        &f,
        "unit/e",
        "agent-e",
        &[("plugin/lib/plan.py", Some("two"))],
    );
    submit("unit/d");
    submit("unit/e");
    give_subject(&f, 5, "Herdr popup");
    sluice(&f, &["step", BRANCH]);
    let (_, out) = sluice(&f, &["overview"]);
    assert_eq!(
        recent(&f)[0],
        "conflict #5 Herdr popup on plugin/lib/plan.py",
        "{out}"
    );
    assert!(
        out.lines()
            .any(|l| l.starts_with(&format!("  testing {BRANCH} + #4 unit/d · "))),
        "{out}"
    );
}

/// 64, 7, 10: the overview names each combination's revision it was
/// formed on, the branch's revision as its read found it, and the landed
/// submissions a queued one's `after` names: `#1` and `#2` under test as
/// one combination, and `#5` queued, after `#3`, which landed.
#[test]
fn the_overview_names_each_combinations_base_and_the_landed_afters() {
    let f = setup();
    for w in ["a", "b", "c"] {
        let worktree = format!("unit/{w}");
        work(&f, &worktree, w, &[(&format!("plugin/{w}"), Some(w))]);
        let (code, out) = sluice(
            &f,
            &["submit", BRANCH, "--worktree", &worktree, "--no-wait"],
        );
        assert_eq!(code, 0, "{out}");
    }
    // #1 and #2 wait on each other, so #3 is tested and lands alone.
    let amend = |id: &str, after: &str| {
        assert_eq!(sluice(&f, &["amend", id, "--after", after]).0, 0);
    };
    amend("1", "2");
    amend("2", "1");
    let landed = |id: &str| json(&sluice(&f, &["status", id, "--json"]).1)["state"] == "landed";
    for _ in 0..10 {
        if landed("3") {
            break;
        }
        sluice(&f, &["step", BRANCH]);
    }
    assert!(landed("3") && !landed("1"));
    f.world.hold_runs(true);
    amend("1", "3");
    sluice(&f, &["step", BRANCH]);
    for (w, after) in [("d", None), ("e", Some("3"))] {
        let worktree = format!("unit/{w}");
        work(&f, &worktree, w, &[(&format!("plugin/{w}"), Some(w))]);
        let mut args = vec!["submit", BRANCH, "--worktree", &worktree, "--no-wait"];
        args.extend(after.iter().flat_map(|a| ["--after", *a]));
        assert_eq!(sluice(&f, &args).0, 0);
    }

    let (code, out) = sluice(&f, &["overview", "--json"]);
    assert_eq!(code, 0, "{out}");
    let b = &json(&out)["branches"][0];
    let tip = f.world.tip(BRANCH);
    assert_eq!(b["revision"], tip.as_str(), "{b}");
    assert_eq!(b["after_landed"], serde_json::json!([3]), "{b}");
    let both = b["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["submissions"] == serde_json::json!([1, 2]))
        .unwrap_or_else(|| panic!("#1 and #2 under test as one combination: {b}"));
    assert_eq!(both["on"], tip.as_str(), "{b}");
    assert_eq!(both["state"], "testing", "{b}");
}

/// ADR 0028: a combination under test carries its run's tasks, as a
/// round kept them, and its text line says how many have ended.
#[test]
fn overview_shows_a_combinations_tasks() {
    use sluice_seams::{Rev, TaskProgress, TaskState};
    let f = setup();
    three_submissions(&f);
    let (_, json) = sluice(&f, &["overview", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let c = &v["branches"][0]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["state"] == "testing")
        .unwrap()
        .clone();
    assert!(c.get("tasks").is_none());
    let task = |target: &str, state: TaskState, exit: Option<i32>| TaskProgress {
        target: target.into(),
        state,
        started: exit.map(|_| "2026-10-18T12:00:00Z".into()),
        ended: exit.map(|_| "2026-10-18T12:01:48Z".into()),
        exit,
    };
    f.world.says_tasks(
        &Rev::new(c["revision"].as_str().unwrap()),
        &[
            task("tests:t-cards", TaskState::Passed, Some(0)),
            task("tests:t-batch-cards", TaskState::Running, None),
            task("plugin:test", TaskState::Waiting, None),
        ],
    );
    sluice(&f, &["step", BRANCH]);

    let (_, json) = sluice(&f, &["overview", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let c = v["branches"][0]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["state"] == "testing")
        .unwrap()
        .clone();
    assert_eq!(
        c["tasks"],
        serde_json::json!([
            {"target": "tests:t-cards", "state": "passed",
             "started": "2026-10-18T12:00:00Z", "ended": "2026-10-18T12:01:48Z", "exit": 0},
            {"target": "tests:t-batch-cards", "state": "running"},
            {"target": "plugin:test", "state": "waiting"},
        ])
    );
    let (_, text) = sluice(&f, &["overview"]);
    assert!(
        text.lines().any(|l| l.starts_with(&format!(
            "  testing {BRANCH} + #3 unit/forecast · 1/3 tasks · running tests:t-batch-cards · "
        ))),
        "{text}"
    );
}

/// ADR 0042: `status` says how far a combination under test has got, a
/// line under the branch's counts, the rows as they were.
#[test]
fn status_shows_a_combinations_progress() {
    use sluice_core::Clock;
    use sluice_seams::{Rev, TaskProgress, TaskState};
    let f = setup();
    three_submissions(&f);
    let (_, before) = sluice(&f, &["status", BRANCH]);
    assert!(
        before.lines().any(
            |l| l == format!("  testing {BRANCH} + #3 unit/forecast").as_str()
                || l.starts_with(&format!("  testing {BRANCH} + #3 unit/forecast · "))
        ),
        "{before}"
    );
    let (_, json) = sluice(&f, &["status", "3", "--json"]);
    let rev = Rev::new(
        serde_json::from_str::<serde_json::Value>(&json).unwrap()["tested"]["combination"]
            .as_str()
            .unwrap(),
    );
    let started = (f.clock.now() - chrono::Duration::minutes(4))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let task = |target: &str, state: TaskState, started: Option<&str>| TaskProgress {
        target: target.into(),
        state,
        started: started.map(String::from),
        ended: None,
        exit: (state == TaskState::Passed).then_some(0),
    };
    f.world.says_tasks(
        &rev,
        &[
            task("tests:t-cards", TaskState::Passed, None),
            task("tests:t-batch-cards", TaskState::Running, Some(&started)),
            task("plugin:test", TaskState::Waiting, None),
        ],
    );
    sluice(&f, &["step", BRANCH]);

    let head = format!(
        "  testing {BRANCH} + #3 unit/forecast · 1/3 tasks · running tests:t-batch-cards 4m"
    );
    let (_, text) = sluice(&f, &["status", BRANCH]);
    let lines: Vec<&str> = text.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.starts_with(&head))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(lines[at].contains(" · "), "{text}");
    assert!(
        lines[at + 1..]
            .iter()
            .any(|l| l.starts_with("  #3 unit/forecast  testing ")
                && l.ends_with(&format!("{BRANCH} + #3"))),
        "{text}"
    );
    let (_, text) = sluice(&f, &["status", "3"]);
    assert!(text.lines().any(|l| l.starts_with(&head)), "{text}");
    // A submission in no combination under test has no line.
    let (_, text) = sluice(&f, &["status", "1"]);
    assert!(!text.contains("  testing "), "{text}");
}

/// 23: submission `id` as if its submit gave `subject`, written on its
/// live record, since no submit here takes one yet.
fn give_subject(f: &Fakes, id: u64, subject: &str) {
    edit_record(f, id, |doc| doc["subject"] = subject.into());
}

/// Submission `id`'s record on `BRANCH`, changed by `edit`, as another
/// version of sluice would have written it.
fn edit_record(f: &Fakes, id: u64, edit: impl FnOnce(&mut serde_json::Value)) {
    let state = f.seams().state;
    let _lock = state.wait_lock(BRANCH).unwrap();
    let read = state.read(BRANCH).unwrap();
    let path = format!("submissions/{id}.json");
    let mut doc: serde_json::Value = serde_json::from_slice(&read.files[&path]).unwrap();
    edit(&mut doc);
    let mut changes = sluice_seams::Changes::default();
    changes
        .put
        .insert(path, serde_json::to_vec_pretty(&doc).unwrap());
    assert!(matches!(
        state.write(BRANCH, read.version, changes).unwrap(),
        sluice_seams::Written::Written(_)
    ));
}

/// The column a row's state begins at.
fn state_column(row: &str) -> usize {
    [" queued ", " testing ", " waiting ", " landed ", " failed "]
        .iter()
        .find_map(|w| row.find(w))
        .unwrap_or_else(|| panic!("no state in {row}"))
}

/// 26: each row begins with its number and its subject, or its branch
/// where its submit gave none, padded so the columns after line up; a
/// combination names each of its submissions once, a row's own number
/// standing alone.
#[test]
fn status_names_each_submission_with_its_subject() {
    let f = setup();
    work(&f, "unit/review", "agent-r", &[("plugin/a", Some("a"))]);
    work(&f, "unit/forecast", "agent-f", &[("plugin/b", Some("b"))]);
    for w in ["unit/review", "unit/forecast"] {
        sluice(&f, &["submit", BRANCH, "--worktree", w, "--no-wait"]);
    }
    give_subject(&f, 1, "Batch review cards");
    f.world.hold_runs(true);
    sluice(&f, &["step", BRANCH]);

    let (_, out) = sluice(&f, &["status", BRANCH]);
    let lines: Vec<&str> = out.lines().collect();
    let one = lines
        .iter()
        .find(|l| l.starts_with("  #1 "))
        .unwrap_or_else(|| panic!("{out}"));
    let two = lines
        .iter()
        .find(|l| l.starts_with("  #2 "))
        .unwrap_or_else(|| panic!("{out}"));
    assert!(
        one.starts_with("  #1 Batch review cards  testing   agent-r  unit/review   "),
        "{out}"
    );
    assert!(one.ends_with(&format!("   {BRANCH} + #1")), "{out}");
    assert!(two.starts_with("  #2 unit/forecast       "), "{out}");
    assert_eq!(state_column(one), state_column(two), "{out}");
    assert!(
        lines.iter().any(|l| l.starts_with(&format!(
            "  testing {BRANCH} + #1 Batch review cards + #2 unit/forecast · "
        ))),
        "{out}"
    );

    // One submission's status names the combination the same way.
    let (_, out) = sluice(&f, &["status", "2"]);
    assert!(
        out.lines().any(|l| l.starts_with(&format!(
            "  testing {BRANCH} + #1 Batch review cards + #2 unit/forecast"
        ))),
        "{out}"
    );
    assert!(
        out.lines()
            .any(|l| l.starts_with("  #2 unit/forecast  testing ")
                && l.ends_with(&format!("{BRANCH} + #1 Batch review cards + #2"))),
        "{out}"
    );
}

/// 26: a subject is shown on its row's one line, each run of control
/// characters a space, and kept as given under `--json`.
#[test]
fn status_shows_a_subject_on_one_line() {
    let f = setup();
    work(&f, "unit/review", "agent-r", &[("plugin/a", Some("a"))]);
    sluice(
        &f,
        &["submit", BRANCH, "--worktree", "unit/review", "--no-wait"],
    );
    give_subject(&f, 1, "Batch review\ncards\u{1b}[31m");
    let (_, out) = sluice(&f, &["status", BRANCH]);
    assert!(
        out.lines()
            .any(|l| l.starts_with("  #1 Batch review cards [31m  queued ")),
        "{out}"
    );
    assert!(!out.contains('\u{1b}'), "{out}");
    let v = json(&sluice(&f, &["status", "1", "--json"]).1);
    assert_eq!(v["subject"], "Batch review\ncards\u{1b}[31m");
}

/// 26, 130: a waiting submission names what it waits on by its subject,
/// a landed one's read from the finished index, or the history's records
/// under `--all`.
#[test]
fn status_names_what_a_submission_waits_on() {
    let f = setup();
    f.world
        .fails_with("tests:t-batch-cards", "tests/t_cards.py", "broken");
    work(&f, "unit/review", "agent-r", &[("plugin/a", Some("a"))]);
    sluice(
        &f,
        &["submit", BRANCH, "--worktree", "unit/review", "--no-wait"],
    );
    give_subject(&f, 1, "Batch review cards");
    sluice(&f, &["run", "--rounds", "6"]);
    assert!(f.world.files(BRANCH).contains_key(".finished/1.json"));
    work(
        &f,
        "unit/b",
        "agent-b",
        &[("tests/t_cards.py", Some("broken"))],
    );
    assert_eq!(
        sluice(&f, &["submit", BRANCH, "--worktree", "unit/b"]).0,
        FAILED
    );
    work(&f, "unit/cards", "agent-c", &[("plugin/c", Some("c"))]);
    sluice(
        &f,
        &[
            "submit",
            BRANCH,
            "--worktree",
            "unit/cards",
            "--after",
            "2,1",
            "--no-wait",
        ],
    );
    sluice(&f, &["step", BRANCH]);
    for args in [&["status", BRANCH][..], &["status", BRANCH, "--all"]] {
        let (_, out) = sluice(&f, args);
        let row = out
            .lines()
            .find(|l| l.starts_with("  #3 "))
            .unwrap_or_else(|| panic!("{out}"));
        assert!(
            row.ends_with("   after #2 unit/b, #1 Batch review cards"),
            "{out}"
        );
    }
    let (_, out) = sluice(&f, &["status", BRANCH, "--all"]);
    assert!(
        out.lines()
            .any(|l| l.starts_with("  #1 Batch review cards  landed ")),
        "{out}"
    );
}

/// ADR 0036: the branches in the order the default branch's policy writes
/// their tables, read at its own revision when it has no records; every
/// other branch is still gated by its own policy (rule core.15).
#[test]
fn overview_orders_branches_as_the_policy_names_them() {
    let f = Fakes::new();
    // The repository's HEAD names release before bolt/*; the bolt branches'
    // own policies name them the other way round.
    f.world.init(
        "main",
        &[(
            ".config/sluice.toml",
            "[branches.release]\nrequired = []\n[branches.\"bolt/*\"]\nrequired = []\n",
        )],
    );
    let theirs = "[branches.\"bolt/*\"]\nrequired = []\n[branches.release]\nrequired = []\n";
    for b in ["bolt/b", "release", "bolt/a"] {
        f.world.init(b, &[(".config/sluice.toml", theirs)]);
        let head = f.world.work(&f.world.tip(b), &[("work", Some(b))]);
        let worktree = format!("w/{b}");
        f.world.session(&worktree, "agent", &head);
        let (code, _) = sluice(&f, &["submit", b, "--worktree", &worktree, "--no-wait"]);
        assert_eq!(code, 0, "submitting to {b}");
    }

    let (_, json) = sluice(&f, &["overview", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let order: Vec<&str> = v["branches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["branch"].as_str().unwrap())
        .collect();
    assert_eq!(order, ["release", "bolt/a", "bolt/b"]);
    let (_, text) = sluice(&f, &["overview"]);
    let heads: Vec<&str> = text
        .lines()
        .filter(|l| !l.starts_with(' '))
        .map(|l| l.split(' ').next().unwrap())
        .collect();
    assert_eq!(heads, ["release", "bolt/a", "bolt/b"]);
}

/// ADR 0036, 56, 129: the default branch's policy orders the overview at
/// the revision its view reads it at, the one this host holds, and not at
/// the revision its records last saw, never fetching (ADR 0046 as amended
/// 2026-11-06; rule core.15).
#[test]
fn overview_orders_branches_by_the_revision_this_host_holds() {
    let f = Fakes::new();
    // The revision main's records last saw names release before bolt/*;
    // the one this host holds names them the other way round.
    let saw = f.world.init(
        "main",
        &[(
            POLICY_PATH,
            "[branches.main]\nrequired = []\n[branches.release]\nrequired = []\n\
             [branches.\"bolt/*\"]\nrequired = []\n",
        )],
    );
    let theirs = "[branches.\"bolt/*\"]\nrequired = []\n[branches.release]\nrequired = []\n";
    for b in ["release", "bolt/a"] {
        f.world.init(b, &[(POLICY_PATH, theirs)]);
    }
    for b in ["main", "release", "bolt/a"] {
        let head = f.world.work(&f.world.tip(b), &[("work", Some(b))]);
        let worktree = format!("w/{b}");
        f.world.session(&worktree, "agent", &head);
        let (code, out) = sluice(&f, &["submit", b, "--worktree", &worktree, "--no-wait"]);
        assert_eq!(code, 0, "submitting to {b}: {out}");
    }
    {
        let _lock = f.state.lock("main").unwrap().unwrap();
        let read = f.state.read("main").unwrap();
        let mut doc: serde_json::Value =
            serde_json::from_slice(&read.files["branch.json"]).unwrap();
        doc["base"] = saw.0.clone().into();
        let mut put = std::collections::BTreeMap::new();
        put.insert("branch.json".to_string(), serde_json::to_vec(&doc).unwrap());
        let changes = Changes {
            put,
            ..Changes::default()
        };
        f.state.write("main", read.version, changes).unwrap();
    }
    let holds = f.world.work(
        &saw,
        &[(
            POLICY_PATH,
            Some(
                "[branches.main]\nrequired = []\n[branches.\"bolt/*\"]\nrequired = []\n\
                 [branches.release]\nrequired = []\n",
            ),
        )],
    );
    f.world.set_branch("main", &holds);

    let (code, json) = sluice(&f, &["overview", "--json"]);
    assert_eq!(code, 0, "{json}");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let branches = v["branches"].as_array().unwrap();
    let main = &branches[0];
    assert_eq!(main["branch"], "main", "{json}");
    assert_eq!(main["at"], saw.0.as_str(), "{json}");
    assert_eq!(main["revision"], holds.0.as_str(), "{json}");
    let order: Vec<&str> = branches
        .iter()
        .map(|b| b["branch"].as_str().unwrap())
        .collect();
    assert_eq!(order, ["main", "bolt/a", "release"]);
    let (_, text) = sluice(&f, &["overview"]);
    let heads: Vec<&str> = text
        .lines()
        .filter(|l| !l.starts_with(' '))
        .map(|l| l.split(' ').next().unwrap())
        .collect();
    assert_eq!(heads, ["main", "bolt/a", "release"]);
}

/// 26 (129, 133): a branch whose policy the overview could not read is
/// shown with the refusal, in the words it was refused with, and hides no
/// other branch; nothing of it is read from the policy it was refused.
#[test]
fn overview_shows_a_branch_whose_policy_is_refused_and_every_other() {
    let f = setup();
    three_submissions(&f);
    // A bolt with records, which last saw a revision whose policy the
    // core has since come to refuse.
    let stale = "bolt/stale";
    let policy = "[branches.\"bolt/*\"]\nrequired = []\n";
    f.world.init(stale, &[(".config/sluice.toml", policy)]);
    let head = f.world.work(&f.world.tip(stale), &[("work", Some("w"))]);
    f.world.session("w/stale", "agent", &head);
    let (code, _) = sluice(&f, &["submit", stale, "--worktree", "w/stale", "--no-wait"]);
    assert_eq!(code, 0);
    let cached =
        format!("{policy}[[checkout.cache]]\nkey = [\"Cargo.lock\"]\npaths = [\"target\"]\n");
    let at = f.world.work(
        &f.world.tip(stale),
        &[(".config/sluice.toml", Some(cached.as_str()))],
    );
    f.world.set_branch(stale, &at);
    {
        let _lock = f.state.lock(stale).unwrap().unwrap();
        let read = f.state.read(stale).unwrap();
        let mut doc: serde_json::Value =
            serde_json::from_slice(&read.files["branch.json"]).unwrap();
        doc["base"] = at.0.clone().into();
        let mut put = std::collections::BTreeMap::new();
        put.insert("branch.json".to_string(), serde_json::to_vec(&doc).unwrap());
        let changes = Changes {
            put,
            ..Changes::default()
        };
        f.state.write(stale, read.version, changes).unwrap();
    }

    let (code, json) = sluice(&f, &["overview", "--json"]);
    assert_eq!(code, 0, "{json}");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let branches = v["branches"].as_array().unwrap();
    let names: Vec<&str> = branches
        .iter()
        .map(|b| b["branch"].as_str().unwrap())
        .collect();
    assert_eq!(names, [BRANCH, stale]);
    let read = &branches[0];
    assert!(read.get("unread").is_none(), "{json}");
    assert_eq!(read["depth"], 1);
    assert_eq!(read["parallel"], 4);
    let refused = &branches[1];
    let words = refused["unread"].as_str().unwrap();
    assert!(
        words.starts_with(&format!("{stale}'s policy at {}: ", at.0))
            && words.contains("[checkout] declares a cache")
            && words.contains("Remove [[checkout.cache]] from .config/sluice.toml"),
        "{words}"
    );
    assert!(refused["parallel"].is_null(), "{json}");
    assert_eq!(refused["candidates"], serde_json::json!([]));

    let (code, text) = sluice(&f, &["overview"]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.starts_with(&format!(
            "{BRANCH} at {} · 1 in the queue",
            f.world.tip(BRANCH).short()
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!("\n{stale} could not be read: {words}\n")),
        "{text}"
    );

    // The home view counts the same branches, the refused one with its
    // refusal.
    let v = home(&f, "unit/forecast");
    let counts = v["branches"].as_array().unwrap();
    assert_eq!(counts[0]["depth"], 1);
    assert!(counts[0].get("unread").is_none(), "{v}");
    assert_eq!(counts[1]["unread"], words);
    let (code, text) = sluice(&f, &["--worktree", "unit/forecast"]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains(&format!("\n{stale} could not be read: {words}\n")),
        "{text}"
    );
}

#[test]
fn overview_of_a_repository_with_no_records_says_so() {
    let f = setup();
    let (code, text) = sluice(&f, &["overview"]);
    assert_eq!(code, 0);
    assert_eq!(text, "no branch has records here\n");
    let (_, json) = sluice(&f, &["overview", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["branches"], serde_json::json!([]));
}

/// A pause that does `between(n)` before the `n`th wait, and stops after
/// `rounds` of them.
struct Between<'a> {
    n: Cell<usize>,
    rounds: usize,
    between: &'a dyn Fn(usize),
}

impl Pause for Between<'_> {
    fn pause(&self) -> bool {
        let n = self.n.get() + 1;
        self.n.set(n);
        if n > self.rounds {
            return false;
        }
        (self.between)(n);
        true
    }
}

#[test]
fn overview_watch_prints_a_line_at_start_and_on_each_change() {
    let f = setup();
    // The host process's passes start its combination, which stays.
    f.world.hold_runs(true);
    work(
        &f,
        "unit/touches",
        "crw-1-unit-1",
        &[("plugin/lib/plan.py", Some("touched"))],
    );
    let between = |n: usize| {
        if n == 2 {
            sluice(
                &f,
                &["submit", BRANCH, "--worktree", "unit/touches", "--no-wait"],
            );
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 4,
        between: &between,
    };
    let cli = Cli::try_parse_from(["sluice", "overview", "--json", "--watch"]).unwrap();
    let mut out = Vec::new();
    let code = run(
        cli,
        f.seams(),
        &support::FakeHost::new(f.world.clone(), 0),
        &(),
        &(),
        &f.clock,
        &pause,
        &mut out,
    )
    .unwrap();
    assert_eq!(code, sluice::STOPPED);
    let out = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    // At the start, then once for the submission; the fake clock moves on
    // at every read, and the time alone prints nothing.
    assert_eq!(lines.len(), 2, "{out}");
    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(first["branches"], serde_json::json!([]));
    assert_eq!(second["branches"][0]["depth"], 1);
}

#[test]
fn watch_needs_json() {
    assert!(Cli::try_parse_from(["sluice", "overview", "--watch"]).is_err());
}

// ---------------------------------------------------------------------------
// One reader of the repository's queues on the host (129; ADR 0056)
// ---------------------------------------------------------------------------

/// The repository the fakes' host names.
const REPOSITORY: &str = "/fake/repo.git";

/// One of `repository`'s reader's files on the fakes' host.
fn readers_file(repository: &str, ext: &str) -> std::path::PathBuf {
    let name = sluice::runs::flat_name(repository);
    support::readers().join(format!("{name}.{ext}"))
}

/// `repository`'s reader's document, as it stands.
fn readers_doc_of(repository: &str) -> Option<serde_json::Value> {
    let bytes = std::fs::read(readers_file(repository, "json")).ok()?;
    Some(serde_json::from_slice(&bytes).unwrap())
}

fn readers_doc() -> Option<serde_json::Value> {
    readers_doc_of(REPOSITORY)
}

/// A live reader of `repository`, the test's own, until dropped; its
/// document `doc`.
fn reader_of(repository: &str, doc: &serde_json::Value) -> sluice::reader::Lead {
    match sluice::reader::Reader::take(&support::readers(), repository, sluice::VERSION).unwrap() {
        sluice::reader::Reader::Lead(mut lead) => {
            lead.write(doc).unwrap();
            lead
        }
        sluice::reader::Reader::Follow(_) => panic!("a reader of {repository} already"),
    }
}

/// Replace the reader's document with `doc`, as it is, time and all.
fn rewrite_readers_doc(doc: &serde_json::Value) {
    std::fs::write(readers_file(REPOSITORY, "json"), doc.to_string()).unwrap();
}

/// The overview as a reader read it of queues no test's records hold:
/// `#7` under test on the branch.
fn seven_testing() -> serde_json::Value {
    let g = setup();
    g.world.hold_runs(true);
    work(&g, "unit/r", "agent-r", &[("plugin/r", Some("r"))]);
    sluice(&g, &["submit", BRANCH, "--worktree", "unit/r", "--no-wait"]);
    sluice(&g, &["step", BRANCH]);
    let (_, out) = sluice(&g, &["overview", "--json"]);
    let mut doc = json(&out);
    let b = &mut doc["branches"][0];
    assert_eq!(b["submissions"][0]["state"], "testing", "{b}");
    b["submissions"][0]["id"] = 7.into();
    b["submissions"][0]["tested"]["submissions"] = serde_json::json!([7]);
    b["candidates"][0]["submissions"] = serde_json::json!([7]);
    doc
}

/// 129: a watch that finds no reader becomes it, and writes each document
/// it prints, whole: at its start, and once the submission is queued.
#[test]
fn watch_with_no_reader_writes_each_document() {
    let f = setup();
    // The host process's passes start its combination, which stays.
    f.world.hold_runs(true);
    work(
        &f,
        "unit/touches",
        "crw-1-unit-1",
        &[("plugin/lib/plan.py", Some("touched"))],
    );
    let written = RefCell::new(Vec::new());
    let between = |n: usize| {
        written.borrow_mut().push(readers_doc());
        if n == 2 {
            sluice(
                &f,
                &["submit", BRANCH, "--worktree", "unit/touches", "--no-wait"],
            );
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 4,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, sluice::STOPPED);
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines.len(), 2, "{out}");
    let written = written.into_inner();
    assert_eq!(written[0].as_ref(), Some(&lines[0]));
    assert_eq!(written[2].as_ref(), Some(&lines[1]));
    assert_eq!(lines[1]["schema"], "sluice/overview@1");
    assert_eq!(lines[1]["branches"][0]["depth"], 1);
}

/// 129: a watch that finds a live reader follows its document: what it
/// prints is what the reader wrote, which no records here hold.
#[test]
fn watch_follows_the_readers_document() {
    let f = setup();
    let doc = seven_testing();
    let _reader = reader_of(REPOSITORY, &doc);
    let pause = Between {
        n: Cell::new(0),
        rounds: 2,
        between: &|_| {},
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, sluice::STOPPED);
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines, [doc], "{out}");
}

/// 129: a follower prints the reader's document again each time it
/// changes, and never for its time alone.
#[test]
fn watch_follows_each_change_once() {
    let f = setup();
    let testing = seven_testing();
    let mut landed = testing.clone();
    let b = &mut landed["branches"][0];
    b["depth"] = 0.into();
    b["submissions"] = serde_json::json!([]);
    b["candidates"] = serde_json::json!([]);
    b["recent"] = serde_json::json!([{
        "at": "2026-10-11T09:05:00Z", "event": "landed", "ids": [7], "to": "f00d",
    }]);
    let mut later = landed.clone();
    later["at"] = "2026-10-11T09:10:00Z".into();
    let _reader = reader_of(REPOSITORY, &testing);
    let between = |n: usize| match n {
        1 => rewrite_readers_doc(&landed),
        2 => rewrite_readers_doc(&later),
        _ => {}
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 3,
        between: &between,
    };
    let (_, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines, [testing, landed], "{out}");
}

/// 129: once the reader stops, a follower is the reader within one pause,
/// and the document it writes is the one it read from the records.
#[test]
fn watch_leads_once_the_reader_stops() {
    let f = setup();
    let doc = seven_testing();
    let reader = RefCell::new(Some(reader_of(REPOSITORY, &doc)));
    let written = RefCell::new(None);
    let between = |n: usize| match n {
        1 => drop(reader.borrow_mut().take()),
        2 => *written.borrow_mut() = readers_doc(),
        _ => {}
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 3,
        between: &between,
    };
    let (_, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert_eq!(lines[0], doc);
    assert_eq!(lines[1]["branches"], serde_json::json!([]));
    assert_eq!(written.into_inner().as_ref(), Some(&lines[1]));
}

/// 129: a reader serves its own repository alone: a watch of another
/// repository on the host is that one's reader.
#[test]
fn watch_of_another_repository_leads() {
    let f = setup();
    let other = "/fake/other.git";
    let doc = seven_testing();
    let _reader = reader_of(other, &doc);
    let pause = Between {
        n: Cell::new(0),
        rounds: 1,
        between: &|_| {},
    };
    let (_, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines.len(), 1, "{out}");
    assert_eq!(lines[0]["branches"], serde_json::json!([]));
    assert_eq!(readers_doc().as_ref(), Some(&lines[0]));
    assert_eq!(readers_doc_of(other), Some(doc));
}

/// 129: a one-shot overview reads the records itself, beside a live
/// reader or with none, and never takes the reader's place.
#[test]
fn one_shot_overview_reads_the_records_beside_a_reader() {
    let f = setup();
    sluice(&f, &["overview", "--json"]);
    assert!(!readers_file(REPOSITORY, "lock").exists());
    let doc = seven_testing();
    let _reader = reader_of(REPOSITORY, &doc);
    let (code, out) = sluice(&f, &["overview", "--json"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(json(&out)["branches"], serde_json::json!([]));
    let (_, text) = sluice(&f, &["overview"]);
    assert_eq!(text, "no branch has records here\n");
    assert_eq!(readers_doc(), Some(doc));
}

/// The overview as a reader read it of queues no test's records hold:
/// `#1` under test on the branch from a head of its own, `done` of its
/// five tasks ended.
fn one_testing(done: usize) -> serde_json::Value {
    use sluice_seams::{TaskProgress, TaskState};
    let g = setup();
    g.world.hold_runs(true);
    work(&g, "unit/r", "agent-r", &[("plugin/r", Some("r"))]);
    sluice(&g, &["submit", BRANCH, "--worktree", "unit/r", "--no-wait"]);
    sluice(&g, &["step", BRANCH]);
    let tasks: Vec<TaskProgress> = (0..5)
        .map(|i| {
            let state = match i {
                i if i < done => TaskState::Passed,
                i if i == done => TaskState::Running,
                _ => TaskState::Waiting,
            };
            TaskProgress {
                target: format!("t{i}:test"),
                state,
                started: None,
                ended: None,
                exit: state.ended().then_some(0),
            }
        })
        .collect();
    g.world.says_tasks(&tested(&g, 1), &tasks);
    // A round keeps what the runner says beside the records.
    sluice(&g, &["step", BRANCH]);
    let (_, out) = sluice(&g, &["overview", "--json"]);
    json(&out)
}

/// overview-reader, "A wait with no reader" (129, 135): the host process
/// the wait reaches is the reader, and writes the whole overview, every
/// branch's, not only the wait's own.
#[test]
fn a_wait_with_no_reader_has_the_host_process_write_the_whole_overview() {
    let f = setup();
    let other = "bolt/other";
    f.world.init(other, &[(".config/sluice.toml", POLICY)]);
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    let head = f
        .world
        .work(&f.world.tip(other), &[("plugin/b", Some("b"))]);
    f.world.session("unit/b", "agent-b", &head);
    f.world.hold_runs(true);
    sluice(&f, &["submit", other, "--worktree", "unit/b", "--no-wait"]);
    f.world.hold_runs(false);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    let (code, out) = sluice(&f, &["wait", "2"]);
    assert_eq!(code, LANDED, "{out}");
    let doc = readers_doc().expect("the reader's document");
    assert_eq!(doc["schema"], "sluice/overview@1");
    let mut branches: Vec<&str> = doc["branches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["branch"].as_str().unwrap())
        .collect();
    branches.sort();
    assert_eq!(branches, [BRANCH, other], "{doc}");
}

/// 129: a wait that finds a live reader says its combination's progress
/// from the reader's document, which no records here hold.
#[test]
fn wait_follows_the_readers_progress() {
    let f = setup();
    f.world.hold_runs(true);
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    let _reader = reader_of(REPOSITORY, &one_testing(3));
    let pause = Between {
        n: Cell::new(0),
        rounds: 2,
        between: &|_| {},
    };
    let (code, out) = sluice_with(&f, &pause, &["wait", "1"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert!(
        lines[0].starts_with(&format!(
            "testing {BRANCH} + #1 · 3/5 tasks · running t3:test"
        )),
        "{out}"
    );
    assert_eq!(lines[1], "still waiting: sluice wait 1");
}

/// 129: out of time, a follower's `--json` is its record as the reader's
/// document holds it.
#[test]
fn wait_out_of_time_prints_the_readers_record() {
    let f = setup();
    f.world.hold_runs(true);
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    let doc = one_testing(3);
    let _reader = reader_of(REPOSITORY, &doc);
    let (code, out) = sluice(&f, &["wait", "1", "--timeout", "2s", "--json"]);
    assert_eq!(code, sluice::WAITING, "{out}");
    let held = &doc["branches"][0]["submissions"][0];
    assert_eq!(json(&out), *held);
    assert_eq!(held["state"], "testing");
    let (_, out) = sluice(&f, &["status", "1", "--json"]);
    assert_ne!(json(&out)["head"], held["head"]);
}

/// 129: once the reader's document no longer holds a follower's
/// submission, the wait reads its record once and says how it ended.
#[test]
fn followed_wait_reports_its_landing() {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    let (_, queued) = sluice(&f, &["overview", "--json"]);
    let reader = RefCell::new(reader_of(REPOSITORY, &json(&queued)));
    let between = |n: usize| {
        if n == 3 {
            // The reader's rounds land it, and its document says so.
            for _ in 0..8 {
                f.sluice().step(BRANCH).unwrap();
            }
            let (_, now) = sluice(&f, &["overview", "--json"]);
            let now = json(&now);
            assert_eq!(now["branches"][0]["submissions"], serde_json::json!([]));
            reader.borrow_mut().write(&now).unwrap();
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 10,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["wait", "1"]);
    assert_eq!(code, LANDED, "{out}");
    // It waited on the document, and ran no round of its own.
    assert_eq!(pause.n.get(), 3, "{out}");
    let tip = f.world.tip(BRANCH);
    assert!(
        out.ends_with(&format!("passed · landed as {}\n", &tip.0[..7])),
        "{out}"
    );
}

/// 129: a follower's submission withdrawn ends its wait as a wait that
/// read the records would.
#[test]
fn followed_wait_reports_its_withdrawal() {
    let f = setup();
    f.world.hold_runs(true);
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    let (_, queued) = sluice(&f, &["overview", "--json"]);
    let reader = RefCell::new(reader_of(REPOSITORY, &json(&queued)));
    let between = |n: usize| match n {
        1 => {
            sluice(&f, &["withdraw", "1"]);
        }
        2 => {
            let (_, now) = sluice(&f, &["overview", "--json"]);
            reader.borrow_mut().write(&json(&now)).unwrap();
        }
        _ => {}
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 10,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["wait", "1", "--json"]);
    assert_eq!(code, sluice::WITHDRAWN, "{out}");
    assert_eq!(json(&out)["state"], "withdrawn");
    assert_eq!(pause.n.get(), 2, "{out}");
}

/// overview-reader, "The host process is killed" (129; design D7): a
/// watch whose host process ended reaches it again at its next tick,
/// starting it, and goes on following the last document until the new one
/// writes the one it read from the records.
#[test]
fn a_watch_follows_a_host_process_started_again() {
    let f = one_queued();
    f.world.hold_runs(true);
    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    let between = |n: usize| {
        if n == 1 {
            support::end_host_process();
            sluice(&f, &["submit", BRANCH, "--worktree", "unit/b", "--no-wait"]);
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 3,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    // The first process's document, which stood while none wrote; then the
    // one the process started again wrote.
    assert_eq!(lines.len(), 2, "{out}");
    let queued = |doc: &serde_json::Value| {
        doc["branches"][0]["submissions"]
            .as_array()
            .map_or(0, Vec::len)
    };
    assert_eq!(queued(&lines[0]), 1, "{out}");
    assert_eq!(queued(&lines[1]), 2, "{out}");
    assert_eq!(readers_doc().map(|d| queued(&d)), Some(2));
}

/// The repository's reader's part as a sluice of `version`, the test's
/// own: another process on the host, until dropped.
fn reader_as(version: &str) -> sluice::reader::Reader {
    sluice::reader::Reader::take(&support::readers(), REPOSITORY, version).unwrap()
}

fn is_reader(r: &sluice::reader::Reader) -> bool {
    matches!(r, sluice::reader::Reader::Lead(_))
}

/// Write `doc` as `r`, the reader.
fn write_as(r: &mut sluice::reader::Reader, doc: &serde_json::Value) {
    match r {
        sluice::reader::Reader::Lead(lead) => lead.write(doc).unwrap(),
        sluice::reader::Reader::Follow(_) => panic!("a follower, where it should read"),
    }
}

/// Whether `r`, stepped as a newer watch steps, is as `done` says within
/// two seconds: the lease or the lock it takes may be held for a moment
/// by a process another thread of this binary starts, until it runs its
/// program, as [`soon`] allows; a watch tries again at its next tick.
fn soon_stepped(
    r: &mut sluice::reader::Reader,
    done: impl Fn(&sluice::reader::Reader) -> bool,
) -> bool {
    soon(|| {
        r.step(true, sluice::reader::Drives::Nobody).unwrap();
        done(r)
    })
}

/// Whether a process holds the repository's reader's lease: asked shared,
/// as the reader asks it, so a probe's lock a process started meanwhile
/// still holds is none (129).
fn leased() -> bool {
    let lease = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(readers_file(REPOSITORY, "lease"))
        .unwrap();
    match lease.try_lock_shared() {
        Ok(()) => false,
        Err(std::fs::TryLockError::WouldBlock) => true,
        Err(e) => panic!("locking the lease: {e}"),
    }
}

/// Whether the repository's reader's lease is free by `until`: a process
/// another test of this binary forks while a watch holds it holds it too,
/// until it runs its program.
fn lease_free_by(until: Instant) -> bool {
    while leased() {
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

/// The version of sluice the repository's reader names.
fn readers_version() -> Option<String> {
    let bytes = std::fs::read(readers_file(REPOSITORY, "version")).ok()?;
    let named: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    named["version"].as_str().map(String::from)
}

/// 129: a watch of a newer sluice than the live reader's follows it, takes
/// its lease at its next tick, and is the reader at a tick of its own once
/// the older reader has let go, naming its version. The tick is the next
/// one but for a process another test of this binary forks meanwhile,
/// which holds the older reader's lock, or the watch's lease, until it
/// runs its program, for as long as a loaded host keeps it from that: the
/// watch then leads at a later tick. Once the older reader has let go,
/// the test waits a little before each tick at which the watch has not
/// led yet, for at most a minute.
#[test]
fn a_newer_watch_takes_an_older_readers_lease() {
    struct Ticks<F: Fn(usize) -> bool>(Cell<usize>, F);

    impl<F: Fn(usize) -> bool> Pause for Ticks<F> {
        fn pause(&self) -> bool {
            self.0.set(self.0.get() + 1);
            (self.1)(self.0.get())
        }
    }

    let f = setup();
    let doc = seven_testing();
    let older = RefCell::new(reader_as("0.52.0"));
    write_as(&mut older.borrow_mut(), &doc);
    support::set_version("0.53.0");
    let until = Instant::now() + Duration::from_secs(60);
    let read_at = Cell::new(None);
    let written = RefCell::new(None);
    let between = |n: usize| {
        match n {
            1 => {}
            2 => {
                assert!(leased(), "the watch took no lease");
                let mut older = older.borrow_mut();
                older.step(true, sluice::reader::Drives::Nobody).unwrap();
                assert!(!is_reader(&older), "the older reader kept reading");
            }
            _ if read_at.get().is_none() => {
                if readers_version().as_deref() != Some("0.53.0") {
                    assert!(leased(), "the watch let its lease go before it read");
                    assert!(Instant::now() < until, "the watch never led");
                    std::thread::sleep(Duration::from_millis(20));
                    return true;
                }
                read_at.set(Some(n));
                *written.borrow_mut() = readers_doc();
                assert!(lease_free_by(until), "the lease kept once the watch reads");
                older
                    .borrow_mut()
                    .step(true, sluice::reader::Drives::Nobody)
                    .unwrap();
                assert!(!is_reader(&older.borrow()));
            }
            _ => {}
        }
        // Three ticks more once the watch has read, which print nothing.
        read_at.get().is_none_or(|at| n <= at + 3)
    };
    let (_, out) = sluice_with(
        &f,
        &Ticks(Cell::new(0), between),
        &["overview", "--json", "--watch"],
    );
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert_eq!(lines[0], doc);
    // Read from the records, which hold no submission.
    assert_eq!(lines[1]["branches"], serde_json::json!([]), "{out}");
    let written = written.into_inner();
    assert!(written.is_some(), "the watch never read: {out}");
    assert_eq!(written.as_ref(), Some(&lines[1]));
}

/// 129: a watch of the live reader's own version follows it for as long as
/// it runs, and never takes its lease.
#[test]
fn a_watch_of_the_same_version_follows() {
    let f = setup();
    let doc = seven_testing();
    let reader = RefCell::new(reader_as("0.53.0"));
    write_as(&mut reader.borrow_mut(), &doc);
    support::set_version("0.53.0");
    let between = |_: usize| {
        assert!(!leased(), "the lease of a reader of its own version");
        let mut reader = reader.borrow_mut();
        reader.step(true, sluice::reader::Drives::Nobody).unwrap();
        assert!(is_reader(&reader));
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 3,
        between: &between,
    };
    let (_, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines, std::slice::from_ref(&doc), "{out}");
    assert_eq!(readers_doc(), Some(doc));
}

/// 129: once a newer watch has taken its lease, the older one that is the
/// reader lets go at its next tick and reads nothing more itself: the line
/// it prints with `#7` queued is the document the newer one wrote.
#[test]
fn an_older_watch_follows_once_its_lease_is_taken() {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    let seven = seven_testing();
    support::set_version("0.52.0");
    let newer = RefCell::new(None);
    let at_lease = RefCell::new(None);
    let between = |n: usize| match n {
        1 => {
            let mut r = reader_as("0.53.0");
            assert!(!is_reader(&r));
            assert!(
                soon_stepped(&mut r, |_| leased()),
                "no lease taken of an older reader"
            );
            *at_lease.borrow_mut() = readers_doc();
            // What the older watch would write, were it to read again.
            sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
            *newer.borrow_mut() = Some(r);
        }
        2 => {
            assert_eq!(readers_doc(), *at_lease.borrow(), "written after its lease");
            let mut newer = newer.borrow_mut();
            let r = newer.as_mut().unwrap();
            assert!(soon_stepped(r, is_reader), "the older reader's lock kept");
            write_as(r, &seven);
            assert!(soon(|| !leased()));
        }
        _ => {}
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 2,
        between: &between,
    };
    let (_, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert_eq!(lines[0]["branches"], serde_json::json!([]), "{out}");
    assert_eq!(lines[1], seven);
    assert_eq!(readers_version().as_deref(), Some("0.53.0"));
}

/// overview-reader, "An older wait lets go" (129; design D7): a wait of an
/// older sluice that is the reader lets go once the host process takes its
/// lease, and the newer wait follows the document the host process writes
/// from then on, saying its combination's progress from it.
#[test]
fn an_older_wait_follows_once_the_host_process_takes_its_lease() {
    let f = setup();
    f.world.hold_runs(true);
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    // The older wait, reading, before any host process; the host process
    // and the wait are a newer sluice.
    let older = RefCell::new(reader_as("0.52.0"));
    support::set_version("0.53.0");
    assert!(is_reader(&older.borrow()));
    let between = |n: usize| match n {
        1 => {
            assert!(leased(), "the host process took the older reader's lease");
            assert_eq!(
                readers_version().as_deref(),
                Some("0.52.0"),
                "it reads nothing before the older reader lets go"
            );
            // The older wait's next tick: it finds its lease taken.
            older
                .borrow_mut()
                .step(false, sluice::reader::Drives::Nobody)
                .unwrap();
            assert!(!is_reader(&older.borrow()));
        }
        2 => {
            assert_eq!(readers_version().as_deref(), Some("0.53.0"));
        }
        _ => {}
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 3,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["wait", "1"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    assert!(
        out.lines()
            .any(|l| l.starts_with(&format!("testing {BRANCH} + #1 · "))),
        "{out}"
    );
    assert!(out.ends_with("still waiting: sluice wait 1\n"), "{out}");
    assert_eq!(readers_version().as_deref(), Some("0.53.0"));
}

// ---------------------------------------------------------------------------
// A read the reader could not make (129, 133)
// ---------------------------------------------------------------------------

/// The words git fails to read the records with on a loaded host.
const UNREADABLE: &str = "fatal: unable to read 4b825dc6: Resource temporarily unavailable";

/// Queues whose records a watch reads at each tick: submission 1, queued.
fn one_queued() -> Fakes {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    f
}

/// The error document a failing overview prints, with `words`.
fn failure(words: &str) -> serde_json::Value {
    sluice::told::document(sluice::told::Kind::Internal, words, &[])
}

/// `doc` is the error document a failing overview prints, its message
/// holding `words` whole (133).
fn assert_failure(doc: &serde_json::Value, words: &str) {
    assert_eq!(doc["error"]["kind"], "internal", "{doc}");
    let message = doc["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains(words), "{doc}");
    assert_eq!(doc["error"]["next"], serde_json::json!([]), "{doc}");
}

/// 129: a watch that is the reader and cannot list the records writes the
/// failure as the reader's document, and the overview once it reads it.
#[test]
fn a_watchs_read_fails_and_then_succeeds() {
    let f = one_queued();
    // The host process's passes start its combination, which stays.
    f.world.hold_runs(true);
    let written = RefCell::new(Vec::new());
    let between = |n: usize| {
        written
            .borrow_mut()
            .push(readers_doc().expect("a document"));
        match n {
            1 => f.world.fail_state_list(Some(UNREADABLE)),
            2 => f.world.fail_state_list(None),
            _ => {}
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 3,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    let written = written.into_inner();
    assert_eq!(written[0]["branches"][0]["depth"], 1, "{}", written[0]);
    assert_failure(&written[1], UNREADABLE);
    assert_eq!(written[2]["schema"], "sluice/overview@1", "{}", written[2]);
    assert_eq!(written[2]["branches"][0]["depth"], 1, "{}", written[2]);
}

/// 129: a watch whose read fails prints the failure as a line and keeps
/// watching: the overview it reads next is printed again, unchanged as it
/// is from the first.
#[test]
fn a_failed_read_under_load() {
    let f = one_queued();
    // The host process's passes start its combination, which stays.
    f.world.hold_runs(true);
    let between = |n: usize| match n {
        1 => f.world.fail_state_list(Some(UNREADABLE)),
        2 => f.world.fail_state_list(None),
        _ => {}
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 4,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines.len(), 3, "{out}");
    assert_failure(&lines[1], UNREADABLE);
    let untimed = sluice::reader::untimed;
    assert_eq!(untimed(&lines[2]), untimed(&lines[0]), "{out}");
}

/// 129: a failure that stands with the same words is written once by the
/// host process: what the test put in its place after the first stands,
/// and the watch, which prints what the document holds, printed the
/// failure once.
#[test]
fn a_standing_failure_is_written_once() {
    let f = one_queued();
    // The host process's passes start its combination, which stays.
    f.world.hold_runs(true);
    let put = serde_json::json!({ "put": "by the test" });
    let between = |n: usize| match n {
        1 => f.world.fail_state_list(Some(UNREADABLE)),
        2 => {
            assert_failure(&readers_doc().expect("a document"), UNREADABLE);
            rewrite_readers_doc(&put);
        }
        _ => {}
    };
    // Five ticks after the first fail.
    let pause = Between {
        n: Cell::new(0),
        rounds: 5,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    assert_eq!(pause.n.get(), 6, "{out}");
    assert_eq!(readers_doc(), Some(put.clone()));
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines.len(), 3, "{out}");
    assert_failure(&lines[1], UNREADABLE);
    assert_eq!(lines[2], put, "{out}");
}

/// 129: a watch whose repository is gone ends at its next read, as a
/// failing overview does, with the read's words.
#[test]
fn a_watch_ends_once_its_repository_is_removed() {
    let f = one_queued();
    let gone = "fatal: not a git repository: '/fake/repo.git'";
    let between = |n: usize| {
        if n == 1 {
            f.world.repository_gone(true);
            f.world.fail_state_list(Some(gone));
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 5,
        between: &between,
    };
    let cli = Cli::try_parse_from(["sluice", "overview", "--json", "--watch"]).unwrap();
    let mut out = Vec::new();
    let host = support::FakeHost::new(f.world.clone(), 0);
    let e = run(cli, f.seams(), &host, &(), &(), &f.clock, &pause, &mut out).unwrap_err();
    // `main` prints it as the error document and exits 3.
    assert_failure(&failure(&format!("{e:#}")), gone);
    assert_eq!(pause.n.get(), 1);
    let out = String::from_utf8(out).unwrap();
    assert_eq!(out.lines().count(), 1, "{out}");
}

/// 129: one write of the reader's document that fails ends no watch: it
/// prints what it read, and writes it at its next tick.
#[test]
fn a_watch_survives_a_failed_write_of_the_readers_document() {
    let f = one_queued();
    // The host process's passes start its combination, which stays.
    f.world.hold_runs(true);
    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    let name = sluice::runs::flat_name(REPOSITORY);
    // Where the document is written before it is renamed into place: a
    // directory there fails the write.
    let tmp = support::readers().join(format!(".{name}.tmp"));
    let written = RefCell::new(Vec::new());
    let between = |n: usize| {
        written
            .borrow_mut()
            .push(readers_doc().expect("a document"));
        match n {
            1 => {
                std::fs::create_dir(&tmp).unwrap();
                sluice(&f, &["submit", BRANCH, "--worktree", "unit/b", "--no-wait"]);
            }
            2 => std::fs::remove_dir(&tmp).unwrap(),
            _ => {}
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 3,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert_eq!(lines[1]["branches"][0]["depth"], 2, "{out}");
    let written = written.into_inner();
    assert_eq!(written[1], written[0]);
    assert_eq!(written[2]["branches"][0]["depth"], 2, "{}", written[2]);
}

/// 129: a lock the follower could not try leaves it following, and it
/// tries the lock again at its next tick.
#[test]
fn a_watch_survives_a_failed_try_of_the_readers_lock() {
    let f = setup();
    let doc = seven_testing();
    let reader = RefCell::new(Some(reader_of(REPOSITORY, &doc)));
    let lock = readers_file(REPOSITORY, "lock");
    let between = |n: usize| match n {
        // The reader stops, and the lock cannot be opened.
        1 => {
            drop(reader.borrow_mut().take());
            std::fs::remove_file(&lock).unwrap();
            std::fs::create_dir(&lock).unwrap();
        }
        2 => std::fs::remove_dir(&lock).unwrap(),
        _ => {}
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 3,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert_eq!(lines[0], doc);
    assert_eq!(lines[1]["branches"], serde_json::json!([]), "{out}");
    assert_eq!(readers_doc().as_ref(), Some(&lines[1]));
}

/// 129: a follower prints the reader's failure once, as any document the
/// reader writes, and the overview after it.
#[test]
fn a_follower_prints_the_readers_failure() {
    let f = setup();
    let doc = seven_testing();
    let _reader = reader_of(REPOSITORY, &doc);
    let between = |n: usize| match n {
        1 => rewrite_readers_doc(&failure("HTTP 503")),
        3 => rewrite_readers_doc(&doc),
        _ => {}
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 4,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines, [doc.clone(), failure("HTTP 503"), doc], "{out}");
}

/// 129: a watch stopped while a failure stands ends as a stopped watch
/// does.
#[test]
fn a_watch_stopped_while_a_failure_stands() {
    let f = one_queued();
    f.world.fail_state_list(Some(UNREADABLE));
    let pause = Between {
        n: Cell::new(0),
        rounds: 9,
        between: &|_| {},
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    assert_eq!(pause.n.get(), 10, "{out}");
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines.len(), 1, "{out}");
    assert_failure(&lines[0], UNREADABLE);
}

/// 26, 129: a wait that is the reader and cannot read another branch's
/// records writes the overview with that branch unread, in the words it
/// was refused with, once while the refusal stands, and goes on waiting on
/// its own submission.
#[test]
fn a_reader_wait_shows_a_branch_it_could_not_read() {
    let f = setup();
    f.world.hold_runs(true);
    let other = "bolt/other";
    f.world.init(other, &[(".config/sluice.toml", POLICY)]);
    for n in 1..=3 {
        let (path, worktree) = (format!("plugin/{n}"), format!("unit/{n}"));
        let head = f.world.work(&f.world.tip(other), &[(&path, Some("n"))]);
        f.world.session(&worktree, "agent", &head);
        sluice(&f, &["submit", other, "--worktree", &worktree, "--no-wait"]);
    }
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    f.world.fail_state_reads(other, Some(UNREADABLE));
    let put = serde_json::json!({ "put": "by the test" });
    let written = RefCell::new(None);
    let between = |n: usize| {
        if n == 1 {
            *written.borrow_mut() = readers_doc();
            rewrite_readers_doc(&put);
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 3,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["wait", "4"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    let doc = written.into_inner().expect("a document");
    assert_eq!(doc["schema"], "sluice/overview@1", "{doc}");
    let part = |name: &str| {
        doc["branches"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["branch"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("{name} in {doc}"))
    };
    let own = part(BRANCH);
    assert!(own.get("unread").is_none(), "{doc}");
    assert_eq!(own["submissions"][0]["id"], 4, "{doc}");
    let unread = part(other);
    assert_eq!(unread["unread"], UNREADABLE, "{doc}");
    assert_eq!(unread["submissions"], serde_json::json!([]), "{doc}");
    // Written once while the refusal stands.
    assert_eq!(readers_doc(), Some(put));
    assert!(out.ends_with("still waiting: sluice wait 4\n"), "{out}");
    let (_, status) = sluice(&f, &["status", "4", "--json"]);
    assert_eq!(json(&status)["state"], "testing");
}

/// Submissions 1 to 7 queued on the branch, each from a worktree of its
/// own.
fn seven_queued(f: &Fakes) {
    for n in 1..=7 {
        let (path, worktree) = (format!("plugin/{n}"), format!("unit/{n}"));
        work(f, &worktree, "agent", &[(&path, Some("n"))]);
        sluice(f, &["submit", BRANCH, "--worktree", &worktree, "--no-wait"]);
    }
}

/// 129: a follower wait reads nothing from the reader's failure, and no
/// record in its place: its own records, which say `#7` withdrawn, would
/// have ended it.
#[test]
fn a_follower_wait_passes_over_the_readers_failure() {
    let f = setup();
    f.world.hold_runs(true);
    seven_queued(&f);
    let doc = seven_testing();
    let _reader = reader_of(REPOSITORY, &doc);
    let between = |n: usize| {
        if n == 1 {
            sluice(&f, &["withdraw", "7"]);
            rewrite_readers_doc(&failure("HTTP 503"));
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 3,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["wait", "7"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert!(
        lines[0].starts_with(&format!("testing {BRANCH} + #7 · ")),
        "{out}"
    );
    assert_eq!(lines[1], "still waiting: sluice wait 7");
}

/// 129: out of time while the reader's document is its failure, a
/// follower's `--json` is its record as the last overview it read held it.
#[test]
fn a_follower_wait_out_of_time_on_a_failure() {
    let f = setup();
    f.world.hold_runs(true);
    seven_queued(&f);
    let doc = seven_testing();
    let _reader = reader_of(REPOSITORY, &doc);
    let step = Then(Cell::new(0), |n| {
        if n == 1 {
            sluice(&f, &["withdraw", "7"]);
            rewrite_readers_doc(&failure("HTTP 503"));
        }
        true
    });
    let (code, out) = sluice_with(&f, &step, &["wait", "7", "--timeout", "30s", "--json"]);
    assert_eq!(code, sluice::WAITING, "{out}");
    assert!(step.0.get() > 1, "{out}");
    let held = &doc["branches"][0]["submissions"][0];
    assert_eq!(json(&out), *held);
    assert_eq!(held["state"], "testing");
}

// ---------------------------------------------------------------------------
// A repository with no sluice (129)
// ---------------------------------------------------------------------------

/// A repository sluice was never set up in: `main` holds no policy, and no
/// branch has records.
fn never_set_up() -> Fakes {
    let f = Fakes::new();
    f.world.init("main", &[("README", "scratch")]);
    f
}

/// The overview a reader writes where the repository has no sluice: none
/// of its branches, and `"set_up": false`.
fn no_sluice_doc() -> serde_json::Value {
    let mut doc = seven_testing();
    doc["branches"] = serde_json::json!([]);
    doc["set_up"] = false.into();
    doc
}

/// 129: a watch where sluice was never set up prints the overview saying
/// so, writes it as the reader's document, and ends at its first tick.
#[test]
fn a_watch_ends_where_sluice_was_never_set_up() {
    let f = never_set_up();
    let pause = Between {
        n: Cell::new(0),
        rounds: 5,
        between: &|_| {},
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(pause.n.get(), 0, "{out}");
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines.len(), 1, "{out}");
    assert_eq!(lines[0]["schema"], "sluice/overview@1", "{out}");
    assert_eq!(lines[0]["branches"], serde_json::json!([]), "{out}");
    assert_eq!(lines[0]["set_up"], false, "{out}");
    assert_eq!(readers_doc().as_ref(), Some(&lines[0]));
}

/// 129: a repository set up with nothing submitted has sluice: its watch
/// goes on, and no document says otherwise.
#[test]
fn a_watch_goes_on_where_sluice_is_set_up_with_nothing_submitted() {
    let f = setup();
    let pause = Between {
        n: Cell::new(0),
        rounds: 4,
        between: &|_| {},
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    assert_eq!(pause.n.get(), 5, "{out}");
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines.len(), 1, "{out}");
    assert_eq!(lines[0]["branches"], serde_json::json!([]), "{out}");
    assert!(lines[0].get("set_up").is_none(), "{out}");
}

/// 129, rule core.27: a policy on a remote's tracking branch as last
/// fetched, and on no branch of the host's own, is sluice set up.
#[test]
fn a_watch_goes_on_where_the_policy_is_only_on_a_remotes_branch() {
    let f = never_set_up();
    let fetched = f
        .world
        .work(&f.world.tip("main"), &[(POLICY_PATH, Some(POLICY))]);
    f.world.set_tracking("origin/main", Some(&fetched));
    let pause = Between {
        n: Cell::new(0),
        rounds: 2,
        between: &|_| {},
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines.len(), 1, "{out}");
    assert!(lines[0].get("set_up").is_none(), "{out}");
    let own = f.world.tree(&f.world.tip("main"));
    assert!(!own.contains_key(POLICY_PATH));
}

/// 129: records are sluice there, whatever the branches hold: the watch
/// goes on.
#[test]
fn a_watch_goes_on_over_records_with_no_policy() {
    let f = one_queued();
    f.world.hand_commit(BRANCH, &[(POLICY_PATH, None)]);
    assert!(!f.world.holds_path(POLICY_PATH).unwrap());
    let pause = Between {
        n: Cell::new(0),
        rounds: 2,
        between: &|_| {},
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines.len(), 1, "{out}");
    assert_eq!(lines[0]["branches"][0]["branch"], BRANCH, "{out}");
    assert!(lines[0].get("set_up").is_none(), "{out}");
}

/// 129, 133: a check of the branches that fails is a read the reader
/// could not make: its words are the document, and the watch checks again
/// at its next tick, ending once it finds no policy.
#[test]
fn a_failed_check_of_the_branches_is_a_read_refused() {
    let f = never_set_up();
    f.world.fail_ref_list(Some(UNREADABLE));
    let written = RefCell::new(Vec::new());
    let between = |n: usize| {
        written
            .borrow_mut()
            .push(readers_doc().expect("a document"));
        if n == 1 {
            f.world.fail_ref_list(None);
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 5,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(pause.n.get(), 1, "{out}");
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert_failure(&lines[0], UNREADABLE);
    assert_eq!(written.into_inner(), [lines[0].clone()]);
    assert_eq!(lines[1]["set_up"], false, "{out}");
    assert_eq!(readers_doc().as_ref(), Some(&lines[1]));
}

/// 129: a follower ends on the reader's document saying the repository
/// has no sluice, reading nothing itself: every read here would fail.
#[test]
fn a_follower_ends_on_a_document_saying_the_repository_has_no_sluice() {
    let f = setup();
    let doc = seven_testing();
    let ended = no_sluice_doc();
    f.world.fail_state_list(Some(UNREADABLE));
    f.world.fail_ref_list(Some(UNREADABLE));
    let _reader = reader_of(REPOSITORY, &doc);
    let between = |n: usize| {
        if n == 1 {
            rewrite_readers_doc(&ended);
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 5,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(pause.n.get(), 1, "{out}");
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines, [doc, ended], "{out}");
}

/// 129: once sluice is set up, a watch that ended leaves nothing that
/// ends the next: it reads the queues again and goes on.
#[test]
fn a_watch_goes_on_once_sluice_is_set_up_after_one_ended() {
    let f = never_set_up();
    let once = Between {
        n: Cell::new(0),
        rounds: 5,
        between: &|_| {},
    };
    let (code, out) = sluice_with(&f, &once, &["overview", "--json", "--watch"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(readers_doc().expect("a document")["set_up"], false);
    f.world.hand_commit("main", &[(POLICY_PATH, Some(POLICY))]);
    let pause = Between {
        n: Cell::new(0),
        rounds: 2,
        between: &|_| {},
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines.len(), 1, "{out}");
    assert!(lines[0].get("set_up").is_none(), "{out}");
    assert_eq!(readers_doc().as_ref(), Some(&lines[0]));
}

/// 129: a watch that starts after a new reader took the lock, and before
/// it wrote, follows: it ends on no earlier reader's finding, and prints
/// the new reader's document once it is written.
#[test]
fn a_watch_that_starts_while_the_new_reader_reads_follows_it() {
    let f = setup();
    let doc = seven_testing();
    rewrite_readers_doc(&no_sluice_doc());
    let reader = match reader_as(sluice::VERSION) {
        sluice::reader::Reader::Lead(lead) => RefCell::new(lead),
        sluice::reader::Reader::Follow(_) => panic!("a reader of {REPOSITORY} already"),
    };
    let between = |n: usize| {
        if n == 2 {
            reader.borrow_mut().write(&doc).unwrap();
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 3,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["overview", "--json", "--watch"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    let lines: Vec<serde_json::Value> = out.lines().map(json).collect();
    assert_eq!(lines, [doc], "{out}");
}

#[test]
fn amend_and_withdraw() {
    let f = setup();
    f.world.hold_runs(true);
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/b", "--no-wait"]);
    assert_eq!(
        sluice(&f, &["amend", "2", "--priority", "1"]),
        (0, "amended #2\n".into())
    );
    sluice(&f, &["step", BRANCH]);
    assert_eq!(
        sluice(&f, &["amend", "2", "--after", "1"]),
        (
            2,
            "refused: #2 is under test; its order is fixed: sluice status 2 follows it, \
             or sluice withdraw 2 takes it out\n"
                .into()
        )
    );
    assert_eq!(
        sluice(&f, &["withdraw", "1", "--as", "agent-b"]),
        (
            2,
            "refused: #1 is not agent-b's to withdraw: ask --as agent-a, its agent, \
             or --as operator\n"
                .into()
        )
    );
    assert_eq!(
        sluice(&f, &["withdraw", "1", "--as", "agent-a"]),
        (0, "withdrew #1\n".into())
    );
    // 5, 48: it left the queue by withdrawal, so asking again is no
    // change; still not another's to ask.
    assert_eq!(
        sluice(&f, &["withdraw", "1", "--as", "agent-a"]),
        (0, "#1 was already withdrawn\n".into())
    );
    assert_eq!(sluice(&f, &["withdraw", "1", "--as", "agent-b"]).0, 2);
    assert_eq!(
        sluice(&f, &["amend", "1", "--priority", "2"]),
        (
            2,
            "refused: #1 is withdrawn, past changing: sluice status 1\n".into()
        )
    );
    assert_eq!(
        sluice(&f, &["withdraw", "99"]),
        (
            2,
            "refused: no submission 99: sluice overview lists the queues\n".into()
        )
    );
    work(&f, "unit/c", "agent-c", &[("plugin/c", Some("c"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/c", "--no-wait"]);
    assert_eq!(
        sluice(&f, &["amend", "3", "--after", "7"]).1,
        format!("refused: --after names no queued submission: #7; sluice status {BRANCH} lists the queue\n")
    );
}

fn json(out: &str) -> serde_json::Value {
    serde_json::from_str(out).unwrap_or_else(|e| panic!("{e}: {out}"))
}

/// `--json` on amend and withdraw: one document each, and a refusal is
/// the error document with the commands its text names.
#[test]
fn amend_and_withdraw_under_json() {
    let f = setup();
    f.world.hold_runs(true);
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/b", "--no-wait"]);
    let (code, out) = sluice(
        &f,
        &["amend", "2", "--priority", "3", "--after", "1", "--json"],
    );
    assert_eq!(code, 0);
    assert_eq!(
        json(&out),
        serde_json::json!({"id": 2, "after": [1], "priority": 3})
    );

    let (code, out) = sluice(
        &f,
        &["amend", "1", "--priority", "1", "--as", "agent-b", "--json"],
    );
    assert_eq!(code, 2);
    assert_eq!(
        json(&out),
        serde_json::json!({"error": {
            "kind": "refused",
            "message": "#1 is not agent-b's to change: ask --as agent-a, its agent, or --as operator",
            "next": ["sluice amend 1 --as agent-a", "sluice amend 1 --as operator"],
        }})
    );
    let (code, out) = sluice(&f, &["amend", "99", "--json"]);
    assert_eq!(code, 2);
    assert_eq!(
        json(&out)["error"]["next"],
        serde_json::json!(["sluice overview"])
    );

    sluice(&f, &["step", BRANCH]);
    let (code, out) = sluice(&f, &["amend", "1", "--priority", "1", "--json"]);
    assert_eq!(code, 2);
    assert_eq!(
        json(&out)["error"]["next"],
        serde_json::json!(["sluice status 1", "sluice withdraw 1"])
    );

    let (code, out) = sluice(&f, &["withdraw", "1", "--as", "agent-b", "--json"]);
    assert_eq!(code, 2);
    assert_eq!(
        json(&out)["error"]["next"],
        serde_json::json!([
            "sluice withdraw 1 --as agent-a",
            "sluice withdraw 1 --as operator"
        ])
    );
    let (code, out) = sluice(&f, &["withdraw", "1", "--as", "agent-a", "--json"]);
    assert_eq!(code, 0);
    assert_eq!(
        json(&out),
        serde_json::json!({"id": 1, "state": "withdrawn"})
    );
    let (code, out) = sluice(&f, &["withdraw", "1", "--as", "agent-a", "--json"]);
    assert_eq!(code, 0);
    assert_eq!(
        json(&out),
        serde_json::json!({"id": 1, "state": "withdrawn", "already": true})
    );
    // Once a round has moved it to the finished records, the same.
    sluice(&f, &["step", BRANCH]);
    let (code, out) = sluice(&f, &["withdraw", "1", "--as", "agent-a", "--json"]);
    assert_eq!(code, 0);
    assert_eq!(json(&out)["already"], true);
    let (code, out) = sluice(&f, &["withdraw", "1", "--as", "agent-b", "--json"]);
    assert_eq!(code, 2);
    assert_eq!(json(&out)["error"]["kind"], "refused");
    let (code, out) = sluice(&f, &["withdraw", "99", "--json"]);
    assert_eq!(code, 2);
    assert_eq!(
        json(&out)["error"]["message"],
        "no submission 99: sluice overview lists the queues"
    );
}

/// 5: a landed submission left the queue by landing, so it is never
/// withdrawn, however often it is asked.
#[test]
fn a_landed_submission_is_not_withdrawn() {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    assert_eq!(
        sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]).0,
        LANDED
    );
    assert_eq!(
        sluice(&f, &["withdraw", "1"]),
        (
            2,
            "refused: #1 is landed, out of the queue already: sluice status 1\n".into()
        )
    );
    let (code, out) = sluice(&f, &["withdraw", "1", "--json"]);
    assert_eq!(code, 2);
    assert_eq!(
        json(&out),
        serde_json::json!({"error": {
            "kind": "refused",
            "message": "#1 is landed, out of the queue already: sluice status 1",
            "next": ["sluice status 1"],
        }})
    );
}

/// `sluice step --json`: the round's outcome as one document.
#[test]
fn step_under_json() {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    let (code, out) = sluice(&f, &["step", BRANCH, "--json"]);
    assert_eq!(code, 0);
    assert_eq!(
        json(&out),
        serde_json::json!({
            "branch": BRANCH, "outcome": "stepped", "released": [], "previews_released": [],
            "releases": [],
        })
    );
    assert_eq!(
        sluice(&f, &["step", BRANCH]),
        (0, format!("{BRANCH}: stepped\n"))
    );
}

// ---------------------------------------------------------------------------
// sluice run, a follower of the host process (135; design D11)
// ---------------------------------------------------------------------------

/// host-driver, "One pass": `sluice run --rounds 1` exits 0 after one pass
/// of the host process, which forms the submission's combination, and the
/// landing a later pass makes is what a run prints (135; design D11).
#[test]
fn sluice_run_follows_the_host_process() {
    let f = one_queued();
    f.world.hold_runs(true);
    let (code, out) = sluice(&f, &["run", "--rounds", "1"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(state_of(&f, 1), "testing");
    assert_eq!(f.world.started().len(), 1, "one combination formed");
    f.world.complete_all();
    f.world.fail_next_release();
    let (code, out) = sluice(&f, &["run", "--rounds", "3"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(state_of(&f, 1), "landed");
    // What the host process's pass said, printed as the pass printed it.
    assert!(out.contains("could not release sluice/try/"), "{out}");
    assert_eq!(f.world.started().len(), 1, "no second combination");
}

/// host-driver, "Started by hand": `sluice run` says once on stderr that
/// nothing needs it, and `sluice --help` names no `run` (135).
#[test]
fn sluice_run_says_nothing_needs_it() {
    if let Some(err) = stderr_of("sluice_run_says_nothing_needs_it") {
        let said: Vec<&str> = err.lines().filter(|l| l.starts_with("sluice: ")).collect();
        assert_eq!(
            said,
            ["sluice: sluice starts its own host process; nothing needs `sluice run`"],
            "{err}"
        );
        return;
    }
    let f = one_queued();
    let (code, out) = sluice(&f, &["run", "--rounds", "2"]);
    assert_eq!(code, 0, "{out}");
    let help = <Cli as clap::CommandFactory>::command()
        .render_long_help()
        .to_string();
    assert!(
        !help.lines().any(|l| l.trim_start().starts_with("run ")),
        "{help}"
    );
}

/// Submission `id`'s state, as its record says.
fn state_of(f: &Fakes, id: u64) -> serde_json::Value {
    let (_, out) = sluice(f, &["status", &id.to_string(), "--json"]);
    json(&out)["state"].clone()
}

/// 129, 135: the host process is the reader: with nothing queued, its
/// pass writes the whole overview as the reader's document, as a one-shot
/// overview reads it but for its time, naming its tick (64).
#[test]
fn a_driver_with_nothing_queued() {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    rewrite_readers_doc(&serde_json::json!({ "put": "by the test" }));
    let (code, out) = sluice(&f, &["run", "--rounds", "1"]);
    assert_eq!((code, out.as_str()), (0, ""));
    let (_, overview) = sluice(&f, &["overview", "--json"]);
    let doc = readers_doc().expect("the host process's document");
    let untimed = sluice::reader::untimed;
    let mut read = json(&overview);
    read["tick"] = 3.into();
    assert_eq!(untimed(&doc), untimed(&read));
    assert_eq!(doc["branches"][0]["branch"], BRANCH, "{doc}");
    // The fakes' host process lets the reader's place go with the command.
    assert!(soon(reader_free));
}

/// host-process, "The queue emptied while a run went on" (66, 129; design
/// D5): the one submission withdrawn while its combination's run goes on
/// and nothing else queued, the host process's next pass runs a round for
/// the branch, which ends the run and releases its checkout.
#[test]
fn a_driver_ends_a_withdrawn_submissions_run_with_nothing_queued() {
    let f = one_queued();
    f.world.hangs("plugin:test");
    f.world.hold_runs(true);
    let (code, out) = sluice(&f, &["step", BRANCH]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(state_of(&f, 1), "testing");
    assert_eq!(f.world.live_runs(), 1);
    let place = f.world.made_places()[0].clone();
    let (code, out) = sluice(&f, &["withdraw", "1", "--as", "agent-a"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = sluice(&f, &["run", "--rounds", "1"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(f.world.live_runs(), 0, "the run goes on");
    assert_eq!(f.world.released_places(), [place]);
    assert!(f.world.live_places().is_empty());
    let (_, status) = sluice(&f, &["status", BRANCH, "--json"]);
    assert!(!status.contains("\"combination\""), "{status}");
}

/// host-process, "Nothing under test and nothing queued" (129; rule
/// core.31): with every submission landed, the host process's pass runs no
/// round for the branch and reads no revision of it: a round's read of the
/// branch, refused, would be said and fail the pass.
#[test]
fn a_driver_runs_no_round_for_a_branch_with_nothing_in_play() {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    f.world.refuse_reads(BRANCH, Some(UNREADABLE));
    let (code, out) = sluice(&f, &["run", "--rounds", "1"]);
    assert_eq!((code, out.as_str()), (0, ""));
    f.world.refuse_reads(BRANCH, None);
    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/b", "--no-wait"]);
    f.world.refuse_reads(BRANCH, Some(UNREADABLE));
    let (code, out) = sluice(&f, &["run", "--rounds", "1"]);
    assert_ne!(code, 0, "a round ran for the queued: {out}");
}

/// Whether the reader's lock is free: the test can take it, as the driver
/// would.
fn reader_free() -> bool {
    matches!(
        sluice::reader::Reader::take_given(
            &support::readers(),
            REPOSITORY,
            sluice::VERSION,
            sluice::reader::Drives::Itself,
        )
        .unwrap(),
        sluice::reader::Reader::Lead(_)
    )
}

/// 129, 133: the host process's read of the overview that fails is its
/// document, in the words it was refused with; the pass goes on, and the
/// run that follows it exits as it would.
#[test]
fn a_driver_whose_read_fails_writes_the_failure() {
    let f = setup();
    f.world.fail_ref_list(Some(UNREADABLE));
    let (code, out) = sluice(&f, &["run", "--rounds", "1"]);
    assert_eq!((code, out.as_str()), (0, ""));
    assert_failure(
        &readers_doc().expect("the host process's document"),
        UNREADABLE,
    );
}

/// 129, 135 (design D7, D9): a wait follows the reader's document as it
/// finds it, and runs no round of its own: while a reader of the same
/// version holds the place, the host process follows it too, and nothing
/// moves.
#[test]
fn a_wait_follows_a_watch() {
    let f = one_queued();
    let (_, overview) = sluice(&f, &["overview", "--json"]);
    let _watch = reader_of(REPOSITORY, &json(&overview));
    let pause = Between {
        n: Cell::new(0),
        rounds: 2,
        between: &|_| {},
    };
    let (code, out) = sluice_with(&f, &pause, &["wait", "1"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    assert!(f.world.started().is_empty());
    assert_eq!(state_of(&f, 1), "queued");
}

// ---------------------------------------------------------------------------
// sluice exec (session-runs-1 clauses 89 to 93)
// ---------------------------------------------------------------------------

/// A pause that runs `then` on its first call, and stops the wait when
/// `stop` says so.
struct Then<F: Fn(usize) -> bool>(Cell<usize>, F);

impl<F: Fn(usize) -> bool> Pause for Then<F> {
    fn pause(&self) -> bool {
        self.0.set(self.0.get() + 1);
        assert!(self.0.get() < 50, "waited too long");
        (self.1)(self.0.get())
    }
}

fn exec_with(f: &Fakes, reserved: usize, pause: &dyn Pause, args: &[&str]) -> (i32, String) {
    let cli =
        Cli::try_parse_from(["sluice", "exec"].into_iter().chain(args.iter().copied())).unwrap();
    let mut out = Vec::new();
    let host = support::FakeHost::new(f.world.clone(), reserved);
    let code = run(cli, f.seams(), &host, &(), &(), &f.clock, pause, &mut out).unwrap();
    (code, String::from_utf8(out).unwrap())
}

fn exec(f: &Fakes, args: &[&str]) -> (i32, String) {
    exec_with(f, 0, &Rounds(Cell::new(0)), args)
}

/// The arguments: a branch, and a command after `--`.
#[test]
fn exec_takes_a_branch_and_a_command() {
    assert!(Cli::try_parse_from(["sluice", "exec"]).is_err());
    let cli =
        Cli::try_parse_from(["sluice", "exec", BRANCH, "--", "moon", "run", ":test"]).unwrap();
    match cli.command {
        Some(sluice::Command::Exec {
            branch,
            command,
            worktree,
            json,
            release,
        }) => {
            assert!(!release);
            assert_eq!(branch, BRANCH);
            assert_eq!(command, ["moon", "run", ":test"]);
            assert_eq!(worktree, ".");
            assert!(!json);
        }
        other => panic!("{other:?}"),
    }
}

/// A place that is no worktree is refused, and nothing runs.
#[test]
fn exec_outside_a_worktree_is_refused() {
    let f = setup();
    let (code, out) = exec(&f, &[BRANCH, "--worktree", "nowhere"]);
    assert_eq!(code, CONFLICT);
    assert!(
        out.starts_with("refused: nowhere is not a worktree"),
        "{out}"
    );
    assert!(f.world.started().is_empty());
    let (code, out) = exec(&f, &["bolt/none", "--worktree", "nowhere"]);
    assert_eq!(code, CONFLICT, "{out}");
}

/// 89: the checks the head affects against the branch run in the
/// worktree's adopted place, count against the bound only while they run, and the
/// exit is theirs.
#[test]
fn exec_runs_the_affected_checks_against_the_branch() {
    let f = setup();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    let base = f.world.tip(BRANCH);
    let (code, out) = exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    let req = f.world.requests().pop().unwrap();
    assert_eq!(req.place.name, "session/unit/a");
    assert_eq!(req.since, Some(base.clone()));
    assert!(!req.replay);
    assert_eq!(f.world.observed().len(), 1, "only the affected check ran");
    assert_eq!(f.world.adopted_places(), ["session/unit/a"]);
    assert_eq!(
        sluice_seams::Tests::running(&f.tests).unwrap().sessions,
        0,
        "the run is no longer counted"
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[0],
        format!(
            "running the checks {} affects against {BRANCH} ({})",
            sluice_seams::Driver::session(&f.driver, "unit/a")
                .unwrap()
                .head
                .short(),
            base.short()
        )
    );
    let n = lines.len();
    assert_eq!(lines[n - 2], "passed: 1 task");
    assert!(lines[n - 1].starts_with("recorded 1 result under its key for "));

    f.world
        .fails_with("plugin:test", "plugin/lib/plan.py", "p2");
    let (code, out) = exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, FAILED);
    assert!(out.contains("failed: plugin:test\n"), "{out}");
}

/// 89, 71: with the bound full, exec waits, and starts when a run ends.
#[test]
fn exec_waits_for_room() {
    let f = setup();
    f.world.host_limit(1);
    f.world.session_run("/w/other", true);
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    let world = f.world.clone();
    let pause = Then(Cell::new(0), move |n| {
        if n == 1 {
            assert!(world.started().is_empty(), "started into a full bound");
            world.kill_session("/w/other");
        }
        true
    });
    let (code, out) = exec_with(&f, 0, &pause, &[BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    assert!(out.contains("waiting: the host is at its bound\n"), "{out}");
    assert_eq!(f.world.started().len(), 1);
}

/// 150, S75 (record 0087; design D12): an agent's check waiting while a
/// combination's run waits too says `a combination's run starts first`
/// once the host has a free run, which the combination's run takes; the
/// agent's check starts in the next run to come free.
#[test]
fn exec_waits_for_a_combinations_run() {
    let f = setup();
    f.world.host_limit(1);
    f.world.hold_runs(true);
    f.world.session_run("/w/other", true);
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    work(&f, "unit/q", "agent-q", &[("plugin/q", Some("q"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/q", "--no-wait"]);
    // The round finds the host full: the combination's run waits.
    sluice(&f, &["step", BRANCH]);
    assert!(f.world.started().is_empty());
    let pause = Then(Cell::new(0), |n| {
        match n {
            1 => f.world.kill_session("/w/other"),
            2 => {
                // The combination's round asks for the free run, and takes it.
                sluice(&f, &["step", BRANCH]);
                assert_eq!(f.world.started().len(), 1, "the combination's run");
            }
            3 => {
                // Runs end from now, the agent's own among them.
                f.world.hold_runs(false);
                f.world.complete_all();
                sluice(&f, &["step", BRANCH]);
            }
            _ => {}
        }
        true
    });
    let (code, out) = exec_with(&f, 0, &pause, &[BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    assert!(out.contains("waiting: the host is at its bound\n"), "{out}");
    assert!(
        out.contains("waiting: a combination's run starts first\n"),
        "{out}"
    );
    assert_eq!(
        f.world.started().len(),
        2,
        "the combination's, then the agent's"
    );
}

/// host-process, "A session's check waits behind a combination" (150;
/// design D12): the overview's room names the runs waiting by kind and
/// whose turn the next contested run is, and its host process; `sluice
/// status` with no target says the same room.
#[test]
fn the_overview_names_the_runs_waiting_and_the_turn() {
    let f = one_queued();
    f.world.host_limit(1);
    f.world.hold_runs(true);
    sluice(&f, &["step", BRANCH]);
    assert_eq!(f.world.started().len(), 1, "the combination's run holds it");
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    let seen = RefCell::new(None);
    let said = RefCell::new(String::new());
    let pause = Then(Cell::new(0), |n| {
        match n {
            1 => {
                *seen.borrow_mut() = Some(overview_doc(&f, 0));
                *said.borrow_mut() = sluice(&f, &["status"]).1;
            }
            2 => {
                f.world.hold_runs(false);
                f.world.complete_all();
                sluice(&f, &["step", BRANCH]);
            }
            _ => {}
        }
        true
    });
    let (code, out) = exec_with(&f, 0, &pause, &[BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    let doc = seen.into_inner().expect("read while the check waited");
    assert_eq!(
        doc["room"]["waiting"],
        serde_json::json!({ "queue": 0, "sessions": 1 }),
        "{doc}"
    );
    assert_eq!(doc["room"]["turn"], "queue", "{doc}");
    assert_eq!(doc["host_process"]["pid"], std::process::id(), "{doc}");
    assert_eq!(doc["host_process"]["version"], sluice::VERSION, "{doc}");
    let said = said.into_inner();
    assert!(
        said.contains("host: 1 of 1 runs · 1 queue · waiting: 1 agent\n"),
        "{said}"
    );
}

/// 71: a start the check seam refuses at the host's bound, another
/// repository's run having taken the last one after the count was read,
/// waits as one the count refused, says so once, and starts on the next
/// try.
#[test]
fn exec_waits_on_a_start_refused_at_the_bound() {
    let f = setup();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    f.world.refuse_next_start();
    let (code, out) = exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(
        out.matches("waiting: the host is at its bound\n").count(),
        1,
        "{out}"
    );
    assert_eq!(f.world.started().len(), 1);
}

/// 91: a session run never uses what the reserve keeps; stopped while
/// waiting, nothing ran and nothing is recorded.
#[test]
fn exec_never_takes_the_reserve() {
    let f = setup();
    f.world.host_limit(2);
    f.world.session_run("/w/other", true);
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    let pause = Then(Cell::new(0), |_| false);
    let (code, out) = exec_with(&f, 1, &pause, &[BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, sluice::STOPPED);
    assert!(
        out.ends_with("waiting: the host is at its bound\nstopped: nothing ran\n"),
        "{out}"
    );
    assert!(f.world.started().is_empty());
    assert_eq!(f.world.results_log(), "");
    // Without the reserve, the room is taken.
    let (code, _) = exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED);
}

/// ADR 0035: with the host full and an agent's check holding a run, a
/// branch with a submission queued and room in its window says so on its
/// counts line, in the overview, `sluice status` and the home view; once
/// the check ends, none of them does.
#[test]
fn the_counts_say_when_agents_checks_hold_the_queue_back() {
    let f = setup();
    f.world.host_limit(2);
    f.world.session_run("/w/agent", true);
    work(
        &f,
        "unit/touches",
        "crw-1-unit-1",
        &[("plugin/lib/plan.py", Some("touched"))],
    );
    work(
        &f,
        "unit/cards",
        "crw-1-unit-2",
        &[("tests/t_cards.py", Some("cards 2"))],
    );
    f.world.hold_runs(true);
    for w in ["unit/touches", "unit/cards"] {
        sluice(&f, &["submit", BRANCH, "--worktree", w, "--no-wait"]);
    }
    sluice(&f, &["run", "--rounds", "2"]);
    let clause = "testing 1 at once (up to 4) · 1 slot to an agent's checks";

    let (_, text) = sluice(&f, &["overview"]);
    assert!(text.lines().next().unwrap().ends_with(clause), "{text}");
    assert!(
        text.contains("\nhost: 2 of 2 runs · 1 queue · 1 agent · waiting: 1 combination\n"),
        "{text}"
    );
    let (_, text) = sluice(&f, &["status", BRANCH]);
    assert!(text.lines().next().unwrap().ends_with(clause), "{text}");
    let (_, text) = sluice(&f, &[]);
    assert!(
        text.contains(&format!("{BRANCH}: 2 in the queue · {clause}\n")),
        "{text}"
    );

    f.world.kill_session("/w/agent");
    for args in [&["overview"][..], &["status", BRANCH], &[]] {
        let (_, text) = sluice(&f, args);
        assert!(!text.contains("checks"), "{text}");
    }
}

/// The overview's document, as a fake host with `reserved` reads it.
fn overview_doc(f: &Fakes, reserved: usize) -> serde_json::Value {
    let cli = Cli::try_parse_from(["sluice", "overview", "--json"]).unwrap();
    let mut out = Vec::new();
    let host = support::FakeHost::new(f.world.clone(), reserved);
    let pause = Rounds(Cell::new(0));
    run(cli, f.seams(), &host, &(), &(), &f.clock, &pause, &mut out).unwrap();
    serde_json::from_slice(&out).unwrap()
}

/// ADR 0035: while an agent's check runs, its worktree's entry carries the
/// tasks its `sluice exec` keeps, and `room` counts it against the bound;
/// keeping them moves no branch's version. Once it ends the run is gone,
/// and its result lists its tasks and says it is reused.
#[test]
fn overview_shows_an_agents_check_and_the_hosts_room() {
    use sluice_seams::{TaskProgress, TaskState};
    let f = setup();
    f.world.host_limit(3);
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    let head = sluice_seams::Driver::session(&f.driver, "unit/a")
        .unwrap()
        .head;
    f.world.hold_runs(true);
    let task = |state| TaskProgress {
        target: "plugin:test".into(),
        state,
        started: None,
        ended: None,
        exit: None,
    };
    let seen = RefCell::new(None);
    let text = RefCell::new(String::new());
    let version = f.world.version(BRANCH);
    let pause = Then(Cell::new(0), |n| {
        match n {
            1 => f.world.says_tasks(&head, &[task(TaskState::Running)]),
            2 => {
                *seen.borrow_mut() = Some(overview_doc(&f, 1));
                *text.borrow_mut() = sluice(&f, &["overview"]).1;
                f.world.complete(&head);
            }
            _ => {}
        }
        true
    });
    let (code, out) = exec_with(&f, 0, &pause, &[BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");

    let during = seen.into_inner().expect("read while the check ran");
    let w = &during["sessions"][0];
    assert_eq!(w["worktree"], "unit/a");
    assert!(w["run"]["started"].is_string());
    assert!(w["run"].get("command").is_none());
    assert_eq!(w["run"]["tasks"][0]["target"], "plugin:test");
    assert_eq!(w["run"]["tasks"][0]["state"], "running");
    assert_eq!(
        during["room"],
        serde_json::json!({
            "bound": 3, "queue": 0, "sessions": 1, "reserve": 1,
            "waiting": {"queue": 0, "sessions": 0}, "turn": "queue",
            "repositories": {},
        })
    );
    assert_eq!(
        f.world.version(BRANCH),
        version,
        "no branch's version moved"
    );
    let text = text.into_inner();
    assert!(
        text.contains(&format!("\nchecking unit/a → {BRANCH} 0/1 since ")),
        "{text}"
    );
    assert!(!text.contains("testing unit/a"), "{text}");

    let after = overview_doc(&f, 0);
    let w = &after["sessions"][0];
    assert!(w.get("run").is_none());
    assert_eq!(after["room"]["sessions"], 0);
    let last = &w["last_result"];
    assert_eq!(last["recorded"], true);
    assert_eq!(last["reused"], true);
    assert_eq!(last["tasks"][0]["target"], "plugin:test");
    assert_eq!(last["tasks"][0]["state"], "passed");
}

/// ADR 0035: a run of a command names its command and lists no tasks; a
/// session run in another repository on the host counts only in `room`.
#[test]
fn overview_names_a_commands_check_and_counts_another_repositorys() {
    let f = setup();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    f.world.session_run("/elsewhere/agent", true);
    f.world.hold_runs(true);
    let seen = RefCell::new(None);
    let pause = Then(Cell::new(0), |n| {
        if n == 1 {
            *seen.borrow_mut() = Some(overview_doc(&f, 0));
            f.world.complete_all();
        }
        true
    });
    exec_with(
        &f,
        0,
        &pause,
        &[BRANCH, "--worktree", "unit/a", "--", "make", "e2e"],
    );
    let during = seen.into_inner().unwrap();
    let sessions = during["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    assert_eq!(sessions[0]["run"]["command"], "make e2e");
    assert!(sessions[0]["run"].get("tasks").is_none());
    assert_eq!(during["room"]["sessions"], 2);
    // A command's result says nothing of what it is worth.
    let after = overview_doc(&f, 0);
    let last = &after["sessions"][0]["last_result"];
    assert_eq!(last["kind"], "command");
    assert!(last.get("reused").is_none() && last.get("tasks").is_none());
}

/// ADR 0035, 93: a check's passes are reused only when recorded under an
/// executor the gate trusts; one with uncommitted changes is not kept.
#[test]
fn a_finished_check_says_whether_it_is_reused() {
    let f = setup();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    f.world.dirty("unit/a", &["scratch"]);
    exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    let last = &overview_doc(&f, 0)["sessions"][0]["last_result"];
    assert_eq!(last["outcome"], "pass");
    assert_eq!(last["recorded"], false);
    assert_eq!(last["reused"], false);
    assert_eq!(last["tasks"].as_array().unwrap().len(), 1);

    // A gate that trusts only confined runs takes none of the session's.
    let f = Fakes::new();
    f.world.init(
        BRANCH,
        &[
            (
                ".config/sluice.toml",
                "[branches.\"bolt/*\"]\nrequired = [\"#gate\"]\ntrust = [\"confined\"]\n",
            ),
            ("plugin/lib/plan.py", "plan"),
        ],
    );
    f.world.task("plugin:test", "plugin/");
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    let last = &overview_doc(&f, 0)["sessions"][0]["last_result"];
    assert_eq!(last["recorded"], true);
    assert_eq!(last["reused"], false);
}

/// ADR 0035: the watch prints again when a check starts and when one of
/// its tasks moves, and not when nothing of it does.
#[test]
fn overview_watch_prints_when_a_checks_task_moves() {
    use sluice_seams::{HostRun, TaskProgress, TaskState};
    let f = setup();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    let running = [TaskProgress {
        target: "plugin:test".into(),
        state: TaskState::Running,
        started: None,
        ended: None,
        exit: None,
    }];
    let between = |n: usize| match n {
        1 => f.world.begin_host_run(HostRun {
            worktree: "unit/a".into(),
            branch: BRANCH.into(),
            started: "2026-10-20T09:00:00Z".into(),
            command: None,
            tasks: Vec::new(),
        }),
        2 | 3 => f.world.session_tasks("unit/a", &running),
        _ => {}
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 3,
        between: &between,
    };
    let cli = Cli::try_parse_from(["sluice", "overview", "--json", "--watch"]).unwrap();
    let mut out = Vec::new();
    let host = support::FakeHost::new(f.world.clone(), 0);
    run(cli, f.seams(), &host, &(), &(), &f.clock, &pause, &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    let docs: Vec<serde_json::Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(docs.len(), 3, "{out}");
    assert!(docs[0]["sessions"][0].get("run").is_none());
    assert!(docs[1]["sessions"][0]["run"].get("tasks").is_none());
    assert_eq!(
        docs[2]["sessions"][0]["run"]["tasks"][0]["state"],
        "running"
    );
}

/// 66, 92: a run past the branch's time limit is reported as timed out,
/// and recorded as neither a pass nor a fail.
#[test]
fn exec_past_its_time_limit_is_timed_out_and_unrecorded() {
    let f = Fakes::new();
    f.world.init(
        BRANCH,
        &[
            (
                ".config/sluice.toml",
                &format!("{POLICY}timeout = \"1m\"\n"),
            ),
            ("plugin/lib/plan.py", "plan"),
        ],
    );
    f.world.task("plugin:test", "plugin/");
    f.world.hangs("plugin:test");
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    let (code, out) = exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, FAILED);
    assert!(out.ends_with("timed out\n"), "{out}");
    assert_eq!(f.world.results_log(), "");
}

/// 89, 92, 147: a command runs in the adopted place in place of the
/// runner's checks, with its words quoted for `sh -c`, its worktree
/// equipped by the runner the policy names; it gives its exit, and records
/// nothing, clean worktree or not.
#[test]
fn exec_runs_a_command_and_records_nothing() {
    let f = setup();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    let (code, out) = exec(
        &f,
        &[BRANCH, "--worktree", "unit/a", "--", "moon", "run", "a b's"],
    );
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(out, "running moon run 'a b'\\''s'\n");
    let req = f.world.requests().pop().unwrap();
    assert!(req.required.is_empty());
    assert_eq!(req.command.as_deref(), Some("moon run 'a b'\\''s'"));
    // `[tests]` is the policy's, which names no runner here.
    assert_eq!(req.tests.get("runner"), None);
    assert_eq!(req.tests.get("command"), None);
    assert_eq!(f.world.results_log(), "");
}

fn result_lines(f: &Fakes) -> Vec<sluice_core::records::ResultLine> {
    sluice_core::records::parse_lines(f.world.results_log().as_bytes()).unwrap()
}

/// 92, 16: a clean worktree's run of the checks writes one record per
/// check it ran, with its executor and host, on its head from its
/// worktree; a dirty worktree's run writes none and says so.
#[test]
fn exec_records_a_clean_run_and_not_a_dirty_one() {
    let f = setup();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[
            ("plugin/lib/plan.py", Some("p2")),
            ("tests/t_cards.py", Some("c2")),
        ],
    );
    let head = sluice_seams::Driver::session(&f.driver, "unit/a")
        .unwrap()
        .head;
    let (code, out) = exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    assert!(
        out.contains(&format!(
            "recorded 2 results under their keys for {}\n",
            head.short()
        )),
        "{out}"
    );
    let lines = result_lines(&f);
    assert_eq!(lines.len(), 2);
    for l in &lines {
        assert_eq!(l.kind, sluice_core::records::ResultKind::Ran);
        assert_eq!(l.combination, head);
        assert_eq!(l.branch, BRANCH);
        assert_eq!(l.worktree.as_deref(), Some("unit/a"));
        assert_eq!(l.host.as_deref(), Some("fake-host"));
        let e = l.executor.as_ref().unwrap();
        assert_eq!((e.kind.as_str(), e.confined), ("open", false));
    }

    f.world.dirty("unit/a", &["notes.txt"]);
    let (code, out) = exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    assert!(
        out.contains("not recorded: worktree is not clean\n"),
        "{out}"
    );
    assert_eq!(result_lines(&f).len(), 2);
}

/// `--json`: one document on stdout, in `sluice status --json`'s field
/// names, with each check's key, outcome and whether it was recorded; the
/// human output goes to stderr.
#[test]
fn exec_json_is_one_document() {
    let f = setup();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    let (code, out) = exec(&f, &[BRANCH, "--worktree", "unit/a", "--json"]);
    assert_eq!(code, LANDED);
    let doc: serde_json::Value =
        serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
    assert_eq!(out.lines().count(), 1, "{out}");
    assert_eq!(doc["branch"], BRANCH);
    assert_eq!(doc["exit"], 0);
    assert_eq!(doc["timed_out"], false);
    assert_eq!(doc["ports"], serde_json::json!({}));
    let tasks = doc["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["task"], "plugin:test");
    assert_eq!(tasks[0]["outcome"], "pass");
    assert_eq!(tasks[0]["recorded"], true);
    assert!(tasks[0]["hash"].is_string());

    f.world.dirty("unit/a", &["notes.txt"]);
    let (_, out) = exec(&f, &[BRANCH, "--worktree", "unit/a", "--json"]);
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["tasks"][0]["recorded"], false);
}

/// 149: the check a branch requires, a task that runs nothing of its own.
const FAILED_CHECK: &str = "root:integration";

/// 149: the task it depends on, which no gate requires.
const FAILED_DEP: &str = "api:integration";

/// 149: `root:integration`, required by name, depends on
/// `api:integration`, which fails on `api/lib` = `broken`; the worktree
/// `unit/a` holds that.
fn failed_by_its_dependency() -> Fakes {
    let f = Fakes::new();
    let policy = POLICY.replace(
        "required = [\"#gate\"]",
        &format!("required = [\"{FAILED_CHECK}\"]"),
    );
    f.world.init(
        BRANCH,
        &[
            (".config/sluice.toml", policy.as_str()),
            ("api/lib", "fine"),
            ("root/lib", "noop"),
        ],
    );
    f.world.task(FAILED_DEP, "api/");
    f.world.task(FAILED_CHECK, "root/");
    f.tests.depends_on(FAILED_CHECK, FAILED_DEP);
    f.world.fails_with(FAILED_DEP, "api/lib", "broken");
    work(&f, "unit/a", "agent-a", &[("api/lib", Some("broken"))]);
    f
}

/// 149, 92, 16 (rules core.7, core.18): a session's run in which the
/// check a branch requires was failed by a task it depends on names the
/// task failed and the check failed by it, never as not run, and exits
/// 1; the task's fail is recorded under its key and nothing under the
/// check's, and the worktree's last result is failed.
#[test]
fn exec_names_a_check_failed_by_its_dependency() {
    let f = failed_by_its_dependency();
    let head = sluice_seams::Driver::session(&f.driver, "unit/a")
        .unwrap()
        .head;
    let (code, out) = exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, FAILED, "{out}");
    assert!(out.contains("failed: api:integration\n"), "{out}");
    assert!(
        out.contains("  root:integration failed by api:integration, a task it depends on\n"),
        "{out}"
    );
    assert!(!out.contains("not run:"), "{out}");
    assert!(!out.contains("passed:"), "{out}");
    assert!(
        out.contains(&format!(
            "recorded 1 result under its key for {}\n",
            head.short()
        )),
        "{out}"
    );
    let lines = result_lines(&f);
    assert_eq!(lines.len(), 1, "{lines:?}");
    let l = &lines[0];
    assert_eq!(l.task, FAILED_DEP);
    assert_eq!(l.outcome, Outcome::Fail);
    assert!(!l.hash.is_empty());
    assert_eq!(l.kind, sluice_core::records::ResultKind::Ran);
    assert_eq!(l.combination, head);
    assert_eq!(l.worktree.as_deref(), Some("unit/a"));
    let last = &overview_doc(&f, 0)["sessions"][0]["last_result"];
    assert_eq!(last["outcome"], "fail", "{last}");
    assert_eq!(last["recorded"], true, "{last}");
    assert_eq!(last["reused"], false, "{last}");
}

/// 149: `sluice exec --json` gives the task a check was failed by in
/// `dependencies`, with its key, its fail and that it was recorded, and
/// the check in `failed_by`, never in `missing`; the exit is 1.
#[test]
fn exec_json_gives_dependencies_and_failed_by() {
    let f = failed_by_its_dependency();
    let (code, out) = exec(&f, &[BRANCH, "--worktree", "unit/a", "--json"]);
    assert_eq!(code, FAILED, "{out}");
    let doc: serde_json::Value =
        serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
    assert_eq!(out.lines().count(), 1, "{out}");
    assert_eq!(doc["exit"], 1);
    assert_eq!(
        doc["failed_by"],
        serde_json::json!([{"task": FAILED_CHECK, "by": [FAILED_DEP]}])
    );
    let deps = doc["dependencies"].as_array().unwrap();
    assert_eq!(deps.len(), 1, "{doc}");
    assert_eq!(deps[0]["task"], FAILED_DEP);
    assert_eq!(deps[0]["outcome"], "fail");
    assert_eq!(deps[0]["recorded"], true);
    assert!(
        deps[0]["hash"].as_str().is_some_and(|h| !h.is_empty()),
        "{doc}"
    );
    assert_eq!(doc["missing"], serde_json::json!([]));
    assert_eq!(doc["tasks"], serde_json::json!([]));
}

/// 149, 20: `sluice wait` for a submission blamed for a check failed by a
/// task it depends on names the task failed, then the check failed by it,
/// and where the task's log is kept; it never names the check not run,
/// nor gives a log of a check that never ran.
#[test]
fn wait_names_the_task_a_check_was_failed_by() {
    let f = failed_by_its_dependency();
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    let (code, out) = sluice(&f, &["wait", "1"]);
    assert_eq!(code, FAILED, "{out}");
    let lines = outcome(&out);
    let at = lines
        .iter()
        .position(|l| *l == "failed: api:integration")
        .unwrap_or_else(|| panic!("{out}"));
    assert_eq!(
        lines[at + 1],
        "  root:integration failed by api:integration, a task it depends on"
    );
    assert!(
        lines[at + 2].starts_with(&format!("  on {BRANCH} + #1 (")),
        "{out}"
    );
    let log = lines
        .iter()
        .find_map(|l| l.strip_prefix("  log of api:integration: "))
        .unwrap_or_else(|| panic!("{out}"));
    assert!(f
        .world
        .kept_log(log)
        .unwrap()
        .starts_with("api:integration failed"));
    assert!(!out.contains("not run:"), "{out}");
    assert!(!out.contains("log of root:integration"), "{out}");
    assert_eq!(
        lines.last(),
        Some(&"the agent working in unit/a fixes it there and submits again"),
        "{out}"
    );
}

/// 149: `sluice wait --json` and `sluice status --json` give the record's
/// `failed` as it is: the task that ran and failed with a non-empty
/// `hash` and its `log`, the shape a tool reads as a check that ran and
/// failed, and the check it failed with `failed_by` and no `hash`.
#[test]
fn the_record_names_a_failed_check_with_its_key() {
    let f = failed_by_its_dependency();
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, FAILED, "{out}");
    let (code, out) = sluice(&f, &["wait", "1", "--json"]);
    assert_eq!(code, FAILED, "{out}");
    let doc = json(&out);
    let failed = doc["failed"].as_array().unwrap();
    assert_eq!(failed.len(), 2, "{doc}");
    let dep = failed.iter().find(|e| e["task"] == FAILED_DEP).unwrap();
    assert!(dep["hash"].as_str().is_some_and(|h| !h.is_empty()), "{doc}");
    assert!(dep["log"].is_string(), "{doc}");
    assert!(dep.get("failed_by").is_none(), "{doc}");
    let check = failed.iter().find(|e| e["task"] == FAILED_CHECK).unwrap();
    assert_eq!(check["failed_by"], serde_json::json!([FAILED_DEP]));
    assert!(
        check.get("hash").is_none() && check.get("log").is_none(),
        "{doc}"
    );
    let (_, out) = sluice(&f, &["status", "1", "--json"]);
    assert_eq!(json(&out)["failed"], doc["failed"]);
}

/// 149: a record written before `failed_by`, as sluice 0.66.0 wrote one,
/// its `failed` naming the required check with no `hash` and nothing of
/// the task it depends on, reads as it did: the check not run, its log
/// given, and its `--json` as it is.
#[test]
fn an_older_record_reads_as_before() {
    let f = failed_by_its_dependency();
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, FAILED, "{out}");
    let older = serde_json::json!([{"task": FAILED_CHECK, "log": "logs/run.log"}]);
    edit_record(&f, 1, |doc| doc["failed"] = older.clone());
    let (code, out) = sluice(&f, &["wait", "1"]);
    assert_eq!(code, FAILED, "{out}");
    let lines = outcome(&out);
    assert!(lines.contains(&"not run: root:integration"), "{out}");
    assert!(
        lines.contains(&"  log of root:integration: logs/run.log"),
        "{out}"
    );
    assert!(!out.contains("failed:"), "{out}");
    assert!(!out.contains("failed by"), "{out}");
    let (_, out) = sluice(&f, &["status", "1", "--json"]);
    assert_eq!(json(&out)["failed"], older);
}

// ---------------------------------------------------------------------------
// sluice release (session-runs-1 clauses 94, 95)
// ---------------------------------------------------------------------------

const RELEASING: &str = "[tests]\nrelease = \"sh release.sh\"\n\n";

fn setup_releasing() -> Fakes {
    let f = Fakes::new();
    f.world.init(
        BRANCH,
        &[
            (".config/sluice.toml", &format!("{RELEASING}{POLICY}")),
            ("plugin/lib/plan.py", "plan"),
        ],
    );
    f.world.task("plugin:test", "plugin/");
    f
}

fn release(f: &Fakes, args: &[&str]) -> (i32, String) {
    let cli = Cli::try_parse_from(
        ["sluice", "release"]
            .into_iter()
            .chain(args.iter().copied()),
    )
    .unwrap();
    let mut out = Vec::new();
    let host = support::FakeHost::new(f.world.clone(), 0);
    let code = run(
        cli,
        f.seams(),
        &host,
        &(),
        &(),
        &f.clock,
        &Rounds(Cell::new(0)),
        &mut out,
    )
    .unwrap();
    (code, String::from_utf8(out).unwrap())
}

/// 94, 95: the worktree's runs leave its place equipped; `sluice release`
/// releases it under the release command its branch's policy states now,
/// and a release runs the policy's release command each time it is asked,
/// held or not (record 0120).
#[test]
fn release_runs_the_policys_release_command_once() {
    let f = setup_releasing();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    assert!(
        f.world.session_releases().is_empty(),
        "released between runs"
    );
    assert_eq!(f.world.adopted_places(), ["session/unit/a"]);

    // The policy's release command changed after the last run: the new
    // one is the one that runs (56).
    f.world.hand_commit(
        BRANCH,
        &[(
            ".config/sluice.toml",
            Some(&format!(
                "[tests]\nrelease = \"sh teardown.sh\"\n\n{POLICY}"
            )),
        )],
    );
    let (code, out) = release(&f, &["--worktree", "unit/a"]);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "released unit/a: its runs ended, the release command run, its ports free\n"
    );
    assert_eq!(
        f.world.session_releases(),
        [(
            "session/unit/a".to_string(),
            Some("sh teardown.sh".to_string())
        )]
    );
    assert!(f.world.adopted_places().is_empty());

    // Released again: sluice holds nothing for it now, and the release
    // command runs all the same, read from the default branch (record
    // 0120).
    let (code, out) = release(&f, &["--worktree", "unit/a"]);
    assert_eq!(
        (code, out.as_str()),
        (
            0,
            "released unit/a: its runs ended, the release command run, its ports free\n"
        )
    );
    assert_eq!(f.world.session_releases().len(), 2);
    let (code, out) = release(&f, &["--worktree", "unit/a", "--json"]);
    assert_eq!(code, 0);
    assert_eq!(
        json(&out),
        serde_json::json!({"worktree": "unit/a", "released": true})
    );
    assert_eq!(
        f.world.session_releases()[1..],
        [
            (
                "session/unit/a".to_string(),
                Some("sh teardown.sh".to_string())
            ),
            (
                "session/unit/a".to_string(),
                Some("sh teardown.sh".to_string())
            )
        ]
    );
    assert!(f.world.adopted_places().is_empty());
    // A folder that is no worktree of the repository has nothing to
    // release.
    let (code, out) = release(&f, &["--worktree", "unit/none"]);
    assert_eq!((code, out.as_str()), (0, "nothing to release\n"));
}

/// 95: with the branch gone, the release command the last run carried
/// runs, and the output says so.
#[test]
fn release_falls_back_to_the_last_runs_command_and_says_so() {
    let f = setup_releasing();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    f.world.delete_branch(BRANCH);
    let (code, out) = release(&f, &["--worktree", "unit/a"]);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[0],
        format!(
            "{BRANCH} cannot be read (no branch {BRANCH}): running the release command saved with the last run"
        )
    );
    assert_eq!(
        f.world.session_releases(),
        [("session/unit/a".to_string(), None)]
    );
}

/// `sluice release --json`: the worktree, and that it was released; what
/// it says beside goes to stderr.
#[test]
fn release_under_json() {
    let f = setup_releasing();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    let (code, out) = release(&f, &["--worktree", "unit/a", "--json"]);
    assert_eq!(code, 0);
    assert_eq!(
        json(&out),
        serde_json::json!({"worktree": "unit/a", "released": true})
    );
    assert_eq!(f.world.session_releases().len(), 1);
}

/// 95: `sluice exec --release` releases once its run ends, and the exit
/// is the run's.
#[test]
fn exec_release_releases_after_the_run() {
    let f = setup_releasing();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    let (code, out) = exec(&f, &[BRANCH, "--worktree", "unit/a", "--release"]);
    assert_eq!(code, LANDED, "{out}");
    assert!(
        out.ends_with("released unit/a: its runs ended, the release command run, its ports free\n"),
        "{out}"
    );
    assert_eq!(
        f.world.session_releases(),
        [(
            "session/unit/a".to_string(),
            Some("sh release.sh".to_string())
        )]
    );
}

/// 25, rule core.16: a submission from a worktree that ran `sluice exec`
/// lands, and no round releases the worktree's place.
#[test]
fn a_landing_leaves_a_sessions_place_alone() {
    let f = setup_releasing();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    let (code, _) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED);
    assert_eq!(f.world.adopted_places(), ["session/unit/a"]);
    assert!(f.world.session_releases().is_empty());
}

// ---------------------------------------------------------------------------
// A worktree removed unreleased, and the worktrees held (session-runs-1
// clause 97)
// ---------------------------------------------------------------------------

const GONE: &str = "freed unit/a: it no longer exists; its release ran once from its scratch directory; its runs ended, its ports free";

/// 97, 123: the next `sluice exec` on the host runs the release a removed
/// worktree's runs stored, once, frees its place, says so first, and logs
/// it `gone` in the branch's records with its release run; no release is
/// built from the policy, and the worktree it runs in is left alone.
#[test]
fn the_next_exec_frees_a_removed_worktrees_place() {
    let f = setup_releasing();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    work(
        &f,
        "unit/b",
        "agent-b",
        &[("plugin/lib/plan.py", Some("p3"))],
    );
    exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    f.world.remove_worktree("unit/a");

    let (code, out) = exec(&f, &[BRANCH, "--worktree", "unit/b"]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(out.lines().next(), Some(GONE), "{out}");
    assert!(f.world.session_releases().is_empty(), "a release was built");
    assert_eq!(f.world.gone_releases(), ["session/unit/a"]);
    assert_eq!(f.world.freed_places(), ["session/unit/a"]);
    assert_eq!(f.world.adopted_places(), ["session/unit/b"]);
    let snap = sluice_seams::State::read(&f.state, BRANCH).unwrap();
    let log = sluice_core::line::Line::read_log(&snap).unwrap();
    assert!(
        log.iter().any(|l| matches!(
            &l.event,
            sluice_core::records::Event::Gone { place, worktree: Some(w), ran: true, failed: None }
                if place == "session/unit/a" && w == "unit/a"
        )),
        "{log:?}"
    );

    // Freed once: the next run names it no more, and runs nothing of it.
    let (_, out) = exec(&f, &[BRANCH, "--worktree", "unit/b"]);
    assert!(!out.contains("unit/a"), "{out}");
    assert_eq!(f.world.freed_places().len(), 1);
    assert_eq!(f.world.gone_releases().len(), 1);
}

/// 97, 123: the next round, `sluice step` or a pass of `sluice run`,
/// frees it as well and says so.
#[test]
fn the_next_round_frees_a_removed_worktrees_place() {
    for args in [&["step", BRANCH][..], &["run", "--rounds", "1"][..]] {
        let f = setup_releasing();
        work(
            &f,
            "unit/a",
            "agent-a",
            &[("plugin/lib/plan.py", Some("p2"))],
        );
        exec(&f, &[BRANCH, "--worktree", "unit/a"]);
        f.world.remove_worktree("unit/a");
        let (code, out) = sluice(&f, args);
        assert_eq!(code, 0, "{out}");
        assert_eq!(out.lines().next(), Some(GONE), "{args:?}: {out}");
        assert!(f.world.adopted_places().is_empty(), "{args:?}");
    }
}

/// 97, 25, rule core.16: a worktree that still exists is never released by
/// a round or a run, however long since its last run.
#[test]
fn a_worktree_that_exists_is_held_however_old_its_last_run() {
    let f = setup_releasing();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    f.world.advance(chrono::Duration::days(90));
    sluice(&f, &["step", BRANCH]);
    sluice(&f, &["run", "--rounds", "3"]);
    work(
        &f,
        "unit/b",
        "agent-b",
        &[("plugin/lib/plan.py", Some("p3"))],
    );
    exec(&f, &[BRANCH, "--worktree", "unit/b"]);
    assert!(f.world.session_releases().is_empty());
    assert_eq!(
        f.world.adopted_places(),
        ["session/unit/a", "session/unit/b"]
    );
}

/// 97: `sluice status` with no argument lists every worktree sluice holds,
/// with its branch, its ports and the time of its last run; `--json` is
/// one document of the same.
#[test]
fn status_lists_the_worktrees_held() {
    let f = setup_releasing();
    let (code, out) = sluice(&f, &["status"]);
    assert_eq!(code, 0);
    assert_eq!(out, "no worktree holds ports or runs here\n");

    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    let (code, out) = sluice(&f, &["status"]);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "worktrees held until released:", "{out}");
    assert!(
        lines[1].starts_with("  unit/a  bolt/concurrent-units  last run 2026-10-11T09:"),
        "{out}"
    );
    assert_eq!(
        lines[2],
        "release one with: sluice release --worktree <worktree>"
    );

    // A removed worktree is listed as such until a run or round frees it.
    f.world.remove_worktree("unit/a");
    let (_, out) = sluice(&f, &["status"]);
    assert!(
        out.contains("(removed: the next run or round runs its stored release once and frees it)"),
        "{out}"
    );
    assert!(f.world.session_releases().is_empty(), "status released");

    let (code, out) = sluice(&f, &["status", "--json"]);
    assert_eq!(code, 0);
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    let w = &doc["worktrees"][0];
    assert_eq!(w["worktree"], "unit/a");
    assert_eq!(w["branch"], BRANCH);
    assert!(w["ports"].is_object());
    assert!(w["last_run"]
        .as_str()
        .unwrap()
        .starts_with("2026-10-11T09:"));
    assert_eq!(w["removed"], true);
}

/// Session-runs-1 clauses 92, 97, rule core.18: `sluice status --json`'s
/// `worktrees[]` and `sluice overview --json`'s `sessions[]` carry the
/// same `last_result`, beside the fields they had; `sluice status` says
/// it on the worktree's line; a watch prints again when a run ends.
#[test]
fn status_and_overview_carry_a_worktrees_last_result() {
    let f = setup_releasing();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    let head = sluice_seams::Driver::session(&f.driver, "unit/a")
        .unwrap()
        .head;
    exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    let (_, out) = sluice(&f, &["status", "--json"]);
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    let w = doc["worktrees"][0].clone();
    let last = &w["last_result"];
    assert_eq!(last["head"], head.0, "{w}");
    assert_eq!(last["kind"], "checks", "{w}");
    assert_eq!(last["outcome"], "pass", "{w}");
    assert_eq!(last["recorded"], true, "{w}");
    assert!(last["at"].is_string(), "{w}");
    assert!(w["last_run"].is_string() && w["ports"].is_object(), "{w}");
    assert_eq!(w["removed"], false, "{w}");
    let (_, out) = sluice(&f, &["overview", "--json"]);
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["sessions"][0], w, "{doc}");
    let (_, out) = sluice(&f, &["status"]);
    assert!(
        out.contains(&format!("last result passed on {}, recorded", head.short())),
        "{out}"
    );

    // A watch prints again when a run ends; this one unclean.
    let between = |n: usize| {
        if n == 2 {
            f.world.dirty("unit/a", &["scratch"]);
            exec(&f, &[BRANCH, "--worktree", "unit/a"]);
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 4,
        between: &between,
    };
    let cli = Cli::try_parse_from(["sluice", "overview", "--json", "--watch"]).unwrap();
    let mut out = Vec::new();
    run(
        cli,
        f.seams(),
        &support::FakeHost::new(f.world.clone(), 0),
        &(),
        &(),
        &f.clock,
        &pause,
        &mut out,
    )
    .unwrap();
    let out = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(
        second["sessions"][0]["last_result"]["recorded"], false,
        "{second}"
    );
    let (_, out) = sluice(&f, &["status"]);
    assert!(out.contains("last result passed on"), "{out}");
    assert!(out.contains(", not recorded"), "{out}");
}

/// Session-runs-1 clause 97: a result that cannot be kept on the worktree
/// changes neither the run's exit nor what it says; the warning is on
/// stderr, and the worktree lists no result.
#[test]
fn a_result_not_kept_changes_nothing_of_the_run() {
    let run = |keeping_fails: bool| {
        let f = setup_releasing();
        work(
            &f,
            "unit/a",
            "agent-a",
            &[("plugin/lib/plan.py", Some("p2"))],
        );
        f.world.keeping_fails(keeping_fails);
        let ran = exec(&f, &[BRANCH, "--worktree", "unit/a"]);
        let (_, out) = sluice(&f, &["status", "--json"]);
        let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
        (ran, doc["worktrees"][0]["last_result"].clone())
    };
    let (kept, listed) = run(false);
    assert_eq!(kept.0, 0, "{}", kept.1);
    assert!(listed.is_object(), "{listed}");
    let (unkept, listed) = run(true);
    assert_eq!(unkept, kept, "the run's exit and output");
    assert!(listed.is_null(), "{listed}");
}

/// ADR 0024: a worktree that has checked out a branch sluice lands on, by
/// its records or by a gate its policy gives it, is named in overview's
/// `sessions[]`, in `sluice status` and by `sluice protect`'s output; a
/// detached one and one on a branch of its own are not, and the landing
/// goes through.
#[test]
fn a_worktree_holding_a_branch_sluice_lands_on_is_warned_of() {
    let f = setup();
    let other = "bolt/no-records";
    f.world.init(other, &[(".config/sluice.toml", POLICY)]);
    f.world.check_out("/w/main-like", Some(BRANCH));
    f.world.check_out("/w/bolt", Some(other));
    f.world.check_out("/w/view", Some(BRANCH));
    f.world.check_out("/w/view", None);
    work(
        &f,
        "unit/mine",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    f.world.init("unit/mine", &[("plugin/lib/plan.py", "p2")]);
    f.world.check_out("/w/unit", Some("unit/mine"));

    let (code, _) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/mine"]);
    assert_eq!(code, LANDED, "a held branch never stops a landing");

    let (_, json) = sluice(&f, &["overview", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let held: Vec<(&str, &str)> = v["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|w| Some((w["worktree"].as_str()?, w["holds_queued_branch"].as_str()?)))
        .collect();
    assert_eq!(held, vec![("/w/bolt", other), ("/w/main-like", BRANCH)]);
    assert!(v["sessions"][0].get("ports").is_none());

    let (_, text) = sluice(&f, &["overview"]);
    assert!(
        text.contains(&format!("warning: /w/main-like has {BRANCH} checked out")),
        "{text}"
    );
    assert!(!text.contains("worktrees held"), "{text}");

    let (code, out) = sluice(&f, &["status"]);
    assert_eq!(code, 0);
    assert!(
        out.contains(&format!(
            "warning: /w/main-like has {BRANCH} checked out: sluice moves {BRANCH} under it"
        )),
        "{out}"
    );
    let (_, out) = sluice(&f, &["status", "--json"]);
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["worktrees"].as_array().unwrap().len(), 2);
}

/// A session's place whose worktree holds a branch sluice lands on carries
/// `holds_queued_branch` on its own entry, not a second one.
#[test]
fn a_held_place_is_one_entry() {
    let f = setup_releasing();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    f.world.check_out("unit/a", Some(BRANCH));
    let (_, out) = sluice(&f, &["status", "--json"]);
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    let w = doc["worktrees"].as_array().unwrap();
    assert_eq!(w.len(), 1, "{out}");
    assert_eq!(w[0]["holds_queued_branch"], BRANCH);
    assert!(w[0]["ports"].is_object());
}

/// `sluice` with a pause of the test's own.
fn sluice_with(f: &Fakes, pause: &dyn Pause, args: &[&str]) -> (i32, String) {
    let cli = Cli::try_parse_from(std::iter::once("sluice").chain(args.iter().copied())).unwrap();
    let mut out = Vec::new();
    let host = support::FakeHost::new(f.world.clone(), 0);
    let code = settled(|| run(cli, f.seams(), &host, &(), &(), &f.clock, pause, &mut out)).unwrap();
    (code, String::from_utf8(out).unwrap())
}

/// The combination submission `id` is under test in.
fn tested(f: &Fakes, id: u64) -> sluice_seams::Rev {
    let (_, out) = sluice(f, &["status", &id.to_string(), "--json"]);
    sluice_seams::Rev::new(json(&out)["tested"]["combination"].as_str().unwrap())
}

/// ADR 0041: `sluice wait` runs rounds to the outcome, as a waiting
/// submit does; a submission already decided returns at once, and no
/// round runs.
#[test]
fn wait_runs_to_the_outcome() {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    let (code, out) = sluice(&f, &["wait", "1"]);
    assert_eq!(code, LANDED, "{out}");
    let tip = f.world.tip(BRANCH);
    assert_eq!(out.lines().next(), Some("#1 queued"), "{out}");
    assert!(
        out.ends_with(&format!("passed · landed as {}\n", &tip.0[..7])),
        "{out}"
    );

    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/b", "--no-wait"]);
    let (code, out) = sluice(&f, &["wait", "1", "--json"]);
    assert_eq!(code, LANDED);
    assert_eq!(json(&out)["state"], "landed");
    // No round ran: #2 is still queued.
    let (_, out) = sluice(&f, &["status", "2", "--json"]);
    assert_eq!(json(&out)["state"], "queued");
}

/// A report waited on from another worktree names the agent working in
/// the one the submission came from as who moves it on, and sends its
/// reader into no worktree; a step that is the operator's, setting a fail
/// aside, still names the operator.
#[test]
fn a_report_waited_on_elsewhere_names_the_agent_in_the_worktree_it_came_from() {
    let f = setup();
    f.world
        .fails_with("tests:t-batch-cards", "tests/t_cards.py", "broken");
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("tests/t_cards.py", Some("broken"))],
    );
    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    let (code, out) = sluice(&f, &["wait", "1", "--worktree", "unit/b"]);
    assert_eq!(code, FAILED, "{out}");
    assert_eq!(
        out.lines().last(),
        Some("the agent working in unit/a fixes it there and submits again"),
        "{out}"
    );
    for word in ["your worktree", "inside"] {
        assert!(!out.contains(word), "{word}: {out}");
    }

    let f = setup();
    f.world
        .flaky("tests:t-batch-cards", &[Outcome::Fail, Outcome::Pass]);
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("tests/t_cards.py", Some("flaky"))],
    );
    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    let submit = ["submit", BRANCH, "--worktree", "unit/a", "--no-wait"];
    sluice(&f, &submit);
    assert_eq!(sluice(&f, &["wait", "1", "--worktree", "unit/b"]).0, FAILED);
    sluice(&f, &submit);
    let (code, out) = sluice(&f, &["wait", "2", "--worktree", "unit/b"]);
    assert_eq!(code, FAILED, "{out}");
    assert!(
        out.contains("the operator can set it aside: sluice set-aside tests:t-batch-cards --at "),
        "{out}"
    );
    for word in ["your worktree", "inside"] {
        assert!(!out.contains(word), "{word}: {out}");
    }
}

/// ADR 0041: the exits tell a blame, a conflict and a withdrawal apart,
/// for `wait` and a waiting submit alike.
#[test]
fn wait_and_submit_tell_the_outcomes_apart() {
    let f = setup();
    f.world
        .fails_with("tests:t-batch-cards", "tests/t_cards.py", "broken");
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("tests/t_cards.py", Some("broken"))],
    );
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    let (code, out) = sluice(&f, &["wait", "1"]);
    assert_eq!(code, FAILED, "{out}");
    assert!(out.contains("failed: tests:t-batch-cards"), "{out}");

    work(
        &f,
        "unit/b",
        "agent-b",
        &[("plugin/lib/plan.py", Some("one"))],
    );
    work(
        &f,
        "unit/c",
        "agent-c",
        &[("plugin/lib/plan.py", Some("two"))],
    );
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/b", "--no-wait"]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/c", "--no-wait"]);
    let (code, out) = sluice(&f, &["wait", "3", "--json"]);
    assert_eq!(code, CONFLICT, "{out}");
    assert_eq!(json(&out)["state"], "conflict");

    f.world.hold_runs(true);
    work(&f, "unit/d", "agent-d", &[("plugin/d", Some("d"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/d", "--no-wait"]);
    let withdraw = |n: usize| {
        if n == 1 {
            sluice(&f, &["withdraw", "4"]);
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 5,
        between: &withdraw,
    };
    let (code, out) = sluice_with(&f, &pause, &["wait", "4", "--json"]);
    assert_eq!(code, sluice::WITHDRAWN, "{out}");
    assert_eq!(json(&out)["state"], "withdrawn");

    work(&f, "unit/e", "agent-e", &[("plugin/e", Some("e"))]);
    let withdraw = |n: usize| {
        if n == 1 {
            sluice(&f, &["withdraw", "5"]);
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 5,
        between: &withdraw,
    };
    let (code, out) = sluice_with(&f, &pause, &["submit", BRANCH, "--worktree", "unit/e"]);
    assert_eq!(code, sluice::WITHDRAWN, "{out}");
}

/// ADR 0041: at its timeout, `wait` returns the record as it stands and
/// exits 5, and the submission stays in the queue; stopped with Ctrl-C,
/// one document still.
#[test]
fn wait_returns_at_its_timeout_with_the_record() {
    let f = setup();
    f.world.hold_runs(true);
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    let (code, out) = sluice(&f, &["wait", "1", "--timeout", "10s", "--json"]);
    assert_eq!(code, sluice::WAITING, "{out}");
    let v = json(&out);
    assert_eq!(
        (v["id"].as_u64(), v["state"].as_str()),
        (Some(1), Some("testing"))
    );
    let (_, out) = sluice(&f, &["status", "1", "--json"]);
    assert_eq!(json(&out)["state"], "testing");

    let (code, out) = sluice(&f, &["wait", "1", "--timeout", "10s"]);
    assert_eq!(code, sluice::WAITING, "{out}");
    assert!(out.contains(&format!("testing {BRANCH} + #1 · ")), "{out}");
    assert!(out.ends_with("still waiting: sluice wait 1\n"), "{out}");

    let stop = Then(Cell::new(0), |_| false);
    let (code, out) = sluice_with(&f, &stop, &["wait", "1", "--json"]);
    assert_eq!(code, sluice::STOPPED);
    assert_eq!(json(&out)["state"], "testing");
}

/// 1: a wait whose time is up before its first pass, as on a host so busy
/// that its start takes its whole limit, still takes that pass: its round
/// starts the queued submission's run, and out of time its record is as
/// that pass left it, never as it stood before the wait moved anything.
#[test]
fn a_wait_out_of_time_at_its_start_takes_its_first_pass() {
    let f = setup();
    f.world.hold_runs(true);
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    assert_eq!(state_of(&f, 1), "queued");
    let (code, out) = sluice(&f, &["wait", "1", "--timeout", "0s", "--json"]);
    assert_eq!(code, sluice::WAITING, "{out}");
    assert_eq!(json(&out)["state"], "testing", "{out}");
    assert_eq!(f.world.started().len(), 1, "{out}");
}

/// ADR 0041: a branch names this worktree's last submission to it;
/// nothing submitted, or no such submission, is not found.
#[test]
fn wait_finds_what_it_waits_on() {
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    work(&f, "unit/c", "agent-c", &[("plugin/c", Some("c"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/b", "--no-wait"]);
    let (code, out) = sluice(&f, &["wait", BRANCH, "--worktree", "unit/a", "--json"]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(json(&out)["id"], 1);

    let (code, out) = sluice(&f, &["wait", BRANCH, "--worktree", "unit/c", "--json"]);
    assert_eq!(code, FAILED);
    assert_eq!(json(&out)["error"]["kind"], "not_found");
    assert_eq!(
        json(&out)["error"]["message"],
        format!("nothing submitted from unit/c to {BRANCH}")
    );

    let (code, out) = sluice(&f, &["wait", "9999", "--json"]);
    assert_eq!(code, FAILED);
    assert_eq!(json(&out)["error"]["message"], "no submission 9999");
}

/// A pause a wait never takes once a read or a round failed as the forge
/// could not answer: the retry policy read again for it (rule core.29).
struct Unpaused;

impl Pause for Unpaused {
    fn pause(&self) -> bool {
        panic!("the wait paused to read again")
    }
}

/// 134, 133 (design D9): a round of the host process's whose call the
/// forge could not answer for the retry policy's time is said once on
/// stderr, in its words, and `sluice wait` goes on following: the round
/// after lands the submission, and the wait reports it, exit 0.
#[test]
fn a_round_the_forge_could_not_answer_is_said_once_and_the_wait_goes_on() {
    const TEST: &str = "a_round_the_forge_could_not_answer_is_said_once_and_the_wait_goes_on";
    if let Some(err) = stderr_of(TEST) {
        let said: Vec<&str> = err
            .lines()
            .filter(|l| l.starts_with(&format!("sluice: {BRANCH}: ")))
            .collect();
        assert_eq!(said.len(), 1, "{err}");
        assert!(said[0].ends_with(": 503 Service Unavailable"), "{err}");
        assert!(!err.contains("could not read #"), "{err}");
        return;
    }
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    f.world.unanswer_starts(1);
    let stop = Then(Cell::new(0), |n| n < 40);
    let (code, out) = sluice_with(&f, &stop, &["wait", "1"]);
    assert_eq!(code, LANDED, "{out}");
    let tip = f.world.tip(BRANCH);
    assert!(
        out.ends_with(&format!("passed · landed as {}\n", &tip.0[..7])),
        "{out}"
    );
}

/// 134: Ctrl-C while a wait's call to a remote is between tries ends the
/// retry policy's pauses, and the call fails as its last try did; the wait
/// stops at once, as a wait stopped by Ctrl-C does, never as one that
/// could not read, and the submission stays queued. Alone in a process of
/// its own, which Ctrl-C reaches whole.
#[test]
fn a_wait_stopped_between_tries_is_a_stopped_wait() {
    const TEST: &str = "a_wait_stopped_between_tries_is_a_stopped_wait";
    if let Some(err) = stderr_of(TEST) {
        assert!(!err.contains("could not read"), "{err}");
        return;
    }
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    f.world.unanswer_starts(1);
    sluice::stop_on_interrupt().unwrap();
    nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT).unwrap();
    let (code, out) = sluice_with(&f, &Unpaused, &["wait", "1"]);
    assert_eq!(
        f.world.unanswered_starts_left(),
        0,
        "the round's call was made"
    );
    assert_eq!(code, sluice::STOPPED, "{out}");
    assert!(out.ends_with("still waiting: sluice wait 1\n"), "{out}");
    let (_, out) = sluice(&f, &["status", "1", "--json"]);
    assert!(
        !["failed", "landed", "withdrawn"].contains(&json(&out)["state"].as_str().unwrap()),
        "{out}"
    );
}

/// Pull request 7 into the branch, from any worktree, waiting; its forge
/// answers 5xx to as many finds and follows as each count holds, each as a
/// retry policy spent after five minutes of them answers it. It keeps the
/// subject of each open.
struct Pull7 {
    finds: Cell<usize>,
    follows: Cell<usize>,
    subjects: RefCell<Vec<Option<String>>>,
}

impl Pull7 {
    fn new(finds: usize, follows: usize) -> Pull7 {
        Pull7 {
            finds: Cell::new(finds),
            follows: Cell::new(follows),
            subjects: RefCell::default(),
        }
    }

    /// A read of the pull request, answered 5xx while `left` holds any.
    fn read(left: &Cell<usize>) -> anyhow::Result<()> {
        if left.get() == 0 {
            return Ok(());
        }
        left.set(left.get() - 1);
        let e: anyhow::Error =
            sluice_seams::Unanswered("GET /repos/o/r/pulls/7: 503 Service Unavailable".into())
                .into();
        Err(e.context(sluice::retry::Spent {
            tries: 9,
            lasted: std::time::Duration::from_secs(300),
        }))
    }
}

impl sluice::PullRequests for Pull7 {
    fn branches(&self, _: sluice_core::Seams<'_>) -> anyhow::Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn intake(
        &self,
        _: sluice_core::Seams<'_>,
        _: &dyn sluice_core::Clock,
        _: &str,
    ) -> anyhow::Result<sluice::Intake> {
        Ok(sluice::Intake {
            takes: true,
            causes: Vec::new(),
        })
    }

    fn open(
        &self,
        _: sluice_core::Seams<'_>,
        _: &dyn sluice_core::Clock,
        _: &sluice_core::SubmitHead,
        _: &str,
        subject: Option<&str>,
    ) -> anyhow::Result<Result<sluice::Opened, sluice_core::SubmitRefusal>> {
        self.subjects.borrow_mut().push(subject.map(String::from));
        Ok(Ok(sluice::Opened {
            number: 7,
            said: "opened #7".into(),
            copy: None,
            submission: None,
        }))
    }

    fn find(
        &self,
        _: sluice_core::Seams<'_>,
        _: &dyn sluice_core::Clock,
        _: &str,
        _: &str,
    ) -> anyhow::Result<Option<u64>> {
        Pull7::read(&self.finds)?;
        Ok(Some(7))
    }

    fn follow(
        &self,
        _: sluice_core::Seams<'_>,
        _: &dyn sluice_core::Clock,
        _: &str,
        _: u64,
    ) -> anyhow::Result<sluice::Followed> {
        Pull7::read(&self.follows)?;
        Ok(sluice::Followed::Open {
            title: "waiting".into(),
            lines: Vec::new(),
            taken_in_by: None,
            queue: None,
            submission: None,
            names: Default::default(),
            head: None,
            draft: false,
        })
    }
}

/// The branch, taking pull requests.
fn pulls_setup() -> Fakes {
    let f = Fakes::new();
    // 137 (design D22, D28): a signer named, so `bolt/*`'s records are kept
    // on its remote and the host that keeps it takes its pull requests in.
    let policy = "[signers.a]\nssh = \".config/sluice.signers\"\n\n".to_string()
        + &POLICY
            .replace(
                "[branches.\"bolt/*\"]\n",
                "[branches.\"bolt/*\"]\nremote = \"origin\"\n",
            )
            .replace(
                "trust = [\"open\", \"sandbox\", \"container\"]\n",
                "trust = [\"a\"]\n",
            )
        + "\n[branches.\"bolt/*\".pull_requests]\n";
    f.world.init(
        BRANCH,
        &[(".config/sluice.toml", &policy), ("plugin/a", "a")],
    );
    f.world.task("plugin:test", "plugin/");
    f
}

/// `sluice <args>` over the fakes, its pull requests from `pulls`.
fn sluice_pulls(
    f: &Fakes,
    pulls: &dyn sluice::PullRequests,
    pause: &dyn Pause,
    args: &[&str],
) -> (i32, String) {
    let cli = Cli::try_parse_from(std::iter::once("sluice").chain(args.iter().copied())).unwrap();
    let mut out = Vec::new();
    let host = support::FakeHost::new(f.world.clone(), 0);
    let code =
        settled(|| run(cli, f.seams(), &host, &(), pulls, &f.clock, pause, &mut out)).unwrap();
    (code, String::from_utf8(out).unwrap())
}

/// C.5 pull request: a subject is one line. An empty one, or one of two
/// lines, is a command line sluice cannot read, which pushes, opens and
/// queues nothing.
#[test]
fn a_subject_is_one_line() {
    let f = pulls_setup();
    work(&f, "unit/b1", "agent-1", &[("plugin/one", Some("1"))]);
    let pulls = Pull7::new(0, 0);
    let one = ["--worktree", "unit/b1", "--subject"];
    for (subject, why) in [
        ("", "--subject is empty"),
        ("  ", "--subject is empty"),
        ("one\ntwo", "a subject is one line"),
        ("one\r\ntwo", "a subject is one line"),
    ] {
        let args = [&["submit", BRANCH][..], &one, &[subject]].concat();
        let (code, out) = sluice_pulls(&f, &pulls, &Rounds(Cell::new(0)), &args);
        assert_eq!(code, sluice::USAGE, "{args:?}: {out}");
        assert!(out.contains(why), "{args:?}: {out}");
    }
    assert!(pulls.subjects.borrow().is_empty());
    assert!(queued(&f).is_empty());
}

/// 125: a group takes one subject per worktree, in their order, or none;
/// one subject for two worktrees, or any other count, is refused before
/// anything is pushed, opened or queued, saying what to give.
#[test]
fn one_subject_for_two_worktrees_is_refused() {
    let give = "give one --subject per --worktree, in their order, or none";
    for f in [setup(), pulls_setup()] {
        work(&f, "unit/b1", "agent-1", &[("plugin/one", Some("1"))]);
        work(&f, "unit/b2", "agent-2", &[("plugin/two", Some("2"))]);
        let pulls = Pull7::new(0, 0);
        let two = ["--worktree", "unit/b1", "--worktree", "unit/b2"];
        for (args, said) in [
            (
                [&two[..], &["--subject", "Both"]].concat(),
                "1 subject for 2 worktrees",
            ),
            (
                [
                    &two[..],
                    &["--subject", "a", "--subject", "b", "--subject", "c"],
                ]
                .concat(),
                "3 subjects for 2 worktrees",
            ),
            (
                vec!["--worktree", "unit/b1", "--subject", "a", "--subject", "b"],
                "2 subjects for 1 worktree",
            ),
        ] {
            let args = [&["submit", BRANCH, "--no-wait"][..], &args].concat();
            let (code, out) = sluice_pulls(&f, &pulls, &Rounds(Cell::new(0)), &args);
            assert_eq!(code, sluice::USAGE, "{args:?}: {out}");
            assert_eq!(out, format!("sluice submit: {said}; {give}\n"), "{args:?}");
        }
        assert!(pulls.subjects.borrow().is_empty());
        assert!(queued(&f).is_empty());
    }
}

/// The subject reaches the pull request a submit opens, as written; a
/// submit with none gives none.
#[test]
fn a_subject_is_handed_to_the_pull_request_opened() {
    let f = pulls_setup();
    work(&f, "unit/b1", "agent-1", &[("plugin/one", Some("1"))]);
    let pulls = Pull7::new(0, 0);
    let submit = |rest: &[&str]| {
        let args = [
            &["submit", BRANCH, "--worktree", "unit/b1", "--no-wait"][..],
            rest,
        ]
        .concat();
        let (code, out) = sluice_pulls(&f, &pulls, &Rounds(Cell::new(0)), &args);
        assert_eq!(code, 0, "{out}");
    };
    submit(&["--subject", "Batch review cards"]);
    submit(&[]);
    assert_eq!(
        *pulls.subjects.borrow(),
        [Some("Batch review cards".to_string()), None]
    );
}

/// A subject titles what a submit opens: neither a forecast nor a
/// worktree's status takes one.
#[test]
fn a_subject_takes_neither_check_nor_status() {
    for flag in ["--check", "--status"] {
        let e =
            Cli::try_parse_from(["sluice", "submit", BRANCH, "--subject", "x", flag]).unwrap_err();
        assert_eq!(e.kind(), clap::error::ErrorKind::ArgumentConflict);
        let said = e.to_string();
        assert!(said.contains("--subject") && said.contains(flag), "{said}");
    }
}

/// Following a pull request, a find or a follow the forge could not
/// answer for the retry policy's time ends `sluice wait` and a waiting
/// submit at once (134; rule core.29): each says it could not read the
/// pull request, with how long it was tried, and exits 3, never that it
/// closed or failed, and never reads again of its own.
#[test]
fn following_a_pull_request_the_forge_could_not_answer_says_it_could_not_read() {
    let f = pulls_setup();
    let (code, out) = sluice_pulls(&f, &Pull7::new(0, 1), &Unpaused, &["wait", BRANCH]);
    assert_eq!(code, sluice::INTERNAL, "{out}");
    assert_eq!(out, format!("could not read #7: sluice wait {BRANCH}\n"));
    let (code, out) = sluice_pulls(
        &f,
        &Pull7::new(0, 1),
        &Unpaused,
        &["wait", BRANCH, "--json"],
    );
    assert_eq!(code, sluice::INTERNAL, "{out}");
    let v = json(&out);
    assert_eq!(v["error"]["kind"], "internal");
    assert_eq!(
        v["error"]["next"],
        serde_json::json!([format!("sluice wait {BRANCH}")])
    );
    let message = v["error"]["message"].as_str().unwrap();
    assert_eq!(
        message,
        "could not read #7 for 5m00s, so whether it landed is not known: \
         GET /repos/o/r/pulls/7: 503 Service Unavailable"
    );

    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("b"))]);
    let (code, out) = sluice_pulls(
        &f,
        &Pull7::new(0, 1),
        &Unpaused,
        &["submit", BRANCH, "--worktree", "unit/a"],
    );
    assert_eq!(code, sluice::INTERNAL, "{out}");
    assert_eq!(
        out,
        format!("opened #7\ncould not read #7: sluice wait {BRANCH}\n")
    );

    let (code, out) = sluice_pulls(
        &f,
        &Pull7::new(1, 0),
        &Unpaused,
        &["wait", BRANCH, "--worktree", "unit/a"],
    );
    assert_eq!(code, sluice::INTERNAL, "{out}");
    assert_eq!(
        out,
        format!(
            "could not read the pull request from unit/a into {BRANCH}: sluice wait {BRANCH}\n"
        )
    );
}

/// ADR 0042: a waiting command says its combination's progress each time
/// the count ended or the tasks running move, never at every round.
#[test]
fn wait_says_progress_when_it_moves() {
    use sluice_seams::{TaskProgress, TaskState};
    let f = setup();
    f.world.hold_runs(true);
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    let task = |target: &str, state: TaskState| TaskProgress {
        target: target.into(),
        state,
        started: None,
        ended: None,
        exit: state.ended().then_some(0),
    };
    let between = |n: usize| match n {
        1 => f.world.says_tasks(
            &tested(&f, 1),
            &[
                task("a:test", TaskState::Running),
                task("b:test", TaskState::Waiting),
            ],
        ),
        3 => f.world.says_tasks(
            &tested(&f, 1),
            &[
                task("a:test", TaskState::Passed),
                task("b:test", TaskState::Running),
            ],
        ),
        5 => {
            f.world.hold_runs(false);
            f.world.complete_all();
        }
        _ => {}
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 10,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["wait", "1"]);
    assert_eq!(code, LANDED, "{out}");
    let said: Vec<&str> = out
        .lines()
        .filter(|l| l.starts_with(&format!("testing {BRANCH} + #1 · ")))
        .filter(|l| !l.contains(" tasks to run, "))
        .collect();
    assert_eq!(said.len(), 3, "{out}");
    assert!(
        said[1].contains(" · 0/2 tasks · running a:test · "),
        "{out}"
    );
    assert!(
        said[2].contains(" · 1/2 tasks · running b:test · "),
        "{out}"
    );
}

// ---------------------------------------------------------------------------
// A checkout held because its release failed (123)
// ---------------------------------------------------------------------------

const RELEASE_FAILS: &str = "usage: scripts/preview up";

/// A submission landed while every release fails, and rounds until its
/// checkout is held; the checkout's name.
fn held_checkout(f: &Fakes) -> String {
    f.world.release_fails(Some(RELEASE_FAILS));
    work(
        f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    let (code, out) = sluice(f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    for _ in 0..4 {
        sluice(f, &["step", BRANCH]);
    }
    let place = format!("sluice/try/{BRANCH}/1");
    assert!(f.world.live_places().contains(&place));
    place
}

/// 123: `sluice release <name>` runs a held checkout's release once
/// more: failing again, it exits 1 saying what failed and the checkout
/// stays; once it succeeds, the checkout is gone.
#[test]
fn release_by_name_releases_a_held_checkout() {
    let f = setup();
    let place = held_checkout(&f);
    let tries = f.world.release_attempts().len();

    let (code, out) = sluice(&f, &["release", &place]);
    assert_eq!(code, FAILED, "{out}");
    assert!(
        out.starts_with(&format!("could not release {place}: ")) && out.contains(RELEASE_FAILS),
        "{out}"
    );
    assert!(out.contains("--accept-failure"), "{out}");
    assert_eq!(f.world.release_attempts().len(), tries + 1);
    assert!(f.world.live_places().contains(&place));

    f.world.release_fails(None);
    let (code, out) = sluice(&f, &["release", &place, "--json"]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(
        json(&out),
        serde_json::json!({"place": place, "released": true, "accepted": false, "failed": null})
    );
    assert!(!f.world.live_places().contains(&place));
}

/// 123: `--accept-failure` frees a held checkout with no release run.
#[test]
fn release_accepting_the_failure_frees_a_held_checkout() {
    let f = setup();
    let place = held_checkout(&f);
    let tries = f.world.release_attempts().len();
    let (code, out) = sluice(&f, &["release", &place, "--accept-failure"]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(
        out,
        format!(
            "freed {place} without its release command: its runs ended, its checkout removed, its ports free\n"
        )
    );
    assert_eq!(f.world.release_attempts().len(), tries, "its release ran");
    assert_eq!(f.world.freed_places(), std::slice::from_ref(&place));
    assert!(!f.world.live_places().contains(&place));
}

/// 123: a name that names no checkout here is nothing to release.
#[test]
fn release_of_an_unknown_checkout_is_nothing() {
    let f = setup();
    for name in [
        format!("sluice/try/{BRANCH}/7"),
        "sluice/try/nothing".to_string(),
    ] {
        let (code, out) = sluice(&f, &["release", &name]);
        assert_eq!((code, out.as_str()), (LANDED, "nothing to release\n"));
    }
}

/// 123: a worktree whose release fails exits 1 with what failed and keeps
/// its place; `--accept-failure` then frees the place with no release run
/// and leaves the worktree.
#[test]
fn release_accepting_the_failure_frees_a_worktrees_place() {
    let f = setup_releasing();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    f.world.release_fails(Some(RELEASE_FAILS));
    let (code, out) = release(&f, &["--worktree", "unit/a"]);
    assert_eq!(code, FAILED, "{out}");
    assert!(
        out.starts_with("could not release unit/a: ") && out.contains(RELEASE_FAILS),
        "{out}"
    );
    assert_eq!(f.world.adopted_places(), ["session/unit/a"]);

    let (code, out) = release(&f, &["--worktree", "unit/a", "--accept-failure", "--json"]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(
        json(&out),
        serde_json::json!({"worktree": "unit/a", "released": true, "accepted": true})
    );
    assert!(f.world.adopted_places().is_empty());
    assert_eq!(f.world.freed_places(), ["session/unit/a"]);
    assert!(f.world.session_releases().is_empty());
    assert!(sluice_seams::Driver::session(&f.driver, "unit/a").is_ok());
    let snap = sluice_seams::State::read(&f.state, BRANCH).unwrap();
    let log = sluice_core::line::Line::read_log(&snap).unwrap();
    assert!(
        log.iter().any(|l| matches!(
            &l.event,
            sluice_core::records::Event::Released { place, accepted: Some(a), .. }
                if place == "session/unit/a" && a.contains(RELEASE_FAILS)
        )),
        "{log:?}"
    );
}

/// The fakes' setup with `release_retries = 1`.
fn setup_one_retry() -> Fakes {
    let f = Fakes::new();
    f.world.init(
        BRANCH,
        &[
            (
                ".config/sluice.toml",
                &format!("{POLICY}release_retries = 1\n"),
            ),
            ("plugin/lib/plan.py", "plan"),
        ],
    );
    f.world.task("plugin:test", "plugin/");
    f
}

/// 123: a waiting submit says each failed try of a checkout's release
/// and the hold, once, on the round that held it; the waits after say
/// nothing of it. `sluice status <branch>` lists it held with what
/// failed and its tries, and how to release it; `--json` as
/// `held_checkouts`.
#[test]
fn a_waiting_submit_says_a_held_checkout_once() {
    let f = setup_one_retry();
    let first = format!("sluice/try/{BRANCH}/1");
    f.world.place_release_fails(&first, Some(RELEASE_FAILS));
    let mut said = Vec::new();
    for (unit, file) in [("unit/a", "p2"), ("unit/b", "p3"), ("unit/c", "p4")] {
        work(&f, unit, unit, &[("plugin/lib/plan.py", Some(file))]);
        let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", unit]);
        assert_eq!(code, LANDED, "{out}");
        said.push(out);
    }
    let tried = |out: &str| {
        out.lines()
            .filter(|l| l.starts_with(&format!("could not release {first} (try 1 of 2): ")))
            .count()
    };
    let held = |out: &str| {
        out.lines()
            .filter(|l| l.starts_with(&format!("{first} is held: its release failed 2 times: ")))
            .count()
    };
    assert_eq!(tried(&said[0]), 1, "{}", said[0]);
    assert!(said[0].contains(RELEASE_FAILS), "{}", said[0]);
    assert_eq!(held(&said[1]), 1, "{}", said[1]);
    assert!(
        said[1].contains(&format!(
            "`sluice release {first} --accept-failure` frees it without"
        )),
        "{}",
        said[1]
    );
    assert!(!said[2].contains(&first), "{}", said[2]);
    assert_eq!(
        f.world
            .release_attempts()
            .iter()
            .filter(|p| **p == first)
            .count(),
        2
    );

    let (code, out) = sluice(&f, &["status", BRANCH]);
    assert_eq!(code, 0);
    assert!(
        out.contains("held checkouts, whose release failed:\n"),
        "{out}"
    );
    assert!(
        out.contains(&format!("  {first}  2 tries, held, last at ")),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "    | the release command of {first} ended exit status: 2: {RELEASE_FAILS}"
        )),
        "{out}"
    );
    assert!(
        out.contains("sluice release <checkout> --accept-failure"),
        "{out}"
    );
    let (_, out) = sluice(&f, &["status", BRANCH, "--json"]);
    let doc = json(&out);
    let held = doc["held_checkouts"]
        .as_array()
        .expect("held checkouts are listed");
    assert_eq!(held.len(), 1, "{doc}");
    assert_eq!(held[0]["place"], first);
    assert_eq!(held[0]["tries"], 2);
    assert_eq!(held[0]["stopped"], true);
    assert!(held[0]["failed"].as_str().unwrap().contains(RELEASE_FAILS));
    assert!(held[0]["at"].is_string());
}

/// 97, 123: a waiting submit says once that a removed worktree was freed
/// and that its stored release ran, or what that release printed when it
/// failed; the waits after say nothing of it and run nothing of it.
#[test]
fn a_waiting_submit_says_a_removed_worktree_was_freed_once() {
    for fails in [false, true] {
        let f = setup_releasing();
        work(
            &f,
            "unit/a",
            "agent-a",
            &[("plugin/lib/plan.py", Some("p2"))],
        );
        exec(&f, &[BRANCH, "--worktree", "unit/a"]);
        f.world.remove_worktree("unit/a");
        if fails {
            f.world
                .place_release_fails("session/unit/a", Some(RELEASE_FAILS));
        }
        work(
            &f,
            "unit/b",
            "agent-b",
            &[("plugin/lib/plan.py", Some("p3"))],
        );
        let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/b"]);
        assert_eq!(code, LANDED, "{out}");
        let said = match fails {
            false => GONE.to_string(),
            true => format!(
                "freed unit/a: it no longer exists; its release failed once from its scratch directory: \
                 the release command of session/unit/a ended exit status: 2: {RELEASE_FAILS}; \
                 its runs ended, its ports free"
            ),
        };
        assert_eq!(out.lines().filter(|l| *l == said).count(), 1, "{out}");
        work(
            &f,
            "unit/c",
            "agent-c",
            &[("plugin/lib/plan.py", Some("p4"))],
        );
        let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/c"]);
        assert_eq!(code, LANDED, "{out}");
        assert!(!out.contains("unit/a"), "{out}");
        assert!(f.world.session_releases().is_empty());
        assert_eq!(f.world.gone_releases(), ["session/unit/a"]);
    }
}

/// 123: `sluice status` marks a worktree whose release failed, with what
/// failed, until it is released or its failure accepted.
#[test]
fn status_marks_a_worktree_whose_release_failed() {
    let f = setup_releasing();
    work(
        &f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    exec(&f, &[BRANCH, "--worktree", "unit/a"]);
    f.world.release_fails(Some(RELEASE_FAILS));
    release(&f, &["--worktree", "unit/a"]);
    let (_, out) = sluice(&f, &["status"]);
    assert!(
        out.contains("    its release failed (1 try): sluice release runs it again"),
        "{out}"
    );
    assert!(out.contains(RELEASE_FAILS), "{out}");
    let (_, out) = sluice(&f, &["status", "--json"]);
    let doc = json(&out);
    let failed = &doc["worktrees"][0]["release_failed"];
    assert_eq!(failed["tries"], 1, "{doc}");
    assert_eq!(failed["stopped"], true, "{doc}");
}

/// The lines of `text` from the one that starts with `head` on.
fn from_line<'a>(text: &'a str, head: &str) -> Vec<&'a str> {
    text.lines().skip_while(|l| !l.starts_with(head)).collect()
}

/// 123, 26: `sluice overview` says nothing of a checkout its next round
/// tries again, which `--json` lists with `stopped` false; once its tries
/// are spent it names it under its branch's line, with the last line of
/// what failed and the two commands, and `--json` lists it held.
#[test]
fn overview_names_a_held_checkout_under_its_branch() {
    let f = setup_one_retry();
    let first = format!("sluice/try/{BRANCH}/1");
    f.world.place_release_fails(&first, Some(RELEASE_FAILS));
    work(
        &f,
        "unit/a",
        "unit/a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");

    let (_, text) = sluice(&f, &["overview"]);
    assert!(!text.contains(&first), "{text}");
    let (_, out) = sluice(&f, &["status", BRANCH]);
    assert!(out.contains("tried again by the next round"), "{out}");
    let (_, out) = sluice(&f, &["overview", "--json"]);
    let held = &json(&out)["branches"][0]["held_checkouts"];
    assert_eq!(held[0]["place"], first, "{held}");
    assert_eq!(held[0]["stopped"], false, "{held}");
    assert_eq!(held[0]["tries"], 1, "{held}");

    work(
        &f,
        "unit/b",
        "unit/b",
        &[("plugin/lib/plan.py", Some("p3"))],
    );
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/b"]);
    assert_eq!(code, LANDED, "{out}");
    let (_, text) = sluice(&f, &["overview"]);
    let lines = from_line(&text, &format!("{BRANCH} at "));
    assert!(
        lines[1].starts_with(&format!("  {first} is held: its release failed 2 times: "))
            && lines[1].ends_with(RELEASE_FAILS),
        "{text}"
    );
    assert_eq!(
        lines[2],
        format!(
            "    sluice release {first} runs it again; \
             sluice release {first} --accept-failure frees it without"
        ),
        "{text}"
    );
    let (_, out) = sluice(&f, &["overview", "--json"]);
    let doc = json(&out);
    let held = doc["branches"][0]["held_checkouts"]
        .as_array()
        .expect("held checkouts are listed");
    assert_eq!(held.len(), 1, "{doc}");
    assert_eq!(held[0]["place"], first);
    assert_eq!(held[0]["tries"], 2);
    assert_eq!(held[0]["stopped"], true);
    assert!(held[0]["failed"].as_str().unwrap().contains(RELEASE_FAILS));
    assert!(held[0]["at"].is_string());
    assert!(held[0].get("host").is_none(), "{doc}");
}

/// A worktree `unit/a` of `BRANCH` whose `sluice release` failed once,
/// which holds it, and its path as the overview names it.
fn held_worktree(f: &Fakes) -> String {
    work(
        f,
        "unit/a",
        "agent-a",
        &[("plugin/lib/plan.py", Some("p2"))],
    );
    exec(f, &[BRANCH, "--worktree", "unit/a"]);
    f.world.release_fails(Some(RELEASE_FAILS));
    release(f, &["--worktree", "unit/a"]);
    f.world.release_fails(None);
    let (_, out) = sluice(f, &["overview", "--json"]);
    let doc = json(&out);
    assert_eq!(
        doc["sessions"][0]["release_failed"]["stopped"], true,
        "{doc}"
    );
    doc["sessions"][0]["worktree"].as_str().unwrap().to_string()
}

/// 123, 26: a held worktree is named under its branch's line, with what
/// failed and `sluice release --worktree <path>`.
#[test]
fn overview_names_a_held_worktree_under_its_branch() {
    let f = setup_releasing();
    work(
        &f,
        "unit/b",
        "agent-b",
        &[("plugin/lib/plan.py", Some("p3"))],
    );
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/b"]);
    assert_eq!(code, LANDED, "{out}");
    let path = held_worktree(&f);
    let (_, text) = sluice(&f, &["overview"]);
    let lines = from_line(&text, &format!("{BRANCH} at "));
    assert!(
        lines[1].starts_with(&format!(
            "  worktree {path} is held: its release failed once: "
        )) && lines[1].ends_with(RELEASE_FAILS),
        "{text}"
    );
    assert_eq!(
        lines[2],
        format!(
            "    sluice release --worktree {path} runs it again; \
             sluice release --worktree {path} --accept-failure frees it without"
        ),
        "{text}"
    );
    assert!(!text.contains("no records"), "{text}");
}

/// 123: a held worktree of a branch with no records is named under a
/// line of its own for that branch.
#[test]
fn overview_names_a_held_worktree_of_a_branch_with_no_records() {
    let f = setup_releasing();
    let path = held_worktree(&f);
    let (code, text) = sluice(&f, &["overview"]);
    assert_eq!(code, 0);
    let lines = from_line(&text, &format!("{BRANCH} · no records"));
    assert!(lines.len() > 2, "{text}");
    assert!(
        lines[1].starts_with(&format!("  worktree {path} is held: ")),
        "{text}"
    );
    assert!(
        lines[2].starts_with(&format!(
            "    sluice release --worktree {path} runs it again"
        )),
        "{text}"
    );
}

// ---------------------------------------------------------------------------
// A branch read without its last landing is held (121)
// ---------------------------------------------------------------------------

/// Land one submission on `BRANCH`, then have the branch read at the
/// revision it was made at: that revision, and the landing it lacks.
fn read_behind(f: &Fakes) -> (String, String) {
    let made = f.world.tip(BRANCH);
    work(f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    let (code, out) = sluice(f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    let landing = f.world.tip(BRANCH);
    f.world.set_branch(BRANCH, &made);
    (made.0, landing.0)
}

/// C.5 branch: `sluice branch accept` exits 0 when recorded, 1 for no
/// such branch and 2 on a refusal, naming it, as create and delete do.
#[test]
fn branch_accept_exits_as_create_and_delete() {
    let f = setup();
    let (made, landing) = read_behind(&f);
    f.world
        .set_branch(BRANCH, &sluice_seams::Rev::new(landing.clone()));
    assert_eq!(
        sluice(&f, &["branch", "accept", BRANCH, &landing]),
        (
            CONFLICT,
            format!(
                "refused: {BRANCH} is not held; it reads {landing}, which holds its last landing\n"
            )
        )
    );
    f.world
        .set_branch(BRANCH, &sluice_seams::Rev::new(made.clone()));
    sluice(&f, &["step", BRANCH]);
    assert_eq!(
        sluice(&f, &["branch", "accept", BRANCH, &landing]),
        (
            CONFLICT,
            format!("refused: {BRANCH} is held at {made}, not at {landing}\n")
        )
    );
    let (code, out) = sluice(&f, &["branch", "accept", BRANCH, &landing, "--json"]);
    assert_eq!(code, CONFLICT);
    assert_eq!(
        json(&out),
        serde_json::json!({"error": {
            "kind": "refused",
            "message": format!("{BRANCH} is held at {made}, not at {landing}"),
            "next": [format!("sluice status {BRANCH}")],
        }})
    );
    assert_eq!(
        sluice(&f, &["branch", "accept", "bolt/none", &made]),
        (FAILED, "no branch bolt/none\n".to_string())
    );

    let (code, out) = sluice(&f, &["branch", "accept", BRANCH, &made[..7], "--json"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        json(&out),
        serde_json::json!({ "branch": BRANCH, "accepted": made, "lacked": landing })
    );
    assert_eq!(
        sluice(&f, &["step", BRANCH]),
        (0, format!("{BRANCH}: stepped\n"))
    );
}

#[test]
fn branch_accept_says_what_it_recorded() {
    let f = setup();
    let (made, landing) = read_behind(&f);
    sluice(&f, &["step", BRANCH]);
    assert_eq!(
        sluice(&f, &["branch", "accept", BRANCH, &made]),
        (
            0,
            format!(
                "accepted {BRANCH} at {made}, which lacks the landing {landing}; \
                 the next round forms the queue on it\n"
            )
        )
    );
}

/// The line a held branch is said with.
fn held_text(read: &str, lacks: &str) -> String {
    format!(
        "held: {BRANCH} reads {}, which lacks the landing {}; put the landing back on \
         {BRANCH}, or run sluice branch accept {BRANCH} {}",
        &read[..7],
        &lacks[..7],
        &read[..7]
    )
}

/// 121: `sluice step` says the branch is held, with both revisions and
/// the two ways out; under `--json`, the outcome `held` and the hold.
#[test]
fn step_says_a_branch_is_held() {
    let f = setup();
    let (made, landing) = read_behind(&f);
    assert_eq!(
        sluice(&f, &["step", BRANCH]),
        (0, format!("{}\n", held_text(&made, &landing)))
    );
    let (code, out) = sluice(&f, &["step", BRANCH, "--json"]);
    assert_eq!(code, 0);
    let v = json(&out);
    assert_eq!(v["outcome"], "held");
    assert_eq!(
        (&v["held"]["read"], &v["held"]["lacks"]),
        (&serde_json::json!(made), &serde_json::json!(landing))
    );
    assert!(v["held"]["since"].is_string());
}

/// 121: `sluice run` says a hold once, not on every pass, and again for
/// another revision read lacking the landing.
#[test]
fn run_says_a_hold_once() {
    let f = setup();
    let (made, landing) = read_behind(&f);
    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/b", "--no-wait"]);
    // Read at another revision lacking it after the second pass.
    let beside = f.world.work(
        &sluice_seams::Rev::new(made.clone()),
        &[("plugin/c", Some("c"))],
    );
    let world = f.world.clone();
    let at = beside.clone();
    let pause = Then(Cell::new(0), move |n| {
        if n == 2 {
            world.set_branch(BRANCH, &at);
        }
        true
    });
    let (code, out) = sluice_with(&f, &pause, &["run", "--rounds", "5"]);
    assert_eq!(code, 0);
    let held: Vec<&str> = out.lines().filter(|l| l.starts_with("held: ")).collect();
    assert_eq!(
        held,
        [held_text(&made, &landing), held_text(&beside.0, &landing)],
        "{out}"
    );
}

/// 121: `sluice status <branch>` and `sluice overview` show the hold
/// above the queue, and under `held` in JSON, for as long as it lasts.
#[test]
fn status_and_overview_show_a_hold_while_it_lasts() {
    let f = setup();
    let (made, landing) = read_behind(&f);
    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/b", "--no-wait"]);
    sluice(&f, &["step", BRANCH]);
    let held = held_text(&made, &landing);

    let (code, out) = sluice(&f, &["status", BRANCH]);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines,
        [
            format!(
                "{BRANCH} at {} · 1 in the queue · testing 0 at once (up to 4)",
                &landing[..7]
            ),
            held.clone(),
            "  #2 unit/b  queued    agent-b  unit/b".to_string(),
            format!("1 no longer in the queue: sluice status {BRANCH} --all"),
            // D13 (51; record 0102): #1's landing, which nobody confirmed.
            "OWED".to_string(),
            "  unit/a · never confirmed".to_string(),
            "    1:landed #1  owed until 2026-10-18T09:00:10Z · handed over by notice at \
             2026-10-11T09:00:14Z, wait at 2026-10-11T09:00:17Z"
                .to_string(),
            "confirm one with: sluice confirm <key>, in its worktree, or sluice confirm <key> \
             --as operator"
                .to_string(),
        ],
        "{out}"
    );
    let v = json(&sluice(&f, &["status", BRANCH, "--json"]).1);
    assert_eq!(
        (&v["held"]["read"], &v["held"]["lacks"]),
        (&serde_json::json!(made), &serde_json::json!(landing))
    );

    let (_, out) = sluice(&f, &["overview"]);
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with(&format!("{BRANCH} at ")), "{out}");
    assert_eq!(lines[1], format!("  {held}"), "{out}");
    let v = json(&sluice(&f, &["overview", "--json"]).1);
    assert_eq!(v["branches"][0]["held"]["read"], serde_json::json!(made));
    assert_eq!(
        v["branches"][0]["held"]["lacks"],
        serde_json::json!(landing)
    );

    // The landing is put back: the next round ends the hold.
    f.world.set_branch(BRANCH, &sluice_seams::Rev::new(landing));
    sluice(&f, &["step", BRANCH]);
    let (_, out) = sluice(&f, &["status", BRANCH]);
    assert!(!out.contains("held: "), "{out}");
    let v = json(&sluice(&f, &["status", BRANCH, "--json"]).1);
    assert!(v["held"].is_null(), "{v}");
    let v = json(&sluice(&f, &["overview", "--json"]).1);
    assert!(v["branches"][0].get("held").is_none(), "{v}");
}

/// 121: a waiting `sluice wait` and `sluice submit` say a hold once in
/// their text; their `--json` record is the submission's, as before.
#[test]
fn a_waiting_submission_says_its_branchs_hold_once() {
    let f = setup();
    let (made, landing) = read_behind(&f);
    let held = held_text(&made, &landing);
    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/b", "--no-wait"]);

    let (code, out) = sluice(&f, &["wait", "2", "--timeout", "10s"]);
    assert_eq!(code, sluice::WAITING, "{out}");
    assert_eq!(out.lines().filter(|l| *l == held).count(), 1, "{out}");
    assert!(out.ends_with("still waiting: sluice wait 2\n"), "{out}");
    let (code, out) = sluice(&f, &["wait", "2", "--timeout", "10s", "--json"]);
    assert_eq!(code, sluice::WAITING, "{out}");
    let v = json(&out);
    assert_eq!(
        (v["id"].as_u64(), v["state"].as_str()),
        (Some(2), Some("queued"))
    );
    assert!(v.get("held").is_none(), "{v}");

    // A waiting submit, stopped after a few rounds.
    work(&f, "unit/c", "agent-c", &[("plugin/c", Some("c"))]);
    let stop = Then(Cell::new(0), |n| n < 4);
    let (code, out) = sluice_with(&f, &stop, &["submit", BRANCH, "--worktree", "unit/c"]);
    assert_eq!(code, sluice::STOPPED, "{out}");
    assert_eq!(out.lines().filter(|l| *l == held).count(), 1, "{out}");
    work(&f, "unit/d", "agent-d", &[("plugin/d", Some("d"))]);
    let stop = Then(Cell::new(0), |n| n < 4);
    let (code, out) = sluice_with(
        &f,
        &stop,
        &["submit", BRANCH, "--worktree", "unit/d", "--json"],
    );
    assert_eq!(code, sluice::STOPPED, "{out}");
    assert_eq!(json(&out)["state"], "queued");
}

/// What `test` says on stderr, which the harness keeps from the test
/// itself: it runs alone in a child of this binary, which must pass.
/// `None` in that child, which runs the test's body.
fn stderr_of(test: &str) -> Option<String> {
    if std::env::var("SLUICE_TEST_ALONE").as_deref() == Ok(test) {
        return None;
    }
    let out = support::isolated(&mut std::process::Command::new(
        std::env::current_exe().unwrap(),
    ))
    .args(["--exact", test, "--nocapture"])
    .env("SLUICE_TEST_ALONE", test)
    .output()
    .unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        out.status.success() && said.contains(&format!("test {test} ... ok")),
        "{test}, alone:\n{said}{err}"
    );
    Some(err)
}

/// The lines of `err` saying what failed after a landing (122).
fn after_the_landing(err: &str) -> Vec<&str> {
    err.lines()
        .filter(|l| l.starts_with("sluice: after the landing: "))
        .collect()
}

/// 122, 123: a release that fails after the landing, in the round a
/// waiting submit ran, leaves the submission landed: the round goes on,
/// so nothing fails after the landing; the failed try is said, then the
/// outcome as for any landing, exit 0. The next round releases the place.
#[test]
fn a_release_failing_after_the_landing_leaves_it_landed() {
    if let Some(err) = stderr_of("a_release_failing_after_the_landing_leaves_it_landed") {
        assert!(after_the_landing(&err).is_empty(), "{err}");
        return;
    }
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    f.world.fail_next_release();
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    let tip = f.world.tip(BRANCH);
    assert_eq!(
        outcome(&out).last().copied(),
        Some(format!("passed · landed as {}", &tip.0[..7]).as_str()),
        "{out}"
    );
    let place = format!("sluice/try/{BRANCH}/1");
    assert!(
        out.contains(&format!(
            "could not release {place} (try 1 of 4): the release command of {place} failed\n"
        )),
        "{out}"
    );
    assert_eq!(f.world.live_places().len(), 1);
    sluice(&f, &["step", BRANCH]);
    assert!(f.world.live_places().is_empty());
}

/// 122 (rule core.48; design D9): pushing the outcome fails after the
/// landing, in the host process's round `sluice wait` follows: landed,
/// exit 0, and nothing said of what failed after it, which is the host
/// process's.
#[test]
fn a_push_failing_after_the_landing_leaves_it_landed() {
    if let Some(err) = stderr_of("a_push_failing_after_the_landing_leaves_it_landed") {
        assert!(after_the_landing(&err).is_empty(), "{err}");
        return;
    }
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    f.world.crash_after_next_notify();
    let (code, out) = sluice(&f, &["wait", "1"]);
    assert_eq!(code, LANDED, "{out}");
    let tip = f.world.tip(BRANCH);
    assert!(
        out.ends_with(&format!("passed · landed as {}\n", &tip.0[..7])),
        "{out}"
    );
}

/// 122, 123: under `--json`, a landing whose release then fails is one
/// document, the submission's record, landed; the failed try is on
/// stderr, and nothing failed the round.
#[test]
fn a_landing_whose_release_fails_is_one_document_under_json() {
    if let Some(err) = stderr_of("a_landing_whose_release_fails_is_one_document_under_json") {
        assert!(after_the_landing(&err).is_empty(), "{err}");
        let place = format!("sluice/try/{BRANCH}/1");
        assert!(
            err.contains(&format!("could not release {place} (try 1 of 4): ")),
            "{err}"
        );
        return;
    }
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    f.world.fail_next_release();
    let (code, out) = sluice(&f, &["wait", "1", "--json"]);
    assert_eq!(code, LANDED, "{out}");
    let v = json(&out);
    assert_eq!(v["state"], "landed");
    assert_eq!(v["landed_as"], f.world.tip(BRANCH).0);
}

/// 122, 24 (design D9): the host process's round a waiting submit follows
/// moves the branch and fails writing that it landed: the submission is
/// reported landed as the landing's commit, and the next round records it
/// so.
#[test]
fn a_landing_the_round_did_not_record_is_reported_landed() {
    if let Some(err) = stderr_of("a_landing_the_round_did_not_record_is_reported_landed") {
        assert!(after_the_landing(&err).is_empty(), "{err}");
        return;
    }
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    // Between the submit's rounds, another process records the landing
    // and stops before the move; the submit's next round makes the move,
    // and its write confirming it fails.
    let between = |n: usize| {
        if n == 1 {
            f.world.crash(sluice_fakes::Crash::BeforeMove);
            assert!(f.sluice().step(BRANCH).is_err());
            f.world.crash_next_write();
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 5,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    let tip = f.world.tip(BRANCH);
    assert!(
        out.ends_with(&format!("passed · landed as {}\n", &tip.0[..7])),
        "{out}"
    );
    let (_, out) = sluice(&f, &["status", "1", "--json"]);
    assert_eq!(json(&out)["state"], "landing");
    sluice(&f, &["step", BRANCH]);
    let (_, out) = sluice(&f, &["status", "1", "--json"]);
    assert_eq!(
        (
            json(&out)["state"].as_str(),
            json(&out)["landed_as"].as_str()
        ),
        (Some("landed"), Some(tip.0.as_str()))
    );
}

/// 122 (design D9): a round of the host process's that fails before the
/// branch moved is said once on stderr, in its words, and the waiting
/// submit goes on: the submission is not reported landed by it, and the
/// round after, finishing the landing, lands it.
#[test]
fn a_round_failing_before_the_landing_is_said_and_the_wait_goes_on() {
    const TEST: &str = "a_round_failing_before_the_landing_is_said_and_the_wait_goes_on";
    if let Some(err) = stderr_of(TEST) {
        let said = err
            .lines()
            .filter(|l| {
                l.starts_with(&format!("sluice: {BRANCH}: ")) && l.contains("crashed before moving")
            })
            .count();
        assert_eq!(said, 1, "{err}");
        return;
    }
    let f = setup();
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    let between = |n: usize| {
        if n == 1 {
            f.world.crash(sluice_fakes::Crash::BeforeMove);
        }
    };
    let pause = Between {
        n: Cell::new(0),
        rounds: 5,
        between: &between,
    };
    let (code, out) = sluice_with(&f, &pause, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(out.matches("landed as").count(), 1, "{out}");
    let (_, out) = sluice(&f, &["status", "1", "--json"]);
    assert_eq!(json(&out)["state"], "landed");
}

// ---------------------------------------------------------------------------
// group-submit (125)
// ---------------------------------------------------------------------------

/// The setup's branch, testing only the deepest combination (14).
fn setup_batched() -> Fakes {
    let f = setup();
    let policy = POLICY.replace("parallel = 4\n", "parallel = 4\nbatch = true\n");
    let tip = f.world.tip(BRANCH);
    let at = f
        .world
        .work(&tip, &[(".config/sluice.toml", Some(&policy))]);
    f.world.set_branch(BRANCH, &at);
    f
}

/// `sluice submit <BRANCH> --worktree <w>...` for `worktrees`, then `rest`.
fn submit_group(f: &Fakes, worktrees: &[&str], rest: &[&str]) -> (i32, String) {
    let mut args = vec!["submit", BRANCH];
    for w in worktrees {
        args.extend(["--worktree", w]);
    }
    args.extend(rest);
    sluice(f, &args)
}

/// The combinations runs were started on, as lists of submissions.
fn tested_combinations(f: &Fakes) -> Vec<Vec<u64>> {
    let snapshot = sluice_seams::State::read(&f.state, BRANCH).unwrap();
    sluice_core::line::Line::read_log(&snapshot)
        .unwrap()
        .into_iter()
        .filter_map(|l| match l.event {
            sluice_core::records::Event::Testing { submissions, .. } => Some(submissions),
            _ => None,
        })
        .collect()
}

/// The submissions in the branch's records, as `status --json` lists them.
fn queued(f: &Fakes) -> Vec<serde_json::Value> {
    let (_, out) = sluice(f, &["status", BRANCH, "--json", "--all"]);
    json(&out)["submissions"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// Three worktrees: three submissions numbered in the order given, each
/// named with its worktree, and nothing run; one worktree answers as a
/// submit always has; under `--json`, one document of them all.
#[test]
fn a_group_is_numbered_in_the_order_given() {
    let f = setup();
    work(&f, "unit/b1", "agent-1", &[("plugin/one", Some("1"))]);
    work(&f, "unit/b2", "agent-2", &[("plugin/two", Some("2"))]);
    work(&f, "unit/b3", "agent-3", &[("tests/three", Some("3"))]);
    let (code, out) = submit_group(&f, &["unit/b1", "unit/b2", "unit/b3"], &["--no-wait"]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(
        out,
        format!(
            "submission 1 to {BRANCH} from unit/b1 · nothing queued ahead\n\
             submission 2 to {BRANCH} from unit/b2 · 1 queued ahead (#1)\n\
             submission 3 to {BRANCH} from unit/b3 · 2 queued ahead (#1, #2)\n"
        )
    );
    assert!(f.world.started().is_empty(), "submitting runs nothing");
    for (s, after) in queued(&f).iter().zip([[0u64; 0]; 3]) {
        assert_eq!(s["after"], serde_json::json!(after), "{s}");
    }

    work(&f, "unit/b4", "agent-4", &[("plugin/four", Some("4"))]);
    let (code, out) = submit_group(&f, &["unit/b4"], &["--no-wait"]);
    assert_eq!(code, LANDED);
    assert_eq!(
        out,
        format!("submission 4 to {BRANCH} · 3 queued ahead (#1, #2, #3)\n")
    );

    work(&f, "unit/b5", "agent-5", &[("plugin/five", Some("5"))]);
    work(&f, "unit/b6", "agent-6", &[("plugin/six", Some("6"))]);
    let (code, out) = submit_group(&f, &["unit/b5", "unit/b6"], &["--no-wait", "--json"]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(
        json(&out),
        serde_json::json!({
            "branch": BRANCH,
            "submissions": [
                { "id": 5, "worktree": "unit/b5", "ahead": [1, 2, 3, 4] },
                { "id": 6, "worktree": "unit/b6", "ahead": [1, 2, 3, 4, 5] },
            ],
        })
    );
}

/// All or none: one worktree holding more than its commits, or one named
/// twice, refuses the act, naming it, and queues nothing for any; under
/// `--json`, the refused document naming each refused worktree.
#[test]
fn a_group_is_queued_whole_or_not_at_all() {
    let f = setup();
    work(&f, "unit/b1", "agent-1", &[("plugin/one", Some("1"))]);
    work(&f, "unit/b2", "agent-2", &[("plugin/two", Some("2"))]);
    f.world.dirty("unit/b2", &["plugin/scratch.py"]);
    let (code, out) = submit_group(&f, &["unit/b1", "unit/b2"], &[]);
    assert_eq!(code, CONFLICT, "{out}");
    assert_eq!(
        out,
        "refused: unit/b2 holds changes beyond its commits:\n  plugin/scratch.py\n\
         the agent working in unit/b2 commits or removes them and submits again\n\
         nothing submitted: the 2 worktrees are submitted together or not at all\n"
    );
    assert!(queued(&f).is_empty());

    let (code, out) = submit_group(&f, &["unit/b1", "unit/b2"], &["--json"]);
    assert_eq!(code, CONFLICT, "{out}");
    let v = json(&out);
    assert_eq!(v["error"]["kind"], "refused");
    assert_eq!(v["error"]["worktrees"], serde_json::json!(["unit/b2"]));
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .starts_with("unit/b2 holds changes beyond its commits"));
    assert_eq!(
        v["error"]["next"],
        serde_json::json!([format!(
            "sluice submit {BRANCH} --worktree unit/b1 --worktree unit/b2"
        )])
    );

    let (code, out) = submit_group(&f, &["unit/b1", "unit/b1"], &[]);
    assert_eq!(code, CONFLICT, "{out}");
    assert!(
        out.starts_with("refused: unit/b1 is named twice\n"),
        "{out}"
    );
    assert!(queued(&f).is_empty());
}

/// Several worktrees take neither `--check` nor `--status`: a command line
/// sluice cannot read, which writes nothing.
#[test]
fn several_worktrees_take_neither_check_nor_status() {
    let f = setup();
    work(&f, "unit/b1", "agent-1", &[("plugin/one", Some("1"))]);
    work(&f, "unit/b2", "agent-2", &[("plugin/two", Some("2"))]);
    for flag in ["--check", "--status"] {
        let (code, out) = submit_group(&f, &["unit/b1", "unit/b2"], &[flag]);
        assert_eq!(code, sluice::USAGE, "{out}");
        assert!(out.contains(&format!("{flag} takes one worktree")), "{out}");
    }
    assert!(queued(&f).is_empty());
}

/// 124, D1: a submit by name reads no worktree, so `--head` with
/// `--worktree`, with `--status` or with two subjects is a command line
/// sluice cannot read, saying why, which submits nothing; `--check
/// --head` still forecasts, as before.
#[test]
fn head_takes_no_worktree_and_no_status() {
    let f = setup();
    f.world.set_branch("bolt/w", &f.world.tip(BRANCH));
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    let head = ["submit", BRANCH, "--head", "bolt/w", "--no-wait"];
    for (args, said) in [
        (
            vec!["--worktree", "unit/a"],
            "--head names what is submitted, from anywhere in the repository; give no \
             --worktree with it",
        ),
        (
            vec!["--status"],
            "--status reads a worktree's last submission; a branch's are in sluice status \
             <target> --all",
        ),
        (
            vec!["--subject", "a", "--subject", "b"],
            "--head submits one branch; give one --subject with it, or none",
        ),
    ] {
        let args = [&head[..], &args].concat();
        let (code, out) = sluice(&f, &args);
        assert_eq!(code, sluice::USAGE, "{args:?}: {out}");
        assert_eq!(out, format!("sluice submit: {said}\n"), "{args:?}");
    }
    assert!(queued(&f).is_empty());
    let (code, out) = sluice(
        &f,
        &[
            "submit",
            BRANCH,
            "--check",
            "--head",
            "bolt/w",
            "--worktree",
            "unit/a",
        ],
    );
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(
        out,
        format!("no conflict with {BRANCH} or anything queued\n")
    );
    assert!(queued(&f).is_empty());
}

/// 124, D2: only a branch sluice lands on is submitted by name: a branch
/// that does not exist, the target itself and a branch no table names
/// are each refused, exit 2, naming why, with nothing written; under
/// `--json`, as the error document.
#[test]
fn head_takes_only_a_branch_sluice_lands_on() {
    let f = setup();
    f.world.set_branch("unit/a", &f.world.tip(BRANCH));
    for (head, said) in [
        (
            "bolt/gone",
            "refused: no branch bolt/gone to submit".to_string(),
        ),
        (
            BRANCH,
            format!("refused: {BRANCH} is not submitted to itself"),
        ),
        (
            "unit/a",
            "refused: unit/a is no branch sluice lands on: a branch of a worktree's own is \
             submitted from that worktree"
                .to_string(),
        ),
    ] {
        let (code, out) = sluice(&f, &["submit", BRANCH, "--head", head, "--no-wait"]);
        assert_eq!(code, CONFLICT, "{head}: {out}");
        assert_eq!(out.lines().next(), Some(said.as_str()), "{head}: {out}");
        let (code, out) = sluice(
            &f,
            &["submit", BRANCH, "--head", head, "--no-wait", "--json"],
        );
        assert_eq!(code, CONFLICT, "{head}: {out}");
        let doc = json(&out);
        let message = said.strip_prefix("refused: ").unwrap();
        assert_eq!(doc["error"]["message"], message, "{doc}");
    }
    assert!(queued(&f).is_empty());
}

/// 124, D9: a branch submitted by name is said by its name, `submission
/// <n> to <target> from <branch> · <ahead>`, and under `--no-wait
/// --json` answered as any submit that does not wait; its head is the
/// branch's revision and its record names no worktree and no session.
#[test]
fn head_takes_a_branch_by_name_and_says_it() {
    let f = setup();
    let w = f
        .world
        .work(&f.world.tip(BRANCH), &[("plugin/w", Some("w"))]);
    f.world.set_branch("bolt/w", &w);
    let (code, out) = sluice(&f, &["submit", BRANCH, "--head", "bolt/w", "--no-wait"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        out,
        format!("submission 1 to {BRANCH} from bolt/w · nothing queued ahead\n")
    );
    let v = f.world.work(&w, &[("plugin/v", Some("v"))]);
    f.world.set_branch("bolt/v", &v);
    let (code, out) = sluice(
        &f,
        &["submit", BRANCH, "--head", "bolt/v", "--no-wait", "--json"],
    );
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        json(&out),
        serde_json::json!({ "id": 2, "branch": BRANCH, "ahead": [1] })
    );
    let subs = queued(&f);
    assert_eq!(subs[0]["head"], w.0.as_str());
    assert_eq!(subs[1]["head"], v.0.as_str());
    assert_eq!(subs[1]["from"]["worktree"], "bolt/v");
    assert_eq!(subs[1]["from"]["head"], "bolt/v");
    assert!(subs[1]["from"].get("path").is_none(), "{}", subs[1]);
    assert!(subs[1]["from"].get("session").is_none(), "{}", subs[1]);
}

/// S59: on a branch that batches, a waiting act of three is tested once as
/// one combination of all three and lands in one move, in the order given;
/// it reports each one's outcome in that order and exits 0. Under
/// `--json`, one document of their records.
#[test]
fn a_waiting_group_lands_together_and_reports_each_in_order() {
    let f = setup_batched();
    work(&f, "unit/b1", "agent-1", &[("plugin/one", Some("1"))]);
    work(&f, "unit/b2", "agent-2", &[("plugin/two", Some("2"))]);
    work(&f, "unit/b3", "agent-3", &[("tests/three", Some("3"))]);
    let (code, out) = submit_group(&f, &["unit/b1", "unit/b2", "unit/b3"], &[]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(tested_combinations(&f), vec![vec![1, 2, 3]]);
    assert_eq!(f.world.moves().len(), 1, "one move");
    let lines = outcome(&out);
    let at = |header: &str| {
        lines
            .iter()
            .position(|l| *l == header)
            .unwrap_or_else(|| panic!("{header:?} in {out}"))
    };
    let heads = [
        at("submission 1 from unit/b1:"),
        at("submission 2 from unit/b2:"),
        at("submission 3 from unit/b3:"),
    ];
    assert!(heads.windows(2).all(|w| w[0] < w[1]), "{out}");
    let history = f.world.first_parents(BRANCH);
    for (k, h) in heads.iter().enumerate() {
        let end = heads.get(k + 1).copied().unwrap_or(lines.len());
        let landed = format!("passed · landed as {}", &history[2 - k].0[..7]);
        assert!(lines[*h..end].contains(&landed.as_str()), "{landed}: {out}");
    }

    work(&f, "unit/b4", "agent-4", &[("plugin/four", Some("4"))]);
    work(&f, "unit/b5", "agent-5", &[("plugin/five", Some("5"))]);
    let (code, out) = submit_group(&f, &["unit/b4", "unit/b5"], &["--json"]);
    assert_eq!(code, LANDED, "{out}");
    let v = json(&out);
    assert_eq!(v["branch"], BRANCH);
    let subs = v["submissions"].as_array().unwrap();
    assert_eq!(subs.len(), 2);
    for (s, (id, w)) in subs.iter().zip([(4, "unit/b4"), (5, "unit/b5")]) {
        assert_eq!(
            (s["id"].as_u64(), s["worktree"].as_str()),
            (Some(id), Some(w))
        );
        assert_eq!(s["state"], "landed");
    }
}

/// S59: the middle one conflicting with the first is set aside alone,
/// reported with its file and the submission it is contested with; the
/// others land, and the act exits 2, the middle one's exit. The middle
/// one blamed alone exits 1.
#[test]
fn a_waiting_group_exits_with_the_first_that_did_not_land() {
    let f = setup();
    work(
        &f,
        "unit/b1",
        "agent-1",
        &[("plugin/lib/plan.py", Some("one"))],
    );
    work(
        &f,
        "unit/b2",
        "agent-2",
        &[("plugin/lib/plan.py", Some("two"))],
    );
    work(&f, "unit/b3", "agent-3", &[("tests/three", Some("3"))]);
    let (code, out) = submit_group(&f, &["unit/b1", "unit/b2", "unit/b3"], &[]);
    assert_eq!(code, CONFLICT, "{out}");
    let lines = outcome(&out);
    let two = lines
        .iter()
        .position(|l| *l == "submission 2 from unit/b2:")
        .unwrap_or_else(|| panic!("{out}"));
    assert_eq!(
        lines[two..two + 5],
        [
            "submission 2 from unit/b2:",
            "conflict: plugin/lib/plan.py",
            "  conflicts with #1 (agent-1, unit/b1)",
            &format!(
                "the agent working in unit/b2 merges or rebases onto {BRANCH} there and submits \
                 again"
            ),
            "submission 3 from unit/b3:",
        ],
        "{out}"
    );
    assert!(
        lines[..two]
            .iter()
            .any(|l| l.starts_with("passed · landed as ")),
        "{out}"
    );
    assert!(
        lines[two..]
            .iter()
            .any(|l| l.starts_with("passed · landed as ")),
        "{out}"
    );
    assert!(tested_combinations(&f).iter().all(|c| !c.contains(&2)));

    f.world
        .fails_with("tests:t-batch-cards", "tests/t_cards.py", "broken");
    work(&f, "unit/b4", "agent-4", &[("plugin/four", Some("4"))]);
    work(
        &f,
        "unit/b5",
        "agent-5",
        &[("tests/t_cards.py", Some("broken"))],
    );
    work(&f, "unit/b6", "agent-6", &[("plugin/six", Some("6"))]);
    let (code, out) = submit_group(&f, &["unit/b4", "unit/b5", "unit/b6"], &["--json"]);
    assert_eq!(code, FAILED, "{out}");
    let states: Vec<String> = json(&out)["submissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["state"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(states, ["landed", "failed", "landed"]);
}

/// Each is its own submission once queued: one withdrawn leaves alone,
/// the other lands without it, and `sluice wait` on either reports that
/// one alone.
#[test]
fn each_of_a_group_is_withdrawn_and_waited_on_alone() {
    let f = setup();
    work(&f, "unit/b1", "agent-1", &[("plugin/one", Some("1"))]);
    work(&f, "unit/b2", "agent-2", &[("plugin/two", Some("2"))]);
    submit_group(&f, &["unit/b1", "unit/b2"], &["--no-wait"]);
    assert_eq!(sluice(&f, &["withdraw", "1", "--as", "agent-1"]).0, 0);
    let (code, out) = sluice(&f, &["wait", "2"]);
    assert_eq!(code, LANDED, "{out}");
    assert!(!out.contains("#1"), "{out}");
    assert!(tested_combinations(&f).iter().all(|c| !c.contains(&1)));
    let (code, out) = sluice(&f, &["wait", "1", "--json"]);
    assert_eq!(code, sluice::WITHDRAWN, "{out}");
    assert_eq!(json(&out)["id"], 1);
}

/// 6: a group's `--after` is every one's, so both wait for it and come in
/// the order given; a later submission of higher priority may come ahead
/// of a group, whose submissions keep their order.
#[test]
fn a_group_keeps_its_order_behind_after_and_priority() {
    let f = setup();
    f.world.hold_runs(true);
    work(&f, "unit/x", "agent-x", &[("plugin/x", Some("x"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/x", "--no-wait"]);
    work(&f, "unit/b1", "agent-1", &[("plugin/one", Some("1"))]);
    work(&f, "unit/b2", "agent-2", &[("plugin/two", Some("2"))]);
    submit_group(&f, &["unit/b1", "unit/b2"], &["--no-wait", "--after", "1"]);
    for s in &queued(&f)[1..] {
        assert_eq!(s["after"], serde_json::json!([1]), "{s}");
    }
    work(&f, "unit/c", "agent-c", &[("tests/c", Some("c"))]);
    sluice(
        &f,
        &[
            "submit",
            BRANCH,
            "--worktree",
            "unit/c",
            "--no-wait",
            "--priority",
            "5",
        ],
    );
    sluice(&f, &["step", BRANCH]);
    assert_eq!(
        tested_combinations(&f),
        vec![vec![4], vec![4, 1], vec![4, 1, 2], vec![4, 1, 2, 3]]
    );
}

/// 125: rounds run beside the act, between its numbering and its write,
/// form nothing of it; the first combination formed holds all three, in
/// the order given.
#[test]
fn a_round_beside_a_group_forms_none_of_it_alone() {
    let f = setup_batched();
    work(&f, "unit/b1", "agent-1", &[("plugin/one", Some("1"))]);
    work(&f, "unit/b2", "agent-2", &[("plugin/two", Some("2"))]);
    work(&f, "unit/b3", "agent-3", &[("tests/three", Some("3"))]);
    f.world.after_allocating(|world, _| {
        let beside = Fakes::of(world.clone());
        assert_eq!(sluice(&beside, &["step", BRANCH]).0, 0);
    });
    let (code, out) = submit_group(&f, &["unit/b1", "unit/b2", "unit/b3"], &["--no-wait"]);
    assert_eq!(code, LANDED, "{out}");
    assert!(tested_combinations(&f).is_empty());
    sluice(&f, &["step", BRANCH]);
    assert_eq!(tested_combinations(&f), vec![vec![1, 2, 3]]);
}

/// Stopped with Ctrl-C, a waiting act exits 130 with its document as it
/// stands: every submission still queued.
#[test]
fn a_waiting_group_stopped_says_where_each_stands() {
    struct Stop;
    impl Pause for Stop {
        fn pause(&self) -> bool {
            false
        }
    }
    let f = setup();
    f.world.hold_runs(true);
    work(&f, "unit/b1", "agent-1", &[("plugin/one", Some("1"))]);
    work(&f, "unit/b2", "agent-2", &[("plugin/two", Some("2"))]);
    let args = [
        "submit",
        BRANCH,
        "--worktree",
        "unit/b1",
        "--worktree",
        "unit/b2",
        "--json",
    ];
    let (code, out) = sluice_with(&f, &Stop, &args);
    assert_eq!(code, sluice::STOPPED, "{out}");
    let v = json(&out);
    let ids: Vec<u64> = v["submissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, [1, 2]);
    assert!(v["submissions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["state"] != "landed"));
    let (code, out) = sluice_with(
        &f,
        &Stop,
        &[
            "submit",
            BRANCH,
            "--worktree",
            "unit/b1",
            "--worktree",
            "unit/b2",
        ],
    );
    assert_eq!(code, sluice::STOPPED, "{out}");
    assert!(out.contains("still waiting: sluice wait 3"), "{out}");
}

/// 124, S64: `sluice status <branch>` shows this host's copy left behind
/// its landing above the queue, and under `copy` in JSON, until a round
/// levels it; then nothing of it.
#[test]
fn status_shows_this_hosts_copy_behind_while_it_lasts() {
    let f = setup();
    f.world.on_remote(BRANCH);
    let made = f.world.tip(BRANCH);
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    let landing = f.world.tip(BRANCH);
    let lock = format!("git holds the lock on refs/heads/{BRANCH}");
    f.world.set_copy(BRANCH, Some(&made));
    f.world.fail_levels(1, &lock);
    sluice(&f, &["step", BRANCH]);

    let (code, out) = sluice(&f, &["status", BRANCH]);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[..2],
        [
            format!(
                "{BRANCH} at {} · 0 in the queue · testing 0 at once (up to 4)",
                &landing.0[..7]
            ),
            format!(
                "this host's {BRANCH} is at {}, behind the landing {}: {lock}",
                &made.0[..7],
                &landing.0[..7]
            ),
        ],
        "{out}"
    );
    let v = json(&sluice(&f, &["status", BRANCH, "--json"]).1);
    assert_eq!(
        (&v["copy"]["at"], &v["copy"]["lacks"], &v["copy"]["why"]),
        (
            &serde_json::json!(made),
            &serde_json::json!(landing),
            &serde_json::json!(lock)
        ),
        "{v}"
    );
    assert!(v["copy"]["since"].is_string(), "{v}");

    sluice(&f, &["step", BRANCH]);
    assert_eq!(f.world.copy_of(BRANCH), Some(landing));
    let (_, out) = sluice(&f, &["status", BRANCH]);
    assert!(!out.contains("this host's"), "{out}");
    let v = json(&sluice(&f, &["status", BRANCH, "--json"]).1);
    assert!(v["copy"].is_null(), "{v}");
}

/// A revision whose first seven digits are `short`.
fn rev_of(short: &str) -> Rev {
    Rev::new(format!("{short:0<40}"))
}

/// The timer's cause on `build-1`, as 148's example names it.
const TIMER: &str = "git -C /home/me/app update-ref refs/heads/main <revision> <revision>";

/// A cause of refused writes as the branch's state keeps it (148).
fn cause(
    host: &str,
    command: &str,
    times: u64,
    first: &str,
    last: &str,
    to: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "host": host,
        "ref": format!("refs/heads/{BRANCH}"),
        "command": command,
        "times": times,
        "first": first,
        "last": last,
        "from": rev_of("10e4791"),
        "to": to.map(rev_of),
    })
}

/// The branch's state given `causes` of refused writes, as the hook's
/// writes would have left them (148), its document made by a round first.
fn refused_writes(f: &Fakes, causes: &[serde_json::Value]) {
    sluice(f, &["step", BRANCH]);
    let state = f.seams().state;
    let _lock = state.wait_lock(BRANCH).unwrap();
    let read = state.read(BRANCH).unwrap();
    let mut doc: serde_json::Value = serde_json::from_slice(&read.files["branch.json"]).unwrap();
    doc["refused_writes"] = causes.into();
    let mut changes = Changes::default();
    changes.put.insert(
        "branch.json".to_string(),
        serde_json::to_vec_pretty(&doc).unwrap(),
    );
    assert!(matches!(
        state.write(BRANCH, read.version, changes).unwrap(),
        sluice_seams::Written::Written(_)
    ));
}

/// The world's clock set to `at`; it moves a second on each ask after.
fn clock_at(f: &Fakes, at: &str) {
    use sluice_core::Clock;
    let at = chrono::DateTime::parse_from_rfc3339(at).unwrap();
    let now = f.clock.now();
    f.world.advance(at.with_timezone(&chrono::Utc) - now);
}

/// 148: status says, above the queue, which writes sluice's protection
/// refused in the last day, by command, with its host, how many times, the
/// first, the last and the last write's revisions, and then, on a branch
/// kept on a remote, that sluice sets each host's copy itself; `--json`
/// carries each cause whole under `refused_writes`.
#[test]
fn status_shows_the_writes_its_protection_refused() {
    let f = setup();
    f.world.on_remote(BRANCH);
    let timer = cause(
        "build-1",
        TIMER,
        2024,
        "2026-10-30T05:50:23Z",
        "2026-11-02T16:01:20Z",
        Some("8079db3"),
    );
    refused_writes(&f, std::slice::from_ref(&timer));
    clock_at(&f, "2026-11-02T17:00:00Z");

    let (code, out) = sluice(&f, &["status", BRANCH]);
    assert_eq!(code, 0, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[1..5],
        [
            format!(
                "sluice refused writes to {BRANCH} in the last day, by commands that are not sluice's:"
            ),
            format!("  {TIMER}"),
            "    on build-1: 2024 times, first 2026-10-30T05:50:23Z, last 2026-11-02T16:01:20Z, \
             from 10e4791 to 8079db3"
                .to_string(),
            format!(
                "sluice sets each host's {BRANCH} to each landing itself: stop what runs these"
            ),
        ],
        "{out}"
    );
    let v = json(&sluice(&f, &["status", BRANCH, "--json"]).1);
    assert_eq!(v["refused_writes"], serde_json::json!([timer]), "{v}");
}

/// 148: with no cause standing, status says nothing of refused writes, and
/// `--json` carries `refused_writes` empty.
#[test]
fn status_says_nothing_of_no_refused_writes() {
    let f = setup();
    sluice(&f, &["step", BRANCH]);

    let (code, out) = sluice(&f, &["status", BRANCH]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("refused"), "{out}");
    let v = json(&sluice(&f, &["status", BRANCH, "--json"]).1);
    assert_eq!(v["refused_writes"], serde_json::json!([]), "{v}");
}

/// 124, 148: on a branch kept in this host's own repository, status's
/// last line of its refused writes is the refusal's own: only sluice
/// moves it.
#[test]
fn status_tells_a_local_branchs_refused_writes_what_to_do() {
    let f = setup();
    let commit = format!("git commit -m 'fix the build' on {BRANCH}");
    refused_writes(
        &f,
        &[cause(
            "build-1",
            &commit,
            1,
            "2026-11-02T16:01:20Z",
            "2026-11-02T16:01:20Z",
            Some("8079db3"),
        )],
    );
    clock_at(&f, "2026-11-02T17:00:00Z");

    let (_, out) = sluice(&f, &["status", BRANCH]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[2..5],
        [
            format!("  {commit}"),
            "    on build-1: 1 time, first 2026-11-02T16:01:20Z, last 2026-11-02T16:01:20Z, \
             from 10e4791 to 8079db3"
                .to_string(),
            format!("only sluice moves {BRANCH}: submit the change to sluice instead"),
        ],
        "{out}"
    );
    assert!(!out.contains("each landing itself"), "{out}");
}

/// 148: a refused deletion is named as one: status says `deleting it`
/// where a move names its revision, and `--json` gives its `to` as null.
#[test]
fn status_names_a_refused_deletion() {
    let f = setup();
    let delete = format!("git branch -D {BRANCH}");
    refused_writes(
        &f,
        &[cause(
            "build-1",
            &delete,
            1,
            "2026-11-02T16:01:20Z",
            "2026-11-02T16:01:20Z",
            None,
        )],
    );
    clock_at(&f, "2026-11-02T17:00:00Z");

    let (_, out) = sluice(&f, &["status", BRANCH]);
    assert!(
        out.contains(&format!(
            "  {delete}\n    on build-1: 1 time, first 2026-11-02T16:01:20Z, \
             last 2026-11-02T16:01:20Z, from 10e4791, deleting it\n"
        )),
        "{out}"
    );
    let v = json(&sluice(&f, &["status", BRANCH, "--json"]).1);
    assert!(v["refused_writes"][0]["to"].is_null(), "{v}");
    assert_eq!(v["refused_writes"][0]["command"], delete.as_str(), "{v}");
}

/// Five causes standing on the branch, refused last in the order of
/// their numbers, set down out of that order; the third's command is a
/// line longer than a hundred characters, and a second line.
fn five_causes(f: &Fakes) -> Vec<serde_json::Value> {
    let long = format!(
        "git -C /home/me/{} update-ref refs/heads/main <revision>\nand more",
        "a".repeat(80)
    );
    let causes: Vec<serde_json::Value> = [3, 1, 5, 2, 4]
        .into_iter()
        .map(|n| {
            let command = match n {
                3 => long.clone(),
                n => format!("git branch -f {BRANCH} unit/{n}"),
            };
            cause(
                &format!("build-{n}"),
                &command,
                n,
                "2026-11-02T10:00:00Z",
                &format!("2026-11-02T1{n}:00:00Z"),
                Some("8079db3"),
            )
        })
        .collect();
    refused_writes(f, &causes);
    clock_at(f, "2026-11-02T17:00:00Z");
    causes
}

/// 26, 148: the overview shows, under the branch's line, the three causes
/// refused last, newest first, each command cut as status cuts it after a
/// hundred characters, and then how many more and where they are.
#[test]
fn the_overview_shows_three_and_how_many_more() {
    let f = setup();
    five_causes(&f);

    let (code, out) = sluice(&f, &["overview"]);
    assert_eq!(code, 0, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    let cut: String = format!(
        "git -C /home/me/{} update-ref refs/heads/main <revision>",
        "a".repeat(80)
    )
    .chars()
    .take(100)
    .collect();
    assert_eq!(
        lines[1..5],
        [
            format!(
                "  sluice refused 5 writes on build-5, last 2026-11-02T15:00:00Z: \
                 git branch -f {BRANCH} unit/5"
            ),
            format!(
                "  sluice refused 4 writes on build-4, last 2026-11-02T14:00:00Z: \
                 git branch -f {BRANCH} unit/4"
            ),
            format!("  sluice refused 3 writes on build-3, last 2026-11-02T13:00:00Z: {cut}…"),
            format!("  and 2 more: sluice status {BRANCH}"),
        ],
        "{out}"
    );
}

/// 26, 129, 148: `sluice overview --json` gives the branch every standing
/// cause, newest first, each whole as the branch's state keeps it.
#[test]
fn the_overviews_document_carries_every_cause() {
    let f = setup();
    let causes = five_causes(&f);

    let v = json(&sluice(&f, &["overview", "--json"]).1);
    let newest_first: Vec<&serde_json::Value> =
        [2, 4, 0, 3, 1].iter().map(|&i| &causes[i]).collect();
    assert_eq!(
        v["branches"][0]["refused_writes"],
        serde_json::json!(newest_first),
        "{v}"
    );
}

/// 148: a cause is shown until a day passes with none of its writes
/// refused: neither status nor the overview, in words or in `--json`,
/// gives it a day after its last.
#[test]
fn neither_status_nor_the_overview_shows_a_cause_a_day_after_its_last() {
    let f = setup();
    refused_writes(
        &f,
        &[cause(
            "build-1",
            TIMER,
            2,
            "2026-11-02T15:00:00Z",
            "2026-11-02T16:01:20Z",
            Some("8079db3"),
        )],
    );
    clock_at(&f, "2026-11-03T16:01:10Z");
    let (_, out) = sluice(&f, &["status", BRANCH]);
    assert!(out.contains(TIMER), "{out}");
    let (_, out) = sluice(&f, &["overview"]);
    assert!(out.contains(TIMER), "{out}");

    clock_at(&f, "2026-11-03T16:01:20Z");
    let (_, out) = sluice(&f, &["status", BRANCH]);
    assert!(!out.contains("refused"), "{out}");
    let v = json(&sluice(&f, &["status", BRANCH, "--json"]).1);
    assert_eq!(v["refused_writes"], serde_json::json!([]), "{v}");
    let (_, out) = sluice(&f, &["overview"]);
    assert!(!out.contains("refused"), "{out}");
    let v = json(&sluice(&f, &["overview", "--json"]).1);
    assert_eq!(
        v["branches"][0]["refused_writes"],
        serde_json::json!([]),
        "{v}"
    );
}

/// 124, S58: a submit from a worktree that has checked out a bolt kept on
/// a remote submits the remote's revision and says how this host's copy
/// stands to it, in text and under `copy` in JSON; level, it says
/// nothing of it.
#[test]
fn submit_from_a_bolt_says_how_its_copy_stands() {
    let f = Fakes::new();
    let on_origin = POLICY.replace("parallel = 4\n", "parallel = 4\nremote = \"origin\"\n");
    f.world.init(
        BRANCH,
        &[
            (".config/sluice.toml", on_origin.as_str()),
            ("plugin/lib/plan.py", "plan"),
        ],
    );
    f.world.task("plugin:test", "plugin/");
    f.world.on_remote(BRANCH);
    f.world.init(
        "main",
        &[(
            ".config/sluice.toml",
            "[branches.main]\nrequired = [\"#gate\"]\nremote = \"origin\"\n",
        )],
    );
    let made = f.world.tip(BRANCH);
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    let landing = f.world.tip(BRANCH);
    f.world.set_copy(BRANCH, Some(&made));
    f.world.session(BRANCH, "ops", &made);

    let (code, out) = sluice(&f, &["submit", "main", "--worktree", BRANCH, "--no-wait"]);
    assert_eq!(code, 0, "{out}");
    let (m, l) = (&made.0[..7], &landing.0[..7]);
    assert_eq!(
        out,
        format!(
            "submission 2 to main · nothing queued ahead\n\
             this host's {BRANCH} is at {m}, behind origin's {l}; submitted {l}\n"
        )
    );
    let (_, out) = sluice(
        &f,
        &[
            "submit",
            "main",
            "--worktree",
            BRANCH,
            "--no-wait",
            "--json",
        ],
    );
    let v = json(&out);
    assert_eq!(
        v["copy"],
        serde_json::json!({
            "branch": BRANCH, "copy": made, "remote": "origin", "at": landing, "how": "behind",
        }),
        "{v}"
    );
    assert_eq!(f.world.copy_of(BRANCH), Some(made));

    f.world.session(BRANCH, "ops", &landing);
    let (_, out) = sluice(&f, &["submit", "main", "--worktree", BRANCH, "--no-wait"]);
    assert_eq!(out.lines().count(), 1, "{out}");
    let (_, out) = sluice(
        &f,
        &[
            "submit",
            "main",
            "--worktree",
            BRANCH,
            "--no-wait",
            "--json",
        ],
    );
    assert!(json(&out).get("copy").is_none(), "{out}");
}

/// 124: a waiting submit under `--json` ends with the submission's record,
/// which carries how this host's copy differed from what it submitted;
/// level, the record alone.
#[test]
fn a_waiting_submit_from_a_bolt_carries_its_copy_under_json() {
    let f = Fakes::new();
    let on_origin = POLICY.replace("parallel = 4\n", "parallel = 4\nremote = \"origin\"\n");
    f.world.init(
        BRANCH,
        &[
            (".config/sluice.toml", on_origin.as_str()),
            ("plugin/lib/plan.py", "plan"),
        ],
    );
    f.world.task("plugin:test", "plugin/");
    f.world.on_remote(BRANCH);
    f.world.init(
        "main",
        &[(
            ".config/sluice.toml",
            "[branches.main]\nrequired = [\"#gate\"]\nremote = \"origin\"\n",
        )],
    );
    let made = f.world.tip(BRANCH);
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    let landing = f.world.tip(BRANCH);
    f.world.set_copy(BRANCH, Some(&made));
    f.world.session(BRANCH, "ops", &made);

    let (_, out) = sluice(&f, &["submit", "main", "--worktree", BRANCH, "--json"]);
    let v = json(&out);
    assert_eq!(
        (v["id"].as_u64(), &v["head"]),
        (Some(2), &serde_json::json!(landing)),
        "{v}"
    );
    assert_eq!(
        v["copy"],
        serde_json::json!({
            "branch": BRANCH, "copy": made, "remote": "origin", "at": landing, "how": "behind",
        }),
        "{v}"
    );
    // A group's document carries it on the record of the place it is of.
    let own = f.world.work(&f.world.tip("main"), &[("m/a", Some("a"))]);
    f.world.session("unit/m", "agent-m", &own);
    let args = [
        "submit",
        "main",
        "--worktree",
        BRANCH,
        "--worktree",
        "unit/m",
    ];
    let (_, out) = sluice(&f, &[&args[..], &["--json"]].concat());
    let v = json(&out);
    let subs = v["submissions"].as_array().expect("a group's records");
    assert_eq!(subs[0]["copy"]["how"], "behind", "{v}");
    assert!(subs[1].get("copy").is_none(), "{v}");

    work(&f, "unit/b", "agent-b", &[("plugin/b", Some("b"))]);
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/b"]);
    assert_eq!(code, LANDED, "{out}");
    let level = f.world.tip(BRANCH);
    f.world.session(BRANCH, "ops", &level);
    let (_, out) = sluice(&f, &["submit", "main", "--worktree", BRANCH, "--json"]);
    let v = json(&out);
    assert_eq!(v["head"], serde_json::json!(level), "{v}");
    assert!(v.get("copy").is_none(), "{v}");
}

/// 124, record 0065: `--status --json` names `sends`, what a submit from
/// the worktree would send now; from a worktree on its own branch, its
/// head.
#[test]
fn status_sends_the_worktrees_head_from_its_own_branch() {
    let f = setup();
    let head = f
        .world
        .work(&f.world.tip(BRANCH), &[("plugin/a", Some("a"))]);
    f.world.session("unit/a", "agent-a", &head);
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = sluice(
        &f,
        &[
            "submit",
            BRANCH,
            "--worktree",
            "unit/a",
            "--status",
            "--json",
        ],
    );
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    assert_eq!(
        (v["id"].as_u64(), v["state"].as_str()),
        (Some(1), Some("queued")),
        "{v}"
    );
    assert_eq!(v["sends"], serde_json::json!(head), "{v}");
    assert_eq!(v["head"], v["sends"], "{v}");
}

/// 124, record 0065: from a worktree on a branch sluice lands on, its copy
/// behind, `sends` is the remote's revision, the submission's `head`, not
/// the worktree's; and reading it moves nothing (rule core.4).
#[test]
fn status_sends_the_remotes_revision_from_a_bolt() {
    let f = Fakes::new();
    let on_origin = POLICY.replace("parallel = 4\n", "parallel = 4\nremote = \"origin\"\n");
    f.world.init(
        BRANCH,
        &[
            (".config/sluice.toml", on_origin.as_str()),
            ("plugin/lib/plan.py", "plan"),
        ],
    );
    f.world.task("plugin:test", "plugin/");
    f.world.on_remote(BRANCH);
    f.world.init(
        "main",
        &[(
            ".config/sluice.toml",
            "[branches.main]\nrequired = [\"#gate\"]\nremote = \"origin\"\n",
        )],
    );
    let made = f.world.tip(BRANCH);
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", "unit/a"]);
    assert_eq!(code, LANDED, "{out}");
    let landing = f.world.tip(BRANCH);
    f.world.set_copy(BRANCH, Some(&made));
    f.world.session(BRANCH, "ops", &made);

    let (code, out) = sluice(&f, &["submit", "main", "--worktree", BRANCH, "--no-wait"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = sluice(
        &f,
        &["submit", "main", "--worktree", BRANCH, "--status", "--json"],
    );
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    assert_eq!(v["sends"], serde_json::json!(landing), "{v}");
    assert_eq!(v["head"], v["sends"], "{v}");
    assert_ne!(v["sends"], serde_json::json!(made), "{v}");
    assert_eq!(f.world.copy_of(BRANCH), Some(made));
    assert_eq!(f.world.tip(BRANCH), landing);
}

// ---------------------------------------------------------------------------
// A branch that cannot be read is refused alone (26, S64)
// ---------------------------------------------------------------------------

mod refused_alone {
    use super::*;

    /// The branch whose records cannot be read, listed before [`BRANCH`].
    const REFUSED: &str = "bolt/b";

    /// Another bolt, listed after `bolt/b`.
    const OTHER: &str = "bolt/c";

    /// Submission 1 queued on [`BRANCH`] and 2 on `bolt/b`; with
    /// `refused`, every read of `bolt/b`'s records then fails with
    /// [`UNREADABLE`].
    fn queued(refused: bool) -> Fakes {
        let f = setup();
        f.world.init(REFUSED, &[(".config/sluice.toml", POLICY)]);
        work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
        sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
        let head = f
            .world
            .work(&f.world.tip(REFUSED), &[("plugin/b", Some("b"))]);
        f.world.session("unit/b", "agent-b", &head);
        sluice(
            &f,
            &["submit", REFUSED, "--worktree", "unit/b", "--no-wait"],
        );
        if refused {
            f.world.fail_state_reads(REFUSED, Some(UNREADABLE));
        }
        f
    }

    /// 26, S64: a wait on a submission to another branch finds it past
    /// the branch that cannot be read and follows it to its landing.
    #[test]
    fn a_wait_follows_its_submission_past_a_branch_that_cannot_be_read() {
        let f = queued(true);
        let (code, out) = sluice(&f, &["wait", "1", "--json"]);
        assert_eq!(code, LANDED, "{out}");
        let v = json(&out);
        assert_eq!(
            (v["id"].clone(), v["state"].clone()),
            (1.into(), "landed".into())
        );
    }

    /// 26: status, amend and withdraw answer for a submission on another
    /// branch as when the branch that cannot be read reads, and say
    /// nothing of it.
    #[test]
    fn status_amend_and_withdraw_answer_as_when_every_branch_reads() {
        let answers = |f: &Fakes| -> Vec<(i32, String)> {
            [
                &["status", "1"][..],
                &["status", "1", "--json"],
                &["amend", "1", "--priority", "1"],
                &["amend", "1", "--priority", "2", "--json"],
                &["withdraw", "1"],
            ]
            .iter()
            .map(|args| sluice(f, args))
            .collect()
        };
        let past = answers(&queued(true));
        assert_eq!(past, answers(&queued(false)));
        for (code, out) in &past {
            assert_eq!(*code, 0, "{out}");
            assert!(!out.contains(REFUSED), "{out}");
        }
    }

    /// 26: a number on the branch that cannot be read is on none of the
    /// branches read: said so with the branch and its words, never no
    /// submission, exit 3; under `--json`, the internal error document.
    #[test]
    fn a_number_on_the_branch_that_cannot_be_read_names_it() {
        let f = queued(true);
        let named = format!("{REFUSED} could not be read: {UNREADABLE}");
        let (code, out) = sluice(&f, &["wait", "2"]);
        assert_eq!(code, sluice::INTERNAL, "{out}");
        assert_eq!(
            out,
            format!("submission 2 is on none of the branches sluice could read\n{named}\n")
        );

        let (code, out) = sluice(&f, &["status", "2", "--json"]);
        assert_eq!(code, sluice::INTERNAL, "{out}");
        let v = json(&out);
        assert_eq!(v["error"]["kind"], "internal", "{v}");
        assert_eq!(
            v["error"]["message"],
            format!("submission 2 is on none of the branches sluice could read; {named}")
        );
        assert_eq!(v["error"]["next"], serde_json::json!(["sluice overview"]));

        for args in [&["amend", "2", "--priority", "1"][..], &["withdraw", "2"]] {
            let (code, out) = sluice(&f, args);
            assert_eq!(code, sluice::INTERNAL, "{args:?}: {out}");
            assert!(out.ends_with(&format!("{named}\n")), "{args:?}: {out}");
            assert!(!out.contains("no submission"), "{args:?}: {out}");
        }
    }

    /// 26: with every branch read, a number on none is still no
    /// submission, exit 1.
    #[test]
    fn a_number_on_no_branch_when_every_branch_reads_is_no_submission() {
        let f = queued(false);
        let (code, out) = sluice(&f, &["wait", "9999"]);
        assert_eq!(code, FAILED, "{out}");
        assert_eq!(out, "no submission 9999\n");
    }

    /// The line `sluice run` says a branch it passed by with.
    fn passed_by(words: &str) -> String {
        format!("{REFUSED} could not be read, passed by: {words}")
    }

    /// A submission from `worktree` to `branch`, on its tip.
    fn submit_to(f: &Fakes, branch: &str, worktree: &str, path: &str) {
        let head = f.world.work(&f.world.tip(branch), &[(path, Some("n"))]);
        f.world.session(worktree, "agent", &head);
        let (code, out) = sluice(f, &["submit", branch, "--worktree", worktree, "--no-wait"]);
        assert_eq!(code, 0, "{out}");
    }

    /// [`queued`], with another bolt's submission queued as well.
    fn two_bolts_queued() -> Fakes {
        let f = queued(true);
        f.world.init(OTHER, &[(".config/sluice.toml", POLICY)]);
        submit_to(&f, OTHER, "unit/c", "plugin/c");
        f
    }

    /// The state of submission `id`, as `sluice status <id>` reads it.
    fn state_of(f: &Fakes, id: u64) -> String {
        let (_, out) = sluice(f, &["status", &id.to_string(), "--json"]);
        json(&out)["state"].as_str().unwrap_or_default().to_string()
    }

    /// 26, S64, rule core.14: `sluice run` passes by the branch it cannot
    /// read, says so once while the same words stand, and lands every
    /// other branch's work; its last round having passed the branch by,
    /// it exits 3 as a step of that branch would.
    #[test]
    fn sluice_run_passes_a_branch_by_and_lands_the_rest() {
        let f = two_bolts_queued();
        let (code, out) = sluice(&f, &["run", "--rounds", "3"]);
        assert_eq!(code, sluice::INTERNAL, "{out}");
        let said: Vec<&str> = out
            .lines()
            .filter(|l| l.starts_with(&format!("{REFUSED} ")))
            .collect();
        assert_eq!(said, [passed_by(UNREADABLE)], "{out}");
        assert_eq!(state_of(&f, 1), "landed", "{out}");
        assert_eq!(state_of(&f, 3), "landed", "{out}");
    }

    /// 26, 121: a branch read on one pass and refused again on the next
    /// is said again, and the run, refused on its last round, exits 3.
    #[test]
    fn sluice_run_says_a_branch_refused_anew_again() {
        let f = two_bolts_queued();
        let between = |n: usize| match n {
            1 => f.world.fail_state_reads(REFUSED, None),
            2 => f.world.fail_state_reads(REFUSED, Some(UNREADABLE)),
            _ => {}
        };
        let pause = Between {
            n: Cell::new(0),
            rounds: 3,
            between: &between,
        };
        let (code, out) = sluice_with(&f, &pause, &["run", "--rounds", "3"]);
        assert_eq!(code, sluice::INTERNAL, "{out}");
        let said = out.lines().filter(|l| *l == passed_by(UNREADABLE)).count();
        assert_eq!(said, 2, "{out}");
    }

    /// 26, S64: a bounded run whose last round read the branch it passed
    /// by before exits 0, having said it once.
    #[test]
    fn sluice_run_read_by_its_last_round_exits_0() {
        let f = two_bolts_queued();
        let between = |n: usize| {
            if n == 1 {
                f.world.fail_state_reads(REFUSED, None);
            }
        };
        let pause = Between {
            n: Cell::new(0),
            rounds: 3,
            between: &between,
        };
        let (code, out) = sluice_with(&f, &pause, &["run", "--rounds", "3"]);
        assert_eq!(code, 0, "{out}");
        let said = out.lines().filter(|l| *l == passed_by(UNREADABLE)).count();
        assert_eq!(said, 1, "{out}");
    }

    /// A fakes' host whose config names the systems it runs (87).
    struct Naming {
        host: support::FakeHost,
        systems: Vec<String>,
    }

    impl sluice::exec::Host for Naming {
        fn reserved(&self) -> usize {
            self.host.reserved()
        }

        fn host_process(&self) -> &dyn sluice::host_process::HostProcess {
            self.host.host_process()
        }

        fn peer(&self) -> sluice_core::Peer {
            sluice_core::Peer {
                systems: Some(self.systems.clone()),
                ..self.host.peer()
            }
        }

        fn worktree(&self, dir: &str) -> anyhow::Result<String> {
            self.host.worktree(dir)
        }

        fn lock_sessions(&self) -> anyhow::Result<Box<dyn std::any::Any>> {
            self.host.lock_sessions()
        }

        fn begin_session(
            &self,
            held: &dyn std::any::Any,
            run: &sluice::sessions::SessionRun,
        ) -> anyhow::Result<()> {
            self.host.begin_session(held, run)
        }

        fn end_session(&self, worktree: &str) -> anyhow::Result<()> {
            self.host.end_session(worktree)
        }

        fn session_tasks(
            &self,
            worktree: &str,
            tasks: &[sluice_seams::TaskProgress],
        ) -> anyhow::Result<()> {
            self.host.session_tasks(worktree, tasks)
        }

        fn run_dir(&self, run: &sluice_seams::RunHandle) -> Option<std::path::PathBuf> {
            self.host.run_dir(run)
        }

        fn end_run(&self, run: &sluice_seams::RunHandle) -> anyhow::Result<()> {
            self.host.end_run(run)
        }

        fn revision(&self, rev: &str) -> anyhow::Result<Option<String>> {
            self.host.revision(rev)
        }

        fn name(&self) -> String {
            self.host.name()
        }

        fn repository(&self) -> anyhow::Result<String> {
            self.host.repository()
        }

        fn repository_gone(&self, repository: &str) -> bool {
            self.host.repository_gone(repository)
        }

        fn tips(&self) -> anyhow::Result<Vec<sluice_seams::Rev>> {
            self.host.tips()
        }

        fn holds_policy(&self, tips: &[sluice_seams::Rev]) -> anyhow::Result<bool> {
            self.host.holds_policy(tips)
        }

        fn readers(&self) -> anyhow::Result<std::path::PathBuf> {
            self.host.readers()
        }

        fn default_branch(&self) -> anyhow::Result<Option<sluice_core::overview::OrderFrom>> {
            self.host.default_branch()
        }
    }

    /// 26, 87, S64, rule core.14: on a host whose config names its
    /// systems, `sluice run` checks them against every branch's policy it
    /// can read, passes by the one whose policy this sluice refuses, runs
    /// the rest's rounds, and exits 3 for the branch it passed by.
    #[test]
    fn sluice_run_on_a_host_naming_its_systems_runs_past_a_refused_policy() {
        let f = queued(false);
        f.world.hand_commit(
            BRANCH,
            &[(
                POLICY_PATH,
                Some(&format!(
                    "{POLICY}\n[systems]\nlinux-x64 = {{ os = \"linux\", arch = \"x86_64\" }}\n"
                )),
            )],
        );
        f.world.hand_commit(
            REFUSED,
            &[(
                POLICY_PATH,
                Some(&format!(
                    "{POLICY}\n[[checkout.cache]]\nkey = [\"Cargo.lock\"]\npaths = [\"target\"]\n"
                )),
            )],
        );
        let host = Naming {
            host: support::FakeHost::new(f.world.clone(), 0),
            systems: vec!["linux-x64".into()],
        };
        let cli = Cli::try_parse_from(["sluice", "run", "--rounds", "1"]).unwrap();
        let mut out = Vec::new();
        let code = run(
            cli,
            f.seams(),
            &host,
            &(),
            &(),
            &f.clock,
            &Rounds(Cell::new(0)),
            &mut out,
        )
        .unwrap();
        let out = String::from_utf8(out).unwrap();
        assert_eq!(code, sluice::INTERNAL, "{out}");
        assert!(
            out.lines().any(|l| l.starts_with(&passed_by(""))
                && l.contains("Remove [[checkout.cache]] from .config/sluice.toml")),
            "{out}"
        );
        assert_eq!(f.world.started().len(), 1, "{BRANCH}'s run started: {out}");
    }
}

/// What `sluice run` reads of a branch's policy and records from pass to
/// pass over the fakes (129).
mod read_once {
    use std::cell::Cell;
    use std::path::Path;

    use clap::Parser;
    use sluice::profiles::local_files::LocalFiles;
    use sluice::{run, Cli, Pause};
    use sluice_core::policy::POLICY_PATH;
    use sluice_core::{Seams, Sluice, SubmitHead, SubmitRequest};
    use sluice_fakes::Fakes;
    use sluice_seams::State as _;

    use super::support::{self, Scratch};
    use super::{setup, work, Between, Rounds, BRANCH};

    /// `sluice <args>` over `seams`, `pause` between its passes.
    fn on(f: &Fakes, seams: Seams<'_>, pause: &dyn Pause, args: &[&str]) -> (i32, String) {
        let cli =
            Cli::try_parse_from(std::iter::once("sluice").chain(args.iter().copied())).unwrap();
        let mut out = Vec::new();
        let host = support::FakeHost::new(f.world.clone(), 0);
        let code = run(cli, seams, &host, &(), &(), &f.clock, pause, &mut out).unwrap();
        (code, String::from_utf8(out).unwrap())
    }

    /// A submission of `worktree`'s work on the tip, queued.
    fn queue(f: &Fakes, worktree: &str, path: &str) {
        work(f, worktree, "agent", &[(path, Some("n"))]);
        let (code, out) =
            super::sluice(f, &["submit", BRANCH, "--worktree", worktree, "--no-wait"]);
        assert_eq!(code, 0, "{out}");
    }

    /// How many times `sluice run --rounds <rounds>` read the policy from
    /// git, over a branch whose submission's run never ends, so every
    /// pass runs a round.
    fn policy_reads_over(rounds: &str) -> usize {
        let f = setup();
        f.world.hangs("plugin:test");
        f.world.hangs("tests:t-batch-cards");
        queue(&f, "unit/a", "plugin/lib/plan.py");
        let before = f.world.reads_of(POLICY_PATH);
        let (_, out) = on(
            &f,
            f.seams(),
            &Rounds(Cell::new(0)),
            &["run", "--rounds", rounds],
        );
        assert!(!out.contains("could not be read"), "{out}");
        f.world.reads_of(POLICY_PATH) - before
    }

    /// reader-tick, "Policy read once while the revision stands": six
    /// passes whose rounds read the policy at one revision read it from
    /// git as often as one pass does.
    #[test]
    fn the_policy_is_read_once_while_the_revision_stands() {
        let one = policy_reads_over("1");
        assert!(one >= 1, "a round reads the policy");
        assert_eq!(policy_reads_over("6"), one);
    }

    /// How a run of `rounds` passes over a branch whose revision holds a
    /// policy sluice refuses went, and how many times it read the policy.
    fn refused_over(rounds: &str) -> (i32, String, usize) {
        let f = setup();
        queue(&f, "unit/a", "plugin/lib/plan.py");
        f.world
            .hand_commit(BRANCH, &[(POLICY_PATH, Some("[branches.\"bolt/*\"\n"))]);
        let before = f.world.reads_of(POLICY_PATH);
        let (code, out) = on(
            &f,
            f.seams(),
            &Rounds(Cell::new(0)),
            &["run", "--rounds", rounds],
        );
        (code, out, f.world.reads_of(POLICY_PATH) - before)
    }

    /// reader-tick, "A refused policy stays refused at its revision": three
    /// passes say the branch passed by in one pass's words, once, and read
    /// the policy from git as often as one pass does.
    #[test]
    fn a_refused_policy_stays_refused_at_its_revision() {
        let (code, one, reads) = refused_over("1");
        assert_eq!(code, sluice::INTERNAL, "{one}");
        let said = format!("{BRANCH} could not be read, passed by: ");
        let words: Vec<&str> = one.lines().filter(|l| l.starts_with(&said)).collect();
        assert_eq!(words.len(), 1, "{one}");
        let (code, three, again) = refused_over("3");
        assert_eq!(code, sluice::INTERNAL, "{three}");
        let lines: Vec<&str> = three.lines().filter(|l| l.starts_with(&said)).collect();
        assert_eq!(lines, words, "{three}");
        assert_eq!(again, reads);
    }

    /// The fakes, with the records in local files under `dir`.
    fn with_files<'a>(f: &'a Fakes, files: &'a LocalFiles) -> Seams<'a> {
        Seams {
            state: files,
            ..f.seams()
        }
    }

    /// The fakes over local files under a scratch directory holding one
    /// landed submission of the branch.
    fn landed_on_files(name: &str) -> (Fakes, Scratch) {
        let f = setup();
        let dir = Scratch::new(name);
        let files = LocalFiles::new(dir.path());
        work(&f, "unit/a", "agent", &[("plugin/lib/plan.py", Some("a"))]);
        let (code, out) = on(
            &f,
            with_files(&f, &files),
            &Rounds(Cell::new(0)),
            &["submit", BRANCH, "--worktree", "unit/a"],
        );
        assert_eq!(code, sluice::LANDED, "{out}");
        (f, dir)
    }

    /// A submission of `worktree`'s work, written to the records under
    /// `dir` by another process.
    fn submit_elsewhere(f: &Fakes, dir: &Path, worktree: &str, path: &str) {
        work(f, worktree, "agent-b", &[(path, Some("b"))]);
        let files = LocalFiles::new(dir);
        let s = Sluice::new(with_files(f, &files), &f.clock);
        s.submit(&SubmitRequest {
            head: SubmitHead::Worktree(worktree.into()),
            branch: BRANCH.into(),
            after: vec![],
            priority: 0,
            subject: None,
        })
        .unwrap()
        .unwrap();
    }

    /// The state of the branch's submission `id` in the records under
    /// `dir`.
    fn state_of(f: &Fakes, dir: &Path, id: u64) -> String {
        let files = LocalFiles::new(dir);
        let (_, out) = on(
            f,
            with_files(f, &files),
            &Rounds(Cell::new(0)),
            &["status", &id.to_string(), "--json"],
        );
        super::json(&out)["state"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    /// reader-tick, "Records read again once they moved": between three
    /// idle passes the branch's records change on disk under their
    /// version, which no write of sluice's does, and are not walked again;
    /// once another process's submission moves their version, the next
    /// pass reads them whole and queues it, as it always has (26, 129).
    #[test]
    fn records_are_read_again_once_they_moved() {
        let (f, dir) = landed_on_files("tick-records");
        let records = dir.path().join("sluice").join(BRANCH).join("branch.json");
        let kept = std::fs::read(&records).unwrap();
        let between = |n: usize| match n {
            1 => std::fs::write(&records, "not the records\n").unwrap(),
            3 => {
                std::fs::write(&records, &kept).unwrap();
                submit_elsewhere(&f, dir.path(), "unit/b", "tests/t_cards.py");
            }
            _ => {}
        };
        let pause = Between {
            n: Cell::new(0),
            rounds: 8,
            between: &between,
        };
        let files = LocalFiles::new(dir.path());
        let (code, out) = on(
            &f,
            with_files(&f, &files),
            &pause,
            &["run", "--rounds", "9"],
        );
        assert_eq!(code, 0, "walked while their version stood: {out}");
        assert!(!out.contains("could not be read"), "{out}");
        assert_eq!(state_of(&f, dir.path(), 2), "landed", "{out}");
    }

    /// Every file under `from`, copied to `to`.
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            let at = to.join(e.file_name());
            match e.file_type().unwrap().is_dir() {
                true => copy(&e.path(), &at),
                false => {
                    std::fs::copy(e.path(), &at).unwrap();
                }
            }
        }
    }

    /// The files of a branch's records under `dir`, by path, leaving out
    /// the version, the lock and anything dotted.
    fn branch_files(dir: &Path) -> std::collections::BTreeMap<String, String> {
        let mut out = std::collections::BTreeMap::new();
        fn visit(at: &Path, base: &Path, out: &mut std::collections::BTreeMap<String, String>) {
            for e in std::fs::read_dir(at).unwrap() {
                let e = e.unwrap();
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') || (at == base && (name == "version" || name == "lock")) {
                    continue;
                }
                match e.file_type().unwrap().is_dir() {
                    true => visit(&e.path(), base, out),
                    false => {
                        let rel = e.path().strip_prefix(base).unwrap().display().to_string();
                        out.insert(rel, std::fs::read_to_string(e.path()).unwrap());
                    }
                }
            }
        }
        visit(dir, dir, &mut out);
        out
    }

    /// reader-tick, "A submission written mid-tick is not lost": a write
    /// of a submission to the branch's records stopped with its pending
    /// write on disk, none of its files changed; the next pass reads the
    /// records as that write leaves them, and the submission lands (51).
    #[test]
    fn a_submission_written_mid_tick_is_not_lost() {
        let (f, dir) = landed_on_files("tick-pending");
        let branch = dir.path().join("sluice").join(BRANCH);
        let between = |n: usize| {
            if n != 1 {
                return;
            }
            // The write as another process made it, in a copy of the
            // records, left pending here.
            let elsewhere = Scratch::new("tick-pending-elsewhere");
            copy(dir.path(), elsewhere.path());
            submit_elsewhere(&f, elsewhere.path(), "unit/b", "tests/t_cards.py");
            let written = elsewhere.path().join("sluice").join(BRANCH);
            let (was, now) = (branch_files(&branch), branch_files(&written));
            let put: std::collections::BTreeMap<&String, &String> = now
                .iter()
                .filter(|(p, t)| was.get(*p) != Some(*t))
                .collect();
            let remove: Vec<&String> = was.keys().filter(|p| !now.contains_key(*p)).collect();
            let version: u64 = std::fs::read_to_string(written.join("version"))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            let pending = serde_json::json!({
                "version": version,
                "put": put,
                "append": {},
                "lengths": {},
                "remove": remove,
            });
            std::fs::write(branch.join(".pending.json"), pending.to_string()).unwrap();
        };
        let pause = Between {
            n: Cell::new(0),
            rounds: 6,
            between: &between,
        };
        let files = LocalFiles::new(dir.path());
        let (code, out) = on(
            &f,
            with_files(&f, &files),
            &pause,
            &["run", "--rounds", "7"],
        );
        assert_eq!(code, 0, "{out}");
        assert!(!branch.join(".pending.json").exists(), "{out}");
        assert_eq!(state_of(&f, dir.path(), 2), "landed", "{out}");
        assert!(files.read(BRANCH).unwrap().version > 0);
    }
}

/// 114, record 0105 (design D7 of the change
/// `a-pass-nothing-proves-blames-no-one`); the run-outcome spec's "Status
/// and wait", "A submission's status and its wait" and "Its next run has a
/// result": a combination tested again after a run that left a check not
/// run says on which host and why, in `status`, its `--json` and
/// `wait --json`, until its next run has a result.
#[test]
fn status_and_wait_say_why_a_combinations_last_run_had_no_outcome() {
    let why = "the engine is down";
    let f = Fakes::new();
    f.world.init(
        BRANCH,
        &[
            (".config/sluice.toml", POLICY),
            ("plugin/lib/plan.py", "plan"),
        ],
    );
    f.world.task("plugin:test", "plugin/");
    f.world.cannot_run("plugin:test", Some(why));
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);

    let said =
        format!("its last run, on this-host, had no outcome: plugin:test could not run: {why}");
    let testing = |text: &str| {
        text.lines()
            .find(|l| l.starts_with(&format!("  testing {BRANCH} + #1 ")))
            .map(str::to_string)
    };
    for _ in 0..8 {
        sluice(&f, &["step", BRANCH]);
        let (_, text) = sluice(&f, &["status", BRANCH]);
        if testing(&text).is_some_and(|l| l.contains("had no outcome")) {
            break;
        }
    }
    let (_, text) = sluice(&f, &["status", BRANCH]);
    assert!(testing(&text).is_some_and(|l| l.ends_with(&said)), "{text}");
    let lost = serde_json::json!([{
        "host": "this-host",
        "not_run": [{ "task": "plugin:test", "reason": why }],
    }]);
    let (_, one) = sluice(&f, &["status", "1", "--json"]);
    assert_eq!(json(&one)["lost"], lost, "{one}");
    let (code, out) = sluice(&f, &["wait", "1", "--timeout", "2s", "--json"]);
    assert_eq!(code, sluice::WAITING, "{out}");
    assert_eq!(json(&out)["lost"], lost, "{out}");

    // Its next run has a result: nothing is said of the one that had none.
    f.world.cannot_run("plugin:test", None);
    let (code, out) = sluice(&f, &["wait", "1", "--timeout", "60s"]);
    assert_eq!(code, 0, "{out}");
    let (_, text) = sluice(&f, &["status", BRANCH]);
    assert!(!text.contains("had no outcome"), "{text}");
    let (_, one) = sluice(&f, &["status", "1", "--json"]);
    let one = json(&one);
    assert_eq!(one["state"], "landed", "{one}");
    assert!(one.get("lost").is_none(), "{one}");
}

/// 84, record 0074; the host-requirements spec's "Status says what it
/// waits for": a combination whose run this host turned down for its
/// container engine says so in `status`, its `--json` and `wait --json`,
/// naming the host and why; one that waits for nothing says nothing of it.
#[test]
fn status_and_wait_say_a_combination_waits_for_a_host_whose_engine_answers() {
    let why = "no container engine answers: no socket at /var/run/docker.sock";
    let f = Fakes::new();
    f.world.init(
        BRANCH,
        &[
            (
                ".config/sluice.toml",
                &format!("{POLICY}\n[executor]\ndocker = true\n"),
            ),
            ("plugin/lib/plan.py", "plan"),
        ],
    );
    f.world.task("plugin:test", "plugin/");
    f.world.engine_unmet(Some(why));
    work(&f, "unit/a", "agent-a", &[("plugin/a", Some("a"))]);
    sluice(&f, &["submit", BRANCH, "--worktree", "unit/a", "--no-wait"]);
    sluice(&f, &["step", BRANCH]);

    let said =
        format!("waiting for a host that can run its tests as its policy asks: this-host: {why}");
    let (_, text) = sluice(&f, &["status", BRANCH]);
    assert!(
        text.lines()
            .any(|l| l.starts_with(&format!("  testing {BRANCH} + #1 ")) && l.ends_with(&said)),
        "{text}"
    );
    let (_, one) = sluice(&f, &["status", "1", "--json"]);
    let one = json(&one);
    let combination = one["tested"]["combination"].clone();
    assert_eq!(one["waiting"], serde_json::json!({ "this-host": why }));
    let (_, out) = sluice(&f, &["status", BRANCH, "--json"]);
    assert_eq!(
        json(&out)["waiting"],
        serde_json::json!([{
            "submissions": [1],
            "combination": combination,
            "waiting": { "this-host": why },
        }])
    );
    let (code, out) = sluice(&f, &["wait", "1", "--timeout", "2s", "--json"]);
    assert_eq!(code, sluice::WAITING, "{out}");
    assert_eq!(
        json(&out)["waiting"],
        serde_json::json!({ "this-host": why })
    );

    // Its engine answers: the run is taken, and nothing waits.
    f.world.engine_unmet(None);
    f.world.hold_runs(true);
    sluice(&f, &["step", BRANCH]);
    let (_, text) = sluice(&f, &["status", BRANCH]);
    assert!(!text.contains("waiting for a host"), "{text}");
    let (_, out) = sluice(&f, &["status", BRANCH, "--json"]);
    assert_eq!(json(&out)["waiting"], serde_json::json!([]));
}

/// Record 0114: a number more than one branch holds names none of them
/// alone; a lookup by it is refused naming each, and `wait`, `status`,
/// `amend` and `withdraw` take `--branch` to name one (26; rule core.11).
mod number_on_two_branches {
    use super::*;

    /// The first branch that holds the number.
    const A: &str = "bolt/a";

    /// The second, listed after `bolt/a`.
    const B: &str = "bolt/b";

    /// A branch whose records cannot be read, listed after both.
    const C: &str = "bolt/c";

    /// The number both branches hold.
    const N: &str = "26";

    /// `branch` made, under the bolts' policy.
    fn branch(f: &Fakes, name: &str) {
        f.world.init(
            name,
            &[(".config/sluice.toml", POLICY), ("plugin/lib/plan.py", "p")],
        );
    }

    /// A submission from `worktree` to `branch`, on its tip, numbered `n`.
    fn submit_as(f: &Fakes, n: u64, branch: &str, worktree: &str) {
        let head = f.world.work(
            &f.world.tip(branch),
            &[(&format!("plugin/{worktree}"), Some("n"))],
        );
        f.world
            .session(worktree, &format!("agent-{worktree}"), &head);
        f.world.numbers_from(n);
        let (code, out) = sluice(f, &["submit", branch, "--worktree", worktree, "--no-wait"]);
        assert_eq!(code, 0, "{out}");
    }

    /// Submission 26 queued on `bolt/a` and on `bolt/b`.
    fn both_queued() -> Fakes {
        let f = setup();
        branch(&f, A);
        branch(&f, B);
        submit_as(&f, 26, A, "unit/a");
        submit_as(&f, 26, B, "unit/b");
        f
    }

    /// [`both_queued`], with `bolt/c` holding a submission of its own and
    /// every read of its records failing with [`UNREADABLE`].
    fn both_and_unread() -> Fakes {
        let f = both_queued();
        branch(&f, C);
        submit_as(&f, 40, C, "unit/c");
        f.world.fail_state_reads(C, Some(UNREADABLE));
        f
    }

    /// `branch`'s submission 26, as `sluice status 26 --branch` shows it.
    fn on(f: &Fakes, branch: &str) -> serde_json::Value {
        let (code, out) = sluice(f, &["status", N, "--branch", branch, "--json"]);
        assert_eq!(code, 0, "{out}");
        let v = json(&out);
        assert_eq!(v["branch"], branch, "{v}");
        v
    }

    /// 26, record 0114: amend, withdraw and status given the branch act on
    /// and show that branch's submission, and leave the other as it was.
    #[test]
    fn each_command_given_the_branch_acts_on_that_branchs_submission() {
        let f = both_queued();
        let (code, out) = sluice(&f, &["amend", N, "--branch", B, "--priority", "5"]);
        assert_eq!((code, out.as_str()), (0, "amended #26\n"));
        let (code, out) = sluice(&f, &["withdraw", N, "--branch", A]);
        assert_eq!((code, out.as_str()), (0, "withdrew #26\n"));

        let b = on(&f, B);
        assert_eq!(
            (b["state"].clone(), b["priority"].clone()),
            ("queued".into(), 5.into())
        );
        let a = on(&f, A);
        assert_eq!(
            (a["state"].clone(), a["priority"].clone()),
            ("withdrawn".into(), 0.into())
        );

        let (code, out) = sluice(&f, &["status", N, "--branch", B]);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains(B) && !out.contains(A), "{out}");
    }

    /// 26, record 0114: a wait given the branch follows that branch's
    /// submission to its landing, never the other's, which the host
    /// process moves as its own (135).
    #[test]
    fn a_wait_given_the_branch_follows_it_to_its_landing() {
        let f = both_queued();
        let (code, out) = sluice(&f, &["wait", N, "--branch", B, "--json"]);
        assert_eq!(code, LANDED, "{out}");
        let v = json(&out);
        assert_eq!(
            (v["id"].clone(), v["branch"].clone(), v["state"].clone()),
            (26.into(), B.into(), "landed".into())
        );
        assert_eq!(on(&f, A)["branch"], A);
    }

    /// 26, record 0114: given the branch, a command reads it alone, and
    /// names no other branch that cannot be read.
    #[test]
    fn another_branch_that_cannot_be_read_is_not_named() {
        let f = both_and_unread();
        for args in [
            &["status", N, "--branch", A][..],
            &["status", N, "--branch", A, "--json"],
        ] {
            let (code, out) = sluice(&f, args);
            assert_eq!(code, 0, "{args:?}: {out}");
            assert!(out.contains(A), "{args:?}: {out}");
            assert!(!out.contains(C) && !out.contains(B), "{args:?}: {out}");
        }
    }

    /// Record 0114: the branch named holds no such number: no submission
    /// there, with the exit no submission has, and its status next.
    #[test]
    fn a_branch_named_holding_no_such_number_is_no_submission_there() {
        let f = both_queued();
        for args in [
            &["status", "27", "--branch", A][..],
            &["wait", "27", "--branch", A],
        ] {
            let (code, out) = sluice(&f, args);
            assert_eq!(
                (code, out.as_str()),
                (FAILED, "no submission 27 on bolt/a\n"),
                "{args:?}"
            );
        }
        let (code, out) = sluice(&f, &["status", "27", "--branch", A, "--json"]);
        assert_eq!(code, FAILED, "{out}");
        let v = json(&out);
        assert_eq!(v["error"]["kind"], "not_found", "{v}");
        assert_eq!(v["error"]["message"], "no submission 27 on bolt/a");
        assert_eq!(
            v["error"]["next"],
            serde_json::json!(["sluice status bolt/a"])
        );

        for args in [
            &["amend", "27", "--branch", A, "--priority", "1"][..],
            &["withdraw", "27", "--branch", A],
        ] {
            let (code, out) = sluice(&f, args);
            assert_eq!(code, 2, "{args:?}: {out}");
            assert_eq!(
                out, "refused: no submission 27 on bolt/a: sluice status bolt/a lists its queue\n",
                "{args:?}"
            );
        }
    }

    /// 26, record 0114: the branch named cannot be read: the number is on
    /// none of the branches sluice could read, that branch named alone,
    /// exit 3.
    #[test]
    fn a_branch_named_that_cannot_be_read_is_named() {
        let f = both_and_unread();
        let (code, out) = sluice(&f, &["wait", N, "--branch", C]);
        assert_eq!(code, sluice::INTERNAL, "{out}");
        assert_eq!(
            out,
            format!(
                "submission 26 is on none of the branches sluice could read\n\
                 {C} could not be read: {UNREADABLE}\n"
            )
        );
    }

    /// Submission 26 queued on `bolt/a`, and submission 26 landed on
    /// `bolt/b`.
    fn queued_and_landed() -> Fakes {
        let f = setup();
        branch(&f, A);
        branch(&f, B);
        submit_as(&f, 26, B, "unit/b");
        let (code, out) = sluice(&f, &["wait", N, "--json"]);
        assert_eq!(code, LANDED, "{out}");
        submit_as(&f, 26, A, "unit/a");
        f
    }

    /// The refusal's first line, for `ask`.
    fn refusal(ask: &str) -> String {
        format!(
            "refused: submission 26 is on more than one branch: bolt/a, bolt/b; name one with \
             --branch, as {ask} 26 --branch bolt/a"
        )
    }

    /// The branches' records' versions, which a write or a round moves.
    fn versions(f: &Fakes) -> (u64, u64) {
        (f.world.version(A), f.world.version(B))
    }

    /// 26, record 0114: a withdrawal of a number two branches hold is
    /// refused naming both, exit 2, and both are as they were.
    #[test]
    fn a_number_two_branches_hold_is_refused_naming_both() {
        let f = queued_and_landed();
        let before = versions(&f);
        let (code, out) = sluice(&f, &["withdraw", N]);
        assert_eq!(code, 2, "{out}");
        assert_eq!(out, format!("{}\n", refusal("sluice withdraw")));
        assert_eq!(versions(&f), before);
        assert_eq!(on(&f, A)["state"], "queued");
        assert_eq!(on(&f, B)["state"], "landed");
    }

    /// 26, record 0114: status and amend are refused the same way, exit 2,
    /// and no priority changes.
    #[test]
    fn status_and_amend_are_refused_the_same_way() {
        let f = both_queued();
        let before = versions(&f);
        for (ask, args) in [
            ("sluice status", &["status", N][..]),
            ("sluice amend", &["amend", N, "--priority", "5"]),
        ] {
            let (code, out) = sluice(&f, args);
            assert_eq!(code, 2, "{args:?}: {out}");
            assert_eq!(out, format!("{}\n", refusal(ask)), "{args:?}");
        }
        assert_eq!(versions(&f), before);
        assert_eq!(on(&f, A)["priority"], 0);
        assert_eq!(on(&f, B)["priority"], 0);
    }

    /// 26, record 0114: a branch that could not be read is named beside
    /// those that hold the number, with its words, a line of its own.
    #[test]
    fn the_refusal_names_a_branch_that_could_not_be_read() {
        let f = both_and_unread();
        let (code, out) = sluice(&f, &["status", N]);
        assert_eq!(code, 2, "{out}");
        assert_eq!(
            out,
            format!(
                "{}\n{C} could not be read: {UNREADABLE}\n",
                refusal("sluice status")
            )
        );
    }

    /// Record 0114: under `--json`, the refused document, its message the
    /// lines joined by `; `, and next the command once per branch.
    #[test]
    fn under_json_the_refusal_names_a_command_per_branch() {
        let f = both_and_unread();
        let (code, out) = sluice(&f, &["withdraw", N, "--json"]);
        assert_eq!(code, 2, "{out}");
        let v = json(&out);
        assert_eq!(v["error"]["kind"], "refused", "{v}");
        let first = refusal("sluice withdraw");
        assert_eq!(
            v["error"]["message"],
            format!(
                "{}; {C} could not be read: {UNREADABLE}",
                first.strip_prefix("refused: ").unwrap()
            )
        );
        assert_eq!(
            v["error"]["next"],
            serde_json::json!([
                "sluice withdraw 26 --branch bolt/a",
                "sluice withdraw 26 --branch bolt/b"
            ])
        );
    }

    /// Record 0114: a wait on a number two branches hold is refused, exits
    /// 64, never a conflict's 2, and runs no round.
    #[test]
    fn a_wait_refused_exits_64_and_runs_no_round() {
        let f = both_queued();
        let before = versions(&f);
        let (code, out) = sluice(&f, &["wait", N, "--json"]);
        assert_eq!(code, sluice::USAGE, "{out}");
        let v = json(&out);
        assert_eq!(v["error"]["kind"], "refused", "{v}");
        assert_eq!(
            v["error"]["next"],
            serde_json::json!([
                "sluice wait 26 --branch bolt/a",
                "sluice wait 26 --branch bolt/b"
            ])
        );
        assert_eq!(versions(&f), before);
        assert!(f.world.started().is_empty(), "a round ran");
        let (code, out) = sluice(&f, &["wait", N]);
        assert_eq!(
            (code, out),
            (sluice::USAGE, format!("{}\n", refusal("sluice wait")))
        );
    }

    /// Record 0114: `sluice wait --help` says 64 is also a number more
    /// than one branch holds, given with no `--branch`.
    #[test]
    fn wait_help_says_64_for_a_number_two_branches_hold() {
        let help = Cli::try_parse_from(["sluice", "wait", "--help"])
            .unwrap_err()
            .to_string();
        assert!(
            help.contains(
                "64   a command line sluice cannot read; or the number is on more than\n       \
                 one branch and no --branch names one: each branch is named"
            ),
            "{help}"
        );
    }

    /// Record 0114: a waiting submit given a number another branch holds
    /// follows its own branch's submission to its landing, never the
    /// other's, which the host process moves as its own (135).
    #[test]
    fn a_waiting_submit_follows_its_own_branch_to_its_landing() {
        let f = setup();
        branch(&f, A);
        branch(&f, B);
        submit_as(&f, 1, A, "unit/a");
        let head = f
            .world
            .work(&f.world.tip(B), &[("plugin/unit/b", Some("n"))]);
        f.world.session("unit/b", "agent-unit/b", &head);
        f.world.numbers_from(1);
        let (code, out) = sluice(&f, &["submit", B, "--worktree", "unit/b", "--json"]);
        assert_eq!(code, LANDED, "{out}");
        let v = json(&out);
        assert_eq!(
            (v["id"].clone(), v["branch"].clone(), v["state"].clone()),
            (1.into(), B.into(), "landed".into())
        );
        let (_, out) = sluice(&f, &["status", "1", "--branch", A, "--json"]);
        assert_eq!(json(&out)["branch"], A, "{out}");
    }

    /// Record 0114: a command given the branch names it in every command
    /// it suggests for the submission, in its text and its `next`.
    #[test]
    fn a_hint_names_the_branch_the_command_was_given() {
        let f = setup();
        branch(&f, A);
        branch(&f, B);
        submit_as(&f, 26, B, "unit/b");
        f.world.hold_runs(true);
        sluice(&f, &["step", B]);
        submit_as(&f, 26, A, "unit/a");

        let args = ["amend", N, "--branch", B, "--priority", "5"];
        let (code, out) = sluice(&f, &args);
        assert_eq!(code, 2, "{out}");
        assert_eq!(
            out,
            "refused: #26 is under test; its order is fixed: sluice status 26 --branch bolt/b \
             follows it, or sluice withdraw 26 --branch bolt/b takes it out\n"
        );
        let (_, out) = sluice(&f, &[&args[..], &["--json"]].concat());
        assert_eq!(
            json(&out)["error"]["next"],
            serde_json::json!([
                "sluice status 26 --branch bolt/b",
                "sluice withdraw 26 --branch bolt/b"
            ])
        );

        // Another's to change: asked again as its agent, on its branch.
        let (code, out) = sluice(
            &f,
            &["withdraw", N, "--branch", A, "--as", "someone", "--json"],
        );
        assert_eq!(code, 2, "{out}");
        assert_eq!(
            json(&out)["error"]["next"],
            serde_json::json!([
                "sluice withdraw 26 --branch bolt/a --as agent-unit/a",
                "sluice withdraw 26 --branch bolt/a --as operator"
            ])
        );
        assert_eq!(on(&f, A)["priority"], 0);
        assert_eq!(on(&f, A)["state"], "queued");
    }

    /// Record 0114: `--branch` goes with a number; with a branch, or with
    /// `status` and no target, it is a command line sluice cannot read.
    #[test]
    fn branch_with_a_branch_target_is_a_usage_error() {
        let f = both_queued();
        let (code, out) = sluice(&f, &["wait", "main", "--branch", A]);
        assert_eq!(code, sluice::USAGE, "{out}");
        assert_eq!(
            out,
            "sluice wait: --branch names the branch of a submission's number; main is a branch\n"
        );
        let (code, out) = sluice(&f, &["status", B, "--branch", A, "--json"]);
        assert_eq!(code, sluice::USAGE, "{out}");
        assert_eq!(
            out,
            "sluice status: --branch names the branch of a submission's number; bolt/b is a \
             branch\n"
        );
        let (code, out) = sluice(&f, &["status", "--branch", A]);
        assert_eq!(code, sluice::USAGE, "{out}");
        assert!(
            out.starts_with("sluice status: --branch names the branch"),
            "{out}"
        );
    }
}

/// settings-take-up, "A run handed over counts its passes" (135; record
/// 0107; design D11): a `sluice run --rounds 3` that followed one pass of
/// the host process and handed over what it carries follows on from what
/// it handed over, and ends after two passes more, saying again nothing it
/// said.
#[test]
fn a_run_handed_over_keeps_following_and_counts_its_passes() {
    use sluice::host_process::ask::Follower;
    use sluice::host_process::protocol::FollowKind;
    use sluice::{Driven, Following};
    let f = one_queued();
    f.world.hold_runs(true);
    let host = support::FakeHost::new(f.world.clone(), 0);
    let s = sluice_core::Sluice::new(f.seams(), &f.clock);
    let here = sluice::host_process::Here {
        s: &s,
        host: &host,
        pulls: &(),
        clock: &f.clock,
    };
    let pause = Rounds(Cell::new(0));
    let mut out = Vec::new();

    let mut following = Following::new(Some(3));
    following.start(&s).unwrap();
    let mut follower = Follower::new(&here, FollowKind::Run);
    let first = following
        .next(&here, &mut follower, &pause, &mut out)
        .unwrap();
    assert_eq!(first, Driven::Next);
    let doc = sluice::hand_over::HandOver {
        settings: String::new(),
        tables: vec!["[telemetry]".into()],
        passes: following.passes(),
        told: following.told(),
    };
    assert_eq!((doc.passes, doc.told), (1, true));

    let mut afresh = Following::handed(Some(3), &doc);
    let mut follower = Follower::new(&here, FollowKind::Run);
    let next = afresh.next(&here, &mut follower, &pause, &mut out).unwrap();
    assert_eq!(next, Driven::Next);
    let last = afresh.next(&here, &mut follower, &pause, &mut out).unwrap();
    assert_eq!(last, Driven::Ended(0));
    assert_eq!(afresh.passes(), 3);
    assert!(afresh.told(), "it says again nothing it said");
}
