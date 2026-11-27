//! A submission's outcome as its reader is handed it (51, S76, S77;
//! record 0102), over the fakes: `sluice inbox`, `sluice confirm`, a wait's
//! hand-over and a status's `delivery`, in the words and documents the
//! commands give. A session whose agent the fakes deliver nothing to stands
//! for one in no pane; one they deliver to, for one in a pane that took the
//! round's notice.

mod support;

use std::cell::Cell;
use std::time::Duration;

use clap::Parser;
use serde_json::Value;
use sluice::{run, Cli, Pause, CONFLICT, FAILED, INTERNAL, LANDED, WAITING};
use sluice_core::policy::POLICY_PATH;
use sluice_fakes::Fakes;

const BRANCH: &str = "bolt/x";

const POLICY: &str = "[branches.\"bolt/*\"]\nrequired = [\"#gate\"]\ntrust = [\"open\"]\n";

thread_local! {
    /// How long this test's host owes an outcome once recorded, the host's
    /// `[deliveries] expire_after` (weir 104).
    static SPAN: Cell<Duration> = const { Cell::new(sluice_core::EXPIRE_AFTER) };
}

/// From now, this test's host owes an outcome for `span` once recorded.
fn owing_for(span: Duration) {
    SPAN.with(|s| s.set(span));
}

struct Rounds(Cell<usize>);

impl Pause for Rounds {
    fn pause(&self) -> bool {
        self.0.set(self.0.get() + 1);
        assert!(self.0.get() < 50, "waited too long");
        true
    }
}

/// `bolt/x` gated on one task over `src/`, which fails on a file named
/// `broken`.
fn setup() -> Fakes {
    let f = Fakes::new();
    f.world
        .init(BRANCH, &[(POLICY_PATH, POLICY), ("src/lib", "lib")]);
    f.world.task("src:test", "src/");
    f.world
        .fails_when("src:test", |t| t.contains_key("src/broken"));
    f
}

/// `sluice <args>` over the fakes: its exit and what it said.
fn sluice(f: &Fakes, args: &[&str]) -> (i32, String) {
    let cli = Cli::try_parse_from(std::iter::once("sluice").chain(args.iter().copied())).unwrap();
    let mut out = Vec::new();
    let code = run(
        cli,
        f.seams(),
        &support::FakeHost::new(f.world.clone(), 0).owing_for(SPAN.with(Cell::get)),
        &(),
        &(),
        &f.clock,
        &Rounds(Cell::new(0)),
        &mut out,
    )
    .unwrap();
    (code, String::from_utf8(out).unwrap())
}

fn json(out: &str) -> Value {
    serde_json::from_str(out).unwrap_or_else(|e| panic!("{e}: {out}"))
}

/// A session `unit/<name>` whose agent is its branch, its head one commit
/// writing `src/<file>`. Its agent takes no notice, as one in no pane.
fn session(f: &Fakes, name: &str, file: &str) -> String {
    let worktree = format!("unit/{name}");
    let head = f.world.work(
        &f.world.tip(BRANCH),
        &[(&format!("src/{file}"), Some(name))],
    );
    f.world.session(&worktree, &worktree, &head);
    f.world.not_delivering(&worktree);
    worktree
}

/// [`session`], its agent taking the round's notice, as one in a pane.
fn in_a_pane(f: &Fakes, name: &str, file: &str) -> String {
    let worktree = format!("unit/{name}");
    let head = f.world.work(
        &f.world.tip(BRANCH),
        &[(&format!("src/{file}"), Some(name))],
    );
    f.world.session(&worktree, &worktree, &head);
    worktree
}

/// A `--no-wait` submit from `worktree`: its number.
fn submit(f: &Fakes, worktree: &str) -> u64 {
    let (code, out) = sluice(
        f,
        &[
            "submit",
            BRANCH,
            "--worktree",
            worktree,
            "--no-wait",
            "--json",
        ],
    );
    assert_eq!(code, 0, "{out}");
    json(&out)["id"].as_u64().unwrap()
}

fn status(f: &Fakes, id: u64) -> Value {
    let (code, out) = sluice(f, &["status", &id.to_string(), "--json"]);
    assert_eq!(code, 0, "{out}");
    json(&out)
}

/// Rounds of `bolt/x` until submission `id` is decided, and one more.
fn decide(f: &Fakes, id: u64) -> Value {
    for _ in 0..20 {
        sluice(f, &["step", BRANCH]);
        let state = status(f, id)["state"].as_str().unwrap_or("").to_string();
        if ["landed", "failed", "conflict"].contains(&state.as_str()) {
            sluice(f, &["step", BRANCH]);
            return status(f, id);
        }
    }
    panic!("never decided: {}", status(f, id));
}

/// The ways each hand-over of submission `n`'s outcome took.
fn handed(f: &Fakes, n: u64) -> Vec<String> {
    status(f, n)["delivery"]["handed"]
        .as_array()
        .map(|all| {
            all.iter()
                .map(|h| h["by"].as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn inbox(f: &Fakes, worktree: &str, args: &[&str]) -> (i32, String) {
    let mut a = vec!["inbox", "--worktree", worktree];
    a.extend(args);
    sluice(f, &a)
}

fn confirm(f: &Fakes, worktree: &str, args: &[&str]) -> (i32, String) {
    let mut a = vec!["confirm", "--worktree", worktree];
    a.extend(args);
    sluice(f, &a)
}

/// 51, 135; inbox spec, "Each outcome collected is handed over and stays
/// owed": under `--json`, one document of the records handed over, each
/// with its `delivery` as the run leaves it, and no `delivered`; collected
/// again, the same record, until it is confirmed, then none (record 0102).
#[test]
fn inbox_json_lists_each_record_until_it_is_confirmed() {
    let f = setup();
    let a = session(&f, "a", "a");
    let n = submit(&f, &a);
    decide(&f, n);
    for times in 1..=2 {
        let (code, out) = inbox(&f, &a, &["--json"]);
        assert_eq!(code, 0, "{out}");
        let v = json(&out);
        let outcomes = v["outcomes"].as_array().unwrap();
        assert_eq!(outcomes.len(), 1, "{v}");
        assert_eq!(outcomes[0]["id"], n);
        assert_eq!(outcomes[0]["state"], "landed");
        assert!(outcomes[0].get("delivered").is_none(), "{v}");
        assert!(outcomes[0].get("delivered_to").is_none(), "{v}");
        let delivery = &outcomes[0]["delivery"];
        assert_eq!(delivery["key"], format!("{n}:landed").as_str(), "{v}");
        assert_eq!(delivery["state"], "owed", "{v}");
        assert_eq!(delivery["handed"].as_array().unwrap().len(), times, "{v}");
        assert_eq!(
            delivery["handed"],
            status(&f, n)["delivery"]["handed"],
            "{v}"
        );
        assert!(v.get("unread").is_none(), "{v}");
    }
    let (code, out) = confirm(&f, &a, &[&format!("{n}:landed")]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = inbox(&f, &a, &["--json"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(json(&out), serde_json::json!({ "outcomes": [] }));
}

/// 51; inbox spec, "A notice already pushed" and "A new outcome with an
/// older one owed": `sluice inbox --new` hands over only what no way has
/// handed its reader: nothing of a landing the round pushed into the
/// worktree's pane; and, once a plain collect has handed that landing over
/// by `inbox`, the worktree's next blame alone (record 0102, D8, D14).
#[test]
fn inbox_new_hands_over_only_what_no_way_has_handed_over() {
    let f = setup();
    let a = in_a_pane(&f, "a", "a");
    let n = submit(&f, &a);
    assert_eq!(decide(&f, n)["state"], "landed");
    assert_eq!(handed(&f, n), ["notice"]);

    let (code, out) = inbox(&f, &a, &["--new"]);
    assert_eq!((code, out.as_str()), (0, ""));
    assert_eq!(handed(&f, n), ["notice"]);

    let (code, out) = inbox(&f, &a, &[]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(handed(&f, n), ["notice", "inbox"]);

    let head = f
        .world
        .work(&f.world.tip(BRANCH), &[("src/broken", Some("x"))]);
    f.world.session(&a, &a, &head);
    f.world.not_delivering(&a);
    let m = submit(&f, &a);
    assert_eq!(decide(&f, m)["state"], "failed");
    assert!(handed(&f, m).is_empty(), "{}", status(&f, m));
    let (code, said) = inbox(&f, &a, &["--new"]);
    assert_eq!(code, 0, "{said}");
    assert!(
        said.lines()
            .next()
            .unwrap_or_default()
            .contains(&format!("#{m} failed")),
        "{said}"
    );
    assert!(!said.contains(&format!("#{n} landed")), "{said}");
    assert_eq!(
        said.lines().last(),
        Some(format!("Confirm it once you have acted on it: sluice confirm {m}:failed").as_str()),
        "{said}"
    );
    assert_eq!(handed(&f, m), ["inbox"]);
    assert_eq!(handed(&f, n), ["notice", "inbox"]);
}

/// 51, 135; inbox spec, "A blame is delivered with its failed checks and
/// logs": a blame is collected naming its failed check, then in the words
/// `sluice wait` reports it with, then the line naming its key. Its wait
/// is read from no worktree, so it hands nothing over before the collect.
#[test]
fn inbox_delivers_a_blame_as_sluice_wait_reports_it() {
    let f = setup();
    let b = session(&f, "b", "broken");
    let n = submit(&f, &b);
    assert_eq!(decide(&f, n)["state"], "failed");
    let (code, waited) = sluice(&f, &["wait", &n.to_string()]);
    assert_eq!(code, FAILED, "{waited}");
    assert!(handed(&f, n).is_empty(), "{}", status(&f, n));

    let (code, said) = inbox(&f, &b, &[]);
    assert_eq!(code, 0, "{said}");
    let (first, rest) = said.split_once('\n').unwrap();
    for word in [&format!("#{n}") as &str, "failed", "src:test"] {
        assert!(first.contains(word), "{word} not in {said}");
    }
    assert_eq!(
        rest,
        format!("{waited}Confirm it once you have acted on it: sluice confirm {n}:failed\n")
    );
    assert_eq!(handed(&f, n), ["inbox"]);
}

/// 51; inbox spec, "Another worktree's outcomes are not delivered":
/// another worktree's outcome is not this session's to collect, and stays
/// owed with no hand-over.
#[test]
fn inbox_delivers_nothing_of_another_worktrees_outcome() {
    let f = setup();
    let a = session(&f, "a", "a");
    let b = session(&f, "b", "b");
    let n = submit(&f, &a);
    decide(&f, n);
    let (code, out) = inbox(&f, &b, &[]);
    assert_eq!((code, out.as_str()), (0, ""));
    assert_eq!(status(&f, n)["delivery"]["state"], "owed");
    assert!(handed(&f, n).is_empty());
}

/// 51; outcome-delivery spec, "A waiting submit hands over its landing": a
/// `sluice submit --json` that waits and reports its landing hands it over
/// by `wait`, in the record it prints and in the branch's records, and the
/// landing stays owed, so a later collect in that worktree hands it over
/// again (record 0102).
#[test]
fn a_waiting_submit_hands_its_landing_over_and_inbox_hands_it_over_again() {
    let f = setup();
    let a = session(&f, "a", "a");
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", &a, "--json"]);
    assert_eq!(code, LANDED, "{out}");
    let record = json(&out);
    assert_eq!(record["state"], "landed", "{record}");
    assert!(record.get("delivered").is_none(), "{record}");
    assert_eq!(record["delivery"]["state"], "owed", "{record}");
    assert_eq!(record["delivery"]["handed"][0]["by"], "wait", "{record}");
    let n = record["id"].as_u64().unwrap();
    assert_eq!(handed(&f, n), ["wait"]);
    let (code, out) = inbox(&f, &a, &["--json"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(json(&out)["outcomes"][0]["id"], n, "{out}");
    assert_eq!(handed(&f, n), ["wait", "inbox"]);
}

/// 51; outcome-delivery spec, "A wait that confirms on receipt": `sluice
/// wait <id> --confirm` in the submitting worktree, reporting a blame the
/// rounds decided, hands it over and confirms it there, and a later collect
/// prints nothing (record 0102).
#[test]
fn a_wait_that_confirms_on_receipt_leaves_inbox_nothing() {
    let f = setup();
    let b = session(&f, "b", "broken");
    let n = submit(&f, &b);
    assert_eq!(decide(&f, n)["state"], "failed");
    let (code, out) = sluice(
        &f,
        &[
            "wait",
            &n.to_string(),
            "--worktree",
            &b,
            "--confirm",
            "--json",
        ],
    );
    assert_eq!(code, FAILED, "{out}");
    let record = json(&out);
    assert_eq!(record["delivery"]["state"], "done", "{record}");
    assert_eq!(handed(&f, n), ["wait"]);
    let (code, out) = inbox(&f, &b, &[]);
    assert_eq!((code, out.as_str()), (0, ""));
}

/// 20, 51; outcome-delivery spec, "A wait on another's submission marks
/// nothing": a blame waited on from another worktree is reported and left
/// owed, for its own session to collect.
#[test]
fn a_wait_from_another_worktree_marks_nothing() {
    let f = setup();
    let a = session(&f, "a", "a");
    let b = session(&f, "b", "broken");
    let n = submit(&f, &b);
    assert_eq!(decide(&f, n)["state"], "failed");
    let (code, out) = sluice(&f, &["wait", &n.to_string(), "--worktree", &a, "--json"]);
    assert_eq!(code, FAILED, "{out}");
    let record = json(&out);
    assert!(record.get("delivered").is_none(), "{record}");
    assert_eq!(record["delivery"]["state"], "owed", "{record}");
    assert!(handed(&f, n).is_empty(), "{}", status(&f, n));
    let (_, out) = inbox(&f, &b, &["--json"]);
    assert_eq!(json(&out)["outcomes"][0]["id"], n, "{out}");
}

/// 51; outcome-delivery spec, "A wait reads an outcome already
/// delivered": a landing the pane took and its worktree confirmed is
/// reported by a later wait in its worktree with its delivery as it
/// stood, and the wait records nothing (record 0102).
#[test]
fn a_wait_reports_an_outcome_the_pane_took_as_it_stands() {
    let f = setup();
    let a = in_a_pane(&f, "a", "a");
    let n = submit(&f, &a);
    assert_eq!(decide(&f, n)["state"], "landed");
    assert_eq!(handed(&f, n), ["notice"]);
    let (code, out) = confirm(&f, &a, &[&format!("{n}:landed")]);
    assert_eq!(code, 0, "{out}");
    let stood = status(&f, n)["delivery"].clone();
    let (code, out) = sluice(&f, &["wait", &n.to_string(), "--worktree", &a, "--json"]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(json(&out)["delivery"], stood, "{out}");
    assert_eq!(status(&f, n)["delivery"], stood);
}

/// 51: a wait that runs out of time before its submission is decided
/// marks nothing, and the outcome, once decided, is collected still.
#[test]
fn a_wait_that_runs_out_of_time_marks_nothing() {
    let f = setup();
    let a = session(&f, "a", "a");
    let n = submit(&f, &a);
    f.world.hold_runs(true);
    let (code, out) = sluice(
        &f,
        &[
            "wait",
            &n.to_string(),
            "--worktree",
            &a,
            "--timeout",
            "1s",
            "--json",
        ],
    );
    assert_eq!(code, WAITING, "{out}");
    let record = json(&out);
    assert!(
        record["state"] == "queued" || record["state"] == "testing",
        "{record}"
    );
    assert!(record.get("delivery").is_none(), "{record}");
    f.world.hold_runs(false);
    f.world.complete_all();
    assert_eq!(decide(&f, n)["state"], "landed");
    assert!(handed(&f, n).is_empty(), "{}", status(&f, n));
    let (_, out) = inbox(&f, &a, &["--json"]);
    assert_eq!(json(&out)["outcomes"][0]["id"], n, "{out}");
}

/// 51; owed-outcomes spec, "A landing pushed and not confirmed", "A
/// landing confirmed and let go of" and "A queued submission": `sluice
/// status <n> --json` gives a queued submission no `delivery`; landed and
/// pushed into its pane, its `delivery` with its key, reader, `owed`,
/// times, the hand-over by `notice`, `confirmed` null and its reader not
/// known, and no `delivered`; confirmed, `done` with its time; let go of
/// by the branch's next write, its key and `done` alone; and a withdrawn
/// submission none (weir 107; record 0102, D3, D13; rule core.50).
#[test]
fn status_json_gives_a_decided_submissions_delivery_and_none_before() {
    let f = setup();
    let a = in_a_pane(&f, "a", "a");
    let n = submit(&f, &a);
    let queued = status(&f, n);
    assert_eq!(queued["state"], "queued", "{queued}");
    assert!(queued.get("delivery").is_none(), "{queued}");
    assert_eq!(decide(&f, n)["state"], "landed");
    let key = format!("{n}:landed");
    let record = status(&f, n);
    let d = &record["delivery"];
    assert_eq!(d["key"], key.as_str(), "{record}");
    assert_eq!(d["reader"], "unit/a", "{record}");
    assert_eq!(d["state"], "owed", "{record}");
    assert!(d["recorded_at"].is_string(), "{record}");
    assert!(d["expires_at"].is_string(), "{record}");
    let handed = d["handed"].as_array().unwrap();
    assert_eq!(handed.len(), 1, "{record}");
    assert_eq!(handed[0]["by"], "notice", "{record}");
    assert!(handed[0]["at"].is_string(), "{record}");
    assert_eq!(d["confirmed"], Value::Null, "{record}");
    assert_eq!(d["known"], false, "{record}");
    assert!(record.get("delivered").is_none(), "{record}");
    assert!(record.get("delivered_to").is_none(), "{record}");

    let (code, out) = confirm(&f, &a, &[&key]);
    assert_eq!(code, 0, "{out}");
    let done = status(&f, n);
    assert_eq!(done["delivery"]["state"], "done", "{done}");
    assert!(done["delivery"]["confirmed"].is_string(), "{done}");
    assert_eq!(done["delivery"]["known"], true, "{done}");

    // The branch's next write, a submit's, lets the done delivery go.
    let c = session(&f, "c", "c");
    let w = submit(&f, &c);
    assert_eq!(
        status(&f, n)["delivery"],
        serde_json::json!({ "key": key, "state": "done" })
    );

    // Withdrawn by its worktree's next submission while queued.
    let head = f.world.work(&f.world.tip(BRANCH), &[("src/c", Some("c2"))]);
    f.world.session(&c, &c, &head);
    let again = submit(&f, &c);
    assert_ne!(again, w);
    let withdrawn = status(&f, w);
    assert_eq!(withdrawn["state"], "withdrawn", "{withdrawn}");
    assert!(withdrawn.get("delivery").is_none(), "{withdrawn}");
}

/// 51, S77; outcome-confirm spec, "A landing confirmed", "Confirmed
/// twice", "Another worktree's blame" and "A removed worktree's blame": a
/// worktree confirms its landing by its key, and its inbox prints nothing
/// of it then; confirmed again, it says when it was and writes nothing;
/// another worktree's blame is refused, naming its reader, and stays owed
/// until the operator confirms it (record 0102).
#[test]
fn confirm_confirms_a_landing_by_its_key_and_says_when_again() {
    let f = setup();
    let a = session(&f, "a", "a");
    let n = submit(&f, &a);
    decide(&f, n);
    let key = format!("{n}:landed");

    let (code, said) = confirm(&f, &a, &[&key]);
    assert_eq!(code, 0, "{said}");
    for word in [key.as_str(), &format!("#{n}"), BRANCH, "landed"] {
        assert!(said.contains(word), "{word} not in {said}");
    }
    assert_eq!(inbox(&f, &a, &[]), (0, String::new()));
    let record = status(&f, n);
    assert_eq!(record["delivery"]["state"], "done", "{record}");
    let at = record["delivery"]["confirmed"]
        .as_str()
        .unwrap_or_else(|| panic!("confirmed: {record}"))
        .to_string();

    // Confirmed twice.
    let (code, out) = confirm(&f, &a, &[&key, "--json"]);
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    assert_eq!(v["confirmed"][0]["key"], key.as_str(), "{v}");
    assert_eq!(v["confirmed"][0]["first"], false, "{v}");
    assert_eq!(v["confirmed"][0]["at"], at.as_str(), "{v}");
    let (_, out) = confirm(&f, &a, &[&key]);
    assert_eq!(out.trim_end(), format!("{key} was confirmed at {at}"));

    // Another worktree's blame.
    let b = session(&f, "b", "broken");
    let m = submit(&f, &b);
    assert_eq!(decide(&f, m)["state"], "failed");
    let blame = format!("{m}:failed");
    let (code, out) = confirm(&f, &a, &[&blame]);
    assert_eq!(code, CONFLICT, "{out}");
    assert!(out.contains("unit/b"), "{out}");
    assert!(out.contains("--as operator"), "{out}");
    assert_eq!(status(&f, m)["delivery"]["state"], "owed");
    let (code, out) = confirm(&f, &a, &[&blame, "--json"]);
    assert_eq!(code, CONFLICT, "{out}");
    let v = json(&out);
    assert_eq!(v["error"]["kind"], "refused", "{v}");
    assert_eq!(
        v["error"]["next"],
        serde_json::json!([format!("sluice confirm {blame} --as operator")]),
        "{v}"
    );

    // Confirmed by the operator for its own reader.
    let (code, out) = sluice(&f, &["confirm", &blame, "--as", "operator"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(status(&f, m)["delivery"]["state"], "done");

    // A key no branch holds.
    let (code, out) = confirm(&f, &a, &["99:landed"]);
    assert_eq!(code, FAILED, "{out}");
    assert!(out.contains("99:landed"), "{out}");
    assert!(out.contains("sluice inbox"), "{out}");
}

/// 51; outcome-confirm spec, "A confirmation's document" and "A key no
/// branch holds is not found": under `--json`, a first confirmation gives
/// its key, branch, reader, time and `first` true; a key no branch holds
/// is the error document, `not_found`, naming `sluice inbox`, exit 1;
/// several keys exit by the worst of them, a refusal's 2 over a key not
/// found; and while a branch's records do not read, a key no branch read
/// holds exits 3, over a refusal, since whether the key is there is not
/// known (rule core.11; record 0102, D7).
#[test]
fn confirm_json_gives_a_first_confirmation_and_the_worst_of_its_exits() {
    let f = setup();
    let a = session(&f, "a", "a");
    let n = submit(&f, &a);
    decide(&f, n);
    let key = format!("{n}:landed");
    let (code, out) = confirm(&f, &a, &[&key, "--json"]);
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    let confirmed = v["confirmed"].as_array().unwrap();
    assert_eq!(confirmed.len(), 1, "{v}");
    assert_eq!(confirmed[0]["key"], key.as_str(), "{v}");
    assert_eq!(confirmed[0]["branch"], BRANCH, "{v}");
    assert_eq!(confirmed[0]["reader"], "unit/a", "{v}");
    assert!(confirmed[0]["at"].is_string(), "{v}");
    assert_eq!(confirmed[0]["first"], true, "{v}");

    let (code, out) = confirm(&f, &a, &["99:landed", "--json"]);
    assert_eq!(code, FAILED, "{out}");
    let v = json(&out);
    assert_eq!(v["error"]["kind"], "not_found", "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("99:landed"),
        "{v}"
    );
    assert_eq!(
        v["error"]["next"],
        serde_json::json!(["sluice inbox"]),
        "{v}"
    );

    let b = session(&f, "b", "broken");
    let m = submit(&f, &b);
    assert_eq!(decide(&f, m)["state"], "failed");
    let blame = format!("{m}:failed");
    for keys in [["99:landed", blame.as_str()], [blame.as_str(), "99:landed"]] {
        let (code, out) = confirm(&f, &a, &keys);
        assert_eq!(code, CONFLICT, "{out}");
        let (_, out) = confirm(&f, &a, &[keys[0], keys[1], "--json"]);
        assert_eq!(json(&out)["error"]["kind"], "refused", "{out}");
    }

    // A branch whose records do not read.
    f.world.set_branch("bolt/y", &f.world.tip(BRANCH));
    sluice(&f, &["step", "bolt/y"]);
    f.world
        .fail_state_reads("bolt/y", Some("deliveries.jsonl does not read"));
    let (code, out) = confirm(&f, &a, &["99:landed"]);
    // Which branch could not be read is said on stderr, as
    // cli's the_refusal_names_a_branch_that_could_not_be_read proves.
    assert_eq!(code, INTERNAL, "{out}");
    let (code, out) = confirm(&f, &a, &["99:landed", &blame, "--json"]);
    assert_eq!(code, INTERNAL, "{out}");
    assert_eq!(json(&out)["error"]["kind"], "internal", "{out}");
    f.world.fail_state_reads("bolt/y", None);
    assert_eq!(status(&f, m)["delivery"]["state"], "owed");
}

/// A session `name` that submits a commit writing `src/<file>`, then a
/// commit writing `src/<more>` on top of it: its first submission, which
/// the second withdrew, and the second, which carries the first's work
/// (67).
fn resubmitted(f: &Fakes, name: &str, file: &str, more: &str) -> (String, u64, u64) {
    let a = session(f, name, file);
    let first = submit(f, &a);
    let held = f.seams().driver.session(&a).unwrap().head;
    let head = f
        .world
        .work(&held, &[(&format!("src/{more}"), Some("more"))]);
    f.world.session(&a, &a, &head);
    f.world.not_delivering(&a);
    let second = submit(f, &a);
    assert_eq!(status(f, first)["state"], "withdrawn");
    (a, first, second)
}

/// 67, 122; record 0100 D2, D3: a wait on a submission its session's next
/// submit withdrew follows the submission that carries its work, says so,
/// and ends with its own submission landed by that one's landing, never
/// with that one's record; the withdrawn one's record stands as it is.
#[test]
fn a_wait_follows_the_resubmission_that_carries_its_work() {
    let f = setup();
    let (_, first, second) = resubmitted(&f, "a", "a", "more");
    let (code, said) = sluice(&f, &["wait", &first.to_string(), "--timeout", "60s"]);
    assert_eq!(code, LANDED, "{said}");
    assert!(
        said.contains(&format!(
            "#{first} withdrawn by resubmission: #{second} carries its work, so this wait follows it"
        )),
        "{said}"
    );
    let landed_as = status(&f, second)["landed_as"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        said.contains(&format!(
            "landed as {}: {BRANCH} holds its work, by #{second}'s landing",
            &landed_as[..7]
        )),
        "{said}"
    );
    assert_eq!(status(&f, first)["state"], "withdrawn");

    // Under --json, its own record, naming what carried its work.
    let (_, first, second) = resubmitted(&f, "b", "b", "more-b");
    let (code, out) = sluice(
        &f,
        &["wait", &first.to_string(), "--timeout", "60s", "--json"],
    );
    assert_eq!(code, LANDED, "{out}");
    let doc = json(&out);
    assert_eq!(doc["id"], first, "{doc}");
    assert_eq!(doc["state"], "landed", "{doc}");
    assert_eq!(doc["landed_elsewhere"], true, "{doc}");
    assert_eq!(doc["landed_as"], status(&f, second)["landed_as"], "{doc}");
    assert_eq!(
        doc["carried_by"],
        serde_json::json!({ "id": second, "state": "landed" }),
        "{doc}"
    );
    assert_eq!(status(&f, first)["state"], "withdrawn");
}

/// 67, 122; record 0100 D2, D3: a wait on a withdrawn submission started
/// only once the resubmission carrying its work has landed ends with its
/// own submission landed by that one's landing and names it, never with
/// the commit a fast-forward would give, its own head.
#[test]
fn a_wait_started_after_the_resubmission_landed_names_it() {
    let f = setup();
    let (_, first, second) = resubmitted(&f, "a", "a", "more");
    decide(&f, second);
    let landed_as = status(&f, second)["landed_as"]
        .as_str()
        .unwrap()
        .to_string();
    let (code, said) = sluice(&f, &["wait", &first.to_string(), "--timeout", "60s"]);
    assert_eq!(code, LANDED, "{said}");
    assert!(
        said.contains(&format!(
            "landed as {}: {BRANCH} holds its work, by #{second}'s landing",
            &landed_as[..7]
        )),
        "{said}"
    );
    let (code, out) = sluice(
        &f,
        &["wait", &first.to_string(), "--timeout", "60s", "--json"],
    );
    assert_eq!(code, LANDED, "{out}");
    let doc = json(&out);
    assert_eq!(doc["landed_elsewhere"], true, "{doc}");
    assert_eq!(doc["landed_as"], landed_as.as_str(), "{doc}");
    assert_ne!(doc["landed_as"], doc["head"], "{doc}");
    assert_eq!(
        doc["carried_by"],
        serde_json::json!({ "id": second, "state": "landed" }),
        "{doc}"
    );
}

/// 67; record 0100 D2, D3: a wait on a submission whose resubmission, which
/// carried its work, is blamed ends with its own submission withdrawn,
/// exit 4, naming the resubmission and how it ended, never with the
/// resubmission's failure as its own.
#[test]
fn a_wait_exits_withdrawn_when_what_carried_its_work_failed() {
    let f = setup();
    let (_, first, second) = resubmitted(&f, "a", "a", "broken");
    let (code, out) = sluice(
        &f,
        &["wait", &first.to_string(), "--timeout", "60s", "--json"],
    );
    assert_eq!(code, sluice::WITHDRAWN, "{out}");
    let doc = json(&out);
    assert_eq!(doc["id"], first, "{doc}");
    assert_eq!(doc["state"], "withdrawn", "{doc}");
    assert_eq!(
        doc["carried_by"],
        serde_json::json!({ "id": second, "state": "failed" }),
        "{doc}"
    );
    let (code, said) = sluice(&f, &["wait", &first.to_string(), "--timeout", "60s"]);
    assert_eq!(code, sluice::WITHDRAWN, "{said}");
    assert!(
        said.contains(&format!(
            "#{first} withdrawn by resubmission: #{second}, which carried its work, failed\n\
             sluice status {second}"
        )),
        "{said}"
    );
}

/// 67; record 0100 D2, D3: a wait out of time while it follows the
/// resubmission that carries its work prints its own submission's record,
/// naming what it follows as it stands, and how to wait on its own again.
#[test]
fn a_wait_out_of_time_while_it_follows_prints_its_own_record() {
    let f = setup();
    let (_, first, second) = resubmitted(&f, "a", "a", "more");
    f.world.hold_runs(true);
    let (code, out) = sluice(
        &f,
        &["wait", &first.to_string(), "--timeout", "2s", "--json"],
    );
    assert_eq!(code, WAITING, "{out}");
    let doc = json(&out);
    assert_eq!(doc["id"], first, "{doc}");
    assert_eq!(doc["state"], "withdrawn", "{doc}");
    assert_eq!(doc["carried_by"]["id"], second, "{doc}");
    let state = &doc["carried_by"]["state"];
    assert!(state == "queued" || state == "testing", "{doc}");
    let (code, said) = sluice(&f, &["wait", &first.to_string(), "--timeout", "2s"]);
    assert_eq!(code, WAITING, "{said}");
    assert!(
        said.contains(&format!("still waiting: sluice wait {first}")),
        "{said}"
    );
}

/// 51; record 0100 D3: a wait that followed the resubmission carrying its
/// work to its landing hands over nothing of the resubmission's landing:
/// it stays owed for its own delivery, which the worktree's inbox then
/// hands over.
#[test]
fn a_wait_leaves_the_resubmissions_landing_pending() {
    let f = setup();
    let (a, first, second) = resubmitted(&f, "a", "a", "more");
    let (code, out) = sluice(
        &f,
        &[
            "wait",
            &first.to_string(),
            "--worktree",
            &a,
            "--timeout",
            "60s",
        ],
    );
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(status(&f, second)["state"], "landed");
    assert!(handed(&f, second).is_empty(), "{}", status(&f, second));
    let (_, out) = inbox(&f, &a, &["--json"]);
    assert_eq!(json(&out)["outcomes"][0]["id"], second, "{out}");
    assert_eq!(handed(&f, second), ["inbox"]);
}

/// 3: a worktree on no branch names no session's work, and its submit is
/// refused, saying so.
#[test]
fn a_detached_worktree_is_refused() {
    let f = setup();
    let a = session(&f, "a", "a");
    f.world.detach(&a);
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", &a, "--no-wait"]);
    assert_eq!(code, CONFLICT, "{out}");
    assert!(out.contains("detached"), "{out}");
}

/// 3, 135: a worktree on no branch has submitted nothing, and collects
/// nothing, its own landing among it.
#[test]
fn inbox_in_a_detached_worktree_delivers_nothing() {
    let f = setup();
    let a = session(&f, "a", "a");
    let n = submit(&f, &a);
    decide(&f, n);
    f.world.detach(&a);
    let (code, out) = inbox(&f, &a, &[]);
    assert_eq!((code, out.as_str()), (0, ""));
    assert!(handed(&f, n).is_empty());
}

/// 51, 104; outcome-deliveries spec, "A day's span": on a host whose span
/// is a day, a landing's delivery expires a day after it is recorded
/// (weir 104; record 0102, D9).
#[test]
fn a_hosts_span_of_a_day_owes_a_landing_for_a_day() {
    owing_for(Duration::from_secs(24 * 60 * 60));
    let f = setup();
    let a = session(&f, "a", "a");
    let n = submit(&f, &a);
    let d = decide(&f, n)["delivery"].clone();
    let at = |field: &str| {
        chrono::DateTime::parse_from_rfc3339(d[field].as_str().unwrap_or_default())
            .unwrap_or_else(|e| panic!("{field}: {e}: {d}"))
    };
    assert_eq!(
        at("expires_at") - at("recorded_at"),
        chrono::Duration::hours(24),
        "{d}"
    );
}

/// 51, 104; inbox spec, "An expired outcome says so": on a host whose
/// span is a second, a blame no carrier handed over expires still owed,
/// and `sluice inbox` in its worktree hands it over all the same, its
/// last line saying when it expired and naming its key (weir 104, 105;
/// record 0102, D8).
#[test]
fn inbox_says_an_expired_outcome_is_still_owed() {
    owing_for(Duration::from_secs(1));
    let f = setup();
    let b = session(&f, "b", "broken");
    let n = submit(&f, &b);
    assert_eq!(decide(&f, n)["state"], "failed");
    f.world.advance(chrono::Duration::seconds(2));
    let record = status(&f, n);
    assert_eq!(record["delivery"]["state"], "expired", "{record}");
    let expires_at = record["delivery"]["expires_at"].as_str().unwrap();
    let (code, said) = inbox(&f, &b, &[]);
    assert_eq!(code, 0, "{said}");
    assert!(
        said.lines()
            .next()
            .unwrap_or_default()
            .contains(&format!("#{n} failed")),
        "{said}"
    );
    assert_eq!(
        said.lines().last(),
        Some(
            format!(
                "Expired at {expires_at}, still owed. Confirm it once you have acted on it: \
                 sluice confirm {n}:failed"
            )
            .as_str()
        ),
        "{said}"
    );
    assert_eq!(handed(&f, n), ["inbox"]);
    assert_eq!(status(&f, n)["delivery"]["state"], "expired");
}

/// 51, S77; owed-outcomes spec, "An expired blame of a removed worktree"
/// and "One owed landing": a branch's status shows, after its queue, what
/// its readers are owed. On a host whose span is a second, a blame the
/// round handed to its pane, of a worktree removed before it confirmed
/// anything, is shown under its reader marked `never confirmed`, expired,
/// with its hand-over by `notice`; under `--json`, a landing handed over
/// by `inbox` to a worktree that confirmed an earlier outcome is its
/// reader's one delivery, owed, and the reader known; nothing owed shows
/// no section (weir 104 to 106; record 0102).
#[test]
fn status_shows_an_expired_blame_with_its_reader_never_confirmed() {
    owing_for(Duration::from_secs(1));
    let f = setup();
    let b = in_a_pane(&f, "b", "broken");
    let n = submit(&f, &b);
    assert_eq!(decide(&f, n)["state"], "failed");
    assert_eq!(handed(&f, n), ["notice"]);
    f.world.remove_worktree(&b);
    f.world.advance(chrono::Duration::seconds(2));
    let (code, said) = sluice(&f, &["status", BRANCH]);
    assert_eq!(code, 0, "{said}");
    let owed = said
        .split_once("OWED\n")
        .unwrap_or_else(|| panic!("no OWED section: {said}"))
        .1;
    let mut lines = owed.lines();
    assert_eq!(lines.next(), Some("  unit/b · never confirmed"), "{said}");
    let row = lines.next().unwrap_or_default();
    let expires_at = status(&f, n)["delivery"]["expires_at"]
        .as_str()
        .unwrap()
        .to_string();
    for word in [
        format!("{n}:failed"),
        format!("#{n}"),
        format!("expired at {expires_at}"),
        "notice at".to_string(),
    ] {
        assert!(row.contains(&word), "{word} not in {said}");
    }
    assert_eq!(status(&f, n)["delivery"]["state"], "expired");

    // One owed landing, to a reader that confirmed an earlier outcome.
    owing_for(sluice_core::EXPIRE_AFTER);
    let f = setup();
    let a = session(&f, "a", "a");
    let first = submit(&f, &a);
    decide(&f, first);
    let (code, out) = confirm(&f, &a, &[&format!("{first}:landed")]);
    assert_eq!(code, 0, "{out}");
    let (_, out) = sluice(&f, &["status", BRANCH]);
    assert!(!out.contains("OWED"), "{out}");
    let (_, out) = sluice(&f, &["status", BRANCH, "--json"]);
    assert_eq!(json(&out)["owed"], serde_json::json!([]), "{out}");

    let head = f.world.work(&f.world.tip(BRANCH), &[("src/a", Some("a2"))]);
    f.world.session(&a, &a, &head);
    f.world.not_delivering(&a);
    let n = submit(&f, &a);
    decide(&f, n);
    let (code, out) = inbox(&f, &a, &[]);
    assert_eq!(code, 0, "{out}");
    for all in [false, true] {
        let mut args = vec!["status", BRANCH, "--json"];
        if all {
            args.push("--all");
        }
        let (_, out) = sluice(&f, &args);
        let v = json(&out);
        let owed = v["owed"].as_array().unwrap();
        assert_eq!(owed.len(), 1, "{v}");
        assert_eq!(owed[0]["reader"], "unit/a", "{v}");
        assert_eq!(owed[0]["known"], true, "{v}");
        let d = &owed[0]["deliveries"];
        assert_eq!(d.as_array().unwrap().len(), 1, "{v}");
        assert_eq!(d[0]["key"], format!("{n}:landed").as_str(), "{v}");
        assert_eq!(d[0]["id"], n, "{v}");
        assert_eq!(d[0]["state"], "owed", "{v}");
        assert!(d[0]["expires_at"].is_string(), "{v}");
        assert_eq!(d[0]["handed"][0]["by"], "inbox", "{v}");
    }
}

/// `sluice inbox --json` with `args` in `worktree`, which hands over one
/// outcome: that outcome, holding the fields every element holds whatever
/// its state (the contract a caller reads, which only gains fields).
fn collected_one(f: &Fakes, worktree: &str, args: &[&str]) -> Value {
    let (code, out) = inbox(f, worktree, args);
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    assert!(v.get("unread").is_none(), "{v}");
    let outcomes = v["outcomes"].as_array().unwrap();
    assert_eq!(outcomes.len(), 1, "{v}");
    let e = outcomes[0].clone();
    assert!(e["id"].is_u64(), "{e}");
    assert_eq!(e["branch"], BRANCH, "{e}");
    assert!(e.get("delivered").is_none(), "{e}");
    let delivery = &e["delivery"];
    assert_eq!(
        delivery["key"],
        format!("{}:{}", e["id"], e["state"].as_str().unwrap()).as_str(),
        "{e}"
    );
    assert_eq!(delivery["reader"], e["from"]["worktree"], "{e}");
    let handed = delivery["handed"].as_array().unwrap();
    assert_eq!(handed.last().unwrap()["by"], "inbox", "{e}");
    let notice = e["notice"].as_str().unwrap();
    assert!(
        notice.starts_with(&format!("[sluice] #{} ", e["id"])),
        "{e}"
    );
    assert_eq!(notice.lines().count(), 1, "{e}");
    e
}

/// 51, 135; inbox spec, "The JSON document is a contract": a landing, a
/// blame and a conflict, each collected as its record with `notice`, and
/// by its state `landed_as`; `result` and `failed[].task`;
/// `conflict.paths` and `conflict.with[].id`.
#[test]
fn inbox_json_holds_the_contracts_fields_for_each_outcome() {
    let f = setup();
    let a = session(&f, "a", "a");
    let b = session(&f, "b", "broken");
    let d = session(&f, "d", "same");
    let c = "unit/c".to_string();
    let head = f
        .world
        .work(&f.world.tip(BRANCH), &[("src/same", Some("c"))]);
    f.world.session(&c, &c, &head);
    f.world.not_delivering(&c);
    let ids: Vec<u64> = [&a, &b, &d, &c].iter().map(|w| submit(&f, w)).collect();
    let (na, nb, nd, nc) = (ids[0], ids[1], ids[2], ids[3]);
    for n in &ids {
        decide(&f, *n);
    }

    let e = collected_one(&f, &a, &["--json"]);
    assert_eq!(
        (e["id"].as_u64(), &e["state"]),
        (Some(na), &"landed".into())
    );
    assert_eq!(e["landed_as"], status(&f, na)["landed_as"], "{e}");
    assert!(e["notice"].as_str().unwrap().contains("landed"), "{e}");

    let e = collected_one(&f, &b, &["--json"]);
    assert_eq!(
        (e["id"].as_u64(), &e["state"]),
        (Some(nb), &"failed".into())
    );
    assert_eq!(e["result"], "failed", "{e}");
    assert_eq!(e["failed"][0]["task"], "src:test", "{e}");
    assert!(e["notice"].as_str().unwrap().contains("failed"), "{e}");

    let e = collected_one(&f, &c, &["--json"]);
    assert_eq!(
        (e["id"].as_u64(), &e["state"]),
        (Some(nc), &"conflict".into())
    );
    assert_eq!(
        e["conflict"]["paths"],
        serde_json::json!(["src/same"]),
        "{e}"
    );
    assert_eq!(e["conflict"]["with"][0]["id"], nd, "{e}");
    assert!(e["notice"].as_str().unwrap().contains("conflicts"), "{e}");
}

/// 51, 135; inbox spec, "The round does not push what the command
/// delivered": a session whose push was refused collects its outcome by
/// `sluice inbox`, and no later round pushes it; its delivery holds that
/// one hand-over (record 0102).
#[test]
fn inbox_delivers_what_a_refused_push_left_and_no_round_pushes_it() {
    let f = setup();
    let a = session(&f, "a", "a");
    let n = submit(&f, &a);
    let v = decide(&f, n);
    assert_eq!(v["state"], "landed", "{v}");
    assert!(handed(&f, n).is_empty(), "{v}");
    let refused = f.world.undelivered().len();
    assert_eq!(refused, 1, "the refused push");
    let (code, out) = inbox(&f, &a, &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with(&format!("[sluice] #{n} landed")), "{out}");
    for _ in 0..2 {
        sluice(&f, &["step", BRANCH]);
    }
    assert_eq!(f.world.undelivered().len(), refused, "no round pushed it");
    assert!(f.world.delivered().is_empty());
    assert_eq!(handed(&f, n), ["inbox"]);
}

/// 51, 135; inbox spec, "A caller that never waits collects a failure" and
/// "A caller that never waits collects a landing": a caller in no pane
/// that never waits collects its blame by `sluice inbox --json` twice,
/// owed each time, then `--confirm` confirms it, and the next collect
/// gives nothing; its push is tried once whatever rounds follow; a landing
/// the same (record 0102).
#[test]
fn a_caller_that_never_waits_collects_a_failure_and_a_landing_by_inbox() {
    let f = setup();
    let none = |w: &str| {
        let (code, out) = inbox(&f, w, &["--json"]);
        assert_eq!(code, 0, "{out}");
        assert_eq!(json(&out), serde_json::json!({ "outcomes": [] }));
    };
    let tries = |n: u64| {
        f.world
            .undelivered()
            .iter()
            .filter(|x| x.submission == n)
            .count()
    };

    let b = session(&f, "b", "broken");
    let n = submit(&f, &b);
    assert!(status(&f, n)["from"].get("session").is_none());
    assert_eq!(decide(&f, n)["state"], "failed");
    assert_eq!(tries(n), 1);
    for _ in 0..2 {
        sluice(&f, &["step", BRANCH]);
    }
    assert_eq!(tries(n), 1, "no further attempt");
    for _ in 0..2 {
        let e = collected_one(&f, &b, &["--json"]);
        assert_eq!(e["id"], n, "{e}");
        assert_eq!(e["state"], "failed", "{e}");
        assert_eq!(e["failed"][0]["task"], "src:test", "{e}");
        assert_eq!(e["delivery"]["state"], "owed", "{e}");
        assert_eq!(e["delivery"]["confirmed"], Value::Null, "{e}");
    }
    let e = collected_one(&f, &b, &["--confirm", "--json"]);
    assert_eq!(e["delivery"]["state"], "done", "{e}");
    assert!(e["delivery"]["confirmed"].is_string(), "{e}");
    none(&b);

    let a = session(&f, "a", "a");
    let n = submit(&f, &a);
    assert_eq!(decide(&f, n)["state"], "landed");
    let e = collected_one(&f, &a, &["--json"]);
    assert_eq!(e["state"], "landed", "{e}");
    assert_eq!(e["landed_as"], f.world.tip(BRANCH).0.as_str(), "{e}");
    assert_eq!(e["delivery"]["state"], "owed", "{e}");
    let e = collected_one(&f, &a, &["--confirm", "--json"]);
    assert_eq!(e["delivery"]["state"], "done", "{e}");
    none(&a);
}

/// 124: `bolt/w`, a branch sluice lands on, with one commit writing
/// `src/<file>` off `bolt/x`: its head.
fn bolt_w(f: &Fakes, file: &str, content: &str) -> sluice_seams::Rev {
    let head = f.world.work(
        &f.world.tip(BRANCH),
        &[(&format!("src/{file}"), Some(content))],
    );
    f.world.set_branch("bolt/w", &head);
    head
}

/// `bolt/x`'s submissions of `bolt/w` by name.
fn by_name(f: &Fakes) -> Vec<Value> {
    let (_, out) = sluice(f, &["status", BRANCH, "--json", "--all"]);
    json(&out)["submissions"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|x| x["from"]["head"] == "bolt/w" && x["from"]["worktree"] == "bolt/w")
        .collect()
}

/// 124: a branch submitted by name, wherever the command runs, answers as
/// any submit that does not wait, its head the branch's revision, and its
/// record the branch as its agent, its work and its head, and this host,
/// with no path and no session (design D1, D2, D3; record 0090; rule
/// core.50).
#[test]
fn a_branch_by_name_is_its_revision_and_no_worktrees() {
    let f = setup();
    let head = bolt_w(&f, "w", "w");
    let (code, out) = sluice(
        &f,
        &["submit", BRANCH, "--head", "bolt/w", "--no-wait", "--json"],
    );
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    let n = v["id"].as_u64().unwrap();
    assert_eq!(
        v,
        serde_json::json!({
            "id": n, "branch": BRANCH, "ahead": [],
            "wait": format!("sluice wait {n} --branch {BRANCH}"),
        })
    );
    let record = status(&f, n);
    assert_eq!(record["head"], head.0.as_str(), "{record}");
    let from = &record["from"];
    assert_eq!(from["worktree"], "bolt/w", "{from}");
    assert_eq!(from["head"], "bolt/w", "{from}");
    assert_eq!(from["agent"], "bolt/w", "{from}");
    assert!(from.get("path").is_none(), "{from}");
    assert!(from.get("session").is_none(), "{from}");
}

/// 64, 67: a forecast of `bolt/w` and its submit by name read one head
/// and one work: the forecast's `head` is the revision the submit then
/// sends, and it leaves out the earlier submission by name, which the
/// submit withdraws as a resubmission.
#[test]
fn the_forecast_names_what_the_submit_sends() {
    let f = setup();
    bolt_w(&f, "w", "w");
    let (_, out) = sluice(
        &f,
        &["submit", BRANCH, "--head", "bolt/w", "--no-wait", "--json"],
    );
    let first = json(&out)["id"].as_u64().unwrap();
    let fix = f
        .world
        .work(&f.world.tip("bolt/w"), &[("src/w", Some("fixed"))]);
    f.world.set_branch("bolt/w", &fix);
    let (code, out) = sluice(
        &f,
        &["submit", BRANCH, "--check", "--head", "bolt/w", "--json"],
    );
    assert_eq!(code, 0, "{out}");
    let forecast = json(&out);
    assert_eq!(forecast["head"], fix.0.as_str(), "{forecast}");
    assert_eq!(forecast["ahead"], serde_json::json!([]), "{forecast}");
    let (code, out) = sluice(
        &f,
        &["submit", BRANCH, "--head", "bolt/w", "--no-wait", "--json"],
    );
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    assert_eq!(v["ahead"], serde_json::json!([]), "{v}");
    let n = v["id"].as_u64().unwrap();
    assert_eq!(status(&f, n)["head"], forecast["head"]);
    let withdrawn = status(&f, first);
    assert_eq!(withdrawn["state"], "withdrawn", "{withdrawn}");
    assert_eq!(withdrawn["withdrawn"]["by"], "resubmission", "{withdrawn}");
}

/// D9, 135: a submit by name that waits runs the rounds while it waits,
/// says `submission <n> to <target> from <branch>`, reports the landing
/// and exits 0. It hands over nothing: no notice is tried, and the
/// submission gives no `delivery`.
#[test]
fn a_waiting_submit_by_name_lands() {
    let f = setup();
    bolt_w(&f, "w", "w");
    let (code, said) = sluice(&f, &["submit", BRANCH, "--head", "bolt/w"]);
    assert_eq!(code, LANDED, "{said}");
    let n: u64 = said
        .lines()
        .next()
        .unwrap_or_default()
        .strip_prefix("submission ")
        .and_then(|l| l.strip_suffix(&format!(" to {BRANCH} from bolt/w · nothing queued ahead")))
        .unwrap_or_else(|| panic!("{said}"))
        .parse()
        .unwrap();
    let record = status(&f, n);
    assert_eq!(record["state"], "landed", "{record}");
    let landed = record["landed_as"].as_str().unwrap();
    assert!(
        said.contains(&format!("passed · landed as {}", &landed[..7])),
        "{said}"
    );
    assert_eq!(f.world.tip(BRANCH).0, landed);
    assert!(record.get("delivery").is_none(), "{record}");
    assert!(f.world.delivered().is_empty() && f.world.undelivered().is_empty());
}

/// 26, core.43: a submit by name with `--subject` lands with the subject
/// as its landing commit's first line, never the branch's own commit
/// message; the record keeps the subject.
#[test]
fn a_subject_names_the_landing_by_name() {
    let f = setup();
    bolt_w(&f, "w", "w");
    let (code, out) = sluice(
        &f,
        &[
            "submit",
            BRANCH,
            "--head",
            "bolt/w",
            "--subject",
            "The w work",
        ],
    );
    assert_eq!(code, LANDED, "{out}");
    let record = status(&f, 1);
    assert_eq!(record["subject"], "The w work", "{record}");
    let landed = sluice_seams::Rev::new(record["landed_as"].as_str().unwrap());
    let first = f.world.message(&landed);
    assert_eq!(
        first.lines().next(),
        Some("chore(land): #1 The w work"),
        "{first}"
    );
}

/// 124, D10: a check failing on `bolt/w`'s work: the waiting submit by name
/// exits 1 and names a fix landed on `bolt/w`, then the submit by name
/// again, never an agent working in it; `sluice status <n>` names the same.
#[test]
fn a_failure_by_name_names_its_way_on() {
    let f = setup();
    bolt_w(&f, "broken", "broken");
    let (code, said) = sluice(&f, &["submit", BRANCH, "--head", "bolt/w"]);
    assert_eq!(code, FAILED, "{said}");
    let way = format!(
        "once a fix lands on bolt/w, sluice submit {BRANCH} --head bolt/w submits it again"
    );
    assert!(said.contains(&way), "{said}");
    assert!(!said.contains("agent working"), "{said}");
    let n = by_name(&f)[0]["id"].as_u64().unwrap();
    let (code, said) = sluice(&f, &["status", &n.to_string()]);
    assert_eq!(code, 0, "{said}");
    assert!(said.contains(&way), "{said}");
    assert!(!said.contains("agent working"), "{said}");
}

/// 124, D10: `bolt/w`, cut before a landing on `bolt/x` that changed the
/// same file: the waiting submit by name exits 2, and its way on merges
/// `bolt/x` at its revision before the submit by name again; its status
/// gives the inbox's notice and its `conflict.paths`, and the session's
/// landing its notice too.
#[test]
fn a_conflict_by_name_names_its_way_on() {
    let f = setup();
    bolt_w(&f, "same", "w");
    let a = session(&f, "a", "same");
    let (code, out) = sluice(&f, &["submit", BRANCH, "--worktree", &a]);
    assert_eq!(code, LANDED, "{out}");
    let tip = f.world.tip(BRANCH);
    let (code, said) = sluice(&f, &["submit", BRANCH, "--head", "bolt/w"]);
    assert_eq!(code, CONFLICT, "{said}");
    assert!(said.contains("conflict: src/same"), "{said}");
    let way = format!(
        "once a fix that merges {BRANCH} at {} lands on bolt/w, sluice submit {BRANCH} --head \
         bolt/w submits it again",
        &tip.0[..7]
    );
    assert!(said.contains(&way), "{said}");
    assert!(!said.contains("agent working"), "{said}");
    let n = by_name(&f)[0]["id"].as_u64().unwrap();
    let (_, said) = sluice(&f, &["status", &n.to_string()]);
    assert!(said.contains(&way), "{said}");
    let e = status(&f, n);
    assert_eq!(
        e["conflict"]["paths"],
        serde_json::json!(["src/same"]),
        "{e}"
    );
    let notice = e["notice"]
        .as_str()
        .unwrap_or_else(|| panic!("no notice: {e}"));
    assert!(
        notice.starts_with(&format!("[sluice] #{n} conflicts on {BRANCH}")),
        "{e}"
    );
    assert!(notice.contains("in src/same"), "{e}");
    let landed = status(&f, 1);
    assert!(
        landed["notice"]
            .as_str()
            .is_some_and(|w| w.starts_with(&format!("[sluice] #1 landed on {BRANCH} as "))),
        "{landed}"
    );
}

/// 51, 124, D8: `bolt/w` by name, blamed: queued, it has no notice;
/// decided, a branch's status and its own give it the inbox's notice, its
/// `failed` and the revision of `bolt/x` it was tested on; it gives no
/// `delivery`, no notice is tried, no inbox takes it, and `sluice confirm`
/// finds nothing owed under its key, as its reader or as the operator
/// (record 0102; rule core.50).
#[test]
fn a_failure_by_name_is_told_by_status_and_owed_to_no_one() {
    let f = setup();
    bolt_w(&f, "broken", "broken");
    let tip = f.world.tip(BRANCH);
    let (code, out) = sluice(
        &f,
        &["submit", BRANCH, "--head", "bolt/w", "--no-wait", "--json"],
    );
    assert_eq!(code, 0, "{out}");
    let n = json(&out)["id"].as_u64().unwrap();
    let queued = status(&f, n);
    assert!(queued.get("notice").is_none(), "{queued}");
    decide(&f, n);
    let notice = format!("[sluice] #{n} failed on {BRANCH}: src:test. sluice wait {n} shows why.");
    let e = by_name(&f).into_iter().find(|x| x["id"] == n).unwrap();
    assert_eq!(e["state"], "failed", "{e}");
    assert_eq!(e["notice"], notice.as_str(), "{e}");
    assert_eq!(e["failed"][0]["task"], "src:test", "{e}");
    assert_eq!(e["tested"]["on"], tip.0.as_str(), "{e}");
    let one = status(&f, n);
    assert_eq!(one["notice"], notice.as_str(), "{one}");
    assert!(one.get("delivery").is_none(), "{one}");
    assert!(f.world.delivered().is_empty() && f.world.undelivered().is_empty());
    // A worktree on bolt/w.
    f.world.session("bolt/w", "bolt/w", &f.world.tip("bolt/w"));
    let (_, out) = inbox(&f, "bolt/w", &["--json"]);
    assert_eq!(json(&out), serde_json::json!({ "outcomes": [] }));
    let key = format!("{n}:failed");
    for (worktree, args) in [
        (Some("bolt/w"), vec![key.as_str()]),
        (None, vec![key.as_str(), "--as", "operator"]),
    ] {
        let (code, out) = match worktree {
            Some(w) => confirm(&f, w, &args),
            None => sluice(&f, &[&["confirm"][..], &args].concat()),
        };
        assert_eq!(code, FAILED, "{out}");
        assert!(
            out.contains(&format!("no outcome is owed under {key}")),
            "{out}"
        );
    }
}
