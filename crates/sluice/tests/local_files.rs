//! The `local files` profile on a real filesystem: section 6's acceptance
//! tests in `docs/plans/phase-1-integrations.md`. It needs no tool, so it
//! is in the fast tier.

mod support;

use std::collections::BTreeMap;
use std::process::Command;
use std::time::{Duration, Instant};

use sluice::profiles::local_files::LocalFiles;
use sluice_fakes::Fakes;
use sluice_seams::{Changes, State, TaskLog, TaskProgress, TaskState, Written, KEPT_LOGS};
use support::Scratch;

fn changes(put: &[(&str, &str)], append: &[(&str, &str)]) -> Changes {
    let m = |xs: &[(&str, &str)]| -> BTreeMap<String, Vec<u8>> {
        xs.iter()
            .map(|(p, b)| (p.to_string(), b.as_bytes().to_vec()))
            .collect()
    };
    Changes {
        put: m(put),
        append: m(append),
        ..Changes::default()
    }
}

/// Waits for the lock: a child another test spawns holds a copy of the
/// lock file until it execs, so a released lock may still read as held.
fn write(s: &LocalFiles, branch: &str, c: Changes) -> Written {
    let _g = s.wait_lock(branch).unwrap();
    let at = s.read(branch).unwrap().version;
    s.write(branch, at, c).unwrap()
}

/// Held by a child process of this test binary until it is killed.
#[test]
fn hold_the_lock_until_killed() {
    let (Ok(common), Ok(branch)) = (
        std::env::var("SLUICE_TEST_COMMON"),
        std::env::var("SLUICE_TEST_BRANCH"),
    ) else {
        return;
    };
    let s = LocalFiles::new(std::path::Path::new(&common));
    let _g = s.lock(&branch).unwrap().expect("the holder takes the lock");
    std::fs::write(std::path::Path::new(&common).join("held"), "").unwrap();
    std::thread::sleep(Duration::from_secs(60));
}

/// 1, 58, core.14: one step at a time; a holder that dies frees the lock.
#[test]
fn the_lock_is_one_holder_and_dies_with_it() {
    let dir = Scratch::new("lock");
    let s = LocalFiles::new(dir.path());
    let held = s.lock("bolt/x").unwrap().expect("free");
    assert!(LocalFiles::new(dir.path())
        .lock("bolt/x")
        .unwrap()
        .is_none());
    drop(held);
    drop(s.lock("bolt/x").unwrap().expect("free again"));

    let mut child = support::isolated(&mut Command::new(std::env::current_exe().unwrap()))
        .args(["--exact", "hold_the_lock_until_killed", "--nocapture"])
        .env("SLUICE_TEST_COMMON", dir.path())
        .env("SLUICE_TEST_BRANCH", "bolt/x")
        .spawn()
        .unwrap();
    let start = Instant::now();
    while !dir.path().join("held").exists() {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the holder never took the lock"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(s.lock("bolt/x").unwrap().is_none());
    child.kill().unwrap(); // SIGKILL
    child.wait().unwrap();
    assert!(s.lock("bolt/x").unwrap().is_some());
}

/// A write outside a step waits for the step holding the lock, and takes
/// it once that step lets go, whether it exits or dies.
#[test]
fn a_waiting_lock_is_taken_when_the_holder_lets_go() {
    let dir = Scratch::new("wait-lock");
    let mut child = support::isolated(&mut Command::new(std::env::current_exe().unwrap()))
        .args(["--exact", "hold_the_lock_until_killed", "--nocapture"])
        .env("SLUICE_TEST_COMMON", dir.path())
        .env("SLUICE_TEST_BRANCH", "bolt/x")
        .spawn()
        .unwrap();
    let start = Instant::now();
    while !dir.path().join("held").exists() {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(20));
    }
    let path = dir.path().to_path_buf();
    let waiter = std::thread::spawn(move || {
        let s = LocalFiles::new(&path);
        let _g = s.wait_lock("bolt/x").unwrap();
        let at = s.read("bolt/x").unwrap().version;
        s.write("bolt/x", at, changes(&[("branch.json", "{}\n")], &[]))
            .unwrap()
    });
    std::thread::sleep(Duration::from_millis(300));
    assert!(!waiter.is_finished(), "took a lock another process holds");
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(waiter.join().unwrap(), Written::Written(1));
}

/// 2, 62, 58: two writes from one version; one is written, one refused.
/// 20, 26: a failed candidate's task outputs are kept beside the branch's
/// records, never among them, readable as files; only the last
/// `KEPT_LOGS` candidates of a branch keep theirs.
#[test]
fn logs_are_kept_beside_the_records_and_only_the_last_ones() {
    let dir = Scratch::new("logs");
    let s = LocalFiles::new(dir.path());
    let log = |task: &str, text: &str| TaskLog {
        task: task.into(),
        text: text.into(),
    };
    let first = s
        .keep_logs(
            "bolt/x",
            "c1",
            &[log("a:test", "one\n"), log("b:test", "two\n")],
        )
        .unwrap();
    assert_eq!(std::fs::read_to_string(&first[0].text).unwrap(), "one\n");
    assert_eq!(std::fs::read_to_string(&first[1].text).unwrap(), "two\n");
    {
        let _g = s.wait_lock("bolt/x").unwrap();
        s.write("bolt/x", 0, changes(&[("branch.json", "{}\n")], &[]))
            .unwrap();
    }
    assert_eq!(s.read("bolt/x").unwrap().files.len(), 1);
    assert_eq!(s.branches().unwrap(), ["bolt/x"]);
    for n in 2..=KEPT_LOGS + 1 {
        std::thread::sleep(Duration::from_millis(15));
        s.keep_logs("bolt/x", &format!("c{n}"), &[log("a:test", "x\n")])
            .unwrap();
    }
    assert!(!std::path::Path::new(&first[0].text).exists());
    let kept = std::fs::read_dir(dir.path().join("sluice/.logs/bolt/x"))
        .unwrap()
        .count();
    assert_eq!(kept, KEPT_LOGS);
}

/// ADR 0028: a run's tasks are kept beside the branch's records, under
/// its lock, replaced whole and removed, and no read of the branch nor
/// its version sees them.
#[test]
fn tasks_are_kept_beside_the_records_and_move_no_version() {
    let dir = Scratch::new("progress");
    let s = LocalFiles::new(dir.path());
    let t = |target: &str, state: TaskState| TaskProgress {
        target: target.into(),
        state,
        started: None,
        ended: None,
        exit: None,
    };
    let one = [t("a:test", TaskState::Running)];
    assert!(s.put_progress("bolt/x", "c1", Some(&one)).is_err());
    write(&s, "bolt/x", changes(&[("branch.json", "{}\n")], &[]));
    {
        let _g = s.wait_lock("bolt/x").unwrap();
        s.put_progress("bolt/x", "c1", Some(&one)).unwrap();
        let two = [
            t("a:test", TaskState::Passed),
            t("b:test", TaskState::Waiting),
        ];
        s.put_progress("bolt/x", "c2", Some(&two)).unwrap();
        s.put_progress("bolt/x", "c2", Some(&two[..1])).unwrap();
        let kept = s.progress("bolt/x").unwrap();
        assert_eq!(kept["c1"], one);
        assert_eq!(kept["c2"], two[..1]);
        s.put_progress("bolt/x", "c1", None).unwrap();
        s.put_progress("bolt/x", "c9", None).unwrap();
    }
    assert_eq!(
        s.progress("bolt/x").unwrap().keys().collect::<Vec<_>>(),
        ["c2"]
    );
    let r = s.read("bolt/x").unwrap();
    assert_eq!(r.version, 1);
    assert_eq!(r.files.keys().collect::<Vec<_>>(), ["branch.json"]);
    assert_eq!(s.branches().unwrap(), ["bolt/x"]);
    let file = dir.path().join("sluice/.progress/bolt/x/c2.json");
    let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(doc["schema"], "sluice/progress@1");
    assert_eq!(doc["tasks"][0]["state"], "passed");
    assert!(s.progress("main").unwrap().is_empty());
}

#[test]
fn two_writes_from_one_version() {
    let dir = Scratch::new("version");
    let s = LocalFiles::new(dir.path());
    let _g = s.wait_lock("bolt/x").unwrap();
    assert_eq!(
        s.write("bolt/x", 0, changes(&[("branch.json", "{}\n")], &[]))
            .unwrap(),
        Written::Written(1)
    );
    assert_eq!(
        s.write("bolt/x", 0, changes(&[("branch.json", "{\"x\":1}\n")], &[]))
            .unwrap(),
        Written::Refused
    );
    assert_eq!(s.read("bolt/x").unwrap().files["branch.json"], b"{}\n");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("sluice/bolt/x/version")).unwrap(),
        "1\n"
    );
}

/// A write needs the branch's lock, held by this process.
#[test]
fn a_write_without_the_lock_is_an_error() {
    let dir = Scratch::new("unlocked");
    let s = LocalFiles::new(dir.path());
    assert!(s
        .write("bolt/x", 0, changes(&[("branch.json", "{}\n")], &[]))
        .is_err());
}

/// 3, 4, 63, 51: a crash after the pending write, part way through
/// applying it, or before the version: a reader with no lock sees the
/// whole write, the next lock finishes it, and no log line is doubled.
#[test]
fn a_crash_mid_write_reads_whole_and_finishes_once() {
    let dir = Scratch::new("crash");
    let s = LocalFiles::new(dir.path());
    write(
        &s,
        "bolt/x",
        changes(
            &[("branch.json", "{\"v\":1}\n")],
            &[("log.jsonl", "{\"a\":1}\n")],
        ),
    );
    let branch = dir.path().join("sluice/bolt/x");
    let pending = r#"{"version":2,"put":{"branch.json":"{\"v\":2}\n","submissions/3.json":"{\"id\":3}\n"},"append":{"log.jsonl":"{\"a\":2}\n"},"lengths":{"log.jsonl":8}}"#;
    let expect_log = "{\"a\":1}\n{\"a\":2}\n";

    for stage in ["pending written", "applied in part", "applied, version not"] {
        // Back to version 1, then the crash at `stage`.
        std::fs::write(branch.join("branch.json"), "{\"v\":1}\n").unwrap();
        std::fs::write(branch.join("log.jsonl"), "{\"a\":1}\n").unwrap();
        let _ = std::fs::remove_file(branch.join("submissions/3.json"));
        std::fs::write(branch.join("version"), "1\n").unwrap();
        std::fs::write(branch.join(".pending.json"), pending).unwrap();
        if stage != "pending written" {
            std::fs::write(branch.join("branch.json"), "{\"v\":2}\n").unwrap();
            std::fs::write(branch.join("log.jsonl"), expect_log).unwrap();
        }
        if stage == "applied, version not" {
            std::fs::create_dir_all(branch.join("submissions")).unwrap();
            std::fs::write(branch.join("submissions/3.json"), "{\"id\":3}\n").unwrap();
        }

        let read = s.read("bolt/x").unwrap();
        assert_eq!(read.version, 2, "{stage}");
        assert_eq!(read.files["log.jsonl"], expect_log.as_bytes(), "{stage}");
        assert_eq!(read.files["submissions/3.json"], b"{\"id\":3}\n", "{stage}");

        drop(s.wait_lock("bolt/x").unwrap());
        assert!(!branch.join(".pending.json").exists(), "{stage}");
        assert_eq!(
            std::fs::read_to_string(branch.join("version")).unwrap(),
            "2\n",
            "{stage}"
        );
        assert_eq!(
            std::fs::read_to_string(branch.join("log.jsonl")).unwrap(),
            expect_log,
            "{stage}"
        );
        assert_eq!(
            std::fs::read_to_string(branch.join("branch.json")).unwrap(),
            "{\"v\":2}\n",
            "{stage}"
        );
    }
}

/// Record 0122, 63 (design D1): a branch's `home.json` is one of its
/// records, read among them and written with them in one write, a write a
/// crash left part done read whole with it.
#[test]
fn a_branchs_home_is_read_and_written_with_its_records() {
    use sluice_core::line::Line;
    use sluice_core::records::{HomeDoc, HomeState, Moving, HERE, HOME_FILE};
    const SUBMISSION: &str = "{\"schema\":\"sluice/submission@1\",\"id\":1,\"branch\":\"bolt/x\",\
        \"head\":\"h1\",\"from\":{\"agent\":\"a\",\"worktree\":\"unit/a\"},\"submitted\":\"t\",\
        \"after\":[],\"priority\":0,\"attempt\":1,\"state\":\"queued\"}\n";
    let dir = Scratch::new("home");
    let s = LocalFiles::new(dir.path());
    let mut home = HomeDoc::open(HERE, Some(sluice_seams::Rev::new("c43609b")));
    let doc = String::from_utf8(sluice_core::records::to_document(&home)).unwrap();
    write(
        &s,
        "bolt/x",
        changes(
            &[
                ("branch.json", "{\"schema\":\"sluice/branch@1\",\"branch\":\"bolt/x\",\"log\":\"sluice/log@1\"}\n"),
                (HOME_FILE, &doc),
                ("submissions/1.json", SUBMISSION),
            ],
            &[],
        ),
    );
    let read = s.read("bolt/x").unwrap();
    assert_eq!(read.files[HOME_FILE], doc.as_bytes());
    let mut line = Line::load("bolt/x", read).unwrap();
    assert_eq!(line.home.as_ref(), Some(&home));

    // A move written with the rest, in one write.
    home.state = HomeState::Moving;
    home.moving = Some(Moving {
        to: "origin".into(),
        chosen_at: None,
        since: "2026-11-14T10:00:00Z".into(),
    });
    line.home = Some(home.clone());
    line.doc.window = 2;
    let c = line.changes();
    assert!(c.put.contains_key("branch.json") && c.put.contains_key(HOME_FILE));
    let _g = s.wait_lock("bolt/x").unwrap();
    assert_eq!(
        s.write("bolt/x", line.version, c).unwrap(),
        Written::Written(2)
    );
    drop(_g);
    let moved = Line::load("bolt/x", s.read("bolt/x").unwrap()).unwrap();
    assert_eq!(moved.home, Some(home.clone()));
    assert_eq!(moved.doc.window, 2);

    // Stopped after the write was pending: read whole, home and all.
    let branch = dir.path().join("sluice/bolt/x");
    let closed = doc.replace("\"open\"", "\"closed\"");
    let pending = serde_json::json!({
        "version": 3,
        "put": { HOME_FILE: closed, "branch.json": "{\"schema\":\"sluice/branch@1\",\"branch\":\"bolt/x\",\"log\":\"sluice/log@1\",\"window\":3}\n" },
        "append": {},
        "lengths": {},
    });
    std::fs::write(branch.join(".pending.json"), pending.to_string()).unwrap();
    let read = s.read("bolt/x").unwrap();
    assert_eq!(read.version, 3);
    assert_eq!(read.files[HOME_FILE], closed.as_bytes());
    assert_eq!(
        Line::load("bolt/x", read).unwrap().home.map(|h| h.state),
        Some(HomeState::Closed)
    );
}

/// 5, 53: the bytes read back are the fake store's bytes for the same
/// writes, so a reader of one profile's records reads the other's.
#[test]
fn the_records_are_the_fake_stores_records() {
    let dir = Scratch::new("bytes");
    let real = LocalFiles::new(dir.path());
    let fakes = Fakes::new();
    let writes = [
        changes(
            &[
                ("branch.json", "{\"schema\":\"sluice/branch@1\"}\n"),
                ("home.json", "{\"schema\":\"sluice/home@1\"}\n"),
                ("submissions/1.json", "{\"id\":1}\n"),
            ],
            &[("log.jsonl", "{\"event\":\"submitted\"}\n")],
        ),
        changes(
            &[("submissions/2.json", "{\"id\":2}\n")],
            &[("log.jsonl", "{\"event\":\"testing\"}\n")],
        ),
    ];
    for c in writes {
        write(&real, "bolt/x", c.clone());
        let _g = fakes.state.lock("bolt/x").unwrap().unwrap();
        let at = fakes.state.read("bolt/x").unwrap().version;
        fakes.state.write("bolt/x", at, c).unwrap();
    }
    assert_eq!(
        real.read("bolt/x").unwrap(),
        fakes.state.read("bolt/x").unwrap()
    );
}

/// 6: branch names nest as directories; a name that cannot be a branch's,
/// or whose records would lie inside another's, is refused.
#[test]
fn branch_names_round_trip() {
    let dir = Scratch::new("names");
    let s = LocalFiles::new(dir.path());
    for b in ["bolt/x", "main", "a/b/c"] {
        write(&s, b, changes(&[("branch.json", "{}\n")], &[]));
    }
    assert_eq!(s.branches().unwrap(), vec!["a/b/c", "bolt/x", "main"]);
    assert!(s.read("results.jsonl").is_err());
    assert!(s.lock("../x").is_err());
    let _g = s.wait_lock("main/inner").unwrap();
    assert!(s
        .write("main/inner", 0, changes(&[("branch.json", "{}\n")], &[]))
        .is_err());
    // A nested branch's records are not its parent directory's.
    assert!(!s
        .read("bolt/x")
        .unwrap()
        .files
        .keys()
        .any(|k| k.contains("branch.json") && k != "branch.json"));
}

/// The host's count is one for every branch whose records it holds
/// (record 0114).
#[test]
fn ids_count_across_the_hosts_branches() {
    let dir = Scratch::new("ids");
    let s = LocalFiles::new(dir.path());
    assert_eq!(
        (
            s.allocate_id("main").unwrap(),
            s.allocate_id("bolt/x").unwrap()
        ),
        (1, 2)
    );
    assert_eq!(LocalFiles::new(dir.path()).allocate_id("main").unwrap(), 3);
}

/// 69: the results log appends whole lines, never one twice, and drops a
/// line a crash cut short.
#[test]
fn results_append_once_and_whole() {
    let dir = Scratch::new("results");
    let s = LocalFiles::new(dir.path());
    s.record_results(b"{\"run\":1}\n{\"run\":2}\n").unwrap();
    s.record_results(b"{\"run\":2}\n{\"run\":3}\n").unwrap();
    assert_eq!(
        s.results_for("main", &sluice_seams::ResultAsk::every())
            .unwrap(),
        b"{\"run\":1}\n{\"run\":2}\n{\"run\":3}\n"
    );
    let log = dir.path().join("sluice/results.jsonl");
    let mut cut = std::fs::read(&log).unwrap();
    cut.extend_from_slice(b"{\"run\":4");
    std::fs::write(&log, cut).unwrap();
    s.record_results(b"{\"run\":4}\n").unwrap();
    assert_eq!(
        s.results_for("main", &sluice_seams::ResultAsk::every())
            .unwrap(),
        b"{\"run\":1}\n{\"run\":2}\n{\"run\":3}\n{\"run\":4}\n"
    );
}

/// Clauses 130, 131, 63: a submission moved to the finished records
/// leaves every read of the live ones in the same write, is read by its
/// name, and a crash part way through the move reads as the whole move.
#[test]
fn a_finished_submission_moves_out_of_the_live_records_all_or_nothing() {
    let dir = Scratch::new("finished");
    let s = LocalFiles::new(dir.path());
    write(
        &s,
        "bolt/x",
        changes(&[("submissions/3.json", "{\"id\":3}\n")], &[]),
    );
    let mut c = changes(&[(".finished/3.json", "{\"id\":3}\n")], &[]);
    c.remove.insert("submissions/3.json".into());
    write(&s, "bolt/x", c);
    let read = s.read("bolt/x").unwrap();
    assert!(
        read.files.keys().all(|k| !k.contains('3')),
        "{:?}",
        read.files.keys()
    );
    assert_eq!(
        s.read_finished("bolt/x", "3.json").unwrap().unwrap(),
        b"{\"id\":3}\n"
    );
    assert_eq!(s.read_all_finished("bolt/x").unwrap().len(), 1);

    // A crash after the pending write, before any file holds it.
    let branch = dir.path().join("sluice/bolt/x");
    std::fs::write(branch.join("submissions/4.json"), "{\"id\":4}\n").unwrap();
    let at = s.read("bolt/x").unwrap().version;
    let pending = format!(
        r#"{{"version":{},"put":{{".finished/4.json":"{{\"id\":4}}\n"}},"append":{{}},"lengths":{{}},"remove":["submissions/4.json"]}}"#,
        at + 1
    );
    std::fs::write(branch.join(".pending.json"), pending).unwrap();
    let read = s.read("bolt/x").unwrap();
    assert!(!read.files.contains_key("submissions/4.json"));
    assert!(read.files.keys().all(|k| !k.starts_with(".finished")));
    assert_eq!(
        s.read_finished("bolt/x", "4.json").unwrap().unwrap(),
        b"{\"id\":4}\n"
    );
    drop(s.wait_lock("bolt/x").unwrap());
    assert!(!branch.join("submissions/4.json").exists());
    assert!(branch.join(".finished/4.json").exists());
}

/// 51, 130: a write putting a finished record that is there replaces it,
/// read back as written by name and with the rest, and the live index is
/// left as it was.
#[test]
fn a_finished_record_put_again_replaces_it_and_leaves_the_index() {
    let dir = Scratch::new("finished-again");
    let s = LocalFiles::new(dir.path());
    let index = "{\"id\":5,\"state\":\"landed\",\"worktree\":\"unit/a\"}\n";
    write(
        &s,
        "bolt/x",
        changes(
            &[(".finished/5.json", "{\"id\":5}\n")],
            &[("finished.jsonl", index)],
        ),
    );
    let delivered = "{\"id\":5,\"delivered\":\"2026-10-28T09:00:00Z\"}\n";
    write(
        &s,
        "bolt/x",
        changes(
            &[(".finished/5.json", delivered)],
            &[("log.jsonl", "delivered\n")],
        ),
    );
    assert_eq!(
        s.read_finished("bolt/x", "5.json").unwrap().unwrap(),
        delivered.as_bytes()
    );
    let all = s.read_all_finished("bolt/x").unwrap();
    assert_eq!(all.keys().collect::<Vec<_>>(), ["5.json"]);
    assert_eq!(all["5.json"], delivered.as_bytes());
    assert_eq!(
        s.read("bolt/x").unwrap().files["finished.jsonl"],
        index.as_bytes()
    );
}

/// 129: a second read at one version, with no write pending, walks
/// nothing: what is on disk is read again only once the version moved.
#[test]
fn a_second_read_at_one_version_walks_nothing() {
    let dir = Scratch::new("kept-read");
    let s = LocalFiles::new(dir.path());
    write(&s, "main", changes(&[("branch.json", "{\"v\":1}\n")], &[]));
    let first = s.read("main").unwrap();
    // Changed under the version, as no write of sluice's ever does.
    let branch = dir.path().join("sluice/main");
    std::fs::write(branch.join("branch.json"), "{\"v\":\"by hand\"}\n").unwrap();
    assert_eq!(s.read("main").unwrap(), first, "walked again");
    write(&s, "main", changes(&[("log.jsonl", "{}\n")], &[]));
    let moved = s.read("main").unwrap();
    assert_eq!(moved.version, first.version + 1);
    assert_eq!(moved.files["branch.json"], b"{\"v\":\"by hand\"}\n");
}

/// 129, 62: a write from another `LocalFiles` on the same directory, as
/// another process's, is seen at the next read.
#[test]
fn a_write_from_another_process_is_seen_at_the_next_read() {
    let dir = Scratch::new("kept-other");
    let reader = LocalFiles::new(dir.path());
    let writer = LocalFiles::new(dir.path());
    write(
        &writer,
        "main",
        changes(&[("branch.json", "{\"v\":1}\n")], &[]),
    );
    assert_eq!(reader.read("main").unwrap().version, 1);
    assert_eq!(reader.read("main").unwrap().version, 1);
    write(
        &writer,
        "main",
        changes(&[("submissions/1.json", "{\"id\":1}\n")], &[]),
    );
    let read = reader.read("main").unwrap();
    assert_eq!(read.version, 2);
    assert_eq!(read.files["submissions/1.json"], b"{\"id\":1}\n");
}

/// 129, 51: a pending write a stopped writer left is read as whole by a
/// reader that had kept the records before it, at every read while it
/// stands.
#[test]
fn a_stopped_writers_pending_write_is_read_whole_over_what_was_kept() {
    let dir = Scratch::new("kept-pending");
    let s = LocalFiles::new(dir.path());
    write(&s, "main", changes(&[("branch.json", "{\"v\":1}\n")], &[]));
    assert_eq!(s.read("main").unwrap().version, 1);
    let branch = dir.path().join("sluice/main");
    std::fs::write(
        branch.join(".pending.json"),
        r#"{"version":2,"put":{"submissions/2.json":"{\"id\":2}\n"},"append":{},"lengths":{}}"#,
    )
    .unwrap();
    for _ in 0..2 {
        let read = s.read("main").unwrap();
        assert_eq!(read.version, 2);
        assert_eq!(read.files["submissions/2.json"], b"{\"id\":2}\n");
    }
    drop(s.wait_lock("main").unwrap());
    let read = s.read("main").unwrap();
    assert_eq!(read.version, 2);
    assert_eq!(read.files["submissions/2.json"], b"{\"id\":2}\n");
}
