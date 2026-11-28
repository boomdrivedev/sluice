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

    /// git's entry for the checkout at `path` dropped, its folder left,
    /// as git's removal that stopped part way leaves it (deneb,
    /// 2026-11-10: `Directory not empty`, then `is not a working tree`).
    fn forget(&self, path: &Path) {
        assert!(path.is_dir());
        self.list.borrow_mut().remove(&FakeWorktrees::key(path));
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
        let common = dir.path().join(name).join(".git");
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
/// temporary file's path; its release ends its run first (34), which the
/// runs end as `tests/runs.rs`'s `removing_a_run_in_progress_ends_it`
/// has it, and leaves nothing, and a release of one released or never
/// made is nothing.
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
    assert_eq!(repo.runs.of(NAME), ["remove"]);
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

fn preview_name(rev: &Rev) -> String {
    format!("preview/{}", &rev.as_str()[..12])
}

fn addresses(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
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

/// Previews-1 clauses 98 to 101: a preview is made at its revision with
/// its ports and a name for each, kept apart in `.previews/` where no
/// session's or candidate's reader looks, given back as it is when asked
/// again, and released with its checkout, its scratch directory and its
/// ports; a second release is nothing.
#[test]
fn a_preview_is_made_kept_apart_and_released() {
    let repo = Repo::new("checkouts-preview");
    let p = repo.places();
    let from = ports(6);
    let settings = ported(&["web", "api"], &from.to_string());
    let name = preview_name(&rev());
    let urls = addresses(&[
        ("web", "https://{name.web}.localhost"),
        ("api", "http://localhost:{port.api}/docs"),
    ]);

    let (place, preview) = p
        .preview(
            &name,
            &rev(),
            "bolt/x",
            &settings,
            &urls,
            "2026-10-14T09:00:00Z",
        )
        .unwrap();
    holds(&repo, &place, &rev());
    let flat = format!("{}.json", name.replace('/', "%2F"));
    let record = repo.common.join("sluice/.previews").join(&flat);
    assert!(record.exists(), "{}", record.display());
    assert!(place.path.starts_with(repo.common.join("sluice/.previews")));
    assert_eq!(preview.revision, rev());
    assert_eq!(preview.branch, "bolt/x");
    let web = &preview.names["SLUICE_NAME_WEB"];
    // `<port>-<rev>--<branch>--<repository>`, the repository named by
    // the directory holding `.git`.
    assert!(
        web.starts_with(&format!("web-{}--bolt-x--", rev().short())),
        "{web}"
    );
    assert!(web.ends_with("checkouts-preview"), "{web}");
    assert_eq!(place.env["SLUICE_NAME_WEB"], *web);
    assert_eq!(place.env["SLUICE_BRANCH"], "bolt/x");
    assert_eq!(place.env["SLUICE_REVISION"], rev().as_str());
    assert_eq!(preview.addresses["web"], format!("https://{web}.localhost"));
    assert_eq!(
        preview.addresses["api"],
        format!("http://localhost:{}/docs", place.env["SLUICE_PORT_API"])
    );
    assert!(p.session_places().unwrap().is_empty());
    assert!(!repo.common.join("sluice/.places").join(&flat).exists());

    // Asked again: the same preview, whatever the settings now say.
    let (again, listed) = p
        .preview(
            &name,
            &rev(),
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
    let cand = p.materialize(NAME, &rev(), &settings).unwrap();
    let session = repo.session("a");
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
    assert!(repo.held().is_empty(), "a port is still held");
}

/// 99: an address that names a port the policy does not list is refused
/// before anything is made.
#[test]
fn a_preview_naming_an_unlisted_port_is_refused() {
    let repo = Repo::new("checkouts-preview-refused");
    let p = repo.places();
    let name = preview_name(&rev());
    let e = p
        .preview(
            &name,
            &rev(),
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
    assert!(!repo.common.join("sluice/.previews").exists());
    assert!(repo.held().is_empty());
    assert!(listed(&repo).is_empty());
}

/// 101: a preview released because its command failed has its run's
/// release command run, and keeps its record, holding nothing, with what
/// the command printed, until it is released again.
#[test]
fn a_failed_preview_is_kept_holding_nothing() {
    let repo = Repo::new("checkouts-preview-failed");
    let p = repo.places();
    let (name, place) = ported_preview(&p, &rev());
    p.preview_run(&name, &sluice_seams::RunHandle(name.clone()))
        .unwrap();

    assert!(p.release_preview(&name, Some("broken\n"), 3).unwrap());
    assert_eq!(repo.runs.of(&name), ["release_keeping", "remove"]);
    left_behind(&repo, &place);
    let kept = p.previews().unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].failed.as_deref(), Some("broken\n"));
    assert!(kept[0].ports.is_empty() && kept[0].run.is_none() && !kept[0].gone);
    assert!(repo.held().is_empty());
    assert!(p.release_preview(&name, None, 3).unwrap());
    assert!(p.previews().unwrap().is_empty());
}

/// 99: a preview whose short revision would give a name another preview
/// holds is named by the longer revision.
#[test]
fn a_name_another_preview_holds_takes_the_longer_revision() {
    let repo = Repo::new("checkouts-preview-clash");
    let p = repo.places();
    let settings = ported(&["web"], &ports(1).to_string());
    let name = preview_name(&rev());
    let (_, first) = p
        .preview(&name, &rev(), "main", &settings, &BTreeMap::new(), "now")
        .unwrap();
    let short = first.names["SLUICE_NAME_WEB"].clone();
    // Another revision's preview that took the same name.
    let dir = repo.common.join("sluice/.previews");
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
        .preview(&name, &rev(), "main", &settings, &BTreeMap::new(), "now")
        .unwrap();
    let long = &again.names["SLUICE_NAME_WEB"];
    assert_ne!(*long, short);
    assert!(
        long.starts_with(&format!("web-{}--", &rev().as_str()[..12])),
        "{long}"
    );
    p.release_preview(&name, None, 3).unwrap();
    p.release_preview(&format!("preview/{}", &rival[..12]), None, 3)
        .unwrap();
}

/// 123: a preview whose release command fails keeps its record with what
/// failed, held once tried more than the retries it was given; freed, its
/// checkout and record go with nothing run.
#[test]
fn a_failed_preview_release_is_held_once_its_retries_are_spent() {
    let repo = Repo::new("checkouts-preview-release-failed");
    let p = repo.places();
    let (name, place) = ported_preview(&p, &rev());
    *repo.runs.failing.borrow_mut() = Some("usage: scripts/preview up".into());

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

    *repo.runs.failing.borrow_mut() = None;
    let tried = repo.runs.of(&name).len();
    p.free(&name).unwrap();
    assert_eq!(
        repo.runs.of(&name)[tried..],
        ["end_keeping", "discard"],
        "a release ran"
    );
    left_behind(&repo, &place);
    assert!(p.previews().unwrap().is_empty());
}

/// What a preview's containers' end saw at each call: the place it was
/// handed, whether the checkout was there, how many ports were held, and
/// what had been asked of its runs.
type Seen = Rc<RefCell<Vec<(Place, bool, usize, Vec<String>)>>>;

/// The place profile of `repo`, ending a preview's containers by noting
/// what each end was handed (record 0121, design D1, D3), refusing while
/// `refusing` counts down; which containers an end removes is the check
/// profile's, `tests/checks.rs`'s.
fn ending(repo: &Repo) -> (Places, Seen, Rc<std::cell::Cell<usize>>) {
    let seen: Seen = Rc::default();
    let refusing = Rc::new(std::cell::Cell::new(0));
    let (noted, refused, held, runs) = (
        seen.clone(),
        refusing.clone(),
        repo.ports.path().join("held"),
        repo.runs.clone(),
    );
    let p = repo.places().with_containers(Box::new(move |place| {
        let count = std::fs::read_dir(&held).map_or(0, |d| d.count());
        noted.borrow_mut().push((
            place.clone(),
            place.path.exists(),
            count,
            runs.of(&place.name),
        ));
        if refused.get() > 0 {
            refused.set(refused.get() - 1);
            bail!("could not remove the containers: publishes: the engine refused");
        }
        Ok(())
    }));
    (p, seen, refusing)
}

/// 34, previews-1 clause 101, record 0121 (design D1, D3): a preview
/// released at its caller's asking has its release command run, then its
/// containers ended while its checkout and its ports are still held, the
/// place handed as its command was given it; then its checkout and ports
/// go.
#[test]
fn a_released_preview_ends_its_containers_before_its_ports_go() {
    let repo = Repo::new("checkouts-preview-containers");
    let (p, seen, _) = ending(&repo);
    let (name, place) = ported_preview(&p, &rev());

    assert!(p.release_preview(&name, None, 3).unwrap());
    assert_eq!(repo.runs.of(&name), ["release_keeping", "remove"]);
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    let (handed, checkout, held, ran) = &seen[0];
    assert_eq!(handed, &place, "the place its command was given");
    assert_eq!(ran, &["release_keeping"], "its release command ran first");
    assert!(checkout, "its checkout went before its containers ended");
    assert_eq!(*held, 1, "its port went before its containers ended");
    left_behind(&repo, &place);
    assert!(repo.held().is_empty());
}

/// 123, previews-1 clause 101, record 0121 (design D3): a preview whose
/// checkout was removed has its stored release run once in its scratch
/// directory, which fails there; its containers are ended all the same,
/// after that release and while its port is held; then it is freed.
#[test]
fn a_gone_previews_containers_end_after_its_stored_release() {
    let repo = Repo::new("checkouts-preview-gone-containers");
    let (p, seen, _) = ending(&repo);
    let (name, place) = ported_preview(&p, &rev());
    *repo.runs.failing.borrow_mut() = Some("sh: infra/release.sh: No such file".into());

    std::fs::remove_dir_all(&place.path).unwrap();
    let gone = p.release_gone(&name).unwrap();
    assert!(gone.ran);
    let failed = gone.failed.expect("its release failed in the scratch");
    assert!(failed.contains("infra/release.sh"), "{failed}");
    assert_eq!(
        repo.runs.of(&name),
        ["release_gone", "end_keeping", "discard"]
    );
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, place);
    assert_eq!(seen[0].2, 1, "its port went before its containers ended");
    assert_eq!(
        seen[0].3,
        ["release_gone", "end_keeping"],
        "after its release"
    );
    left_behind(&repo, &place);
    assert!(p.previews().unwrap().is_empty());
    assert!(repo.held().is_empty());
}

/// 123, previews-1 clause 101, record 0121 (design D3): a preview freed
/// at its release's failure's acceptance runs nothing in its name, its
/// release command included, and has its containers ended while its
/// checkout and port are still held; then it is freed.
#[test]
fn an_accepted_previews_containers_end_with_nothing_run() {
    let repo = Repo::new("checkouts-preview-accepted");
    let (p, seen, _) = ending(&repo);
    let (name, place) = ported_preview(&p, &rev());

    p.free(&name).unwrap();
    assert_eq!(repo.runs.of(&name), ["end_keeping", "discard"]);
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        (&seen[0].0, seen[0].1, seen[0].2, &seen[0].3[..]),
        (&place, true, 1, &["end_keeping".to_string()][..])
    );
    left_behind(&repo, &place);
    assert!(p.previews().unwrap().is_empty());
    assert!(repo.held().is_empty());
}

/// 123, record 0121 (design D5): a container the engine will not remove
/// keeps its preview whole. A release fails, keeping the failure with one
/// try, the checkout and the port; with the checkout then removed, a gone
/// release fails and lets nothing go; once the engine removes it, the next
/// gone release frees the preview, its release command having run once
/// in all.
#[test]
fn a_container_left_keeps_the_preview_and_its_ports() {
    let repo = Repo::new("checkouts-preview-container-left");
    let (p, _, refusing) = ending(&repo);
    let (name, place) = ported_preview(&p, &rev());
    let refused = "publishes: the engine refused";

    refusing.set(1);
    let e = format!("{:#}", p.release_preview(&name, None, 3).unwrap_err());
    assert!(e.contains("could not remove the containers"), "{e}");
    assert!(e.contains(refused), "{e}");
    let kept = p.previews().unwrap()[0].release_failed.clone().unwrap();
    assert!(kept.failed.contains(refused), "{kept:?}");
    assert_eq!((kept.tries, kept.stopped), (1, false));
    assert!(place.path.exists(), "its checkout stays");
    assert_eq!(repo.held().len(), 1, "its port stays held");

    std::fs::remove_dir_all(&place.path).unwrap();
    refusing.set(1);
    let e = format!("{:#}", p.release_gone(&name).unwrap_err());
    assert!(e.contains(refused), "{e}");
    let listed = p.previews().unwrap();
    assert_eq!(listed.len(), 1, "the preview stays listed");
    assert!(listed[0].gone);
    assert_eq!(repo.held().len(), 1, "its port stays held");
    assert!(place.tmp.exists(), "its scratch stays");

    let gone = p.release_gone(&name).unwrap();
    assert_eq!((gone.ran, gone.failed), (false, None));
    let of = repo.runs.of(&name);
    assert_eq!(
        of.iter().filter(|c| *c == "release_keeping").count()
            + of.iter().filter(|c| *c == "release_gone").count(),
        2,
        "{of:?}"
    );
    left_behind(&repo, &place);
    assert!(p.previews().unwrap().is_empty());
    assert!(repo.held().is_empty());
}

/// 123: a combination's checkout deleted by hand is gone, and freed with
/// nothing run in its name: its run removed, git no longer listing it,
/// its scratch directory and record gone and its port free.
#[test]
fn a_deleted_checkout_is_gone_and_freed_with_nothing_run() {
    let repo = Repo::new("checkouts-gone");
    let p = repo.places();
    let settings = ported(&["web"], &ports(1).to_string());
    let place = p.materialize(NAME, &rev(), &settings).unwrap();
    assert!(!p.gone(NAME).unwrap());
    assert!(!p.gone("sluice/try/bolt/x/99").unwrap(), "no record");

    std::fs::remove_dir_all(&place.path).unwrap();
    assert!(p.gone(NAME).unwrap());
    p.free(NAME).unwrap();
    assert_eq!(repo.runs.of(NAME), ["discard"], "a release ran");
    left_behind(&repo, &place);
    assert!(p.made().unwrap().is_empty());
    assert!(!p.gone(NAME).unwrap());

    let next = p
        .materialize("sluice/try/bolt/x/2", &rev(), &settings)
        .unwrap();
    assert_eq!(
        port(&next, "SLUICE_PORT_WEB"),
        port(&place, "SLUICE_PORT_WEB")
    );
    p.release("sluice/try/bolt/x/2").unwrap();
}

/// 123: a combination's checkout deleted by hand has the release its run
/// stored run once, from its scratch directory; then it is freed: git no
/// longer lists it, its scratch directory and record are gone and its
/// port is free. A second release of it runs nothing.
#[test]
fn a_deleted_checkouts_stored_release_runs_once_from_its_scratch_directory() {
    let repo = Repo::new("checkouts-release-gone");
    let p = repo.places();
    let settings = ported(&["web"], &ports(1).to_string());
    let place = p.materialize(NAME, &rev(), &settings).unwrap();
    repo.runs.stored.borrow_mut().insert(NAME.into());

    std::fs::remove_dir_all(&place.path).unwrap();
    assert!(p.gone(NAME).unwrap());
    let gone = p.release_gone(NAME).unwrap();
    assert_eq!((gone.ran, gone.failed), (true, None));
    assert!(called(
        &repo,
        &format!("release_gone {NAME} {}", place.tmp.display())
    ));
    assert_eq!(repo.runs.of(NAME), ["release_gone", "discard"]);
    left_behind(&repo, &place);
    assert!(p.made().unwrap().is_empty());

    let again = p.release_gone(NAME).unwrap();
    assert_eq!((again.ran, again.failed), (false, None));
    assert_eq!(
        repo.runs
            .of(NAME)
            .iter()
            .filter(|c| *c == "release_gone")
            .count(),
        1
    );

    let next = p
        .materialize("sluice/try/bolt/x/2", &rev(), &settings)
        .unwrap();
    assert_eq!(
        port(&next, "SLUICE_PORT_WEB"),
        port(&place, "SLUICE_PORT_WEB")
    );
    p.release("sluice/try/bolt/x/2").unwrap();
}

/// 123: a gone checkout whose stored release fails is freed all the same,
/// the release's failure returned with what it said; one whose gone
/// release was noted started, as a free that stopped part way leaves it,
/// runs nothing; and one whose runs stored no release runs nothing.
#[test]
fn a_gone_checkout_is_freed_whatever_its_stored_release_did() {
    let repo = Repo::new("checkouts-release-gone-fails");
    let p = repo.places();
    let settings = ported(&["web"], &ports(1).to_string());
    let place = p.materialize(NAME, &rev(), &settings).unwrap();
    *repo.runs.failing.borrow_mut() = Some("exit status: 2: usage: scripts/preview up".into());
    std::fs::remove_dir_all(&place.path).unwrap();
    let gone = p.release_gone(NAME).unwrap();
    assert!(gone.ran);
    let failed = gone.failed.expect("its failure is returned");
    assert!(failed.contains("usage: scripts/preview up"), "{failed}");
    *repo.runs.failing.borrow_mut() = None;
    left_behind(&repo, &place);
    assert!(p.made().unwrap().is_empty());
    assert!(!p.gone(NAME).unwrap());
    let next_name = "sluice/try/bolt/x/2";
    let next = p.materialize(next_name, &rev(), &settings).unwrap();
    assert_eq!(
        port(&next, "SLUICE_PORT_WEB"),
        port(&place, "SLUICE_PORT_WEB"),
        "its port is free"
    );

    // Its gone release noted started before: nothing runs again.
    repo.runs.stored.borrow_mut().insert(next_name.into());
    std::fs::remove_dir_all(&next.path).unwrap();
    let file = "sluice%2Ftry%2Fbolt%2Fx%2F2.json";
    let mut made = repo.record(file);
    made["gone_release"] = "2026-10-23T09:00:00Z".into();
    std::fs::write(
        repo.common.join("sluice/.places").join(file),
        serde_json::to_vec(&made).unwrap(),
    )
    .unwrap();
    let gone = p.release_gone(next_name).unwrap();
    assert_eq!((gone.ran, gone.failed), (false, None));
    assert_eq!(repo.runs.of(next_name), ["discard"], "it ran again");
    left_behind(&repo, &next);

    // No run stored a release.
    let third = p
        .materialize("sluice/try/bolt/x/3", &rev(), &settings)
        .unwrap();
    std::fs::remove_dir_all(&third.path).unwrap();
    let gone = p.release_gone("sluice/try/bolt/x/3").unwrap();
    assert_eq!((gone.ran, gone.failed), (false, None));
    left_behind(&repo, &third);
    assert!(p.made().unwrap().is_empty());
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
/// from what sluice holds and freed: the release its run stored runs
/// once, or nothing where it ran already, as on deneb, and sluice removes
/// what is left of the folder, its scratch, its record and its ports
/// going with it. The operator's acceptance frees one the same way, with
/// nothing run.
#[test]
fn a_checkout_git_no_longer_lists_is_freed() {
    let repo = Repo::new("checkouts-unlisted-freed");
    let p = repo.places();
    let settings = ported(&["web"], &ports(1).to_string());
    let freed = |name: &str, place: &Place| {
        left_behind(&repo, place);
        assert!(p.made().unwrap().is_empty());
        assert!(!p.gone(name).unwrap());
    };

    // Released once: its stored release runs, and what is left goes.
    let first = p.materialize(NAME, &rev(), &settings).unwrap();
    repo.runs.stored.borrow_mut().insert(NAME.into());
    let _unlock = read_only(&first);
    repo.worktrees.forget(&first.path);
    assert!(p.gone(NAME).unwrap());
    let gone = p.release_gone(NAME).unwrap();
    assert_eq!((gone.ran, gone.failed), (true, None));
    freed(NAME, &first);

    // The incident: its release ran as its run was removed, before git
    // stopped part way; nothing runs again.
    let second_name = "sluice/try/bolt/x/2";
    let second = p.materialize(second_name, &rev(), &settings).unwrap();
    assert_eq!(
        port(&second, "SLUICE_PORT_WEB"),
        port(&first, "SLUICE_PORT_WEB"),
        "its port is free"
    );
    let _unlock = read_only(&second);
    repo.worktrees.forget(&second.path);
    let gone = p.release_gone(second_name).unwrap();
    assert_eq!((gone.ran, gone.failed), (false, None));
    freed(second_name, &second);

    // The operator's acceptance: nothing runs, and what is left goes.
    let third_name = "sluice/try/bolt/x/3";
    let third = p.materialize(third_name, &rev(), &settings).unwrap();
    let _unlock = read_only(&third);
    repo.worktrees.forget(&third.path);
    p.free(third_name).unwrap();
    assert_eq!(repo.runs.of(third_name), ["discard"], "a release ran");
    freed(third_name, &third);
}

/// Where a candidate's or a session's place's record is.
fn record_path(repo: &Repo, name: &str) -> PathBuf {
    repo.common.join(format!(
        "sluice/.places/{}.json",
        sluice::runs::flat_name(name)
    ))
}

/// A place's record, as it is written.
fn record_of(repo: &Repo, name: &str) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(record_path(repo, name)).unwrap()).unwrap()
}

/// The repository's directory under the one standing for the user's.
fn repository_dir(repo: &Repo) -> PathBuf {
    repo.user_dir()
        .join(sluice::profiles::places::repository_key(&repo.common))
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
    let repo = Repo::new("checkouts-scratch-made");
    let p = repo.places();
    assert!(!repo.user_dir().exists());
    let place = adopted(&p, &repo.session("a")).unwrap();
    for dir in [repo.user_dir(), repository_dir(&repo)] {
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
    let repo = Repo::new("checkouts-scratch-refused");
    let p = repo.places();
    let session = repo.session("a");
    let user = repo.user_dir();
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
    let repo = Repo::new("checkouts-scratch-emptied");
    let p = repo.places();
    let session = repo.session("a");
    let first = adopted(&p, &session).unwrap();
    std::fs::remove_dir_all(repo.user_dir()).unwrap();

    let second = adopted(&p, &session).unwrap();
    assert_eq!(second.tmp, first.tmp);
    assert!(second.tmp.is_dir());
    for dir in [repo.user_dir(), repository_dir(&repo)] {
        assert!(users_own(&dir), "{}", dir.display());
    }
    p.release(&second.name).unwrap();
}

/// 35 as amended, rule core.30: two repositories' checkouts of one name,
/// under one user's directory, are given scratch paths of the same length
/// that differ in the repository's 10 hex digits alone.
#[test]
fn two_repositories_keep_their_scratch_apart() {
    let one = Repo::new("checkouts-scratch-one");
    let two = Repo::new("checkouts-scratch-two");
    let user = one.user_dir();
    let a = one.places().with_scratch_root(user.clone());
    let b = two.places().with_scratch_root(user.clone());
    let at_a = a.materialize(NAME, &rev(), &Settings::new()).unwrap();
    let at_b = b.materialize(NAME, &rev(), &Settings::new()).unwrap();
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
    let repo = Repo::new("checkouts-scratch-record");
    let p = repo.places();
    let place = p.materialize(NAME, &rev(), &Settings::new()).unwrap();
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
    let repo = Repo::new("checkouts-scratch-keep");
    let p = repo.places();
    let session = repo.session("a");
    let other = repo.session("other-with-a-longer-name");
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
    let repo = Repo::new("checkouts-scratch-before");
    let p = repo.places();
    let session = std::fs::canonicalize(repo.session("a")).unwrap();
    let former = repo
        .common
        .join("sluice/.tmp")
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
    let repo = Repo::new("checkouts-scratch-in-repository");
    let p = repo.places();
    let root = repo.common.join("sluice/.tmp");

    let session = std::fs::canonicalize(repo.session("a")).unwrap();
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
        .common
        .join("sluice/.places")
        .join(sluice::runs::dir_name(NAME));
    repo.worktrees.add(&checkout, &rev()).unwrap();
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
    let listed = |path: &std::path::Path| repo.worktrees.listed(path).unwrap();
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
    let repo = Repo::new("checkouts-scratch-going");
    let p = repo.places();
    let session = std::fs::canonicalize(repo.session("a")).unwrap();
    let kept = repo.common.join("sluice/.tmp/fedcba9876");
    let name = session_from_before(&repo, &session, Some(&kept), &kept);
    // A run of it going, as its supervisor holds it.
    repo.runs.going.borrow_mut().insert(name.clone());

    let beside = adopted(&p, &session).unwrap();
    assert_eq!(beside.tmp, kept);
    assert_eq!(record_of(&repo, &name)["scratch"].as_str(), kept.to_str());
    assert!(kept.join("kept").exists(), "its scratch was emptied");

    repo.runs.going.borrow_mut().remove(&name);
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
    let repo = Repo::new("checkouts-scratch-unmoved");
    let p = repo.places();
    let session = std::fs::canonicalize(repo.session("a")).unwrap();
    let kept = repo.common.join("sluice/.tmp/fedcba9876");
    let name = session_from_before(&repo, &session, Some(&kept), &kept);
    let fresh = adopted(&p, &repo.session("fresh")).unwrap();
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
    let repo = Repo::new("checkouts-scratch-sweep");
    let p = repo.places();
    let held = adopted(&p, &repo.session("a")).unwrap();
    std::fs::write(held.tmp.join("socket"), "").unwrap();
    let root = repo.common.join("sluice/.tmp");

    let older = std::fs::canonicalize(repo.session("older")).unwrap();
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

    let before = std::fs::canonicalize(repo.session("before")).unwrap();
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
    let repo = Repo::new("checkouts-scratch-unreadable");
    let p = repo.places();
    let held = p.materialize(NAME, &rev(), &Settings::new()).unwrap();
    std::fs::write(held.tmp.join("socket"), "").unwrap();
    let record = record_path(&repo, NAME);
    let written = std::fs::read(&record).unwrap();
    std::fs::write(&record, "{\"kind\":").unwrap();
    let root = repo.common.join("sluice/.tmp");
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

    let cut = repo.common.join(format!(
        "sluice/.places/{}%~0123456789abcdef.json",
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
    let one = Repo::new("checkouts-sweep-one");
    let two = Repo::new("checkouts-sweep-two");
    let user = one.user_dir();
    let a = one.places();
    let b = two.places().with_scratch_root(user.clone());
    make_users(&user);
    let ones = repository_dir(&one).join("abcdef0123");
    let twos = user
        .join(sluice::profiles::places::repository_key(&two.common))
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
    let repo = Repo::new("checkouts-sweep-open");
    let p = repo.places();
    let user = repo.user_dir();
    std::fs::create_dir(&user).unwrap();
    std::fs::set_permissions(&user, std::fs::Permissions::from_mode(0o755)).unwrap();
    let users_stray = repository_dir(&repo).join("abcdef0123");
    std::fs::create_dir_all(&users_stray).unwrap();
    let stray = repo.common.join("sluice/.tmp/0123abcdef");
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
