//! Branch protection with the hook installed by `sluice protect`, against
//! the git command: section 2's acceptance tests in
//! `docs/plans/phase-1-integrations.md`. Needs git, so it is in the
//! check-tools tier.

#![cfg(feature = "tools")]

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use sluice::profiles::local_refs::LocalRefs;
use sluice_seams::{Branch, Moved, Rev};
use ssh_key::{Algorithm, PrivateKey};
use support::{Repo, Scratch};

const POLICY: &str = "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\n\
[branches.\"bolt/*\"]\ntrust = [\"open\", \"sandbox\", \"container\"]\n";

fn sluice() -> &'static str {
    env!("CARGO_BIN_EXE_sluice")
}

/// A repository whose policy protects `bolt/*`, with `bolt/x`, a work
/// branch `unit/a` one commit ahead of it, a worktree on `bolt/x`, and
/// the hook installed.
fn protected(name: &str) -> (Repo, PathBuf) {
    protected_with(name, POLICY)
}

/// [`protected`], under `policy`.
fn protected_with(name: &str, policy: &str) -> (Repo, PathBuf) {
    let repo = Repo::new(name);
    repo.write(".config/sluice.toml", policy);
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    repo.git(&["branch", "bolt/x", "main"]);
    repo.branch("unit/a", "main", &[("a.txt", Some("a\n"))]);
    let held = repo.dir().with_extension("bolt");
    let _ = std::fs::remove_dir_all(&held);
    let out = sl(&repo, &["protect"]);
    assert!(out.status.success(), "{}", text(&out));
    repo.git(&["worktree", "add", "-q", held.to_str().unwrap(), "bolt/x"]);
    (repo, held)
}

fn sl(repo: &Repo, args: &[&str]) -> Output {
    support::isolated(&mut Command::new(sluice()))
        .current_dir(repo.dir())
        .args(args)
        .env("XDG_CONFIG_HOME", repo.dir().join(".git/no-config"))
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

/// The host's settings, written where every command the test runs, git's
/// hook among them, reads them; returns their path.
fn host_settings(repo: &Repo, settings: &str) -> PathBuf {
    let path = repo.dir().join(".git/no-config/sluice/config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, settings).unwrap();
    path
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// git in `dir`, as a session runs it.
fn git_in(repo: &Repo, dir: &Path, args: &[&str]) -> Output {
    let mut a = vec!["-C", dir.to_str().unwrap()];
    a.extend(args);
    repo.git_raw(&a)
}

/// The `protected` lines of `bolt/x`'s log: one for each cause, at its
/// first refusal (148).
fn protected_events(repo: &Repo) -> Vec<serde_json::Value> {
    let log = repo.dir().join(".git/sluice/bolt/x/log.jsonl");
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .filter(|v| v["event"] == "protected")
        .collect()
}

/// The causes of refused writes `bolt/x`'s state keeps standing, from its
/// `branch.json`, each with how many times it was refused (148).
fn causes(repo: &Repo) -> Vec<serde_json::Value> {
    let doc = repo.dir().join(".git/sluice/bolt/x/branch.json");
    std::fs::read(doc)
        .map(|b| serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        .ok()
        .and_then(|v| v["refused_writes"].as_array().cloned())
        .unwrap_or_default()
}

/// 1, S1, rule core.4: every way a session's git moves a branch is
/// refused, the branch unmoved, and each refusal reported in its log:
/// each command is a cause of its own, logged once and counted once (148;
/// rule core.47).
#[test]
fn every_way_git_moves_a_protected_branch_is_refused() {
    let (repo, held) = protected("protect-caught");
    let tip = repo.rev("bolt/x");
    let head = repo.rev("unit/a");
    let root = repo.dir().to_path_buf();
    let attempts: Vec<(PathBuf, Vec<&str>)> = vec![
        (held.clone(), vec!["merge", "-q", "unit/a"]),
        (
            held.clone(),
            vec!["merge", "-q", "--no-ff", "-m", "m", "unit/a"],
        ),
        (
            held.clone(),
            vec!["commit", "-q", "--allow-empty", "-m", "on bolt"],
        ),
        (held.clone(), vec!["reset", "-q", "--hard", "unit/a"]),
        (held.clone(), vec!["rebase", "-q", "unit/a"]),
        (root.clone(), vec!["update-ref", "refs/heads/bolt/x", &head]),
        (
            root.clone(),
            vec!["update-ref", "refs/heads/bolt/x", &head, &tip],
        ),
        (root.clone(), vec!["branch", "-f", "bolt/x", "unit/a"]),
        (root.clone(), vec!["fetch", "-q", ".", "unit/a:bolt/x"]),
    ];
    for (n, (dir, args)) in attempts.iter().enumerate() {
        // git refuses to force a checked-out branch itself; these reach
        // the hook once no worktree holds it.
        if args[0] == "branch" {
            repo.git(&["worktree", "remove", "--force", held.to_str().unwrap()]);
        }
        let out = git_in(&repo, dir, args);
        assert_eq!(out.status.code(), Some(128), "git {args:?}: {}", text(&out));
        assert!(
            text(&out)
                .contains("bolt/x is protected by sluice; submit the change to sluice instead"),
            "git {args:?}: {}",
            text(&out)
        );
        assert_eq!(repo.rev("bolt/x"), tip, "git {args:?} moved it");
        // Bring the worktree level again, as the message says.
        if held.exists() {
            let _ = git_in(&repo, &held, &["merge", "--abort"]);
            let _ = git_in(&repo, &held, &["rebase", "--abort"]);
            let out = git_in(&repo, &held, &["reset", "-q", "--hard"]);
            assert!(out.status.success(), "{}", text(&out));
        }
        assert_eq!(protected_events(&repo).len(), n + 1, "git {args:?}");
    }
    let e = &protected_events(&repo)[0];
    assert_eq!(e["ref"], "refs/heads/bolt/x");
    assert_eq!(e["from"], tip.as_str());
    assert!(e["by"].as_str().unwrap().contains("git"), "{e}");
    let causes = causes(&repo);
    assert_eq!(causes.len(), attempts.len(), "{causes:?}");
    assert!(causes.iter().all(|c| c["times"] == 1), "{causes:?}");
}

/// What a session does in its own work, and git's own no-op updates,
/// pass: a worktree on the branch, a reset to where it is, a new branch,
/// commits on a work branch.
#[test]
fn what_moves_no_protected_branch_is_allowed() {
    let (repo, held) = protected("protect-allowed");
    let ok = |dir: &Path, args: &[&str]| {
        let out = git_in(&repo, dir, args);
        assert!(out.status.success(), "git {args:?}: {}", text(&out));
    };
    ok(&held, &["reset", "-q", "--hard"]);
    ok(repo.dir(), &["branch", "bolt/new", "main"]);
    ok(repo.dir(), &["switch", "-q", "unit/a"]);
    ok(
        repo.dir(),
        &["commit", "-q", "--allow-empty", "-m", "unit work"],
    );
    // A branch the policy does not name.
    ok(repo.dir(), &["branch", "-f", "main", "unit/a"]);
    assert!(protected_events(&repo).is_empty());
}

/// 56: a branch whose own revision's policy does not name it is not
/// protected, whatever another branch's policy says.
#[test]
fn the_branchs_own_revision_decides() {
    let (repo, _) = protected("protect-own-revision");
    // bolt/z's revision has no policy at all.
    let bare = repo.branch("bare", "main", &[(".config/sluice.toml", None)]);
    repo.git(&["branch", "bolt/z", &bare]);
    let out = repo.git_raw(&["branch", "-f", "bolt/z", "main"]);
    assert!(out.status.success(), "{}", text(&out));
}

/// A protected repository whose `bolt/x` holds submission #1, from a
/// session on `unit/s`; no worktree holds `bolt/x`.
fn queued(name: &str) -> Repo {
    let (repo, held) = protected(name);
    repo.git(&["worktree", "remove", held.to_str().unwrap()]);
    let session = repo.dir().with_extension("session");
    let _ = std::fs::remove_dir_all(&session);
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "unit/s",
        session.to_str().unwrap(),
        "bolt/x",
    ]);
    std::fs::write(session.join("s.txt"), "s\n").unwrap();
    git_in(&repo, &session, &["add", "-A"]);
    git_in(&repo, &session, &["commit", "-q", "-m", "s"]);
    let out = support::isolated(&mut Command::new(sluice()))
        .current_dir(&session)
        .args(["submit", "bolt/x", "--no-wait"])
        .env("XDG_CONFIG_HOME", repo.dir().join(".git/no-config"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    repo
}

/// A deletion is allowed only when no submission still needs the branch.
#[test]
fn a_branch_with_submissions_in_its_queue_is_not_deleted() {
    let repo = queued("protect-delete");
    let out = repo.git_raw(&["branch", "-D", "bolt/x"]);
    assert_eq!(out.status.code(), Some(128), "{}", text(&out));
    assert!(text(&out).contains("#1"), "{}", text(&out));
    assert!(repo
        .git_raw(&["rev-parse", "--verify", "bolt/x"])
        .status
        .success());

    let out = sl(&repo, &["withdraw", "1", "--as", "operator"]);
    assert!(out.status.success(), "{}", text(&out));
    let out = repo.git_raw(&["branch", "-D", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
}

/// git's `pack-refs`, which `git fetch` runs in its maintenance, writes
/// a protected branch into `packed-refs` and prunes its loose file, its
/// value unchanged: allowed, its queue full or not. A deletion of the
/// loose file alone, or a move or deletion once it is packed, is refused,
/// the deletion tried twice one cause, logged once and counted twice
/// (148).
#[test]
fn packing_a_protected_branch_is_allowed() {
    let repo = queued("protect-pack");
    let tip = repo.rev("bolt/x");
    let refused = |args: &[&str]| {
        let out = repo.git_raw(args);
        assert_eq!(out.status.code(), Some(128), "git {args:?}: {}", text(&out));
        assert_eq!(repo.rev("bolt/x"), tip, "git {args:?} moved it");
    };
    refused(&["update-ref", "-d", "refs/heads/bolt/x", &tip]);
    let events = protected_events(&repo).len();

    let out = repo.git_raw(&["pack-refs", "--all", "--prune"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!repo.dir().join(".git/refs/heads/bolt/x").exists());
    assert_eq!(repo.rev("bolt/x"), tip);
    assert_eq!(protected_events(&repo).len(), events);

    refused(&["update-ref", "refs/heads/bolt/x", "unit/a"]);
    refused(&["update-ref", "-d", "refs/heads/bolt/x", &tip]);
    refused(&["branch", "-D", "bolt/x"]);
    assert_eq!(protected_events(&repo).len(), events + 2);
    let causes = causes(&repo);
    let deleting: Vec<_> = causes
        .iter()
        .filter(|c| c["command"].as_str().unwrap().contains("update-ref -d"))
        .collect();
    assert_eq!(deleting.len(), 1, "{causes:?}");
    assert_eq!(deleting[0]["times"], 2, "{causes:?}");
    assert_eq!(deleting[0]["to"], serde_json::Value::Null, "{causes:?}");
}

/// 3: a hook already there still runs, with the same updates, and its
/// refusal still refuses.
#[test]
fn a_hook_already_there_still_runs() {
    let repo = Repo::new("protect-chain");
    repo.write(".config/sluice.toml", POLICY);
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    let hooks = repo.dir().join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let theirs = "#!/bin/sh\ninput=$(cat)\necho \"$input\" >> \"$(dirname \"$0\")/../theirs.log\"\n\
                  [ \"$1\" = prepared ] && echo \"$input\" | grep -q refs/heads/refused && exit 1\nexit 0\n";
    std::fs::write(hooks.join("reference-transaction"), theirs).unwrap();
    std::fs::set_permissions(
        hooks.join("reference-transaction"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    assert!(sl(&repo, &["protect"]).status.success());
    assert_eq!(
        std::fs::read_to_string(hooks.join("reference-transaction.pre-sluice")).unwrap(),
        theirs
    );
    repo.git(&["branch", "fine", "main"]);
    let log = std::fs::read_to_string(repo.dir().join(".git/theirs.log")).unwrap();
    assert!(log.contains("refs/heads/fine"), "{log}");
    let out = repo.git_raw(&["branch", "refused", "main"]);
    assert_eq!(out.status.code(), Some(128), "{}", text(&out));

    // Removing sluice's puts theirs back.
    let out = sl(&repo, &["protect", "--remove", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap(),
        serde_json::json!({"protection": "removed", "changed": true})
    );
    assert_eq!(
        std::fs::read_to_string(hooks.join("reference-transaction")).unwrap(),
        theirs
    );
}

/// Apple's git, which ignores hooks named in config, runs the file hook.
#[test]
fn apples_git_is_refused_too() {
    if !Path::new("/usr/bin/git").exists() {
        return;
    }
    let (repo, _) = protected("protect-apple");
    let tip = repo.rev("bolt/x");
    let out = support::isolated(&mut Command::new("/usr/bin/git"))
        .args(["-c", "credential.helper="])
        .current_dir(repo.dir())
        .args(["update-ref", "refs/heads/bolt/x", "unit/a"])
        .env_remove("GIT_DIR")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(128), "{}", text(&out));
    assert_eq!(repo.rev("bolt/x"), tip);
}

/// 45, rule core.4: sluice's own move, through gix, runs no hook.
#[test]
fn sluices_own_move_is_not_refused() {
    let (repo, _) = protected("protect-gix");
    let tip = Rev::new(repo.rev("bolt/x"));
    let to = Rev::new(repo.rev("unit/a"));
    let refs = LocalRefs::open(repo.dir()).unwrap();
    assert_eq!(refs.move_to("bolt/x", &tip, &to).unwrap(), Moved::Moved);
    assert_eq!(repo.rev("bolt/x"), to.as_str());
    assert!(protected_events(&repo).is_empty());
}

/// Where the commands a test of refused writes runs keep the host's
/// state: the test's own, never the host's.
fn state_of(repo: &Repo) -> PathBuf {
    repo.dir().join(".git/no-state")
}

/// `sluice` in the repository, under the test's own state.
fn sl_with_state(repo: &Repo, args: &[&str]) -> Output {
    support::isolated(&mut Command::new(sluice()))
        .current_dir(repo.dir())
        .args(args)
        .env("XDG_CONFIG_HOME", repo.dir().join(".git/no-config"))
        .env("XDG_STATE_HOME", state_of(repo))
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

/// A timer's write, `git -C <repository> update-ref refs/heads/bolt/x
/// <new> <old>`, refused twice, each time with full revisions of its own:
/// sluice moves `bolt/x` between the two, as its landings move a branch
/// between a timer's runs. Returns the revisions `bolt/x` was at for each
/// refusal and the one the second would have set.
fn a_timer_refused_twice(repo: &Repo) -> [String; 3] {
    let tip = repo.rev("bolt/x");
    let a = repo.rev("unit/a");
    let b = repo.branch("unit/b", "main", &[("b.txt", Some("b\n"))]);
    let timer = |new: &str, old: &str| {
        let dir = repo.dir().to_str().unwrap();
        let out = support::git_command()
            .args(["-C", dir, "update-ref", "refs/heads/bolt/x", new, old])
            .env("XDG_CONFIG_HOME", repo.dir().join(".git/no-config"))
            .env("XDG_STATE_HOME", state_of(repo))
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(128), "{}", text(&out));
        assert!(
            text(&out).contains("bolt/x is protected by sluice"),
            "{}",
            text(&out)
        );
    };
    timer(&a, &tip);
    let refs = LocalRefs::open(repo.dir()).unwrap();
    assert_eq!(
        refs.move_to("bolt/x", &Rev::new(tip.as_str()), &Rev::new(a.as_str()))
            .unwrap(),
        Moved::Moved
    );
    timer(&b, &a);
    assert_eq!(repo.rev("bolt/x"), a);
    [tip, a, b]
}

/// The causes `sluice status bolt/x --json` and `sluice overview --json`
/// give `bolt/x`.
fn shown(repo: &Repo) -> (serde_json::Value, serde_json::Value) {
    let out = sl_with_state(repo, &["status", "bolt/x", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let status: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let out = sl_with_state(repo, &["overview", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let overview: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let branch = overview["branches"]
        .as_array()
        .and_then(|b| b.iter().find(|b| b["branch"] == "bolt/x"))
        .unwrap_or_else(|| panic!("no bolt/x in {overview}"));
    (
        status["refused_writes"].clone(),
        branch["refused_writes"].clone(),
    )
}

/// 124, 148, rule core.47: a timer's `git -C <repository> update-ref
/// refs/heads/bolt/x <new> <old>`, refused twice with full revisions of
/// its own each time, is one cause: one `protected` line, naming the host
/// and the first write as git ran it, and one cause in `branch.json`
/// refused twice, the last write's revisions on it, which status and the
/// overview give as it stands.
#[test]
fn a_timers_writes_are_one_cause() {
    let (repo, _held) = protected("protect-timer");
    let [tip, a, b] = a_timer_refused_twice(&repo);
    let host = sluice::profiles::checks::hostname();

    let lines = protected_events(&repo);
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = &lines[0];
    assert_eq!(line["host"], host.as_str(), "{line}");
    assert_eq!(line["ref"], "refs/heads/bolt/x", "{line}");
    assert_eq!(
        (&line["from"], &line["to"]),
        (&tip.as_str().into(), &a.as_str().into())
    );
    let by = line["by"].as_str().unwrap();
    assert!(
        by.ends_with(&format!("update-ref refs/heads/bolt/x {a} {tip}")),
        "{line}"
    );

    let causes = causes(&repo);
    assert_eq!(causes.len(), 1, "{causes:?}");
    let cause = &causes[0];
    assert_eq!(cause["host"], host.as_str(), "{cause}");
    assert_eq!(cause["ref"], "refs/heads/bolt/x", "{cause}");
    assert_eq!(
        cause["command"],
        format!(
            "{} update-ref refs/heads/bolt/x <revision> <revision>",
            by.strip_suffix(&format!(" update-ref refs/heads/bolt/x {a} {tip}"))
                .unwrap()
        ),
        "{cause}"
    );
    assert!(
        cause["command"]
            .as_str()
            .unwrap()
            .contains(&format!("-C {}", repo.dir().display())),
        "{cause}"
    );
    assert_eq!(cause["times"], 2, "{cause}");
    assert_eq!(cause["first"], line["at"], "{cause}");
    assert!(cause["last"].as_str() >= cause["first"].as_str(), "{cause}");
    assert_eq!(
        (&cause["from"], &cause["to"]),
        (&a.as_str().into(), &b.as_str().into())
    );

    let (status, overview) = shown(&repo);
    assert_eq!(status, serde_json::json!(causes), "{status}");
    assert_eq!(overview, serde_json::json!(causes), "{overview}");
}

/// 131, 132, 148, rule core.41: the count is kept in the branch's state,
/// never counted from the log: with `log.jsonl` moved to `log/1.jsonl`,
/// as a part's close moves it, status and the overview still give the
/// timer's cause refused twice.
#[test]
fn a_refused_writes_count_is_in_the_state_not_the_log() {
    let (repo, _held) = protected("protect-count-kept");
    a_timer_refused_twice(&repo);
    let records = repo.dir().join(".git/sluice/bolt/x");
    std::fs::create_dir_all(records.join("log")).unwrap();
    std::fs::rename(records.join("log.jsonl"), records.join("log/1.jsonl")).unwrap();
    assert!(protected_events(&repo).is_empty());

    let (status, overview) = shown(&repo);
    for (from, causes) in [("status", &status), ("overview", &overview)] {
        assert_eq!(causes.as_array().map(Vec::len), Some(1), "{from}: {causes}");
        assert_eq!(causes[0]["times"], 2, "{from}: {causes}");
    }
    assert_eq!(status, overview);
}

/// Installing twice changes nothing; a moved binary's path is rewritten.
#[test]
fn installing_is_idempotent_and_follows_the_binary() {
    let (repo, _) = protected("protect-install");
    let hook = repo.dir().join(".git/hooks/reference-transaction");
    let first = std::fs::read_to_string(&hook).unwrap();
    assert!(first.contains(
        &std::fs::canonicalize(sluice())
            .unwrap()
            .display()
            .to_string()
    ));
    let modified = std::fs::metadata(&hook).unwrap().modified().unwrap();
    assert!(sl(&repo, &["protect"]).status.success());
    // Under --json: already in place, and the worktree holding bolt/x.
    let out = sl(&repo, &["protect", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["protection"], "installed");
    assert_eq!(doc["changed"], false);
    assert_eq!(doc["held"][0]["branch"], "bolt/x");
    assert_eq!(
        std::fs::metadata(&hook).unwrap().modified().unwrap(),
        modified
    );

    let moved = repo.dir().join(".git/moved-sluice");
    copy_executable(Path::new(sluice()), &moved);
    let out = run_copied(&moved, || {
        support::isolated(&mut Command::new(&moved))
            .current_dir(repo.dir())
            .arg("protect")
            .stdin(Stdio::null())
            .output()
    });
    assert!(out.status.success(), "{}", text(&out));
    let now = std::fs::read_to_string(&hook).unwrap();
    assert!(now.contains(&std::fs::canonicalize(&moved).unwrap().display().to_string()));
    assert!(!repo
        .dir()
        .join(".git/hooks/reference-transaction.pre-sluice")
        .exists());
}

/// Copy `from` to `to` as an executable through a file of our own,
/// written, synced and closed before anything may run it: a copy still
/// open for writing cannot be executed (ETXTBSY).
fn copy_executable(from: &Path, to: &Path) {
    use std::os::unix::fs::OpenOptionsExt;
    let mut src = std::fs::File::open(from).unwrap();
    let mut dst = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o755)
        .open(to)
        .unwrap();
    std::io::copy(&mut src, &mut dst).unwrap();
    dst.sync_all().unwrap();
    drop(dst);
}

/// Run a freshly copied executable, again after a short sleep while the
/// system calls it busy: a child another test forks while the copy is
/// open inherits the descriptor until it execs, and holds the copy busy
/// that long.
fn run_copied(moved: &Path, mut run: impl FnMut() -> std::io::Result<Output>) -> Output {
    for _ in 0..20 {
        match run() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            other => return other.unwrap_or_else(|e| panic!("running {}: {e}", moved.display())),
        }
    }
    panic!("{} stayed busy", moved.display())
}

/// A hooks directory inside a worktree is tracked: sluice will not write
/// it, and says so.
#[test]
fn a_tracked_hooks_directory_is_not_written() {
    let repo = Repo::new("protect-tracked");
    std::fs::create_dir_all(repo.dir().join(".githooks")).unwrap();
    repo.git(&["config", "core.hooksPath", ".githooks"]);
    let out = sl(&repo, &["protect"]);
    assert!(!out.status.success());
    assert!(text(&out).contains("core.hooksPath"), "{}", text(&out));
    assert!(!repo.dir().join(".githooks/reference-transaction").exists());
    // 124: nor does a command that only re-installs, and it says nothing.
    let out = sl(&repo, &["status", "main"]);
    assert!(!text(&out).contains("protection"), "{}", text(&out));
    assert!(!repo.dir().join(".githooks/reference-transaction").exists());
}

/// ADR 0024: `sluice protect` names a worktree that has checked out a
/// branch it protects, and still succeeds.
#[test]
fn protect_names_a_worktree_holding_a_protected_branch() {
    let (repo, held) = protected("protect-held");
    let out = sl(&repo, &["protect"]);
    assert!(out.status.success(), "{}", text(&out));
    let held = std::fs::canonicalize(held).unwrap();
    assert!(
        text(&out).contains(&format!(
            "warning: {} has bolt/x checked out: sluice moves bolt/x under it",
            held.display()
        )),
        "{}",
        text(&out)
    );
}

/// A protected repository whose `bolt/x` names `origin`, a bare
/// repository, where another host has moved `bolt/x` on: this host's
/// `bolt/x`, its copy, stands apart from origin's, which is fetched here.
/// Returns the repository, the worktree on `bolt/x`, the copy's revision,
/// origin's, and the bare repository.
fn copy_apart(name: &str) -> (Scratch, Repo, PathBuf, String, String) {
    copy_apart_with(name, "")
}

/// [`copy_apart`], `bolt/*`'s table given `extra` as well.
fn copy_apart_with(name: &str, extra: &str) -> (Scratch, Repo, PathBuf, String, String) {
    let repo = Repo::new(name);
    repo.write(
        ".config/sluice.toml",
        &POLICY.replace("trust = ", &format!("remote = \"origin\"\n{extra}trust = ")),
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    repo.git(&["branch", "bolt/x", "main"]);
    repo.branch("unit/a", "main", &[("a.txt", Some("a\n"))]);
    let bare = repo.bare_origin();
    let other = Repo::clone_of(bare.path(), &format!("{name}-other"));
    other.git(&["switch", "-q", "bolt/x"]);
    other.write("o.txt", "o\n");
    other.git(&["add", "-A"]);
    other.git(&["commit", "-q", "-m", "on origin"]);
    other.git(&["push", "-q", "origin", "bolt/x"]);
    let remote = other.rev("bolt/x");
    let out = sl(&repo, &["protect"]);
    assert!(out.status.success(), "{}", text(&out));
    repo.git(&["fetch", "-q", "origin"]);
    let held = repo.dir().with_extension("bolt");
    let _ = std::fs::remove_dir_all(&held);
    repo.git(&["worktree", "add", "-q", held.to_str().unwrap(), "bolt/x"]);
    let copy = repo.rev("bolt/x");
    assert_ne!(copy, remote);
    (bare, repo, held, copy, remote)
}

/// 124: on a branch kept on a remote, the protection judges a write by
/// this host's own `bolt/x`, fetching nothing: a reset to where it stands
/// and git's packing pass, and a move to origin's revision is refused
/// from the copy's value.
#[test]
fn a_copy_apart_from_its_remote_is_judged_by_its_own_value() {
    let (_bare, repo, held, copy, remote) = copy_apart("protect-copy");
    let out = git_in(&repo, &held, &["reset", "-q", "--hard"]);
    assert!(out.status.success(), "{}", text(&out));
    let out = repo.git_raw(&["pack-refs", "--all", "--prune"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!repo.dir().join(".git/refs/heads/bolt/x").exists());
    assert_eq!(repo.rev("bolt/x"), copy);
    assert!(protected_events(&repo).is_empty());

    let out = repo.git_raw(&["update-ref", "refs/heads/bolt/x", &remote]);
    assert_eq!(out.status.code(), Some(128), "{}", text(&out));
    assert_eq!(repo.rev("bolt/x"), copy);
    let events = protected_events(&repo);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        (&events[0]["from"], &events[0]["to"]),
        (&serde_json::json!(copy), &serde_json::json!(remote))
    );
    assert!(
        events[0]["by"]
            .as_str()
            .unwrap()
            .contains("update-ref refs/heads/bolt/x"),
        "{events:?}"
    );
}

/// A protected repository whose `bolt/x` and its records are kept on
/// `origin`, its policy naming a signer, so a read of the records fetches
/// them (C.6); and the bare origin.
fn kept_on_origin(name: &str) -> (Scratch, Repo) {
    let key = PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap();
    let repo = Repo::new(name);
    repo.write(
        ".config/sluice.toml",
        &format!(
            "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\n\
             [signers.box]\nssh = \"box {}\"\n\n\
             [branches.\"bolt/*\"]\nremote = \"origin\"\ntrust = [\"box\"]\n",
            key.public_key().to_openssh().unwrap()
        ),
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    repo.git(&["branch", "bolt/x", "main"]);
    let bare = repo.bare_origin();
    let out = sl(&repo, &["protect"]);
    assert!(out.status.success(), "{}", text(&out));
    (bare, repo)
}

/// [`sl_with_state`], ended and failed when it has not ended within a
/// minute: a sluice that waits on a lock its own ancestor holds never ends,
/// and ending the command lets its lock go.
fn sl_ended(repo: &Repo, args: &[&str]) -> Output {
    let mut child = support::isolated(&mut Command::new(sluice()))
        .current_dir(repo.dir())
        .args(args)
        .env("XDG_CONFIG_HOME", repo.dir().join(".git/no-config"))
        .env("XDG_STATE_HOME", state_of(repo))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > until {
            child.kill().unwrap();
            let out = child.wait_with_output().unwrap();
            panic!(
                "sluice {args:?} had not ended after a minute: {}",
                text(&out)
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    child.wait_with_output().unwrap()
}

/// S64, record 0110: a round's fetch of a branch kept on its remote, with
/// enough refs in loose files that git's maintenance after the fetch packs
/// them, runs the hook on the pruning of `bolt/x`'s loose file while
/// sluice's fetch holds the repository's fetch lock. The packing passes
/// with no record read, and the round ends (deneb, 2026-11-10: a submit's
/// fetch of #235 and the hook it ran waited on each other for forty
/// minutes).
#[test]
fn a_fetch_whose_packing_runs_the_hook_ends() {
    let (_bare, repo) = kept_on_origin("protect-fetch-packs");
    // The tests' git configuration turns git's maintenance off; this test
    // is of a fetch whose maintenance packs, so the repository turns it on.
    repo.git(&["config", "maintenance.auto", "true"]);
    repo.git(&["config", "gc.auto", "6700"]);
    let tip = repo.rev("bolt/x");
    for i in 0..20 {
        repo.git(&["branch", &format!("loose/{i}"), "main"]);
    }
    assert!(repo.dir().join(".git/refs/heads/bolt/x").exists());

    let out = sl_ended(&repo, &["step", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        !repo.dir().join(".git/refs/heads/bolt/x").exists(),
        "the fetch's maintenance packed nothing, so ran no hook on bolt/x"
    );
    assert_eq!(repo.rev("bolt/x"), tip);
}

/// Record 0110: a hook run under a fetch of sluice's own takes no fetch
/// lock and fetches nothing, though it records a refusal in records kept
/// on the remote. Here a hook sluice's chains hands the hook a move of
/// `bolt/y`, whose lock the round on `bolt/x` does not hold, in each
/// transaction the round's git makes. The hook refuses it under the
/// round's fetch, told the lock its ancestor holds, and ends; so does the
/// round.
#[test]
fn a_hook_under_sluices_own_fetch_fetches_nothing() {
    let (_bare, repo) = kept_on_origin("protect-fetch-hook");
    repo.git(&["branch", "bolt/y", "main"]);
    repo.git(&["push", "-q", "origin", "bolt/y"]);
    let tip = repo.rev("bolt/y");
    let other = repo.branch("unit/a", "main", &[("a.txt", Some("a\n"))]);
    let judged = repo.dir().join(".git/judged");
    let chained = repo
        .dir()
        .join(".git/hooks/reference-transaction.pre-sluice");
    // Run once per transaction of the round's, never again for the
    // transactions the hook's own recording makes.
    std::fs::write(
        &chained,
        format!(
            "#!/bin/sh\n[ \"$1\" = prepared ] && [ -z \"$PROTECT_TEST_HOOK\" ] || exit 0\n\
             export PROTECT_TEST_HOOK=1\ncat >/dev/null\n\
             printf '%s %s refs/heads/bolt/y\\n' {tip} {other} | '{sluice}' hook \
             reference-transaction prepared 2>/dev/null\n\
             echo \"judged $? under ${{SLUICE_FETCH_HELD:-no fetch}}\" >> '{judged}'\nexit 0\n",
            sluice = sluice(),
            judged = judged.display(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(
        &chained,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();

    let out = sl_ended(&repo, &["step", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    let lock = std::fs::canonicalize(repo.dir().join(".git"))
        .unwrap()
        .join("sluice-fetch.lock");
    let said = std::fs::read_to_string(&judged).unwrap_or_default();
    assert!(
        said.lines()
            .any(|l| l == format!("judged 1 under {}", lock.display())),
        "{said}"
    );
    assert_eq!(repo.rev("bolt/y"), tip);
}

/// 1, 124: a refusal tells whoever ran it, before git's own error, that
/// the branch is protected, the command, both revisions, that sluice
/// levels this host's copy itself, and how to bring the worktree level.
#[test]
fn a_refusal_names_the_command_and_both_revisions() {
    let (_bare, repo, held, copy, remote) = copy_apart("protect-words");
    let said = |out: &Output, command: &str, to: &str| {
        let t = text(out);
        assert_eq!(out.status.code(), Some(128), "{t}");
        let ours = t
            .find("error: bolt/x is protected by sluice; submit the change to sluice instead")
            .unwrap_or_else(|| panic!("{t}"));
        let theirs = t.find("fatal:").unwrap_or(t.len());
        assert!(ours < theirs, "sluice's lines come first: {t}");
        assert!(
            t.contains(&format!(
                "{command}` would move it from {} to {}",
                &copy[..7],
                &to[..7]
            )),
            "{t}"
        );
        assert!(
            t.contains("sluice levels this host's bolt/x to each landing itself"),
            "{t}"
        );
        assert!(
            t.contains(
                "the worktree may hold the attempted change: git merge --abort or git reset --hard"
            ),
            "{t}"
        );
    };
    let out = repo.git_raw(&["update-ref", "refs/heads/bolt/x", &remote]);
    said(
        &out,
        &format!("update-ref refs/heads/bolt/x {remote}"),
        &remote,
    );
    let head = repo.rev("unit/a");
    let out = git_in(&repo, &held, &["reset", "-q", "--hard", &head]);
    said(&out, &format!("reset -q --hard {head}"), &head);
    assert_eq!(repo.rev("bolt/x"), copy);
}

/// 1: a branch kept in this host's own refs is told the command and both
/// revisions too, without the copy's line; a deletion's refusal names
/// what still needs the branch.
#[test]
fn a_local_branchs_refusals_say_the_command() {
    let repo = queued("protect-local-words");
    let tip = repo.rev("bolt/x");
    let head = repo.rev("unit/a");
    let out = repo.git_raw(&["update-ref", "refs/heads/bolt/x", &head]);
    let t = text(&out);
    assert_eq!(out.status.code(), Some(128), "{t}");
    assert!(
        t.contains(&format!(
            "update-ref refs/heads/bolt/x {head}` would move it from {} to {}",
            &tip[..7],
            &head[..7]
        )),
        "{t}"
    );
    assert!(!t.contains("levels this host's"), "{t}");
    let out = repo.git_raw(&["branch", "-D", "bolt/x"]);
    let t = text(&out);
    assert_eq!(out.status.code(), Some(128), "{t}");
    assert!(
        t.contains(&format!(
            "branch -D bolt/x` would delete it at {}; it still holds #1",
            &tip[..7]
        )),
        "{t}"
    );
}

/// git in `dir` naming the repository `git_dir`, as a fetch into a bare
/// repository run from elsewhere does.
fn git_named(git_dir: &Path, dir: &Path, args: &[&str]) -> Output {
    support::git_command()
        .current_dir(dir)
        .arg("--git-dir")
        .arg(git_dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

/// The `protected` lines of `bolt/x`'s log in the repository whose
/// common directory is `common`.
fn protected_in(common: &Path, branch: &str) -> usize {
    std::fs::read_to_string(common.join(format!("sluice/{branch}/log.jsonl")))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains("\"event\":\"protected\""))
        .count()
}

/// 124: the protection installed in a bare repository judges the
/// repository git names, wherever the command runs: a fetch from a folder
/// that is no repository passes, a write to a protected branch through
/// `--git-dir` from there is refused, and run from inside another
/// repository, only the named one's policy counts.
#[test]
fn the_protection_judges_the_repository_git_names() {
    let src = Repo::new("protect-named-src");
    src.write(".config/sluice.toml", POLICY);
    src.git(&["add", "-A"]);
    src.git(&["commit", "-q", "-m", "policy"]);
    src.git(&["branch", "bolt/x", "main"]);
    src.branch("unit/a", "main", &[("a.txt", Some("a\n"))]);
    let x = Scratch::new("protect-named-x.git");
    src.git(&["clone", "-q", "--bare", ".", x.path().to_str().unwrap()]);
    let out = support::isolated(&mut Command::new(sluice()))
        .current_dir(x.path())
        .arg("protect")
        .env("XDG_CONFIG_HOME", x.path().join("no-config"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let tip = src.rev("bolt/x");
    // Work on origin, x's source, for the fetch to bring.
    src.git(&["switch", "-q", "unit/a"]);
    src.git(&["commit", "-q", "--allow-empty", "-m", "more"]);
    let more = src.rev("unit/a");
    src.git(&["switch", "-q", "main"]);
    src.git(&["commit", "-q", "--allow-empty", "-m", "main more"]);
    let main = src.rev("main");

    let outside = Scratch::new("protect-named-outside");
    let out = git_named(
        x.path(),
        outside.path(),
        &[
            "fetch",
            "-q",
            "origin",
            "+refs/heads/main:refs/remotes/origin/main",
            "+refs/heads/unit/a:refs/heads/unit/a",
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert!(!text(&out).contains("sluice"), "{}", text(&out));
    let rev = |name: &str| {
        let out = git_named(x.path(), outside.path(), &["rev-parse", name]);
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    assert_eq!(rev("refs/remotes/origin/main"), main);
    assert_eq!(rev("refs/heads/unit/a"), more);
    assert_eq!(protected_in(x.path(), "bolt/x"), 0);

    let out = git_named(
        x.path(),
        outside.path(),
        &["update-ref", "refs/heads/bolt/x", &more],
    );
    assert_eq!(out.status.code(), Some(128), "{}", text(&out));
    assert_eq!(rev("refs/heads/bolt/x"), tip);
    assert_eq!(protected_in(x.path(), "bolt/x"), 1);

    // A second repository, whose policy protects `main`, holds the folder.
    let y = Repo::new("protect-named-y");
    y.write(
        ".config/sluice.toml",
        "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\n[branches.main]\n",
    );
    y.git(&["add", "-A"]);
    y.git(&["commit", "-q", "-m", "policy"]);
    let out = sl(&y, &["protect"]);
    assert!(out.status.success(), "{}", text(&out));
    let y_main = y.rev("main");
    let out = git_named(x.path(), y.dir(), &["update-ref", "refs/heads/main", &more]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(rev("refs/heads/main"), more);
    assert_eq!(y.rev("main"), y_main);
    assert_eq!(protected_in(x.path(), "main"), 0);
    assert_eq!(protected_in(&y.dir().join(".git"), "main"), 0);
}

/// 124: a transaction the protection cannot read for a reason of its own,
/// here two branches whose policy cannot be parsed, passes with exactly
/// one warning saying why.
#[test]
fn a_transaction_the_protection_cannot_read_passes_said_once() {
    let (repo, _) = protected("protect-unread");
    let broken = repo.branch(
        "broken",
        "main",
        &[(
            ".config/sluice.toml",
            Some("[branches.\"bolt/*\"\nnot toml"),
        )],
    );
    repo.git(&["branch", "bolt/p", &broken]);
    repo.git(&["branch", "bolt/q", &broken]);
    let head = repo.rev("unit/a");
    let out = support::git_command()
        .current_dir(repo.dir())
        .args(["update-ref", "--stdin"])
        .env_remove("GIT_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            use std::io::Write as _;
            c.stdin.take().unwrap().write_all(
                format!("update refs/heads/bolt/p {head}\nupdate refs/heads/bolt/q {head}\n")
                    .as_bytes(),
            )?;
            c.wait_with_output()
        })
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(repo.rev("bolt/p"), head);
    assert_eq!(repo.rev("bolt/q"), head);
    let t = text(&out);
    assert_eq!(
        t.matches("warning: sluice's branch protection let this through unread: ")
            .count(),
        1,
        "{t}"
    );
    assert!(!t.contains("protected by sluice"), "{t}");
    for b in ["bolt/p", "bolt/q"] {
        assert_eq!(protected_in(&repo.dir().join(".git"), b), 0);
    }
}

/// 124: host settings the hook does not know change nothing it guards: a
/// move is refused and reported once, nothing said of the settings, and a
/// reset that moves nothing passes saying nothing.
#[test]
fn settings_the_hook_does_not_know_change_nothing_it_guards() {
    let (repo, held) = protected("protect-later-settings");
    host_settings(&repo, "[runs]\ncores = 12\n\n[later]\nx = 1\n");
    let tip = repo.rev("bolt/x");
    let head = repo.rev("unit/a");
    let out = repo.git_raw(&["update-ref", "refs/heads/bolt/x", &head]);
    let t = text(&out);
    assert_eq!(out.status.code(), Some(128), "{t}");
    assert!(
        t.contains("error: bolt/x is protected by sluice; submit the change to sluice instead")
            && t.contains(&format!(
                "update-ref refs/heads/bolt/x {head}` would move it"
            )),
        "{t}"
    );
    assert!(!t.contains("warning"), "{t}");
    assert_eq!(repo.rev("bolt/x"), tip);
    assert_eq!(protected_events(&repo).len(), 1);

    let out = git_in(&repo, &held, &["reset", "-q", "--hard"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(text(&out), "");
    assert_eq!(repo.rev("bolt/x"), tip);
    assert_eq!(protected_events(&repo).len(), 1);
}

/// 124: every other command still reads the host's settings in full, and
/// refuses a field it does not know, naming the file and the field.
#[test]
fn every_other_command_refuses_a_setting_it_does_not_know() {
    let (repo, _) = protected("protect-strict-settings");
    let path = host_settings(&repo, "[runs]\nlater = 1\n");
    let out = sl(&repo, &["status", "bolt/x"]);
    let t = text(&out);
    assert_eq!(out.status.code(), Some(3), "{t}");
    assert!(
        t.contains(&path.display().to_string()) && t.contains("later"),
        "{t}"
    );
}

/// 124: a policy field and table the hook does not know change nothing it
/// guards.
#[test]
fn a_policy_field_the_hook_does_not_know_changes_nothing_it_guards() {
    let policy = format!(
        "{}\n[later]\nx = 1\n",
        POLICY.replace("trust = ", "later = true\ntrust = ")
    );
    let (repo, held) = protected_with("protect-later-policy", &policy);
    // git refuses to force a checked-out branch itself.
    repo.git(&["worktree", "remove", "--force", held.to_str().unwrap()]);
    let tip = repo.rev("bolt/x");
    let out = repo.git_raw(&["branch", "-f", "bolt/x", "unit/a"]);
    let t = text(&out);
    assert_eq!(out.status.code(), Some(128), "{t}");
    assert!(t.contains("bolt/x is protected by sluice"), "{t}");
    assert!(!t.contains("warning"), "{t}");
    assert_eq!(repo.rev("bolt/x"), tip);
    assert_eq!(protected_events(&repo).len(), 1);
}

/// 124: a branch kept on a remote, under a policy and host settings each
/// holding a field no sluice yet reads, is still judged as sluice's copy.
#[test]
fn a_copy_is_judged_under_fields_the_hook_does_not_know() {
    let (_bare, repo, _held, copy, remote) =
        copy_apart_with("protect-later-copy", "later = true\n");
    host_settings(&repo, "[later]\nx = 1\n");
    let out = repo.git_raw(&["update-ref", "refs/heads/bolt/x", &remote]);
    let t = text(&out);
    assert_eq!(out.status.code(), Some(128), "{t}");
    assert!(
        t.contains("sluice levels this host's bolt/x to each landing itself"),
        "{t}"
    );
    assert!(!t.contains("warning"), "{t}");
    assert_eq!(repo.rev("bolt/x"), copy);
    assert_eq!(protected_events(&repo).len(), 1);
}

/// 124: host settings the hook cannot read at all, and a policy whose
/// `remote` is of the wrong kind, each pass the transaction with exactly
/// one warning naming what could not be read, and nothing refused.
#[test]
fn settings_or_a_policy_the_hook_cannot_read_pass_said_once() {
    let unread = |repo: &Repo, names: &[&str]| {
        let head = repo.rev("unit/a");
        let out = repo.git_raw(&["update-ref", "refs/heads/bolt/x", &head]);
        let t = text(&out);
        assert!(out.status.success(), "{t}");
        assert_eq!(repo.rev("bolt/x"), head);
        let said = "warning: sluice's branch protection let this through unread: ";
        assert_eq!(t.matches(said).count(), 1, "{t}");
        let why = &t[t.find(said).unwrap()..];
        for name in names {
            assert!(why.contains(name), "{name}: {t}");
        }
        assert!(!t.contains("protected by sluice"), "{t}");
        assert!(protected_events(repo).is_empty());
    };

    let (repo, _) = protected("protect-unread-settings");
    let path = host_settings(&repo, "[runs\nnot toml");
    unread(&repo, &[&path.display().to_string()]);

    let (repo, _) = protected_with(
        "protect-unread-remote",
        &POLICY.replace("trust = ", "remote = 1\ntrust = "),
    );
    unread(
        &repo,
        &["bolt/x's policy at ", "[branches.\"bolt/*\"] remote"],
    );
}

/// 124: the next command a newer sluice runs in the repository puts the
/// hook back for itself, the hook it chains unchanged and nothing said of
/// it; a hook `sluice protect --remove` took out stays out.
#[test]
fn a_newer_sluices_next_command_reinstalls_the_hook() {
    let repo = Repo::new("protect-reinstall");
    repo.write(".config/sluice.toml", POLICY);
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "policy"]);
    repo.git(&["branch", "bolt/x", "main"]);
    let hooks = repo.dir().join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let theirs = "#!/bin/sh\nexit 0\n";
    std::fs::write(hooks.join("reference-transaction"), theirs).unwrap();
    std::fs::set_permissions(
        hooks.join("reference-transaction"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let older = repo.dir().join(".git/older-sluice");
    copy_executable(Path::new(sluice()), &older);
    let out = run_copied(&older, || {
        support::isolated(&mut Command::new(&older))
            .current_dir(repo.dir())
            .arg("protect")
            .env("XDG_CONFIG_HOME", repo.dir().join(".git/no-config"))
            .stdin(Stdio::null())
            .output()
    });
    assert!(out.status.success(), "{}", text(&out));
    let hook = hooks.join("reference-transaction");
    let chained = hooks.join("reference-transaction.pre-sluice");
    let older = std::fs::canonicalize(&older).unwrap().display().to_string();
    let ours = std::fs::canonicalize(sluice())
        .unwrap()
        .display()
        .to_string();
    assert!(std::fs::read_to_string(&hook).unwrap().contains(&older));

    let out = sl(&repo, &["status", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!text(&out).contains("protection"), "{}", text(&out));
    let now = std::fs::read_to_string(&hook).unwrap();
    assert!(now.contains(&ours) && !now.contains(&older), "{now}");
    assert_eq!(std::fs::read_to_string(&chained).unwrap(), theirs);

    let out = sl(&repo, &["protect", "--remove"]);
    assert!(out.status.success(), "{}", text(&out));
    let out = sl(&repo, &["status", "bolt/x"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(std::fs::read_to_string(&hook).unwrap(), theirs);
    assert!(!chained.exists());
}

/// 1, 124: the hook runs the sluice that installed it, by its path: a
/// program called `sluice` first on the caller's `PATH`, which lets
/// everything through, judges nothing.
#[test]
fn a_sluice_on_the_callers_path_is_not_the_one_that_judges() {
    let (repo, _) = protected("protect-path");
    let bin = repo.dir().join(".git/other-bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("sluice"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(
        bin.join("sluice"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let tip = repo.rev("bolt/x");
    let head = repo.rev("unit/a");
    let out = support::git_command()
        .current_dir(repo.dir())
        .args(["update-ref", "refs/heads/bolt/x", &head])
        .env("PATH", path)
        .env("XDG_CONFIG_HOME", repo.dir().join(".git/no-config"))
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let t = text(&out);
    assert_eq!(out.status.code(), Some(128), "{t}");
    assert!(t.contains("bolt/x is protected by sluice"), "{t}");
    assert_eq!(repo.rev("bolt/x"), tip);
}

/// 124: the hook reads the whole transaction git hands it before it
/// leaves, in a state it does not judge and where it cannot read the
/// repository, so whatever writes the transaction to it, the shim's
/// `printf` among them, never meets a closed pipe and says so on the
/// caller's terminal. The transaction is larger than a pipe holds, so a
/// hook that left without reading it would fail the write every time.
#[test]
fn the_hook_reads_its_whole_transaction_before_it_leaves() {
    let scratch = Scratch::new("protect-hook-drains");
    let zero = "0".repeat(40);
    let one = "1".repeat(40);
    let transaction: String = (0..4096)
        .map(|n| format!("{zero} {one} refs/heads/bolt/n{n}\n"))
        .collect();
    assert!(transaction.len() > 256 * 1024);
    for (state, git_dir) in [
        ("preparing", None),
        ("committed", None),
        ("aborted", None),
        ("prepared", Some(scratch.0.join("absent"))),
    ] {
        let mut c = Command::new(sluice());
        support::isolated(&mut c)
            .current_dir(&scratch.0)
            .args(["hook", "reference-transaction", state])
            .env("XDG_CONFIG_HOME", scratch.0.join("no-config"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match &git_dir {
            Some(d) => c.env("GIT_DIR", d),
            None => c.env_remove("GIT_DIR"),
        };
        let mut child = c.spawn().unwrap();
        let wrote = {
            use std::io::Write as _;
            let mut stdin = child.stdin.take().unwrap();
            stdin.write_all(transaction.as_bytes())
        };
        let out = child.wait_with_output().unwrap();
        assert!(wrote.is_ok(), "{state}: {wrote:?}: {}", text(&out));
        assert_eq!(out.status.code(), Some(0), "{state}: {}", text(&out));
    }
}
