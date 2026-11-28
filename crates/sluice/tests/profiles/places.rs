//! The place seam's profile on scratch repositories: section 5's
//! acceptance tests in `docs/plans/phase-1-integrations.md`, for the
//! one method, `git-worktree`. Needs git on the path, so it is in the
//! check-tools tier.

#![cfg(feature = "tools")]

#[path = "../support/mod.rs"]
mod support;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use sluice::profiles::places::{GitWorktrees, Places};
use sluice::runs::{Request, Runs};
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

fn method(m: &str) -> Settings {
    Settings::from([("method".into(), Setting::Text(m.into()))])
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

/// 34: releasing a place ends the run in it before removing it.
#[test]
fn releasing_a_place_ends_its_run() {
    let _spawns = spawns();
    let (repo, rev) = scratch("places-run");
    let p = places(&repo);
    let place = p.materialize(NAME, &rev, &Settings::new()).unwrap();
    let r = runs(&repo);
    r.start(
        NAME,
        &Request {
            argv: vec!["/bin/sh".into(), "-c".into(), "sleep 300".into()],
            cwd: place.path.clone(),
            env: BTreeMap::new(),
            limit: None,
            grace: 1,
            meta: serde_json::Value::Null,
            release: None,
            watch: None,
        },
    )
    .unwrap();
    let pgid_file = r.dir(NAME).join("pgid");
    let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !pgid_file.exists() {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let pgid: i32 = std::fs::read_to_string(&pgid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    p.release(NAME).unwrap();
    assert_eq!(
        nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pgid), None),
        Err(nix::errno::Errno::ESRCH)
    );
    assert!(!r.dir(NAME).exists());
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

/// A second session's worktree off main, `<repo>.<name>`.
fn another_session(repo: &Repo, name: &str) -> PathBuf {
    let path = repo.dir().with_extension(name);
    let _ = std::fs::remove_dir_all(&path);
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        &format!("unit/{name}"),
        path.to_str().unwrap(),
        "main",
    ]);
    path
}

fn preview_name(rev: &Rev) -> String {
    format!("preview/{}", &rev.as_str()[..12])
}

/// ADR 0024: every worktree with a branch checked out, with its branch
/// and head; a detached one, a candidate's place among them, is left out.
#[test]
fn checked_out_lists_the_worktrees_on_a_branch() {
    let _spawns = spawns();
    let (repo, cand) = scratch("checked-out");
    // The repository's own worktree back on main, as `<repo>/main` is.
    repo.git(&["switch", "-q", "main"]);
    let p = places(&repo);
    let detached = repo.dir().with_extension("view");
    let _ = std::fs::remove_dir_all(&detached);
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "--detach",
        detached.to_str().unwrap(),
        "main",
    ]);
    p.materialize(NAME, &cand, &method("git-worktree")).unwrap();

    let mut found = p.checked_out().unwrap();
    found.sort_by(|a, b| a.branch.cmp(&b.branch));
    let names: Vec<(&str, PathBuf)> = found
        .iter()
        .map(|c| (c.branch.as_str(), c.worktree.clone()))
        .collect();
    let canon = |p: &std::path::Path| std::fs::canonicalize(p).unwrap();
    assert_eq!(
        names,
        vec![
            ("main", canon(repo.dir())),
            ("unit/a", canon(&repo.dir().with_extension("session"))),
        ]
    );
    assert_eq!(found[0].head.0, repo.rev("main"));
    p.release(NAME).unwrap();
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

/// Where a candidate's or a session's place's record is.
fn record_path(repo: &Repo, name: &str) -> PathBuf {
    repo.dir().join(format!(
        ".git/sluice/.places/{}.json",
        sluice::runs::flat_name(name)
    ))
}

/// A place's record, as it is written.
fn record_of(repo: &Repo, name: &str) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(record_path(repo, name)).unwrap()).unwrap()
}

/// The repository's directory under the one standing for the user's.
fn repository_dir(repo: &Repo) -> PathBuf {
    user_dir(repo).join(sluice::profiles::places::repository_key(
        &repo.dir().join(".git"),
    ))
}

/// Whether `path` is the repository's directory under the user's and 10
/// hex digits.
fn short_scratch(repo: &Repo, path: &std::path::Path) -> bool {
    let name = path.file_name().unwrap().to_string_lossy();
    path.parent() == Some(&repository_dir(repo))
        && name.len() == 10
        && name.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

/// Whether `dir` is a directory of this user's own, closed to everyone
/// else.
fn users_own(dir: &std::path::Path) -> bool {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    let meta = std::fs::symlink_metadata(dir).unwrap();
    meta.is_dir()
        && meta.uid() == nix::unistd::geteuid().as_raw()
        && meta.permissions().mode() & 0o777 == 0o700
}

/// A session's place adopted for the worktree `worktree`'s run.
fn adopted(p: &Places, worktree: &std::path::Path) -> anyhow::Result<Place> {
    p.adopt(
        worktree.to_str().unwrap(),
        "bolt/x",
        &Settings::new(),
        "2026-11-03T09:00:00Z",
    )
}

/// 35 as amended, rule core.30: a worktree's checkout made for its first
/// run, with neither the user's scratch directory nor the repository's
/// there, makes both, the user's own and closed to everyone else, and its
/// scratch is under them.
#[test]
fn the_users_scratch_directory_is_made_its_own() {
    let _spawns = spawns();
    let (repo, _) = scratch("places-scratch-made");
    let p = places(&repo);
    assert!(!user_dir(&repo).exists());
    let place = adopted(&p, &repo.dir().with_extension("session")).unwrap();
    for dir in [user_dir(&repo), repository_dir(&repo)] {
        assert!(users_own(&dir), "{}", dir.display());
    }
    assert!(short_scratch(&repo, &place.tmp), "{}", place.tmp.display());
    assert!(place.tmp.is_dir());
    p.release(&place.name).unwrap();
}

/// 35 as amended, rule core.30: where the user's scratch directory is a
/// link, or a directory others may enter, no checkout is made under it,
/// the error names it and says what is wrong, and it is as it was, its
/// mode included.
#[test]
fn a_scratch_directory_not_the_users_own_is_refused() {
    use std::os::unix::fs::PermissionsExt as _;
    let _spawns = spawns();
    let (repo, _) = scratch("places-scratch-refused");
    let p = places(&repo);
    let session = repo.dir().with_extension("session");
    let user = user_dir(&repo);
    let elsewhere = user.with_file_name("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();

    std::os::unix::fs::symlink(&elsewhere, &user).unwrap();
    let e = format!("{:#}", adopted(&p, &session).unwrap_err());
    assert!(
        e.contains(&format!(
            "{} is not a directory of this user's own, closed to everyone else: is a link",
            user.display()
        )),
        "{e}"
    );
    assert_eq!(std::fs::read_link(&user).unwrap(), elsewhere);
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
    let name = adopted_name(&session);
    p.release(&name).unwrap();
    std::fs::remove_file(&user).unwrap();

    std::fs::create_dir(&user).unwrap();
    std::fs::set_permissions(&user, std::fs::Permissions::from_mode(0o755)).unwrap();
    let e = format!("{:#}", adopted(&p, &session).unwrap_err());
    assert!(
        e.contains(&format!(
            "{} is not a directory of this user's own, closed to everyone else: may be entered by others",
            user.display()
        )),
        "{e}"
    );
    let mode = std::fs::metadata(&user).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o755);
    assert_eq!(std::fs::read_dir(&user).unwrap().count(), 0);
    p.release(&name).unwrap();
}

/// The name of the session's place of the worktree `worktree`.
fn adopted_name(worktree: &std::path::Path) -> String {
    format!(
        "session/{}",
        std::fs::canonicalize(worktree).unwrap().display()
    )
}

/// 35 as amended, rule core.30: a worktree's scratch, the repository's
/// directory and the user's, removed between two runs as a restart or the
/// system's cleaning removes them, are made again, the scratch at the path
/// its record names and both directories the user's own.
#[test]
fn a_scratch_the_system_emptied_is_made_again() {
    let _spawns = spawns();
    let (repo, _) = scratch("places-scratch-emptied");
    let p = places(&repo);
    let session = repo.dir().with_extension("session");
    let first = adopted(&p, &session).unwrap();
    std::fs::remove_dir_all(user_dir(&repo)).unwrap();

    let second = adopted(&p, &session).unwrap();
    assert_eq!(second.tmp, first.tmp);
    assert!(second.tmp.is_dir());
    for dir in [user_dir(&repo), repository_dir(&repo)] {
        assert!(users_own(&dir), "{}", dir.display());
    }
    p.release(&second.name).unwrap();
}

/// 35 as amended, rule core.30: two repositories' checkouts of one name,
/// under one user's directory, are given scratch paths of the same length
/// that differ in the repository's 10 hex digits alone.
#[test]
fn two_repositories_keep_their_scratch_apart() {
    let _spawns = spawns();
    let (one, one_rev) = scratch("places-scratch-one");
    let (two, two_rev) = scratch("places-scratch-two");
    let user = user_dir(&one);
    let a = places(&one).with_scratch_root(user.clone());
    let b = places(&two).with_scratch_root(user.clone());
    let at_a = a.materialize(NAME, &one_rev, &Settings::new()).unwrap();
    let at_b = b.materialize(NAME, &two_rev, &Settings::new()).unwrap();
    assert_eq!(at_a.tmp.as_os_str().len(), at_b.tmp.as_os_str().len());
    assert_ne!(at_a.tmp, at_b.tmp);
    assert_eq!(at_a.tmp.file_name(), at_b.tmp.file_name());
    let (ka, kb) = (at_a.tmp.parent().unwrap(), at_b.tmp.parent().unwrap());
    assert_eq!(ka.parent(), Some(user.as_path()));
    assert_eq!(kb.parent(), Some(user.as_path()));
    assert_ne!(ka.file_name(), kb.file_name());
    a.release(NAME).unwrap();
    b.release(NAME).unwrap();
}

/// 35 as amended, 34, rule core.30: a candidate's checkout's record names
/// its scratch, under the repository's directory in the user's and 10 hex
/// digits long, and its checks are given that directory; its release
/// removes the directory and the record.
#[test]
fn a_place_names_its_scratch_on_its_record() {
    let (repo, rev) = scratch("places-scratch-record");
    let p = places(&repo);
    let place = p.materialize(NAME, &rev, &Settings::new()).unwrap();
    let named = PathBuf::from(record_of(&repo, NAME)["scratch"].as_str().unwrap());
    assert!(short_scratch(&repo, &named), "{}", named.display());
    assert_eq!(place.tmp, named);
    assert!(place.tmp.is_dir());

    p.release(NAME).unwrap();
    assert!(!place.tmp.exists(), "{} left", place.tmp.display());
    assert!(!record_path(&repo, NAME).exists());
}

/// 35 as amended, 34, rule core.30: a worktree is given one scratch on
/// every run, under the repository's directory in the user's, of the same
/// length as another worktree's and not the same, holding nothing of the
/// worktree's path.
#[test]
fn a_worktree_keeps_one_short_scratch() {
    let (repo, _) = scratch("places-scratch-keep");
    let p = places(&repo);
    let session = repo.dir().with_extension("session");
    let other = another_session(&repo, "other-with-a-longer-name");
    let adopt = |w: &PathBuf| {
        p.adopt(
            w.to_str().unwrap(),
            "bolt/x",
            &Settings::new(),
            "2026-10-27T09:00:00Z",
        )
        .unwrap()
    };
    let (a, b) = (adopt(&session), adopt(&other));
    assert_eq!(adopt(&session).tmp, a.tmp);
    assert_eq!(adopt(&other).tmp, b.tmp);
    assert_ne!(a.tmp, b.tmp);
    for (place, worktree) in [(&a, &session), (&b, &other)] {
        assert!(short_scratch(&repo, &place.tmp), "{}", place.tmp.display());
        let tmp = place.tmp.to_string_lossy();
        let worktree = std::fs::canonicalize(worktree).unwrap();
        assert!(!tmp.contains(worktree.to_str().unwrap()), "{tmp}");
        let own = worktree.file_name().unwrap().to_string_lossy();
        assert!(!tmp.contains(own.as_ref()), "{tmp}");
    }
    p.release(&a.name).unwrap();
    p.release(&b.name).unwrap();
}

/// A session's record for the worktree `session`, as an older sluice
/// wrote it, `scratch` naming its scratch or none, and its scratch made
/// at `tmp` holding a file `kept`: the place's name.
fn session_from_before(
    repo: &Repo,
    session: &std::path::Path,
    scratch: Option<&std::path::Path>,
    tmp: &std::path::Path,
) -> String {
    let name = format!("session/{}", session.display());
    let mut record = serde_json::json!({
        "kind": "session",
        "method": "session",
        "branch": "bolt/x",
        "path": session,
    });
    if let Some(s) = scratch {
        record["scratch"] = serde_json::json!(s);
    }
    let path = record_path(repo, &name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, record.to_string()).unwrap();
    std::fs::create_dir_all(tmp).unwrap();
    std::fs::write(tmp.join("kept"), "").unwrap();
    name
}

/// 35 as amended, 34, rule core.30 (record 0063 as amended 2026-11-05):
/// a worktree's checkout an older sluice adopted, whose record names no
/// scratch, takes a scratch under the repository's directory in the
/// user's at its next run, its record naming it and its former scratch,
/// at the checkout's name as one directory name, moved there with what it
/// held.
#[test]
fn a_place_from_before_takes_the_short_scratch_at_its_next_run() {
    let _spawns = spawns();
    let (repo, _) = scratch("places-scratch-before");
    let p = places(&repo);
    let session = std::fs::canonicalize(repo.dir().with_extension("session")).unwrap();
    let former = repo
        .dir()
        .join(".git/sluice/.tmp")
        .join(sluice::runs::dir_name(&format!(
            "session/{}",
            session.display()
        )));
    let name = session_from_before(&repo, &session, None, &former);

    let place = adopted(&p, &session).unwrap();
    assert_eq!(place.name, name);
    assert!(short_scratch(&repo, &place.tmp), "{}", place.tmp.display());
    assert_eq!(
        record_of(&repo, &name)["scratch"].as_str(),
        place.tmp.to_str()
    );
    assert!(place.tmp.join("kept").exists(), "its scratch was emptied");
    assert!(!former.exists(), "{} left", former.display());
    p.release(&name).unwrap();
    assert!(!place.tmp.exists(), "{} left", place.tmp.display());
}

/// 35 as amended, 34, rule core.30 (record 0063 as amended 2026-11-05):
/// a checkout whose record names a scratch under the repository's
/// `sluice/.tmp/`, as a sluice before the user's directory made it. A
/// worktree's next run is given a scratch under the user's directory,
/// its record naming it and its former scratch moved there with what it
/// held, and the run after is given the same. A candidate's release
/// removes its scratch where its record names, its checkout and its
/// record.
#[test]
fn a_place_in_the_repository_takes_the_short_scratch_at_its_next_run() {
    let _spawns = spawns();
    let (repo, rev) = scratch("places-scratch-in-repository");
    let p = places(&repo);
    let root = repo.dir().join(".git/sluice/.tmp");

    let session = std::fs::canonicalize(repo.dir().with_extension("session")).unwrap();
    let kept = root.join("fedcba9876");
    let name = session_from_before(&repo, &session, Some(&kept), &kept);
    let place = adopted(&p, &session).unwrap();
    assert_eq!(place.name, name);
    assert!(short_scratch(&repo, &place.tmp), "{}", place.tmp.display());
    assert!(users_own(&repository_dir(&repo)));
    assert_eq!(
        record_of(&repo, &name)["scratch"].as_str(),
        place.tmp.to_str()
    );
    assert!(place.tmp.join("kept").exists(), "its scratch was emptied");
    assert!(!kept.exists(), "{} left", kept.display());
    assert_eq!(adopted(&p, &session).unwrap().tmp, place.tmp);
    p.release(&name).unwrap();
    assert!(!place.tmp.exists(), "{} left", place.tmp.display());

    let checkout = repo
        .dir()
        .join(".git/sluice/.places")
        .join(sluice::runs::dir_name(NAME));
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "--detach",
        checkout.to_str().unwrap(),
        rev.as_str(),
    ]);
    let candidates = root.join("0123456789");
    let path = record_path(&repo, NAME);
    std::fs::write(
        &path,
        serde_json::json!({
            "kind": "made",
            "method": "git-worktree",
            "path": checkout,
            "scratch": candidates,
        })
        .to_string(),
    )
    .unwrap();
    std::fs::create_dir_all(&candidates).unwrap();
    let listed = |path: &std::path::Path| {
        repo.git(&["worktree", "list"])
            .contains(&path.display().to_string())
    };
    assert!(listed(&checkout));
    p.release(NAME).unwrap();
    assert!(!candidates.exists(), "{} left", candidates.display());
    assert!(!record_path(&repo, NAME).exists());
    assert!(!checkout.exists(), "{} left", checkout.display());
    assert!(!listed(&checkout));
    assert!(p.made().unwrap().is_empty());
}

/// 35 as amended, rule core.30 (record 0063 as amended 2026-11-05): while
/// a run of a worktree whose record names a scratch under the
/// repository's `sluice/.tmp/` is going, a run beside it is given that
/// scratch, which the going run's sandbox writes, and the record keeps
/// naming it; the first run once it has ended takes the short scratch,
/// with what the former held.
#[test]
fn a_place_with_a_run_going_keeps_its_former_scratch_for_it() {
    let _spawns = spawns();
    let (repo, _) = scratch("places-scratch-going");
    let p = places(&repo);
    let session = std::fs::canonicalize(repo.dir().with_extension("session")).unwrap();
    let kept = repo.dir().join(".git/sluice/.tmp/fedcba9876");
    let name = session_from_before(&repo, &session, Some(&kept), &kept);
    let r = runs(&repo);
    let going = r.next_run(&name).unwrap();
    r.start(
        &going,
        &Request {
            argv: vec!["/bin/sh".into(), "-c".into(), "sleep 300".into()],
            cwd: session.clone(),
            env: BTreeMap::new(),
            limit: None,
            grace: 1,
            meta: serde_json::Value::Null,
            release: None,
            watch: None,
        },
    )
    .unwrap();

    let beside = adopted(&p, &session).unwrap();
    assert_eq!(beside.tmp, kept);
    assert_eq!(record_of(&repo, &name)["scratch"].as_str(), kept.to_str());
    assert!(kept.join("kept").exists(), "its scratch was emptied");

    r.end(&going).unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while r.any_going(&name).unwrap() {
        assert!(std::time::Instant::now() < until, "the run never ended");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let next = adopted(&p, &session).unwrap();
    assert!(short_scratch(&repo, &next.tmp), "{}", next.tmp.display());
    assert!(next.tmp.join("kept").exists(), "its scratch was emptied");
    assert!(!kept.exists(), "{} left", kept.display());
    p.release_session(&name, None).unwrap();
}

/// 35 as amended, rule core.30 (record 0063 as amended 2026-11-05): a
/// worktree's former scratch that cannot be renamed to its short path,
/// where a directory that is not empty stands, is removed, and the run is
/// given the short path, its record naming it.
#[test]
fn a_former_scratch_that_cannot_be_moved_is_removed() {
    let _spawns = spawns();
    let (repo, _) = scratch("places-scratch-unmoved");
    let p = places(&repo);
    let session = std::fs::canonicalize(repo.dir().with_extension("session")).unwrap();
    let kept = repo.dir().join(".git/sluice/.tmp/fedcba9876");
    let name = session_from_before(&repo, &session, Some(&kept), &kept);
    let fresh = adopted(&p, &another_session(&repo, "fresh")).unwrap();
    let standing = fresh.tmp.with_file_name(
        // The short path the worktree's place is given: the same
        // repository's directory, and the 10 hex digits of its name.
        sluice::profiles::places::scratch_name(&name),
    );
    std::fs::create_dir_all(&standing).unwrap();
    std::fs::write(standing.join("standing"), "").unwrap();

    let place = adopted(&p, &session).unwrap();
    assert_eq!(place.tmp, standing);
    assert_eq!(
        record_of(&repo, &name)["scratch"].as_str(),
        standing.to_str()
    );
    assert!(!kept.exists(), "{} left", kept.display());
    p.release(&name).unwrap();
    p.release(&fresh.name).unwrap();
}

/// What `run` warned of, each warning's words, beside what it returned.
fn warnings<T>(run: impl FnOnce() -> T) -> (T, Vec<String>) {
    use tracing_subscriber::layer::SubscriberExt as _;
    #[derive(Clone, Default)]
    struct Warned(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
    struct Words(String);
    impl tracing::field::Visit for Words {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if field.name() == "message" {
                self.0 = format!("{value:?}");
            }
        }
    }
    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Warned {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _: tracing_subscriber::layer::Context<'_, S>,
        ) {
            if *event.metadata().level() == tracing::Level::WARN {
                let mut words = Words(String::new());
                event.record(&mut words);
                self.0.lock().unwrap().push(words.0);
            }
        }
    }
    let warned = Warned::default();
    let subscriber = tracing_subscriber::registry().with(warned.clone());
    let out = tracing::subscriber::with_default(subscriber, run);
    let seen = warned.0.lock().unwrap().clone();
    (out, seen)
}

/// `dir` made as sluice makes the user's scratch directory, the user's
/// own and closed to everyone else.
fn make_users(dir: &std::path::Path) {
    use std::os::unix::fs::DirBuilderExt as _;
    std::fs::DirBuilder::new().mode(0o700).create(dir).unwrap();
}

/// 35 as amended, 123, rule core.30: of the repository's scratch
/// directories, under its directory in the user's and under its former
/// `sluice/.tmp/`, only those no record names are removed; a held
/// worktree's checkout's scratch, a scratch an older record names under
/// `.tmp/`, and the former-name scratch of a checkout whose record names
/// none are left as they were.
#[test]
fn scratch_no_record_names_is_removed() {
    let (repo, _) = scratch("places-scratch-sweep");
    let p = places(&repo);
    let held = adopted(&p, &repo.dir().with_extension("session")).unwrap();
    std::fs::write(held.tmp.join("socket"), "").unwrap();
    let root = repo.dir().join(".git/sluice/.tmp");

    let older = std::fs::canonicalize(another_session(&repo, "older")).unwrap();
    let older_name = format!("session/{}", older.display());
    let older_tmp = root.join("fedcba9876");
    std::fs::write(
        record_path(&repo, &older_name),
        serde_json::json!({
            "kind": "session",
            "method": "session",
            "branch": "bolt/x",
            "path": older,
            "scratch": older_tmp,
        })
        .to_string(),
    )
    .unwrap();
    std::fs::create_dir_all(&older_tmp).unwrap();
    std::fs::write(older_tmp.join("kept"), "").unwrap();

    let before = std::fs::canonicalize(another_session(&repo, "before")).unwrap();
    let name = format!("session/{}", before.display());
    std::fs::write(
        record_path(&repo, &name),
        serde_json::json!({
            "kind": "session",
            "method": "session",
            "branch": "bolt/x",
            "path": before,
        })
        .to_string(),
    )
    .unwrap();
    let former = root.join(sluice::runs::dir_name(&name));
    std::fs::create_dir_all(&former).unwrap();
    std::fs::write(former.join("kept"), "").unwrap();

    let users_stray = repository_dir(&repo).join("abcdef0123");
    std::fs::create_dir_all(users_stray.join("deep")).unwrap();
    let stray = root.join("0123abcdef");
    std::fs::create_dir_all(stray.join("deep")).unwrap();

    assert_eq!(
        p.remove_unnamed_scratch().unwrap(),
        vec![users_stray.clone(), stray.clone()]
    );
    assert!(!users_stray.exists() && !stray.exists());
    assert!(
        held.tmp.join("socket").exists(),
        "a held scratch was removed"
    );
    assert!(
        older_tmp.join("kept").exists(),
        "an older record's scratch was removed"
    );
    assert!(former.join("kept").exists(), "a former scratch was removed");
    assert_eq!(p.remove_unnamed_scratch().unwrap(), Vec::<PathBuf>::new());

    p.release(&held.name).unwrap();
    p.release(&older_name).unwrap();
    p.release(&name).unwrap();
    assert!(!older_tmp.exists());
    assert!(!former.exists());
}

/// 35 as amended, 123, rule core.30: a checkout's record that exists and
/// cannot be read names its scratch still: the scratch of the place its
/// file name reads back as is kept, of either form, while a directory no
/// record names, in either of the repository's scratch roots, is removed,
/// and the record is warned of by its path; and while such a record's
/// file name was cut short, so that it names no place, nothing is
/// removed. The sweep succeeds both times.
#[test]
fn an_unreadable_record_keeps_its_scratch() {
    let (repo, rev) = scratch("places-scratch-unreadable");
    let p = places(&repo);
    let held = p.materialize(NAME, &rev, &Settings::new()).unwrap();
    std::fs::write(held.tmp.join("socket"), "").unwrap();
    let record = record_path(&repo, NAME);
    let written = std::fs::read(&record).unwrap();
    std::fs::write(&record, "{\"kind\":").unwrap();
    let root = repo.dir().join(".git/sluice/.tmp");
    let former = root.join(sluice::runs::dir_name(NAME));
    std::fs::create_dir_all(&former).unwrap();
    let users_stray = repository_dir(&repo).join("abcdef0123");
    std::fs::create_dir_all(&users_stray).unwrap();
    let stray = root.join("0123abcdef");
    std::fs::create_dir_all(&stray).unwrap();

    let (removed, warned) = warnings(|| p.remove_unnamed_scratch().unwrap());
    assert_eq!(removed, vec![users_stray.clone(), stray.clone()]);
    let reading = format!(
        "reading {}, whose checkout's scratch is kept: ",
        record.display()
    );
    assert!(warned.iter().any(|w| w.starts_with(&reading)), "{warned:?}");
    assert!(
        held.tmp.join("socket").exists(),
        "an unreadable record's scratch was removed"
    );
    assert!(
        former.is_dir(),
        "an unreadable record's former scratch was removed"
    );

    let cut = repo.dir().join(format!(
        ".git/sluice/.places/{}%~0123456789abcdef.json",
        "x".repeat(160)
    ));
    std::fs::write(&cut, "").unwrap();
    let strays = [
        repository_dir(&repo).join("fedcba9876"),
        root.join("fedcba9876"),
        root.join("session+2F+2Fgone"),
    ];
    for s in &strays {
        std::fs::create_dir_all(s).unwrap();
    }
    assert_eq!(p.remove_unnamed_scratch().unwrap(), Vec::<PathBuf>::new());
    for s in &strays {
        assert!(s.is_dir(), "{} was removed", s.display());
    }

    std::fs::remove_file(&cut).unwrap();
    std::fs::write(&record, written).unwrap();
    p.release(NAME).unwrap();
    assert!(!held.tmp.exists());
}

/// 35 as amended, 123, rule core.30: a repository's sweep weighs only its
/// own directory under the user's: another repository's directory there,
/// and a directory in the user's own itself, are left as they were though
/// no record of the first names them, and the other repository's sweep
/// removes its own.
#[test]
fn another_repositorys_scratch_is_left() {
    let _spawns = spawns();
    let one = Repo::new("places-sweep-one");
    let two = Repo::new("places-sweep-two");
    let user = user_dir(&one);
    let a = places(&one);
    let b = places(&two).with_scratch_root(user.clone());
    make_users(&user);
    let ones = repository_dir(&one).join("abcdef0123");
    let twos = user
        .join(sluice::profiles::places::repository_key(
            &two.dir().join(".git"),
        ))
        .join("abcdef0123");
    let top = user.join("0123456789");
    for dir in [&ones, &twos, &top] {
        std::fs::create_dir_all(dir).unwrap();
    }

    assert_eq!(a.remove_unnamed_scratch().unwrap(), vec![ones.clone()]);
    assert!(!ones.exists());
    assert!(twos.is_dir(), "another repository's scratch was removed");
    assert!(top.is_dir(), "the user's directory was swept");
    assert_eq!(b.remove_unnamed_scratch().unwrap(), vec![twos.clone()]);
    assert!(top.is_dir(), "the user's directory was swept");
}

/// 35 as amended, 123, rule core.30: where the user's scratch directory
/// may be entered by others, the sweep removes nothing under it and warns
/// naming it, as its making refuses it, and still removes what no record
/// names under the repository's `sluice/.tmp/`; the directory is as it
/// was, its mode included.
#[test]
fn a_users_directory_not_its_own_is_not_swept() {
    use std::os::unix::fs::PermissionsExt as _;
    let _spawns = spawns();
    let repo = Repo::new("places-sweep-open");
    let p = places(&repo);
    let user = user_dir(&repo);
    std::fs::create_dir(&user).unwrap();
    std::fs::set_permissions(&user, std::fs::Permissions::from_mode(0o755)).unwrap();
    let users_stray = repository_dir(&repo).join("abcdef0123");
    std::fs::create_dir_all(&users_stray).unwrap();
    let stray = repo.dir().join(".git/sluice/.tmp/0123abcdef");
    std::fs::create_dir_all(&stray).unwrap();

    let (removed, warned) = warnings(|| p.remove_unnamed_scratch().unwrap());
    assert_eq!(removed, vec![stray.clone()]);
    assert!(!stray.exists());
    assert!(
        users_stray.is_dir(),
        "a directory others may enter was swept"
    );
    let mode = std::fs::metadata(&user).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o755);
    let refusal = format!(
        "{} is not a directory of this user's own, closed to everyone else: may be entered by others",
        user.display()
    );
    assert!(warned.iter().any(|w| w.contains(&refusal)), "{warned:?}");
}
