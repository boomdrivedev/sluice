//! The place seam's profile on scratch repositories, git's own part of
//! section 5's acceptance tests in `docs/plans/phase-1-integrations.md`
//! for the one method, `git-worktree`: what a checkout holds, how git
//! locks one and lists the worktrees, and how it lists a folder whatever
//! spells its path. The profile's own logic, its ports, scratch, records,
//! releases and previews, is `tests/checkouts.rs`'s, over fakes of git
//! and the runs. Needs git on the path, so it is in the check-tools tier.

#![cfg(feature = "tools")]

#[path = "../support/mod.rs"]
mod support;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use sluice::profiles::places::{GitWorktrees, Places};
use sluice::runs::Runs;
use sluice_seams::{Checkout, Place, Rev, Setting, Settings};
use support::Repo;

const NAME: &str = "sluice/try/bolt/x/1";

/// Held by every test: alone by one that asks the host for ports, shared
/// by every other, each of which spawns git. On macOS a socket is made
/// close-on-exec only after it is made, so a child spawned on another
/// thread between the two keeps the port the socket was checking,
/// listening, for the child's whole life, and that port reads as in use
/// to the test that checked it free.
static PORTS: std::sync::RwLock<()> = std::sync::RwLock::new(());

/// Held by a test that asks the host for ports: no other test spawns a
/// process while it does.
fn ports_lock() -> std::sync::RwLockWriteGuard<'static, ()> {
    PORTS.write().unwrap_or_else(|e| e.into_inner())
}

/// Held by a test that asks the host for no ports.
fn spawns() -> std::sync::RwLockReadGuard<'static, ()> {
    PORTS.read().unwrap_or_else(|e| e.into_inner())
}

thread_local! {
    /// The short directories each test's repositories keep their scratch
    /// under, one per repository, removed when the test's thread ends.
    static USERS: std::cell::RefCell<BTreeMap<PathBuf, support::Scratch>> =
        std::cell::RefCell::default();
}

/// The directory standing for the user's scratch directory (35 as
/// amended, rule core.30) in this test of `repo`: a path not yet made, in
/// a short directory of the test's own, so a scratch under it stays short
/// and no test touches the user's own.
fn user_dir(repo: &Repo) -> PathBuf {
    USERS.with(|u| {
        u.borrow_mut()
            .entry(repo.dir().to_path_buf())
            .or_insert_with(support::Scratch::short)
            .path()
            .join("u")
    })
}

fn places(repo: &Repo) -> Places {
    let common = repo.dir().join(".git");
    Places::new(
        &common,
        Rc::new(GitWorktrees::new(&common)),
        Rc::new(runs(repo)),
    )
    .with_scratch_root(user_dir(repo))
}

fn runs(repo: &Repo) -> Runs {
    Runs::new(
        &repo.dir().join(".git"),
        PathBuf::from(env!("CARGO_BIN_EXE_sluice")),
    )
}

/// A repository with a candidate commit off main, a session's worktree on
/// its own branch, and a hook of each kind that logs it ran.
fn scratch(name: &str) -> (Repo, Rev) {
    let repo = Repo::new(name);
    let cand = repo.branch("cand", "main", &[("src/lib", Some("candidate\n"))]);
    repo.git(&["branch", "-D", "cand"]);
    let session = repo.dir().with_extension("session");
    let _ = std::fs::remove_dir_all(&session);
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "unit/a",
        session.to_str().unwrap(),
        "main",
    ]);
    std::fs::write(session.join("work"), "uncommitted\n").unwrap();
    let hooks = repo.dir().join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join("post-checkout");
    std::fs::write(
        &hook,
        "#!/bin/sh\necho ran >> \"$(git rev-parse --git-common-dir)/post-checkout.log\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    (repo, Rev::new(cand))
}

/// Every ref and every worktree with its head: what no place may change.
fn others(repo: &Repo) -> String {
    let refs = repo.git(&["for-each-ref", "--format=%(refname) %(objectname)"]);
    let refs: Vec<&str> = refs
        .lines()
        .filter(|l| !l.starts_with("refs/heads/sluice/try/"))
        .collect();
    let session = repo.dir().with_extension("session");
    format!(
        "{}\n{}\n{}",
        refs.join("\n"),
        repo.git(&["-C", session.to_str().unwrap(), "rev-parse", "HEAD"]),
        std::fs::read_to_string(session.join("work")).unwrap()
    )
}

/// 33, 35: the place holds exactly the candidate, with a scratch directory
/// of its own outside it.
fn holds_the_candidate(repo: &Repo, place: &Place, rev: &Rev) {
    let at = |args: &[&str]| {
        let mut a = vec!["-C", place.path.to_str().unwrap()];
        a.extend(args);
        repo.git(&a)
    };
    assert_eq!(at(&["rev-parse", "HEAD"]), rev.as_str());
    assert_eq!(at(&["status", "--porcelain", "--untracked-files=all"]), "");
    assert_eq!(
        std::fs::read_to_string(place.path.join("src/lib")).unwrap(),
        "candidate\n"
    );
    assert!(place.tmp.is_dir());
    assert_eq!(std::fs::read_dir(&place.tmp).unwrap().count(), 0);
    assert!(!place.tmp.starts_with(&place.path));
}

fn left_behind(repo: &Repo, place: &Place) {
    assert!(!place.path.exists(), "{} left", place.path.display());
    assert!(!place.tmp.exists());
    assert!(!repo
        .git(&["worktree", "list"])
        .contains(&place.path.display().to_string()));
    assert_eq!(repo.git(&["branch", "--list", "sluice/*"]), "");
}

/// 33 to 35, 25, rule core.16: a git worktree holds exactly the
/// candidate, with no hook run in it, and its release leaves nothing in
/// git's worktrees or refs; every other ref, worktree and file is as it
/// was. What the profile makes and lets go of beside git is
/// `tests/checkouts.rs`'s.
#[test]
fn a_git_worktree_place_is_the_candidate_and_leaves_nothing() {
    let _spawns = spawns();
    let (repo, rev) = scratch("places-git");
    let before = others(&repo);
    let p = places(&repo);
    let place = p.materialize(NAME, &rev, &Settings::new()).unwrap();
    holds_the_candidate(&repo, &place, &rev);
    // The method runs nothing in the place: no post-checkout hook.
    assert!(!repo.dir().join(".git/post-checkout.log").exists());

    p.release(NAME).unwrap();
    left_behind(&repo, &place);
    assert_eq!(others(&repo), before);
}

/// Lock the checkout of `place` as `git worktree lock --reason` does.
fn lock_checkout(repo: &Repo, place: &Place, reason: &str) {
    repo.git(&[
        "worktree",
        "lock",
        "--reason",
        reason,
        place.path.to_str().unwrap(),
    ]);
    assert!(repo
        .git(&["worktree", "list", "--porcelain"])
        .lines()
        .any(|l| l == format!("locked {reason}")));
}

/// 123, rule core.16, record 0122 (design D4): a checkout git left locked
/// `initializing`, as an add stopped part way leaves it, is unlocked,
/// removed and pruned by its release.
#[test]
fn a_checkout_git_left_locked_initializing_is_released() {
    let _spawns = spawns();
    let (repo, rev) = scratch("places-locked-release");
    let p = places(&repo);
    let place = p.materialize(NAME, &rev, &Settings::new()).unwrap();
    lock_checkout(&repo, &place, "initializing");
    p.release(NAME).unwrap();
    left_behind(&repo, &place);
}

/// 123, record 0122 (design D4): a checkout locked for any other reason is
/// a hold sluice keeps: its release fails naming the lock and git's words,
/// and the checkout stays.
#[test]
fn a_checkout_locked_for_another_reason_is_held() {
    let _spawns = spawns();
    let (repo, rev) = scratch("places-locked-held");
    let p = places(&repo);
    let place = p.materialize(NAME, &rev, &Settings::new()).unwrap();
    lock_checkout(&repo, &place, "a person looks at it");
    let err = format!("{:#}", p.release(NAME).unwrap_err());
    assert!(err.contains("locked"), "{err}");
    assert!(err.contains("a person looks at it"), "{err}");
    assert!(err.contains("git worktree remove"), "{err}");
    assert!(place.path.is_dir(), "the checkout stays");
    repo.git(&["worktree", "unlock", place.path.to_str().unwrap()]);
    p.release(NAME).unwrap();
    left_behind(&repo, &place);
}

fn ported(names: &[&str], from: &str) -> Settings {
    let mut s = Settings::new();
    s.insert(
        "ports".into(),
        Setting::List(names.iter().map(|n| n.to_string()).collect()),
    );
    s.insert("ports_from".into(), Setting::Text(from.into()));
    s
}

/// Whether nothing on the host holds `port`, asked as the place seam asks.
fn free(port: u16) -> bool {
    let local = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpListener::bind(("0.0.0.0", port)).is_ok()
        && std::net::TcpStream::connect_timeout(&local, std::time::Duration::from_millis(100))
            .is_err()
}

/// The first of `n` ports in a row for the test that asks, under
/// `ports_lock`: the next that no test of this process was given and that
/// the host holds none of, from where the process's forty begin. Its id
/// places them, so two runs of these tests on one host, as two candidates
/// under test at once are, ask for different ports; the suite asks for
/// fewer than forty. All are from 25000 to 32767: above the ports the
/// host's own places take from 20000, and below 32768, where Linux starts
/// the ports it gives a host's own connections for a moment, which would
/// make a port look busy; macOS starts at 49152.
fn ports(n: u16) -> u16 {
    static NEXT: std::sync::Mutex<Option<u16>> = std::sync::Mutex::new(None);
    let mut next = NEXT.lock().unwrap_or_else(|e| e.into_inner());
    let mut from = next.unwrap_or(25000 + (std::process::id() % 194) as u16 * 40);
    loop {
        if from + n > 32768 {
            from = 25000;
        }
        match (from..from + n).find(|p| !free(*p)) {
            Some(held) => from = held + 1,
            None => break,
        }
    }
    *next = Some(from + n);
    from
}

/// The worktrees git lists for the repository.
fn worktrees(repo: &Repo) -> Vec<String> {
    repo.git(&["worktree", "list", "--porcelain"])
        .lines()
        .filter_map(|l| l.strip_prefix("worktree ").map(str::to_string))
        .collect()
}

fn preview_name(rev: &Rev) -> String {
    format!("preview/{}", &rev.as_str()[..12])
}

/// 129: the worktrees on a branch, read with no process, are what
/// `git worktree list --porcelain -z` names, as the listing's reading
/// takes it: the main worktree and a linked one on their branches, a
/// locked one, and a locked one whose directory is gone, but no detached
/// one and none whose directory is gone that is not locked (prunable).
#[test]
fn checked_out_reads_the_worktrees_as_git_lists_them() {
    let repo = Repo::new("listed");
    let at = support::Scratch::new("listed-worktrees");
    let add = |name: &str, args: &[&str]| -> PathBuf {
        let path = at.path().join(name);
        let mut a = vec!["worktree", "add", "-q"];
        a.extend(args);
        a.push(path.to_str().unwrap());
        a.push("main");
        repo.git(&a);
        path
    };
    add("linked", &["-b", "unit/linked"]);
    add("detached", &["--detach"]);
    let gone = add("gone", &["-b", "unit/gone"]);
    std::fs::remove_dir_all(&gone).unwrap();
    let locked = add("locked", &["-b", "unit/locked"]);
    repo.git(&["worktree", "lock", locked.to_str().unwrap()]);
    let held = add("held", &["-b", "unit/held"]);
    repo.git(&[
        "worktree",
        "lock",
        "--reason",
        "away",
        held.to_str().unwrap(),
    ]);
    std::fs::remove_dir_all(&held).unwrap();
    let p = places(&repo);

    let out = repo.git_raw(&["worktree", "list", "--porcelain", "-z"]);
    assert!(out.status.success());
    let canon = |c: &mut sluice_seams::CheckedOut| {
        if let Ok(p) = std::fs::canonicalize(&c.worktree) {
            c.worktree = p;
        }
    };
    let mut listed = sluice::profiles::places::parse_worktree_list(&out.stdout);
    listed.iter_mut().for_each(canon);
    listed.sort_by(|a, b| a.worktree.cmp(&b.worktree));
    let mut read = p.checked_out().unwrap();
    read.sort_by(|a, b| a.worktree.cmp(&b.worktree));
    assert_eq!(read, listed);
    let mut branches: Vec<&str> = read.iter().map(|c| c.branch.as_str()).collect();
    branches.sort();
    assert_eq!(
        branches,
        ["main", "unit/held", "unit/linked", "unit/locked"]
    );
}

/// Drop git's own entry for the worktree `folder`, as git's removal that
/// stopped part way leaves it (deneb, 2026-11-10: `Directory not empty`,
/// then `is not a working tree`): the entry its `.git` file names is
/// removed, the folder stays, and `git worktree list` no longer names it.
fn drop_entry(repo: &Repo, folder: &std::path::Path) {
    let dot_git = std::fs::read_to_string(folder.join(".git")).unwrap();
    let entry = dot_git.trim().strip_prefix("gitdir: ").unwrap();
    std::fs::remove_dir_all(entry).unwrap();
    assert!(folder.is_dir());
    let real = std::fs::canonicalize(folder).unwrap();
    assert!(
        !worktrees(repo)
            .iter()
            .any(|w| std::path::Path::new(w) == folder || std::path::Path::new(w) == real),
        "git still lists {}",
        folder.display()
    );
}

/// 123, record 0115: a combination's checkout or a preview whose folder
/// git no longer lists is gone, whatever is left in the folder; one git
/// lists under another spelling of its path is not; a worktree's place git
/// no longer lists is not gone while its folder is there.
#[test]
fn a_checkout_git_no_longer_lists_is_gone() {
    let _ports = ports_lock();
    let (repo, rev) = scratch("places-unlisted");
    let blocks = support::Scratch::new("places-unlisted-blocks");
    let link = repo.dir().with_extension("link");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(repo.dir(), &link).unwrap();
    // A place made through the link: its record spells its path so, while
    // git lists the folder by the path it resolves to.
    let common = link.join(".git");
    let through = Places::new(
        &common,
        Rc::new(GitWorktrees::new(&common)),
        Rc::new(runs(&repo)),
    )
    .with_scratch_root(user_dir(&repo))
    .with_ports(blocks.path().to_path_buf());
    let place = through.materialize(NAME, &rev, &Settings::new()).unwrap();
    assert!(place.path.starts_with(&link), "{}", place.path.display());
    assert!(
        !worktrees(&repo).contains(&place.path.display().to_string()),
        "git lists it as spelled"
    );
    assert!(!through.gone(NAME).unwrap());
    let p = places(&repo).with_ports(blocks.path().to_path_buf());
    assert!(!p.gone(NAME).unwrap());

    drop_entry(&repo, &place.path);
    assert!(through.gone(NAME).unwrap());
    assert!(p.gone(NAME).unwrap());

    // A preview.
    let name = preview_name(&rev);
    let (preview, _) = p
        .preview(
            &name,
            &rev,
            "main",
            &ported(&["web"], &ports(1).to_string()),
            &BTreeMap::new(),
            "now",
        )
        .unwrap();
    assert!(!p.previews().unwrap()[0].gone);
    drop_entry(&repo, &preview.path);
    let listed = p.previews().unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].gone, "{:?}", listed[0]);
    assert!(p.gone(&name).unwrap());

    // A worktree's place keeps its reading: not gone while its folder is.
    let session = repo.dir().with_extension("session");
    let adopted = p
        .adopt(session.to_str().unwrap(), "bolt/x", &Settings::new(), "now")
        .unwrap();
    drop_entry(&repo, &session);
    let held = p.session_places().unwrap();
    assert_eq!(held.len(), 1);
    assert!(!held[0].gone);
    assert!(!p.gone(&adopted.name).unwrap());

    for n in [NAME, name.as_str(), adopted.name.as_str()] {
        p.free(n).unwrap();
    }
    assert!(p.made().unwrap().is_empty());
    assert!(p.previews().unwrap().is_empty());
    assert!(p.session_places().unwrap().is_empty());
    std::fs::remove_file(&link).unwrap();
}
