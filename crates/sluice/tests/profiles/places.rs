//! The place seam's profile on scratch repositories: section 5's
//! acceptance tests in `docs/plans/phase-1-integrations.md`, for the
//! one method, `git-worktree`. Needs git on the path, so it is in the
//! check-tools tier.

#![cfg(feature = "tools")]

#[path = "../support/mod.rs"]
mod support;

use std::collections::BTreeMap;
use std::path::PathBuf;

use sluice::profiles::places::Places;
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
    Places::new(&common, runs(repo)).with_scratch_root(user_dir(repo))
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

/// 33 to 35, 25, rule core.16: git-worktree.
#[test]
fn a_git_worktree_place_is_the_candidate_and_leaves_nothing() {
    let _spawns = spawns();
    let (repo, rev) = scratch("places-git");
    let before = others(&repo);
    let p = places(&repo);
    let place = p.materialize(NAME, &rev, &Settings::new()).unwrap();
    holds_the_candidate(&repo, &place, &rev);
    assert!(place
        .path
        .starts_with(repo.dir().join(".git/sluice/.places")));
    // clang and other tools read every `%` of a temporary file's path as a
    // random character, so the paths checks work in hold none.
    // The directories sluice names, not the scratch repository's own.
    for dir in [&place.path, &place.tmp] {
        let name = dir.file_name().unwrap().to_string_lossy();
        assert!(!name.contains('%'), "{}", dir.display());
    }
    // The method runs nothing in the place: no post-checkout hook.
    assert!(!repo.dir().join(".git/post-checkout.log").exists());

    p.release(NAME).unwrap();
    left_behind(&repo, &place);
    p.release(NAME).unwrap();
    p.release("sluice/try/bolt/x/never").unwrap();
    assert_eq!(others(&repo), before);
}

/// 25, 59: a place left by a step that stopped part way is made again
/// under its name; one whose checkout was deleted by hand is released.
#[test]
fn a_git_worktree_place_left_by_a_crash() {
    let _spawns = spawns();
    let (repo, rev) = scratch("places-git-crash");
    let p = places(&repo);
    let first = p.materialize(NAME, &rev, &Settings::new()).unwrap();
    std::fs::write(first.path.join("debris"), "x").unwrap();
    let again = p.materialize(NAME, &rev, &Settings::new()).unwrap();
    holds_the_candidate(&repo, &again, &rev);
    std::fs::remove_dir_all(&again.path).unwrap();
    p.release(NAME).unwrap();
    left_behind(&repo, &again);
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

/// Section 5's acceptance test 8: a method other than `"git-worktree"`
/// is refused before anything is made (ADR 0013).
#[test]
fn an_unknown_method_is_refused() {
    let _spawns = spawns();
    let (repo, rev) = scratch("places-method");
    let p = places(&repo);
    for m in [method("worktrunk"), method("archive"), method("rsync")] {
        assert!(p.materialize(NAME, &rev, &m).is_err());
    }
    assert!(!repo
        .dir()
        .join(".git/sluice/.places")
        .join("sluice%2Ftry%2Fbolt%2Fx%2F1")
        .exists());
}

/// ADR 0013, rule core.15: `[checkout]` names no method, and an older
/// revision's policy that names the one there is still makes a place.
#[test]
fn a_policy_need_not_name_the_method() {
    let _spawns = spawns();
    let (repo, rev) = scratch("places-no-method");
    let p = places(&repo);
    for m in [Settings::new(), method("git-worktree")] {
        let place = p.materialize(NAME, &rev, &m).unwrap();
        holds_the_candidate(&repo, &place, &rev);
        p.release(NAME).unwrap();
        assert!(!place.path.exists());
    }
}

/// A candidate off main with `Cargo.lock` as `lock`.
fn locked(repo: &Repo, lock: &str) -> Rev {
    let rev = repo.branch(
        "cand5",
        "main",
        &[
            ("src/lib", Some("candidate\n")),
            ("Cargo.lock", Some(lock)),
            (".gitignore", Some("target\n")),
        ],
    );
    repo.git(&["branch", "-D", "cand5"]);
    Rev::new(rev)
}

/// Build `target` in `place`, as a check would.
fn build(place: &Place) {
    std::fs::create_dir_all(place.path.join("target/debug")).unwrap();
    std::fs::write(place.path.join("target/debug/a"), "built\n").unwrap();
}

/// 35; record 0049 as amended: a place holds its candidate and nothing
/// restored from another; one released as passed saves nothing, so the
/// next of the same lock file starts with no `target`. The caches an
/// earlier sluice saved go when a place is made, and nothing else of
/// sluice's beside them.
#[test]
fn a_checkout_starts_with_nothing_but_its_revision() {
    let _spawns = spawns();
    let (repo, _) = scratch("places-no-cache");
    let p = places(&repo);
    let one = locked(&repo, "one\n");
    let root = repo.dir().join(".git/sluice");
    let saved = root.join(".caches/bucket/key/target/debug");
    std::fs::create_dir_all(&saved).unwrap();
    std::fs::write(saved.join("a"), "saved by an earlier sluice\n").unwrap();
    // `caches`, without the dot, may be a branch's records.
    std::fs::create_dir_all(root.join("caches")).unwrap();
    std::fs::write(root.join("caches/version"), "1\n").unwrap();

    let place = p.materialize(NAME, &one, &Settings::new()).unwrap();
    holds_the_candidate(&repo, &place, &one);
    assert!(!place.path.join("target").exists());
    assert!(!root.join(".caches").exists(), "the saved caches are gone");
    assert!(root.join("caches/version").exists());
    build(&place);
    p.release(NAME).unwrap();
    left_behind(&repo, &place);

    let next = p.materialize(NAME, &one, &Settings::new()).unwrap();
    holds_the_candidate(&repo, &next, &one);
    assert!(!next.path.join("target").exists());
    assert!(!root.join(".caches").exists());
    p.release(NAME).unwrap();
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

fn port(place: &Place, name: &str) -> u16 {
    place.env[name].parse().unwrap()
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

/// The ports the host holds in `dir`, `held/<port>`, each naming the
/// record of the place that holds it.
fn held_ports(dir: &std::path::Path) -> Vec<u16> {
    let mut held: Vec<u16> = match std::fs::read_dir(dir.join("held")) {
        Ok(entries) => entries
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .map(|n| n.parse().unwrap())
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => panic!("{e}"),
    };
    held.sort();
    held
}

/// The ports `place` was given, sorted.
fn given(place: &Place) -> Vec<u16> {
    let mut ports: Vec<u16> = place
        .env
        .iter()
        .filter(|(k, _)| k.starts_with("SLUICE_PORT_"))
        .map(|(_, v)| v.parse().unwrap())
        .collect();
    ports.sort();
    ports
}

/// 31, 32, record 0016 as amended 2026-11-13: two places alive at once
/// hold their own ports, each its names' ports in a row, in the order
/// written, no port shared, and a short name each; a place's ports are
/// free again once it is released. A port something else on the host
/// takes is passed over, so which ports are given is the host's to say:
/// the test reads the ports each place was given, and the ports held,
/// rather than expecting numbers.
#[test]
fn places_alive_at_once_hold_their_own_ports() {
    let _ports = ports_lock();
    let (repo, rev) = scratch("places-ports");
    let held_dir = support::Scratch::new("places-held");
    let p = places(&repo).with_ports(held_dir.path().to_path_buf());
    let from = ports(6);
    let settings = ported(&["web", "api-server"], &from.to_string());
    let in_a_row = |place: &Place| {
        let web = port(place, "SLUICE_PORT_WEB");
        assert_eq!(
            port(place, "SLUICE_PORT_API_SERVER"),
            web + 1,
            "not in a row"
        );
        assert!(web >= from, "below ports_from: {web}");
    };
    let held = || held_ports(held_dir.path());
    let a = p.materialize("sluice/try/a/1", &rev, &settings).unwrap();
    let b = p.materialize("sluice/try/b/1", &rev, &settings).unwrap();
    in_a_row(&a);
    in_a_row(&b);
    let mut both = [given(&a), given(&b)].concat();
    both.sort();
    both.dedup();
    assert_eq!(both.len(), 4, "two places hold one port");
    assert_eq!(held(), both);
    assert_ne!(a.env["SLUICE_PLACE"], b.env["SLUICE_PLACE"]);
    assert!(a.env["SLUICE_PLACE"].starts_with("sluice-"));

    p.release("sluice/try/a/1").unwrap();
    assert_eq!(held(), given(&b), "a's ports are free again");
    let c = p.materialize("sluice/try/c/1", &rev, &settings).unwrap();
    assert!(
        given(&c).iter().all(|p| !given(&b).contains(p)),
        "c was given a port b holds"
    );
    p.release("sluice/try/b/1").unwrap();
    p.release("sluice/try/c/1").unwrap();
    assert!(held().is_empty());
}

/// 31, record 0016 as amended 2026-11-13: a port something else holds is
/// passed over, with the run it would begin.
#[test]
fn a_block_with_a_port_in_use_is_passed_over() {
    let _ports = ports_lock();
    let (repo, rev) = scratch("places-busy");
    let held_dir = support::Scratch::new("places-busy-held");
    let p = places(&repo).with_ports(held_dir.path().to_path_buf());
    let from = ports(4);
    let _held = std::net::TcpListener::bind(("127.0.0.1", from + 1)).unwrap();
    let a = p
        .materialize(NAME, &rev, &ported(&["web", "api"], &from.to_string()))
        .unwrap();
    // The run from `from` holds the busy port, so a later run is given:
    // which one depends on what else the host holds at that moment.
    let web = port(&a, "SLUICE_PORT_WEB");
    assert!(web > from + 1, "a run with the busy port was given: {web}");
    assert_eq!(port(&a, "SLUICE_PORT_API"), web + 1);
    p.release(NAME).unwrap();
}

/// 31, record 0016 as amended 2026-11-13: a worktree's place keeps the
/// ports it was given at its first run, read from its record, however its
/// branch's policy changes: a port listed before its name moves nothing.
#[test]
fn a_places_ports_are_read_from_its_record() {
    let _ports = ports_lock();
    let (repo, _) = scratch("places-ports-record");
    let held_dir = support::Scratch::new("places-record-held");
    let p = places(&repo).with_ports(held_dir.path().to_path_buf());
    let session = repo.dir().with_extension("session");
    let from = ports(6);
    let first = p
        .adopt(
            session.to_str().unwrap(),
            "bolt/x",
            &ported(&["web", "api"], &from.to_string()),
            "2026-11-13T09:00:00Z",
        )
        .unwrap();
    let api = port(&first, "SLUICE_PORT_API");
    // The policy now lists a port more, before both.
    let next = p
        .adopt(
            session.to_str().unwrap(),
            "bolt/x",
            &ported(&["db", "web", "api"], &from.to_string()),
            "2026-11-13T09:05:00Z",
        )
        .unwrap();
    assert_eq!(port(&next, "SLUICE_PORT_API"), api);
    assert_eq!(given(&next), given(&first));
    assert!(!next.env.contains_key("SLUICE_PORT_DB"));
    // The record names them, and the host holds them for it.
    let record: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            repo.dir()
                .join(".git/sluice/.places")
                .read_dir()
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.extension().is_some_and(|x| x == "json"))
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(record["ports"]["SLUICE_PORT_API"], api.to_string());
    assert_eq!(held_ports(held_dir.path()), given(&first));
    p.release(&first.name).unwrap();
    assert!(held_ports(held_dir.path()).is_empty());
}

/// 31, record 0016 as amended 2026-11-13: a candidate's place holding
/// three ports, released, leaves none of the three held on the host, and
/// the next place may take them.
#[test]
fn a_release_frees_the_ports_its_record_names() {
    let _ports = ports_lock();
    let (repo, rev) = scratch("places-release-ports");
    let held_dir = support::Scratch::new("places-release-held");
    let p = places(&repo).with_ports(held_dir.path().to_path_buf());
    let from = ports(3);
    let settings = ported(&["web", "api", "db"], &from.to_string());
    let a = p.materialize(NAME, &rev, &settings).unwrap();
    assert_eq!(given(&a).len(), 3);
    assert_eq!(held_ports(held_dir.path()), given(&a));
    p.release(NAME).unwrap();
    assert!(
        held_ports(held_dir.path()).is_empty(),
        "a port is still held"
    );
    let b = p.materialize(NAME, &rev, &settings).unwrap();
    assert_eq!(given(&b), given(&a));
    p.release(NAME).unwrap();
}

/// A place's ports are names a check can be given: refused otherwise.
#[test]
fn ports_with_bad_names_are_refused() {
    let _spawns = spawns();
    let (repo, rev) = scratch("places-bad-ports");
    let blocks = support::Scratch::new("places-bad-blocks");
    let p = places(&repo).with_ports(blocks.path().to_path_buf());
    for (names, from) in [
        (&["a b"][..], "41200"),
        (&["web", "WEB"][..], "41200"),
        (&["web"][..], "80"),
    ] {
        assert!(p.materialize(NAME, &rev, &ported(names, from)).is_err());
    }
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

/// Session-runs-1 clause 90, 25: a session's worktree is adopted as
/// found, never made: no git worktree is added and nothing of it changes;
/// it holds the same ports on every run until it is released, and no two
/// worktrees share a port.
#[test]
fn a_sessions_worktree_is_adopted_with_ports_it_keeps() {
    let _ports = ports_lock();
    let (repo, _) = scratch("places-adopt");
    let blocks = support::Scratch::new("places-adopt-blocks");
    let p = places(&repo).with_ports(blocks.path().to_path_buf());
    let session = repo.dir().with_extension("session");
    let other = another_session(&repo, "other");
    let from = ports(4);
    let settings = ported(&["web", "db"], &from.to_string());
    let (before, listed) = (others(&repo), worktrees(&repo));

    let a = p
        .adopt(
            session.to_str().unwrap(),
            "bolt/x",
            &settings,
            "2026-10-14T09:00:00Z",
        )
        .unwrap();
    assert_eq!(a.path, std::fs::canonicalize(&session).unwrap());
    assert!(a.name.starts_with("session/"), "{}", a.name);
    assert!(a.tmp.is_dir());
    assert_eq!(port(&a, "SLUICE_PORT_DB"), port(&a, "SLUICE_PORT_WEB") + 1);
    assert!(a.env["SLUICE_PLACE"].starts_with("sluice-"));

    // Its second run: the same place, the same ports, the same short name,
    // whatever the policy now lists.
    let again = p
        .adopt(
            session.to_str().unwrap(),
            "bolt/x",
            &ported(&["web"], &from.to_string()),
            "2026-10-14T09:00:00Z",
        )
        .unwrap();
    assert_eq!(again, a);

    let b = p
        .adopt(
            other.to_str().unwrap(),
            "bolt/x",
            &settings,
            "2026-10-14T09:00:00Z",
        )
        .unwrap();
    // Other ports: a port something else holds is passed over, so which
    // are given is the host's to say.
    let (aw, ad) = (port(&a, "SLUICE_PORT_WEB"), port(&a, "SLUICE_PORT_DB"));
    let (bw, bd) = (port(&b, "SLUICE_PORT_WEB"), port(&b, "SLUICE_PORT_DB"));
    assert_eq!(bd, bw + 1);
    assert!(bw >= from, "{bw}");
    assert!(![aw, ad].contains(&bw) && ![aw, ad].contains(&bd));
    assert_ne!(a.env["SLUICE_PLACE"], b.env["SLUICE_PLACE"]);

    assert_eq!(others(&repo), before, "the session's worktree was touched");
    assert_eq!(worktrees(&repo), listed, "a git worktree was added");
    p.release(&a.name).unwrap();
    p.release(&b.name).unwrap();
}

/// 34, 95, core.16: releasing a session's place ends its runs, runs the
/// release command once in the worktree, removes its scratch directory and
/// lets its ports go, and leaves the worktree as it was; a second release
/// is nothing.
#[test]
fn releasing_a_sessions_place_leaves_its_worktree() {
    let _ports = ports_lock();
    let (repo, _) = scratch("places-session-release");
    let blocks = support::Scratch::new("places-session-blocks");
    let p = places(&repo).with_ports(blocks.path().to_path_buf());
    let session = repo.dir().with_extension("session");
    let settings = ported(&["web"], &ports(1).to_string());
    let (before, listed) = (others(&repo), worktrees(&repo));
    let place = p
        .adopt(
            session.to_str().unwrap(),
            "bolt/x",
            &settings,
            "2026-10-14T09:00:00Z",
        )
        .unwrap();

    let r = runs(&repo);
    let log = repo.dir().with_extension("released.log");
    let _ = std::fs::remove_file(&log);
    let request = |script: &str| Request {
        argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
        cwd: place.path.clone(),
        env: place.env.clone(),
        limit: None,
        grace: 1,
        watch: None,
        meta: serde_json::Value::Null,
        release: Some(vec![
            "/bin/sh".into(),
            "-c".into(),
            format!("echo \"$SLUICE_PORT_WEB\" >> {}", log.display()),
        ]),
    };
    let first = r.next_run(&place.name).unwrap();
    r.start(&first, &request("true")).unwrap();
    let second = r.next_run(&place.name).unwrap();
    r.start(&second, &request("sleep 300")).unwrap();
    let pgid_file = r.dir(&second).join("pgid");
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

    p.release(&place.name).unwrap();
    assert_eq!(
        nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pgid), None),
        Err(nix::errno::Errno::ESRCH)
    );
    let web = format!("{}\n", port(&place, "SLUICE_PORT_WEB"));
    assert_eq!(std::fs::read_to_string(&log).unwrap(), web);
    assert!(!place.tmp.exists());
    assert!(!r.dir(&first).exists() && !r.dir(&second).exists());
    assert!(session.join("work").exists());
    assert_eq!(others(&repo), before);
    assert_eq!(worktrees(&repo), listed, "the worktree is still listed");

    // Its ports are free for another worktree.
    let other = another_session(&repo, "next");
    let next = p
        .adopt(
            other.to_str().unwrap(),
            "bolt/x",
            &settings,
            "2026-10-14T09:00:00Z",
        )
        .unwrap();
    assert_eq!(
        port(&next, "SLUICE_PORT_WEB"),
        port(&place, "SLUICE_PORT_WEB")
    );
    p.release(&next.name).unwrap();

    // Again: nothing to release, and nothing runs.
    p.release(&place.name).unwrap();
    assert_eq!(std::fs::read_to_string(&log).unwrap(), web);
    assert!(session.exists());
}

/// 56, 95: a session's place is released under the release command its
/// branch's policy states now, built with the executor and the place's
/// environment, ports included; given no policy, the one its last run
/// carried runs.
#[test]
fn a_sessions_place_is_released_under_the_policy_read_now() {
    use sluice_seams::{Limits, ReleasePolicy, Released};
    let _ports = ports_lock();
    let (repo, _) = scratch("places-release-now");
    let blocks = support::Scratch::new("places-release-now-blocks");
    let jobs = 2;
    let p = places(&repo)
        .with_ports(blocks.path().to_path_buf())
        .with_release(Box::new(move |place, tests, limits, dir| {
            sluice::profiles::checks::release_request(place, tests, limits, jobs, &[], dir)
        }));
    let session = repo.dir().with_extension("session");
    let settings = ported(&["web"], &ports(1).to_string());
    let log = repo.dir().with_extension("release-now.log");
    let _ = std::fs::remove_file(&log);
    let release = |what: &str| {
        format!(
            "echo \"{what} $SLUICE_PORT_WEB $SLUICE_CHECKOUT\" >> {}",
            log.display()
        )
    };

    let run = |cmd: &str| {
        let place = p
            .adopt(
                session.to_str().unwrap(),
                "bolt/x",
                &settings,
                "2026-10-14T09:00:00Z",
            )
            .unwrap();
        let r = runs(&repo);
        let name = r.next_run(&place.name).unwrap();
        r.start(
            &name,
            &Request {
                argv: vec!["/bin/sh".into(), "-c".into(), "true".into()],
                cwd: place.path.clone(),
                // As a run's request carries it.
                env: place
                    .env
                    .clone()
                    .into_iter()
                    .chain([(
                        "SLUICE_CHECKOUT".to_string(),
                        place.path.display().to_string(),
                    )])
                    .collect(),
                limit: None,
                grace: 1,
                watch: None,
                meta: serde_json::Value::Null,
                release: Some(vec!["/bin/sh".into(), "-c".into(), cmd.into()]),
            },
        )
        .unwrap();
        place
    };
    let policy = |cmd: &str| ReleasePolicy {
        branch: "bolt/x".into(),
        tests: Settings::from([("release".into(), Setting::Text(cmd.into()))]),
        limits: Limits::default(),
    };

    // Run under the old command, released under the policy's new one.
    let place = run(&release("old"));
    let port = place.env["SLUICE_PORT_WEB"].clone();
    let canon = place.path.display().to_string();
    assert_eq!(
        p.release_session(&place.name, Some(&policy(&release("new"))))
            .unwrap(),
        Released::Policy
    );
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        format!("new {port} {canon}\n")
    );
    assert!(p.adopted(session.to_str().unwrap()).unwrap().is_none());

    // Given no policy: the last run's.
    let place = run(&release("last"));
    assert_eq!(
        p.adopted(session.to_str().unwrap())
            .unwrap()
            .map(|a| a.branch),
        Some("bolt/x".to_string())
    );
    assert_eq!(
        p.release_session(&place.name, None).unwrap(),
        Released::LastRun
    );
    let port = place.env["SLUICE_PORT_WEB"].clone();
    assert!(std::fs::read_to_string(&log)
        .unwrap()
        .ends_with(&format!("last {port} {canon}\n")));
    assert_eq!(
        p.release_session(&place.name, None).unwrap(),
        Released::Nothing
    );
    assert!(session.join("work").exists());
}

fn preview_name(rev: &Rev) -> String {
    format!("preview/{}", &rev.as_str()[..12])
}

fn addresses(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Previews-1 clauses 98 to 101: a preview is made at its revision with
/// its ports and a name for each, kept apart in `.previews/` where no
/// session's or candidate's reader looks, given back as it is when asked
/// again, and released with its checkout, its scratch directory and its
/// ports; a second release is nothing.
#[test]
fn a_preview_is_made_kept_apart_and_released() {
    let _ports = ports_lock();
    let (repo, rev) = scratch("places-preview");
    let blocks = support::Scratch::new("places-preview-blocks");
    let p = places(&repo).with_ports(blocks.path().to_path_buf());
    let from = ports(6);
    let settings = ported(&["web", "api"], &from.to_string());
    let name = preview_name(&rev);
    let urls = addresses(&[
        ("web", "https://{name.web}.localhost"),
        ("api", "http://localhost:{port.api}/docs"),
    ]);

    let (place, preview) = p
        .preview(
            &name,
            &rev,
            "bolt/x",
            &settings,
            &urls,
            "2026-10-14T09:00:00Z",
        )
        .unwrap();
    holds_the_candidate(&repo, &place, &rev);
    let record = repo
        .dir()
        .join(".git/sluice/.previews")
        .join(format!("{}.json", name.replace('/', "%2F")));
    assert!(record.exists(), "{}", record.display());
    assert!(place
        .path
        .starts_with(repo.dir().join(".git/sluice/.previews")));
    assert_eq!(preview.revision, rev);
    assert_eq!(preview.branch, "bolt/x");
    let web = &preview.names["SLUICE_NAME_WEB"];
    // `<port>-<rev>--<branch>--<repository>`, the repository named by
    // the directory holding `.git`.
    assert!(
        web.starts_with(&format!("web-{}--bolt-x--", rev.short())),
        "{web}"
    );
    assert!(web.ends_with("places-preview"), "{web}");
    assert_eq!(place.env["SLUICE_NAME_WEB"], *web);
    assert_eq!(place.env["SLUICE_BRANCH"], "bolt/x");
    assert_eq!(place.env["SLUICE_REVISION"], rev.as_str());
    assert_eq!(preview.addresses["web"], format!("https://{web}.localhost"));
    assert_eq!(
        preview.addresses["api"],
        format!("http://localhost:{}/docs", place.env["SLUICE_PORT_API"])
    );
    assert!(p.session_places().unwrap().is_empty());
    assert!(!repo
        .dir()
        .join(".git/sluice/.places")
        .join(format!("{}.json", name.replace('/', "%2F")))
        .exists());

    // Asked again: the same preview, whatever the settings now say.
    let (again, listed) = p
        .preview(
            &name,
            &rev,
            "main",
            &ported(&["web"], &from.to_string()),
            &BTreeMap::new(),
            "later",
        )
        .unwrap();
    assert_eq!(again.path, place.path);
    assert_eq!(listed.names, preview.names);
    assert_eq!(listed.ports, preview.ports);
    assert_eq!(p.previews().unwrap(), vec![listed]);

    // A candidate's place and a session's worktree beside it share nothing.
    let cand = p.materialize(NAME, &rev, &settings).unwrap();
    let session = repo.dir().with_extension("session");
    let adopted = p
        .adopt(session.to_str().unwrap(), "bolt/x", &settings, "now")
        .unwrap();
    for other in [&cand, &adopted] {
        for k in ["SLUICE_PORT_WEB", "SLUICE_PORT_API"] {
            assert!(
                !place.env.values().any(|v| *v == other.env[k]),
                "{k} shared"
            );
        }
    }
    p.release(NAME).unwrap();
    p.release(&adopted.name).unwrap();

    assert!(p.release_preview(&name, None, 3).unwrap());
    left_behind(&repo, &place);
    assert!(!record.exists());
    assert!(p.previews().unwrap().is_empty());
    assert!(!p.release_preview(&name, None, 3).unwrap());
    assert!(held_ports(blocks.path()).is_empty(), "a port is still held");
}

/// 99: an address that names a port the policy does not list is refused
/// before anything is made.
#[test]
fn a_preview_naming_an_unlisted_port_is_refused() {
    let _spawns = spawns();
    let (repo, rev) = scratch("places-preview-refused");
    let blocks = support::Scratch::new("places-preview-refused-blocks");
    let p = places(&repo).with_ports(blocks.path().to_path_buf());
    let name = preview_name(&rev);
    let e = p
        .preview(
            &name,
            &rev,
            "main",
            &ported(&["web"], "41750"),
            &addresses(&[("api", "https://{name.api}.localhost")]),
            "now",
        )
        .unwrap_err();
    assert!(
        format!("{e:#}").contains("[preview] addresses api names {name.api}"),
        "{e:#}"
    );
    assert!(p.previews().unwrap().is_empty());
    assert!(!repo.dir().join(".git/sluice/.previews").exists());
    assert_eq!(
        std::fs::read_dir(blocks.path())
            .map(|d| d.count())
            .unwrap_or(0),
        0
    );
}

/// 101: a preview released because its command failed keeps its record,
/// holding nothing, with what the command printed, until it is released
/// again; its release command runs from the request its run carried, so a
/// preview is released with no policy read at all.
#[test]
fn a_failed_preview_is_kept_holding_nothing() {
    let _ports = ports_lock();
    let (repo, rev) = scratch("places-preview-failed");
    let blocks = support::Scratch::new("places-preview-failed-blocks");
    let p = places(&repo).with_ports(blocks.path().to_path_buf());
    let name = preview_name(&rev);
    let (place, _) = p
        .preview(
            &name,
            &rev,
            "main",
            &ported(&["web"], &ports(1).to_string()),
            &BTreeMap::new(),
            "now",
        )
        .unwrap();
    let log = repo.dir().with_extension("preview-release.log");
    let _ = std::fs::remove_file(&log);
    let r = runs(&repo);
    r.start(
        &name,
        &Request {
            argv: vec!["/bin/sh".into(), "-c".into(), "echo broken; exit 3".into()],
            cwd: place.path.clone(),
            env: place.env.clone(),
            limit: None,
            grace: 1,
            watch: None,
            meta: serde_json::Value::Null,
            release: Some(vec![
                "/bin/sh".into(),
                "-c".into(),
                format!("echo \"released $SLUICE_NAME_WEB\" >> {}", log.display()),
            ]),
        },
    )
    .unwrap();
    while matches!(r.state(&name).unwrap(), sluice::runs::RunState::Running) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    p.preview_run(&name, &sluice_seams::RunHandle(name.clone()))
        .unwrap();

    assert!(p.release_preview(&name, Some("broken\n"), 3).unwrap());
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        format!("released {}\n", place.env["SLUICE_NAME_WEB"])
    );
    left_behind(&repo, &place);
    let kept = p.previews().unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].failed.as_deref(), Some("broken\n"));
    assert!(kept[0].ports.is_empty() && kept[0].run.is_none() && !kept[0].gone);
    assert!(p.release_preview(&name, None, 3).unwrap());
    assert!(p.previews().unwrap().is_empty());
}

/// 99: a preview whose short revision would give a name another preview
/// holds is named by the longer revision.
#[test]
fn a_name_another_preview_holds_takes_the_longer_revision() {
    let _ports = ports_lock();
    let (repo, rev) = scratch("places-preview-clash");
    let blocks = support::Scratch::new("places-preview-clash-blocks");
    let p = places(&repo).with_ports(blocks.path().to_path_buf());
    let settings = ported(&["web"], &ports(1).to_string());
    let name = preview_name(&rev);
    let (_, first) = p
        .preview(&name, &rev, "main", &settings, &BTreeMap::new(), "now")
        .unwrap();
    let short = first.names["SLUICE_NAME_WEB"].clone();
    // Another revision's preview that took the same name.
    let dir = repo.dir().join(".git/sluice/.previews");
    let record = |n: &str| dir.join(format!("{}.json", n.replace('/', "%2F")));
    let mut other: serde_json::Value =
        serde_json::from_slice(&std::fs::read(record(&name)).unwrap()).unwrap();
    let rival = "f".repeat(40);
    other["revision"] = rival.clone().into();
    other["block"] = serde_json::Value::Null;
    std::fs::write(
        record(&format!("preview/{}", &rival[..12])),
        other.to_string(),
    )
    .unwrap();
    p.release_preview(&name, None, 3).unwrap();

    let (_, again) = p
        .preview(&name, &rev, "main", &settings, &BTreeMap::new(), "now")
        .unwrap();
    let long = &again.names["SLUICE_NAME_WEB"];
    assert_ne!(*long, short);
    assert!(
        long.starts_with(&format!("web-{}--", &rev.as_str()[..12])),
        "{long}"
    );
    p.release_preview(&name, None, 3).unwrap();
    p.release_preview(&format!("preview/{}", &rival[..12]), None, 3)
        .unwrap();
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

/// 129: with nothing under the scratch root, unnamed scratch is looked
/// for with no place's record read: records that could not be listed
/// fail nothing.
#[test]
fn an_empty_scratch_root_reads_no_record() {
    let repo = Repo::new("empty-tmp");
    let p = places(&repo);
    let root = repo.dir().join(".git/sluice");
    std::fs::create_dir_all(root.join(".tmp")).unwrap();
    // A file where the records' directory is: a read of it fails.
    std::fs::write(root.join(".places"), "not a directory\n").unwrap();
    assert!(p.remove_unnamed_scratch().unwrap().is_empty());
    std::fs::create_dir_all(root.join(".tmp/0123456789")).unwrap();
    assert!(p.remove_unnamed_scratch().is_err(), "a record read");
}

/// A run of the place `name` in `place` that ends at once, whose release
/// command runs `release`.
fn ended_run(r: &Runs, name: &str, place: &Place, release: &str) {
    r.start(
        name,
        &Request {
            argv: vec!["/usr/bin/true".into()],
            cwd: place.path.clone(),
            env: place.env.clone(),
            limit: None,
            grace: 1,
            watch: None,
            meta: serde_json::Value::Null,
            release: Some(vec!["/bin/sh".into(), "-c".into(), release.into()]),
        },
    )
    .unwrap();
    while matches!(r.state(name).unwrap(), sluice::runs::RunState::Running) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// 123: a combination's checkout deleted by hand is gone; its release is
/// refused rather than run, and it is freed with nothing run in its name:
/// its run removed, git no longer listing it, its scratch directory and
/// record gone and its block of ports free.
#[test]
fn a_deleted_checkout_is_gone_and_freed_with_nothing_run() {
    let _ports = ports_lock();
    let (repo, rev) = scratch("places-gone");
    let blocks = support::Scratch::new("places-gone-blocks");
    let p = places(&repo).with_ports(blocks.path().to_path_buf());
    let settings = ported(&["web"], &ports(1).to_string());
    let place = p.materialize(NAME, &rev, &settings).unwrap();
    let r = runs(&repo);
    let marker = repo.dir().with_extension("gone-released");
    let _ = std::fs::remove_file(&marker);
    ended_run(&r, NAME, &place, &format!("touch {}", marker.display()));
    assert!(!p.gone(NAME).unwrap());
    assert!(!p.gone("sluice/try/bolt/x/99").unwrap(), "no record");

    std::fs::remove_dir_all(&place.path).unwrap();
    assert!(p.gone(NAME).unwrap());
    let e = p.release(NAME).unwrap_err();
    assert!(format!("{e:#}").contains("was not run"), "{e:#}");
    p.free(NAME).unwrap();
    assert!(!marker.exists(), "the release command ran");
    assert!(!r.dir(NAME).exists());
    left_behind(&repo, &place);
    assert!(p.made().unwrap().is_empty());
    assert!(!p.gone(NAME).unwrap());

    let next = p
        .materialize("sluice/try/bolt/x/2", &rev, &settings)
        .unwrap();
    assert_eq!(
        port(&next, "SLUICE_PORT_WEB"),
        port(&place, "SLUICE_PORT_WEB")
    );
    p.release("sluice/try/bolt/x/2").unwrap();
}

/// 123, core.16: a session's place freed at its failure's acceptance has
/// its runs removed and its scratch directory and ports let go, with no
/// release command run, and its worktree left as it was.
#[test]
fn freeing_a_sessions_place_leaves_its_worktree() {
    let _ports = ports_lock();
    let (repo, _) = scratch("places-session-free");
    let blocks = support::Scratch::new("places-session-free-blocks");
    let p = places(&repo).with_ports(blocks.path().to_path_buf());
    let session = repo.dir().with_extension("session");
    let settings = ported(&["web"], &ports(1).to_string());
    let (before, listed) = (others(&repo), worktrees(&repo));
    let place = p
        .adopt(session.to_str().unwrap(), "bolt/x", &settings, "now")
        .unwrap();
    let r = runs(&repo);
    let marker = repo.dir().with_extension("session-freed");
    let _ = std::fs::remove_file(&marker);
    let run = r.next_run(&place.name).unwrap();
    ended_run(&r, &run, &place, &format!("touch {}", marker.display()));
    assert!(!p.gone(&place.name).unwrap());

    p.free(&place.name).unwrap();
    assert!(!marker.exists(), "the release command ran");
    assert!(!r.dir(&run).exists());
    assert!(!place.tmp.exists());
    assert!(p.session_places().unwrap().is_empty());
    assert!(session.join("work").exists());
    assert_eq!(others(&repo), before);
    assert_eq!(worktrees(&repo), listed, "the worktree is still listed");
    let other = another_session(&repo, "after-free");
    let next = p
        .adopt(other.to_str().unwrap(), "bolt/x", &settings, "now")
        .unwrap();
    assert_eq!(
        port(&next, "SLUICE_PORT_WEB"),
        port(&place, "SLUICE_PORT_WEB")
    );
    p.release(&next.name).unwrap();
}

/// 123: a session's place whose release command fails keeps its record,
/// held at once, with what failed and its tries, its ports and its scratch
/// directory; freed, it is gone.
#[test]
fn a_failed_session_release_keeps_its_place_with_what_failed() {
    let _ports = ports_lock();
    let (repo, _) = scratch("places-session-failed");
    let blocks = support::Scratch::new("places-session-failed-blocks");
    let p = places(&repo).with_ports(blocks.path().to_path_buf());
    let session = repo.dir().with_extension("session");
    let place = p
        .adopt(
            session.to_str().unwrap(),
            "bolt/x",
            &ported(&["web"], &ports(1).to_string()),
            "now",
        )
        .unwrap();
    let r = runs(&repo);
    let run = r.next_run(&place.name).unwrap();
    ended_run(&r, &run, &place, "echo usage: scripts/preview up; exit 2");

    let e = p.release_session(&place.name, None).unwrap_err();
    assert!(
        format!("{e:#}").contains("usage: scripts/preview up"),
        "{e:#}"
    );
    let held = p.session_places().unwrap();
    assert_eq!(held.len(), 1);
    let failure = held[0].release_failed.clone().expect("what failed is kept");
    assert!(
        failure.failed.contains("usage: scripts/preview up"),
        "{failure:?}"
    );
    assert_eq!((failure.tries, failure.stopped), (1, true));
    assert_eq!(
        held[0].ports.get("SLUICE_PORT_WEB"),
        place.env.get("SLUICE_PORT_WEB"),
        "its ports are kept"
    );
    assert!(place.tmp.exists());

    p.release_session(&place.name, None).unwrap_err();
    let again = p.session_places().unwrap()[0]
        .release_failed
        .clone()
        .unwrap();
    assert_eq!(again.tries, 2);

    p.free(&place.name).unwrap();
    assert!(p.session_places().unwrap().is_empty());
    assert!(!place.tmp.exists());
    assert!(session.exists());
}

/// 123: a preview whose release command fails keeps its record with what
/// failed, held once tried more than the retries it was given; freed, its
/// checkout and record go with nothing run.
#[test]
fn a_failed_preview_release_is_held_once_its_retries_are_spent() {
    let _ports = ports_lock();
    let (repo, rev) = scratch("places-preview-release-failed");
    let blocks = support::Scratch::new("places-preview-release-failed-blocks");
    let p = places(&repo).with_ports(blocks.path().to_path_buf());
    let name = preview_name(&rev);
    let (place, _) = p
        .preview(
            &name,
            &rev,
            "main",
            &ported(&["web"], &ports(1).to_string()),
            &BTreeMap::new(),
            "now",
        )
        .unwrap();
    let r = runs(&repo);
    ended_run(&r, &name, &place, "echo usage: scripts/preview up; exit 2");
    p.preview_run(&name, &sluice_seams::RunHandle(name.clone()))
        .unwrap();

    p.release_preview(&name, None, 1).unwrap_err();
    let kept = p.previews().unwrap()[0].release_failed.clone().unwrap();
    assert!(
        kept.failed.contains("usage: scripts/preview up"),
        "{kept:?}"
    );
    assert_eq!((kept.tries, kept.stopped), (1, false));
    assert!(place.path.exists());
    p.release_preview(&name, None, 1).unwrap_err();
    let kept = p.previews().unwrap()[0].release_failed.clone().unwrap();
    assert_eq!((kept.tries, kept.stopped), (2, true));

    p.free(&name).unwrap();
    left_behind(&repo, &place);
    assert!(p.previews().unwrap().is_empty());
}

/// The ports held in `dir`: the files naming their holders.
fn held_count(dir: &std::path::Path) -> usize {
    held_ports(dir).len()
}

/// Record 0121's containers for the preview `place`: one mounting a file
/// of its checkout, one publishing its `web` port, one labelled with its
/// `SLUICE_PLACE`, and one of another checkout that does none of these.
fn preview_containers(place: &Place) -> serde_json::Value {
    let other = place.path.with_extension("other").join("tls");
    serde_json::json!([
        support::container("mounts", &[&place.path.join("tls/cert.pem")], &[], &[]),
        support::container("publishes", &[], &[port(place, "SLUICE_PORT_WEB")], &[]),
        support::container(
            "labelled",
            &[],
            &[],
            &[("sluice.place", &place.env["SLUICE_PLACE"])]
        ),
        support::container("other", &[&other], &[], &[]),
    ])
}

/// What a preview's containers' end saw of the place it was handed, and
/// of the preview's checkout and block, at each call.
type Seen = std::rc::Rc<std::cell::RefCell<Vec<(Place, bool, usize)>>>;

/// The place profile of `repo`, holding its blocks in `blocks` and ending
/// a preview's containers at `engine`, noting in what it returns what
/// each end was handed and whether the checkout was there and how many
/// blocks were held as it began (record 0121, design D1, D3).
fn ending(repo: &Repo, blocks: &std::path::Path, engine: &support::Engine) -> (Places, Seen) {
    let seen: Seen = Default::default();
    let (noted, held, socket) = (seen.clone(), blocks.to_path_buf(), engine.socket());
    let p = places(repo)
        .with_ports(blocks.to_path_buf())
        .with_containers(Box::new(move |place| {
            noted
                .borrow_mut()
                .push((place.clone(), place.path.exists(), held_count(&held)));
            sluice::profiles::checks::end_containers(
                &[sluice::profiles::checks::Endpoint::Unix(socket.clone())],
                place,
            )
        }));
    (p, seen)
}

/// 34, previews-1 clause 101, record 0121 (design D1, D3): a preview
/// released at its caller's asking has its release command run, then the
/// containers it left ended while its checkout and its block of ports are
/// still held, the place handed as its command was given it; a container
/// of another checkout is left; then its checkout and block go.
#[test]
fn a_released_preview_ends_its_containers_before_its_ports_go() {
    let _ports = ports_lock();
    let (repo, rev) = scratch("places-preview-containers");
    let blocks = support::Scratch::new("places-preview-containers-blocks");
    let short = support::Scratch::short();
    let engine = support::containers_engine(short.path());
    let (p, seen) = ending(&repo, blocks.path(), &engine);
    let name = preview_name(&rev);
    let (place, _) = p
        .preview(
            &name,
            &rev,
            "main",
            &ported(&["web"], &ports(1).to_string()),
            &BTreeMap::new(),
            "now",
        )
        .unwrap();
    let marker = repo.dir().with_extension("preview-released");
    let _ = std::fs::remove_file(&marker);
    let r = runs(&repo);
    ended_run(&r, &name, &place, &format!("touch {}", marker.display()));
    // The engine lists the preview's containers only once its release
    // command has run, so an end before it would remove nothing.
    let (held, released) = (preview_containers(&place), marker.clone());
    engine.hold_with(move || match released.exists() {
        true => held.clone(),
        false => serde_json::json!([]),
    });

    assert!(p.release_preview(&name, None, 3).unwrap());
    assert!(marker.exists(), "its release command ran");
    assert_eq!(engine.removed(), ["mounts", "publishes", "labelled"]);
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    let (handed, checkout, held) = &seen[0];
    assert_eq!(handed, &place, "the place its command was given");
    assert!(checkout, "its checkout went before its containers ended");
    assert_eq!(*held, 1, "its block went before its containers ended");
    left_behind(&repo, &place);
    assert_eq!(held_count(blocks.path()), 0);
}

/// A preview of `rev` in `p`, holding a `web` port.
fn ported_preview(p: &Places, rev: &Rev) -> (String, Place) {
    let name = preview_name(rev);
    let (place, _) = p
        .preview(
            &name,
            rev,
            "main",
            &ported(&["web"], &ports(1).to_string()),
            &BTreeMap::new(),
            "now",
        )
        .unwrap();
    (name, place)
}

/// 123, previews-1 clause 101, record 0121 (design D3): a preview whose
/// checkout was removed has its stored release run once in its scratch
/// directory, where the script it names in the checkout cannot be found
/// and it fails; its containers are ended all the same, after that
/// release and while its block is held; then it is freed.
#[test]
fn a_gone_previews_containers_end_after_its_stored_release() {
    let _ports = ports_lock();
    let (repo, rev) = scratch("places-preview-gone-containers");
    let blocks = support::Scratch::new("places-preview-gone-containers-blocks");
    let short = support::Scratch::short();
    let engine = support::containers_engine(short.path());
    let (p, seen) = ending(&repo, blocks.path(), &engine);
    let (name, place) = ported_preview(&p, &rev);
    let marker = repo.dir().with_extension("preview-gone-tried");
    let _ = std::fs::remove_file(&marker);
    let r = runs(&repo);
    ended_run(
        &r,
        &name,
        &place,
        &format!("echo tried >> {}; sh infra/release.sh", marker.display()),
    );
    let (held, tried) = (preview_containers(&place), marker.clone());
    engine.hold_with(move || match tried.exists() {
        true => held.clone(),
        false => serde_json::json!([]),
    });

    std::fs::remove_dir_all(&place.path).unwrap();
    let gone = p.release_gone(&name).unwrap();
    assert!(gone.ran);
    let failed = gone.failed.expect("its release failed in the scratch");
    assert!(failed.contains("infra/release.sh"), "{failed}");
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "tried\n");
    assert_eq!(engine.removed(), ["mounts", "publishes", "labelled"]);
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, place);
    assert_eq!(seen[0].2, 1, "its block went before its containers ended");
    left_behind(&repo, &place);
    assert!(p.previews().unwrap().is_empty());
    assert_eq!(held_count(blocks.path()), 0);
}

/// 123, previews-1 clause 101, record 0121 (design D3): a preview freed
/// at its release's failure's acceptance runs nothing in its name, its
/// release command included, and has its containers ended while its
/// checkout and block are still held; then it is freed.
#[test]
fn an_accepted_previews_containers_end_with_nothing_run() {
    let _ports = ports_lock();
    let (repo, rev) = scratch("places-preview-accepted-containers");
    let blocks = support::Scratch::new("places-preview-accepted-containers-blocks");
    let short = support::Scratch::short();
    let engine = support::containers_engine(short.path());
    let (p, seen) = ending(&repo, blocks.path(), &engine);
    let (name, place) = ported_preview(&p, &rev);
    engine.hold(preview_containers(&place));
    let marker = repo.dir().with_extension("preview-accepted-released");
    let _ = std::fs::remove_file(&marker);
    let r = runs(&repo);
    ended_run(&r, &name, &place, &format!("touch {}", marker.display()));

    p.free(&name).unwrap();
    assert!(!marker.exists(), "its release command ran");
    assert_eq!(engine.removed(), ["mounts", "publishes", "labelled"]);
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0], (place.clone(), true, 1));
    assert!(!r.dir(&name).exists());
    left_behind(&repo, &place);
    assert!(p.previews().unwrap().is_empty());
    assert_eq!(held_count(blocks.path()), 0);
}

/// 123, record 0121 (design D5): a container the engine will not remove
/// keeps its preview whole. A release fails, keeping the failure with one
/// try, the checkout and the block; with the checkout then removed, a
/// gone release fails and lets nothing go; once the engine removes it, the
/// next gone release frees the preview, its release command having run
/// once in all.
#[test]
fn a_container_left_keeps_the_preview_and_its_ports() {
    let _ports = ports_lock();
    let (repo, rev) = scratch("places-preview-container-left");
    let blocks = support::Scratch::new("places-preview-container-left-blocks");
    let short = support::Scratch::short();
    let engine = support::containers_engine(short.path());
    let (p, _) = ending(&repo, blocks.path(), &engine);
    let (name, place) = ported_preview(&p, &rev);
    engine.hold(preview_containers(&place));
    let marker = repo.dir().with_extension("preview-left-released");
    let _ = std::fs::remove_file(&marker);
    let r = runs(&repo);
    ended_run(
        &r,
        &name,
        &place,
        &format!("echo ran >> {}", marker.display()),
    );
    let refused = format!("publishes: {}", support::REFUSED.1);

    engine.refuse("publishes", 1);
    let e = format!("{:#}", p.release_preview(&name, None, 3).unwrap_err());
    assert!(e.contains("could not remove the containers"), "{e}");
    assert!(e.contains(&refused), "{e}");
    assert_eq!(engine.removed(), ["mounts", "labelled"], "the rest went on");
    let kept = p.previews().unwrap()[0].release_failed.clone().unwrap();
    assert!(kept.failed.contains(&refused), "{kept:?}");
    assert_eq!((kept.tries, kept.stopped), (1, false));
    assert!(place.path.exists(), "its checkout stays");
    assert_eq!(held_count(blocks.path()), 1, "its block stays held");

    std::fs::remove_dir_all(&place.path).unwrap();
    engine.refuse("publishes", 1);
    let e = format!("{:#}", p.release_gone(&name).unwrap_err());
    assert!(e.contains(&refused), "{e}");
    let listed = p.previews().unwrap();
    assert_eq!(listed.len(), 1, "the preview stays listed");
    assert!(listed[0].gone);
    assert_eq!(held_count(blocks.path()), 1, "its block stays held");
    assert!(place.tmp.exists(), "its scratch stays");

    let gone = p.release_gone(&name).unwrap();
    assert_eq!((gone.ran, gone.failed), (false, None));
    assert_eq!(engine.removed(), ["mounts", "labelled", "publishes"]);
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "ran\n");
    left_behind(&repo, &place);
    assert!(p.previews().unwrap().is_empty());
    assert_eq!(held_count(blocks.path()), 0);
}

/// A run of the place `name` in `place` that ends at once, as the check
/// seam starts one under `[tests] toolchain`: the toolchain, which notes
/// each time it runs in `tooled`, is run once as the run is prepared, and
/// the release command its request stores reads what it printed rather
/// than running it again.
fn tooled_run(r: &Runs, name: &str, place: &Place, tooled: &std::path::Path, release: &str) {
    let dir = r.prepare(name).unwrap();
    let script = dir.join("toolchain.sh");
    let out = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(format!(
            "echo ran >> {}; echo export FROM_TOOLCHAIN=yes",
            tooled.display()
        ))
        .output()
        .unwrap();
    std::fs::write(&script, out.stdout).unwrap();
    r.start(
        name,
        &Request {
            argv: vec!["/usr/bin/true".into()],
            cwd: place.path.clone(),
            env: place.env.clone(),
            limit: None,
            grace: 1,
            watch: None,
            meta: serde_json::Value::Null,
            release: Some(vec![
                "bash".into(),
                "-c".into(),
                r#"source "$0" >&2; exec "$@""#.into(),
                script.display().to_string(),
                "/bin/sh".into(),
                "-c".into(),
                release.into(),
            ]),
        },
    )
    .unwrap();
    while matches!(r.state(name).unwrap(), sluice::runs::RunState::Running) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// 123: a combination's checkout deleted by hand has the release its run
/// stored run once, from its scratch directory with its port and what its
/// toolchain gave the run, the toolchain not run again; then it is freed:
/// git no longer lists it, its scratch directory and record are gone and
/// its block of ports is free. A second release of it runs nothing.
#[test]
fn a_deleted_checkouts_stored_release_runs_once_from_its_scratch_directory() {
    let _ports = ports_lock();
    let (repo, rev) = scratch("places-release-gone");
    let blocks = support::Scratch::new("places-release-gone-blocks");
    let p = places(&repo).with_ports(blocks.path().to_path_buf());
    let settings = ported(&["web"], &ports(1).to_string());
    let place = p.materialize(NAME, &rev, &settings).unwrap();
    let tmp = std::fs::canonicalize(&place.tmp).unwrap();
    let r = runs(&repo);
    let marker = repo.dir().with_extension("gone-release");
    let tooled = repo.dir().with_extension("gone-toolchain");
    for m in [&marker, &tooled] {
        let _ = std::fs::remove_file(m);
    }
    tooled_run(
        &r,
        NAME,
        &place,
        &tooled,
        &format!(
            "echo \"$(pwd -P) $SLUICE_PORT_WEB $FROM_TOOLCHAIN\" >> {}",
            marker.display()
        ),
    );
    assert_eq!(std::fs::read_to_string(&tooled).unwrap(), "ran\n");

    std::fs::remove_dir_all(&place.path).unwrap();
    assert!(p.gone(NAME).unwrap());
    let gone = p.release_gone(NAME).unwrap();
    assert_eq!((gone.ran, gone.failed), (true, None));
    assert_eq!(
        std::fs::read_to_string(&marker).unwrap(),
        format!(
            "{} {} yes\n",
            tmp.display(),
            port(&place, "SLUICE_PORT_WEB")
        )
    );
    assert_eq!(
        std::fs::read_to_string(&tooled).unwrap(),
        "ran\n",
        "the toolchain ran again"
    );
    assert!(!r.dir(NAME).exists());
    left_behind(&repo, &place);
    assert!(p.made().unwrap().is_empty());

    let again = p.release_gone(NAME).unwrap();
    assert_eq!((again.ran, again.failed), (false, None));
    assert_eq!(std::fs::read_to_string(&marker).unwrap().lines().count(), 1);

    let next = p
        .materialize("sluice/try/bolt/x/2", &rev, &settings)
        .unwrap();
    assert_eq!(
        port(&next, "SLUICE_PORT_WEB"),
        port(&place, "SLUICE_PORT_WEB")
    );
    p.release("sluice/try/bolt/x/2").unwrap();
}

/// 123: a gone checkout whose stored release exits 2 is freed all the
/// same, the release's failure returned with what it printed; one whose
/// gone release was noted started, as a free that stopped part way leaves
/// it, runs nothing; and one whose runs stored no release runs nothing.
#[test]
fn a_gone_checkout_is_freed_whatever_its_stored_release_did() {
    let _ports = ports_lock();
    let (repo, rev) = scratch("places-release-gone-fails");
    let blocks = support::Scratch::new("places-release-gone-fails-blocks");
    let p = places(&repo).with_ports(blocks.path().to_path_buf());
    let settings = ported(&["web"], &ports(1).to_string());
    let place = p.materialize(NAME, &rev, &settings).unwrap();
    let r = runs(&repo);
    ended_run(&r, NAME, &place, "echo usage: scripts/preview up; exit 2");
    std::fs::remove_dir_all(&place.path).unwrap();
    let gone = p.release_gone(NAME).unwrap();
    assert!(gone.ran);
    let failed = gone.failed.expect("its failure is returned");
    assert!(
        failed.contains("exit status: 2") && failed.contains("usage: scripts/preview up"),
        "{failed}"
    );
    left_behind(&repo, &place);
    assert!(p.made().unwrap().is_empty());
    assert!(!p.gone(NAME).unwrap());
    let next = p
        .materialize("sluice/try/bolt/x/2", &rev, &settings)
        .unwrap();
    assert_eq!(
        port(&next, "SLUICE_PORT_WEB"),
        port(&place, "SLUICE_PORT_WEB"),
        "its block of ports is free"
    );

    // Its gone release noted started before: nothing runs again.
    let marker = repo.dir().with_extension("gone-noted");
    let _ = std::fs::remove_file(&marker);
    ended_run(
        &r,
        "sluice/try/bolt/x/2",
        &next,
        &format!("touch {}", marker.display()),
    );
    std::fs::remove_dir_all(&next.path).unwrap();
    let record = repo
        .dir()
        .join(".git/sluice/.places/sluice%2Ftry%2Fbolt%2Fx%2F2.json");
    let mut made: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
    made["gone_release"] = "2026-10-23T09:00:00Z".into();
    std::fs::write(&record, serde_json::to_vec(&made).unwrap()).unwrap();
    let gone = p.release_gone("sluice/try/bolt/x/2").unwrap();
    assert_eq!((gone.ran, gone.failed), (false, None));
    assert!(!marker.exists(), "a started gone release ran again");
    left_behind(&repo, &next);

    // No run stored a release.
    let third = p
        .materialize("sluice/try/bolt/x/3", &rev, &settings)
        .unwrap();
    std::fs::remove_dir_all(&third.path).unwrap();
    let gone = p.release_gone("sluice/try/bolt/x/3").unwrap();
    assert_eq!((gone.ran, gone.failed), (false, None));
    left_behind(&repo, &third);
    assert!(p.made().unwrap().is_empty());
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
    let through = Places::new(&link.join(".git"), runs(&repo))
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

/// Makes `dir` its owner's to write again when dropped, so a test that
/// fails with it read-only leaves nothing its scratch cannot remove.
struct Unlock(PathBuf);

impl Drop for Unlock {
    fn drop(&mut self) {
        let _ =
            std::fs::set_permissions(&self.0, std::os::unix::fs::PermissionsExt::from_mode(0o755));
    }
}

/// A directory of `place` a check left read-only, holding a file.
fn read_only(place: &Place) -> Unlock {
    let dir = place.path.join("locked");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("file"), "x").unwrap();
    std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o555)).unwrap();
    Unlock(dir)
}

/// 123, record 0115: a combination's checkout whose folder git no longer
/// lists, holding a directory a check left read-only, is released once
/// from what sluice holds and freed: the release its run stored runs once,
/// or nothing where it ran already, as on deneb, and sluice removes what
/// is left of the folder, its scratch, its record and its ports going with
/// it. The operator's acceptance frees one the same way, with nothing run.
#[test]
fn a_checkout_git_no_longer_lists_is_freed() {
    let _ports = ports_lock();
    let (repo, rev) = scratch("places-unlisted-freed");
    let blocks = support::Scratch::new("places-unlisted-freed-blocks");
    let p = places(&repo).with_ports(blocks.path().to_path_buf());
    let settings = ported(&["web"], &ports(1).to_string());
    let r = runs(&repo);
    let mark = repo.dir().with_extension("unlisted-released");
    let _ = std::fs::remove_file(&mark);
    let lines = || {
        std::fs::read_to_string(&mark)
            .map(|s| s.lines().count())
            .unwrap_or(0)
    };
    let release = format!("echo ran >> {}", mark.display());
    let freed = |name: &str, place: &Place| {
        left_behind(&repo, place);
        assert!(p.made().unwrap().is_empty());
        assert!(!p.gone(name).unwrap());
    };

    // Released once: its stored release runs, and what is left goes.
    let first = p.materialize(NAME, &rev, &settings).unwrap();
    ended_run(&r, NAME, &first, &release);
    let _unlock = read_only(&first);
    drop_entry(&repo, &first.path);
    assert!(p.gone(NAME).unwrap());
    let gone = p.release_gone(NAME).unwrap();
    assert_eq!((gone.ran, gone.failed), (true, None));
    assert_eq!(lines(), 1);
    assert!(!r.dir(NAME).exists());
    freed(NAME, &first);

    // The incident: its release ran as its run was removed, before git
    // stopped part way; nothing runs again.
    let second_name = "sluice/try/bolt/x/2";
    let second = p.materialize(second_name, &rev, &settings).unwrap();
    assert_eq!(
        port(&second, "SLUICE_PORT_WEB"),
        port(&first, "SLUICE_PORT_WEB"),
        "its block of ports is free"
    );
    ended_run(&r, second_name, &second, &release);
    r.remove(second_name).unwrap();
    assert_eq!(lines(), 2);
    let _unlock = read_only(&second);
    drop_entry(&repo, &second.path);
    let gone = p.release_gone(second_name).unwrap();
    assert_eq!((gone.ran, gone.failed), (false, None));
    assert_eq!(lines(), 2, "its release ran again");
    freed(second_name, &second);

    // The operator's acceptance: nothing runs, and what is left goes.
    let third_name = "sluice/try/bolt/x/3";
    let third = p.materialize(third_name, &rev, &settings).unwrap();
    assert_eq!(
        port(&third, "SLUICE_PORT_WEB"),
        port(&first, "SLUICE_PORT_WEB")
    );
    ended_run(&r, third_name, &third, &release);
    let _unlock = read_only(&third);
    drop_entry(&repo, &third.path);
    p.free(third_name).unwrap();
    assert_eq!(lines(), 2, "the release command ran");
    assert!(!r.dir(third_name).exists());
    freed(third_name, &third);

    let next = p
        .materialize("sluice/try/bolt/x/4", &rev, &settings)
        .unwrap();
    assert_eq!(
        port(&next, "SLUICE_PORT_WEB"),
        port(&first, "SLUICE_PORT_WEB")
    );
    p.release("sluice/try/bolt/x/4").unwrap();
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
