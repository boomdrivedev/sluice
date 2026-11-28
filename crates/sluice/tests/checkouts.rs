//! The place seam's profile (C.1; 25, 31 to 35, 90 to 101, 123; rules
//! core.10, core.16, core.30) over fakes of what it reaches: the
//! repository's worktrees as git would keep them, and the runs, whose
//! supervisors and release commands would be processes of their own. Its
//! ports, scratch, records, releases and previews are proven here in the
//! test's own process, starting no process and no git; git's own part,
//! what a checkout holds and how git lists and locks one, is
//! `tests/profiles/places.rs`'s.

mod support;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{bail, Result};
use sluice::profiles::places::{Listed, PlaceRuns, Places, Worktrees};
use sluice::runs::ReleaseWith;
use sluice_seams::{CheckedOut, Checkout, Place, Rev, Setting, Settings};
use support::Scratch;

const NAME: &str = "sluice/try/bolt/x/1";

/// A checkout as the fake keeps it.
#[derive(Clone, Debug)]
struct Entry {
    head: Rev,
    locked: Option<String>,
    branch: Option<String>,
}

/// The repository's worktrees as git would keep them: a checkout added is
/// a folder holding nothing, which is clean while nothing is written in
/// it; one locked is refused removal, as `git worktree remove --force`
/// refuses one.
#[derive(Default)]
struct FakeWorktrees {
    list: RefCell<BTreeMap<PathBuf, Entry>>,
}

impl FakeWorktrees {
    /// `path` as the fake lists it, resolved where it exists, as git
    /// matches a folder by what it is on disk.
    fn key(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    /// A worktree at `path` with `branch` checked out at `head`, as a
    /// session's is.
    fn on_branch(&self, path: &Path, branch: &str, head: &Rev) {
        std::fs::create_dir_all(path).unwrap();
        self.list.borrow_mut().insert(
            FakeWorktrees::key(path),
            Entry {
                head: head.clone(),
                locked: None,
                branch: Some(branch.into()),
            },
        );
    }

    /// The checkout at `path` locked with `reason`, as `git worktree lock`
    /// or an add stopped part way leaves it.
    fn lock(&self, path: &Path, reason: &str) {
        let mut list = self.list.borrow_mut();
        list.get_mut(&FakeWorktrees::key(path)).unwrap().locked = Some(reason.into());
    }
}

impl Worktrees for FakeWorktrees {
    fn listed(&self, path: &Path) -> Result<bool> {
        Ok(self.list.borrow().contains_key(&FakeWorktrees::key(path)))
    }

    fn entry(&self, path: &Path) -> Result<Option<Listed>> {
        Ok(self
            .list
            .borrow()
            .get(&FakeWorktrees::key(path))
            .map(|e| Listed {
                head: Some(e.head.clone()),
                locked: e.locked.clone(),
            }))
    }

    fn checked_out(&self) -> Result<Vec<CheckedOut>> {
        Ok(self
            .list
            .borrow()
            .iter()
            .filter_map(|(path, e)| {
                e.branch.as_ref().map(|b| CheckedOut {
                    worktree: path.clone(),
                    branch: b.clone(),
                    head: e.head.clone(),
                })
            })
            .collect())
    }

    fn add(&self, path: &Path, rev: &Rev) -> Result<()> {
        if path.exists() {
            bail!("'{}' already exists", path.display());
        }
        std::fs::create_dir_all(path)?;
        self.list.borrow_mut().insert(
            FakeWorktrees::key(path),
            Entry {
                head: rev.clone(),
                locked: None,
                branch: None,
            },
        );
        Ok(())
    }

    fn clean(&self, path: &Path) -> Result<bool> {
        Ok(std::fs::read_dir(path)?.next().is_none())
    }

    fn unlock(&self, path: &Path) -> Result<()> {
        if let Some(e) = self.list.borrow_mut().get_mut(&FakeWorktrees::key(path)) {
            e.locked = None;
        }
        Ok(())
    }

    fn remove(&self, path: &Path) -> Result<std::result::Result<(), String>> {
        let key = FakeWorktrees::key(path);
        if let Some(why) = self.list.borrow().get(&key).and_then(|e| e.locked.clone()) {
            return Ok(Err(format!(
                "fatal: cannot remove a locked working tree, lock reason: {why}"
            )));
        }
        if path.exists() {
            std::fs::remove_dir_all(path)?;
        }
        self.list.borrow_mut().remove(&key);
        Ok(Ok(()))
    }

    fn prune(&self) -> Result<()> {
        self.list.borrow_mut().retain(|p, _| p.exists());
        Ok(())
    }
}

/// The runs of the repository's places as their supervisors would keep
/// them: each call the profile makes is noted; a run is going while the
/// test says so, and a release fails while the test gives it words.
#[derive(Default)]
struct FakeRuns {
    dir: PathBuf,
    calls: RefCell<Vec<String>>,
    going: RefCell<BTreeSet<String>>,
    failing: RefCell<Option<String>>,
    /// The places whose runs stored a release command.
    stored: RefCell<BTreeSet<String>>,
}

impl FakeRuns {
    fn noted(&self, call: String) -> Result<()> {
        self.calls.borrow_mut().push(call);
        match self.failing.borrow().as_ref() {
            Some(words) => bail!("{words}"),
            None => Ok(()),
        }
    }

    /// The calls made of `name`, each by its method alone.
    fn of(&self, name: &str) -> Vec<String> {
        self.calls
            .borrow()
            .iter()
            .filter_map(|c| {
                let (method, rest) = c.split_once(' ')?;
                (rest == name || rest.starts_with(&format!("{name} "))).then(|| method.into())
            })
            .collect()
    }
}

impl PlaceRuns for FakeRuns {
    fn any_going(&self, place: &str) -> Result<bool> {
        Ok(self.going.borrow().contains(place))
    }
    fn discard(&self, name: &str) -> Result<()> {
        self.calls.borrow_mut().push(format!("discard {name}"));
        Ok(())
    }
    fn discard_all(&self, place: &str) -> Result<()> {
        self.calls.borrow_mut().push(format!("discard_all {place}"));
        Ok(())
    }
    fn end_keeping(&self, name: &str) -> Result<()> {
        self.calls.borrow_mut().push(format!("end_keeping {name}"));
        Ok(())
    }
    fn release_dir(&self, place: &str) -> PathBuf {
        self.dir.join(sluice::runs::flat_name(place))
    }
    fn release_gone(&self, name: &str, cwd: &Path) -> Result<bool> {
        self.noted(format!("release_gone {name} {}", cwd.display()))?;
        Ok(self.stored.borrow().contains(name))
    }
    fn remove(&self, name: &str) -> Result<()> {
        self.noted(format!("remove {name}"))
    }
    fn release_keeping(&self, name: &str) -> Result<()> {
        self.noted(format!("release_keeping {name}"))
    }
    fn remove_all_with(&self, place: &str, with: ReleaseWith) -> Result<()> {
        let with = match with {
            ReleaseWith::LastRun => "last-run".to_string(),
            ReleaseWith::Fresh(None) => "none".to_string(),
            ReleaseWith::Fresh(Some(r)) => r.release.unwrap_or(r.argv).join(" "),
        };
        self.noted(format!("remove_all_with {place} {with}"))
    }
}

/// A repository's places over the fakes: its common directory, the
/// directory standing for the user's scratch, and the host's ports.
struct Repo {
    common: PathBuf,
    worktrees: Rc<FakeWorktrees>,
    runs: Rc<FakeRuns>,
    user: Scratch,
    ports: Scratch,
    _dir: Scratch,
}

impl Repo {
    fn new(name: &str) -> Repo {
        let dir = Scratch::new(name);
        let common = dir.path().join("repo/.git");
        std::fs::create_dir_all(&common).unwrap();
        let runs = Rc::new(FakeRuns {
            dir: dir.path().join("runs"),
            ..FakeRuns::default()
        });
        Repo {
            common,
            worktrees: Rc::default(),
            runs,
            user: Scratch::short(),
            ports: Scratch::new(&format!("{name}-ports")),
            _dir: dir,
        }
    }

    /// The directory standing for the user's scratch directory: a path
    /// not yet made, in a short directory of the test's own (35 as
    /// amended, rule core.30).
    fn user_dir(&self) -> PathBuf {
        self.user.path().join("u")
    }

    fn places(&self) -> Places {
        Places::new(&self.common, self.worktrees.clone(), self.runs.clone())
            .with_scratch_root(self.user_dir())
            .with_ports(self.ports.path().to_path_buf())
    }

    /// The ports the host holds, `held/<port>`, each naming the record of
    /// the place that holds it.
    fn held(&self) -> Vec<u16> {
        let mut held: Vec<u16> = match std::fs::read_dir(self.ports.path().join("held")) {
            Ok(entries) => entries
                .map(|e| e.unwrap().file_name().to_string_lossy().parse().unwrap())
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => panic!("{e}"),
        };
        held.sort();
        held
    }

    /// The record of the place `name`, as written.
    fn record(&self, file: &str) -> serde_json::Value {
        let path = self.common.join("sluice/.places").join(file);
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    /// The one record under `.places/`.
    fn only_record(&self) -> serde_json::Value {
        let dir = self.common.join("sluice/.places");
        let file = dir
            .read_dir()
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|x| x == "json"))
            .unwrap();
        self.record(&file.file_name().unwrap().to_string_lossy())
    }

    /// A session's worktree on `unit/<name>`, as git lists it.
    fn session(&self, name: &str) -> PathBuf {
        let path = self._dir.path().join(format!("{name}.session"));
        self.worktrees
            .on_branch(&path, &format!("unit/{name}"), &rev());
        path
    }
}

fn rev() -> Rev {
    Rev::new("c0ffee0000000000000000000000000000000000")
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

/// Whether nothing on the host holds `port`, asked as the place seam asks.
fn free(port: u16) -> bool {
    let local = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpListener::bind(("0.0.0.0", port)).is_ok()
        && std::net::TcpStream::connect_timeout(&local, std::time::Duration::from_millis(100))
            .is_err()
}

/// The first of `n` ports in a row for the test that asks: the next that
/// no test of this process was given and that the host holds none of,
/// from where the process's forty begin. Its id places them, so two runs
/// of these tests on one host ask for different ports. All are from 25000
/// to 32767: above the ports the host's own places take from 20000, and
/// below 32768, where Linux starts the ports it gives a host's own
/// connections for a moment, which would make a port look busy.
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

/// 31, 32, record 0016 as amended 2026-11-13: two places alive at once
/// hold their own ports, each its names' ports in a row, in the order
/// written, no port shared, and a short name each; a place's ports are
/// free again once it is released. A port something else on the host
/// takes is passed over, so which ports are given is the host's to say:
/// the test reads the ports each place was given, and the ports held,
/// rather than expecting numbers.
#[test]
fn places_alive_at_once_hold_their_own_ports() {
    let repo = Repo::new("checkouts-ports");
    let p = repo.places();
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
    let a = p.materialize("sluice/try/a/1", &rev(), &settings).unwrap();
    let b = p.materialize("sluice/try/b/1", &rev(), &settings).unwrap();
    in_a_row(&a);
    in_a_row(&b);
    let mut both = [given(&a), given(&b)].concat();
    both.sort();
    both.dedup();
    assert_eq!(both.len(), 4, "two places hold one port");
    assert_eq!(repo.held(), both);
    assert_ne!(a.env["SLUICE_PLACE"], b.env["SLUICE_PLACE"]);
    assert!(a.env["SLUICE_PLACE"].starts_with("sluice-"));

    p.release("sluice/try/a/1").unwrap();
    assert_eq!(repo.held(), given(&b), "a's ports are free again");
    let c = p.materialize("sluice/try/c/1", &rev(), &settings).unwrap();
    assert!(
        given(&c).iter().all(|p| !given(&b).contains(p)),
        "c was given a port b holds"
    );
    p.release("sluice/try/b/1").unwrap();
    p.release("sluice/try/c/1").unwrap();
    assert!(repo.held().is_empty());
}

/// 31, record 0016 as amended 2026-11-13: a port something else holds is
/// passed over, with the run it would begin.
#[test]
fn a_block_with_a_port_in_use_is_passed_over() {
    let repo = Repo::new("checkouts-busy");
    let p = repo.places();
    let from = ports(4);
    let _held = std::net::TcpListener::bind(("127.0.0.1", from + 1)).unwrap();
    let a = p
        .materialize(NAME, &rev(), &ported(&["web", "api"], &from.to_string()))
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
    let repo = Repo::new("checkouts-ports-record");
    let p = repo.places();
    let session = repo.session("a");
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
    assert_eq!(
        repo.only_record()["ports"]["SLUICE_PORT_API"],
        api.to_string()
    );
    assert_eq!(repo.held(), given(&first));
    p.release(&first.name).unwrap();
    assert!(repo.held().is_empty());
}

/// 31, record 0016 as amended 2026-11-13: a candidate's place holding
/// three ports, released, leaves none of the three held on the host, and
/// the next place may take them.
#[test]
fn a_release_frees_the_ports_its_record_names() {
    let repo = Repo::new("checkouts-release-ports");
    let p = repo.places();
    let from = ports(3);
    let settings = ported(&["web", "api", "db"], &from.to_string());
    let a = p.materialize(NAME, &rev(), &settings).unwrap();
    assert_eq!(given(&a).len(), 3);
    assert_eq!(repo.held(), given(&a));
    p.release(NAME).unwrap();
    assert!(repo.held().is_empty(), "a port is still held");
    let b = p.materialize(NAME, &rev(), &settings).unwrap();
    assert_eq!(given(&b), given(&a));
    p.release(NAME).unwrap();
}

/// A place's ports are names a check can be given: refused otherwise.
#[test]
fn ports_with_bad_names_are_refused() {
    let repo = Repo::new("checkouts-bad-ports");
    let p = repo.places();
    for (names, from) in [
        (&["a b"][..], "41200"),
        (&["web", "WEB"][..], "41200"),
        (&["web"][..], "80"),
    ] {
        assert!(p.materialize(NAME, &rev(), &ported(names, from)).is_err());
    }
}

/// The worktrees git lists for the repository.
fn listed(repo: &Repo) -> Vec<PathBuf> {
    repo.worktrees
        .checked_out()
        .unwrap()
        .into_iter()
        .map(|c| c.worktree)
        .collect()
}

/// Whether the call `call` was made, as [`FakeRuns`] notes it.
fn called(repo: &Repo, call: &str) -> bool {
    repo.runs.calls.borrow().iter().any(|c| c == call)
}

/// Session-runs-1 clause 90, 25: a session's worktree is adopted as
/// found, never made: no worktree is added and nothing of it changes; it
/// holds the same ports on every run until it is released, and no two
/// worktrees share a port.
#[test]
fn a_sessions_worktree_is_adopted_with_ports_it_keeps() {
    let repo = Repo::new("checkouts-adopt");
    let p = repo.places();
    let session = repo.session("a");
    let other = repo.session("other");
    let from = ports(4);
    let settings = ported(&["web", "db"], &from.to_string());
    let before = listed(&repo);

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

    assert_eq!(
        std::fs::read_dir(&session).unwrap().count(),
        0,
        "the session's worktree was touched"
    );
    assert_eq!(listed(&repo), before, "a worktree was added");
    p.release(&a.name).unwrap();
    p.release(&b.name).unwrap();
}

/// 34, 95, core.16: releasing a session's place ends its runs and runs
/// the release command once, as its last run carried it, removes its
/// scratch directory and lets its ports go, and leaves the worktree as it
/// was; a second release runs nothing.
#[test]
fn releasing_a_sessions_place_leaves_its_worktree() {
    let repo = Repo::new("checkouts-session-release");
    let p = repo.places();
    let session = repo.session("a");
    let settings = ported(&["web"], &ports(1).to_string());
    let before = listed(&repo);
    let place = p
        .adopt(
            session.to_str().unwrap(),
            "bolt/x",
            &settings,
            "2026-10-14T09:00:00Z",
        )
        .unwrap();

    p.release(&place.name).unwrap();
    assert_eq!(repo.runs.of(&place.name), ["remove_all_with"]);
    assert!(called(
        &repo,
        &format!("remove_all_with {} last-run", place.name)
    ));
    assert!(!place.tmp.exists());
    assert!(repo.held().is_empty());
    assert!(session.is_dir());
    assert_eq!(listed(&repo), before, "the worktree is still listed");

    // Its ports are free for another worktree.
    let other = repo.session("next");
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

    // Again: nothing to release, and no release command runs, though any
    // run a stop left ends.
    p.release(&place.name).unwrap();
    assert!(called(
        &repo,
        &format!("remove_all_with {} none", place.name)
    ));
    assert!(session.exists());
}

/// 56, 95: a session's place is released under the release command its
/// branch's policy states now, built by the check seam; given no policy,
/// the one its last run carried runs; released, it has nothing more to
/// release.
#[test]
fn a_sessions_place_is_released_under_the_policy_read_now() {
    use sluice_seams::{Limits, ReleasePolicy, Released};
    let repo = Repo::new("checkouts-release-now");
    let p = repo.places().with_release(
        Box::new(sluice::profiles::checks::names_release),
        Box::new(|place, tests, limits, dir| {
            sluice::profiles::checks::release_request(place, tests, limits, 2, &[], dir)
        }),
    );
    let session = repo.session("a");
    let settings = ported(&["web"], &ports(1).to_string());
    let adopt = || {
        p.adopt(
            session.to_str().unwrap(),
            "bolt/x",
            &settings,
            "2026-10-14T09:00:00Z",
        )
        .unwrap()
    };
    let policy = |cmd: &str| ReleasePolicy {
        branch: "bolt/x".into(),
        tests: Settings::from([("release".into(), Setting::Text(cmd.into()))]),
        limits: Limits::default(),
    };

    // Released under the policy's command.
    let place = adopt();
    assert_eq!(
        p.release_session(&place.name, Some(&policy("scripts/new")))
            .unwrap(),
        Released::Policy
    );
    assert!(called(
        &repo,
        &format!("remove_all_with {} /bin/sh -c scripts/new", place.name)
    ));
    assert!(p.adopted(session.to_str().unwrap()).unwrap().is_none());

    // Given no policy: the last run's.
    let place = adopt();
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
    assert!(called(
        &repo,
        &format!("remove_all_with {} last-run", place.name)
    ));
    assert_eq!(
        p.release_session(&place.name, None).unwrap(),
        Released::Nothing
    );
    assert!(session.is_dir());
}

/// 123, core.16: a session's place freed at its failure's acceptance has
/// its runs removed and its scratch directory and ports let go, with no
/// release command run, and its worktree left as it was.
#[test]
fn freeing_a_sessions_place_leaves_its_worktree() {
    let repo = Repo::new("checkouts-session-free");
    let p = repo.places();
    let session = repo.session("a");
    let settings = ported(&["web"], &ports(1).to_string());
    let before = listed(&repo);
    let place = p
        .adopt(session.to_str().unwrap(), "bolt/x", &settings, "now")
        .unwrap();
    assert!(!p.gone(&place.name).unwrap());

    p.free(&place.name).unwrap();
    assert_eq!(repo.runs.of(&place.name), ["discard"], "a release ran");
    assert!(!place.tmp.exists());
    assert!(p.session_places().unwrap().is_empty());
    assert!(session.is_dir());
    assert_eq!(listed(&repo), before, "the worktree is still listed");
    let other = repo.session("after-free");
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
    let repo = Repo::new("checkouts-session-failed");
    let p = repo.places();
    let session = repo.session("a");
    let place = p
        .adopt(
            session.to_str().unwrap(),
            "bolt/x",
            &ported(&["web"], &ports(1).to_string()),
            "now",
        )
        .unwrap();
    *repo.runs.failing.borrow_mut() = Some("release: usage: scripts/preview up".into());

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

    *repo.runs.failing.borrow_mut() = None;
    p.free(&place.name).unwrap();
    assert!(p.session_places().unwrap().is_empty());
    assert!(!place.tmp.exists());
    assert!(session.exists());
}

fn method(m: &str) -> Settings {
    Settings::from([("method".into(), Setting::Text(m.into()))])
}

/// The place holds its revision, with a scratch directory of its own
/// outside it, as git made it.
fn holds(repo: &Repo, place: &Place, rev: &Rev) {
    let entry = repo.worktrees.entry(&place.path).unwrap().expect("listed");
    assert_eq!(entry.head.as_ref(), Some(rev));
    assert!(repo.worktrees.clean(&place.path).unwrap());
    assert!(place.tmp.is_dir());
    assert_eq!(std::fs::read_dir(&place.tmp).unwrap().count(), 0);
    assert!(!place.tmp.starts_with(&place.path));
}

/// Nothing of `place` is left: its checkout, its scratch, its listing.
fn left_behind(repo: &Repo, place: &Place) {
    assert!(!place.path.exists(), "{} left", place.path.display());
    assert!(!place.tmp.exists());
    assert!(!repo.worktrees.listed(&place.path).unwrap());
}

/// 33 to 35, 25, rule core.16: a place is made under the repository's
/// `.places/`, holding its revision, its directories' names holding no
/// `%`, which clang and other tools read as a random character in a
/// temporary file's path; its release leaves nothing, and a release of
/// one released or never made is nothing.
#[test]
fn a_place_is_its_candidate_and_leaves_nothing() {
    let repo = Repo::new("checkouts-made");
    let p = repo.places();
    let place = p.materialize(NAME, &rev(), &Settings::new()).unwrap();
    holds(&repo, &place, &rev());
    assert!(place.path.starts_with(repo.common.join("sluice/.places")));
    for dir in [&place.path, &place.tmp] {
        let name = dir.file_name().unwrap().to_string_lossy();
        assert!(!name.contains('%'), "{}", dir.display());
    }
    p.release(NAME).unwrap();
    left_behind(&repo, &place);
    p.release(NAME).unwrap();
    p.release("sluice/try/bolt/x/never").unwrap();
}

/// 25, 59: a place left by a step that stopped part way is made again
/// under its name; one whose checkout was deleted by hand is released.
#[test]
fn a_place_left_by_a_crash() {
    let repo = Repo::new("checkouts-crash");
    let p = repo.places();
    let first = p.materialize(NAME, &rev(), &Settings::new()).unwrap();
    std::fs::write(first.path.join("debris"), "x").unwrap();
    let again = p.materialize(NAME, &rev(), &Settings::new()).unwrap();
    holds(&repo, &again, &rev());
    std::fs::remove_dir_all(&again.path).unwrap();
    p.release(NAME).unwrap();
    left_behind(&repo, &again);
}

/// 25, 59, record 0122 (design D4): a checkout git left locked
/// `initializing` is unlocked, removed and made again under its name.
#[test]
fn a_checkout_left_locked_initializing_is_made_again() {
    let repo = Repo::new("checkouts-locked-again");
    let p = repo.places();
    let place = p.materialize(NAME, &rev(), &Settings::new()).unwrap();
    repo.worktrees.lock(&place.path, "initializing");
    let again = p.materialize(NAME, &rev(), &Settings::new()).unwrap();
    holds(&repo, &again, &rev());
    let entry = repo.worktrees.entry(&again.path).unwrap().unwrap();
    assert_eq!(entry.locked, None);
    p.release(NAME).unwrap();
    left_behind(&repo, &again);
}

/// Section 5's acceptance test 8: a method other than `"git-worktree"` is
/// refused before anything is made (ADR 0013).
#[test]
fn an_unknown_method_is_refused() {
    let repo = Repo::new("checkouts-method");
    let p = repo.places();
    for m in [method("worktrunk"), method("archive"), method("rsync")] {
        assert!(p.materialize(NAME, &rev(), &m).is_err());
    }
    assert!(!repo.common.join("sluice/.places").exists());
    assert!(listed(&repo).is_empty());
}

/// ADR 0013, rule core.15: `[checkout]` names no method, and an older
/// revision's policy that names the one there is still makes a place.
#[test]
fn a_policy_need_not_name_the_method() {
    let repo = Repo::new("checkouts-no-method");
    let p = repo.places();
    for m in [Settings::new(), method("git-worktree")] {
        let place = p.materialize(NAME, &rev(), &m).unwrap();
        holds(&repo, &place, &rev());
        p.release(NAME).unwrap();
        assert!(!place.path.exists());
    }
}

/// 35; record 0049 as amended: a place holds its candidate and nothing
/// restored from another; one released as passed saves nothing, so the
/// next starts with no `target`. The caches an earlier sluice saved go
/// when a place is made, and nothing else of sluice's beside them.
#[test]
fn a_checkout_starts_with_nothing_but_its_revision() {
    let repo = Repo::new("checkouts-no-cache");
    let p = repo.places();
    let root = repo.common.join("sluice");
    let saved = root.join(".caches/bucket/key/target/debug");
    std::fs::create_dir_all(&saved).unwrap();
    std::fs::write(saved.join("a"), "saved by an earlier sluice\n").unwrap();
    // `caches`, without the dot, may be a branch's records.
    std::fs::create_dir_all(root.join("caches")).unwrap();
    std::fs::write(root.join("caches/version"), "1\n").unwrap();

    let place = p.materialize(NAME, &rev(), &Settings::new()).unwrap();
    holds(&repo, &place, &rev());
    assert!(!root.join(".caches").exists(), "the saved caches are gone");
    assert!(root.join("caches/version").exists());
    // Built, as a check would.
    std::fs::create_dir_all(place.path.join("target/debug")).unwrap();
    std::fs::write(place.path.join("target/debug/a"), "built\n").unwrap();
    p.release(NAME).unwrap();
    left_behind(&repo, &place);

    let next = p.materialize(NAME, &rev(), &Settings::new()).unwrap();
    holds(&repo, &next, &rev());
    assert!(!next.path.join("target").exists());
    assert!(!root.join(".caches").exists());
    p.release(NAME).unwrap();
}

/// 129: with nothing under the scratch root, unnamed scratch is looked
/// for with no place's record read: records that could not be listed
/// fail nothing.
#[test]
fn an_empty_scratch_root_reads_no_record() {
    let repo = Repo::new("checkouts-empty-tmp");
    let p = repo.places();
    let root = repo.common.join("sluice");
    std::fs::create_dir_all(root.join(".tmp")).unwrap();
    // A file where the records' directory is: a read of it fails.
    std::fs::write(root.join(".places"), "not a directory\n").unwrap();
    assert!(p.remove_unnamed_scratch().unwrap().is_empty());
    std::fs::create_dir_all(root.join(".tmp/0123456789")).unwrap();
    assert!(p.remove_unnamed_scratch().is_err(), "a record read");
}
