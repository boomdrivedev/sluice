//! Pull requests into a branch on GitHub (C.5 pull request, C.7
//! `pull_requests`), over the fakes: the pull request driver takes them in
//! from a file standing for GitHub, and reaches the repository's git
//! through a fake of its refs over the fakes' world, in the test's own
//! process. A round is `sluice step main` as the fakes' host process runs
//! it, its clock moved past the host's interval first, as a test that
//! changes GitHub as a person would and then steps.

mod support;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use clap::Parser;
use serde_json::Value;
use sluice::profiles::github::fake::FileForge;
use sluice::profiles::github::forge::{Forge as _, Standing, Status};
use sluice::profiles::github::kept::Kept;
use sluice::profiles::github::pulls::{self, Pulls};
use sluice::profiles::github::request_refs::{RemoteRefs, RequestRefs};
use sluice::profiles::github::Repo as GitHubRepo;
use sluice::{run, Cli, Pause};
use sluice_core::policy::POLICY_PATH;
use sluice_fakes::{Fakes, World};
use sluice_seams::{Delivery, Driver, Notification, Rev, Session, Submitter};
use support::Scratch;

/// The repository on GitHub every test's `main` is on.
const ADDRESS: &str = "https://github.com/o/r.git";

/// The least interval the host's settings take between two askings of
/// GitHub for a branch's open pull requests (136).
const DISCOVERY: Duration = Duration::from_secs(10);

/// This host as its peers name it, signing as `a`, one of the two signers
/// `main`'s policy names (C.6; 137; design D28).
fn peer() -> sluice_core::Peer {
    sluice_core::Peer {
        name: "a".into(),
        repository: "this-repository".into(),
        signer: Some("a".into()),
        ..sluice_core::Peer::default()
    }
}

/// The repository's git as the pull request driver reaches it, over the
/// fakes' world: a pull request's head on GitHub is the head the file
/// standing for GitHub holds for it, as GitHub's `refs/pull/<n>/head` is.
struct FakeRefs {
    world: World,
    forge: Rc<FileForge>,
    /// The host's own refs the driver fetched into.
    own: RefCell<BTreeMap<String, Rev>>,
    /// Each branch pushed to GitHub, in order, with what was pushed.
    pushed: RefCell<Vec<(String, Rev)>>,
    /// The pull requests whose head GitHub serves no longer.
    headless: RefCell<BTreeSet<u64>>,
}

impl RequestRefs for FakeRefs {
    fn remote_url(&self, remote: &str) -> Result<String> {
        match remote {
            "origin" => Ok(ADDRESS.into()),
            other => bail!("no remote {other}"),
        }
    }

    fn default_branch(&self) -> Result<Option<String>> {
        Ok(self.world.head().map(|(name, _)| name))
    }

    fn branch_names(&self) -> Result<Vec<String>> {
        Ok(self.world.branches())
    }

    fn messages(&self, tip: &Rev, head: &Rev) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let mut at = Some(head.clone());
        while let Some(rev) = at.filter(|r| r != tip) {
            out.push(self.world.message(&rev));
            at = self.world.parents(&rev).first().cloned();
        }
        out.reverse();
        Ok(out)
    }

    fn remote(&self, remote: &str) -> Result<Box<dyn RemoteRefs + '_>> {
        self.remote_url(remote)?;
        Ok(Box::new(FakeRemote(self)))
    }
}

struct FakeRemote<'a>(&'a FakeRefs);

impl RemoteRefs for FakeRemote<'_> {
    fn name(&self) -> &str {
        "origin"
    }

    fn read_ref(&self, name: &str) -> Result<Option<Rev>> {
        Ok(self.0.own.borrow().get(name).cloned())
    }

    fn fetch_ref(&self, src: &str, dst: &str) -> Result<Option<Rev>> {
        let n: u64 = src
            .strip_prefix("refs/pull/")
            .and_then(|s| s.strip_suffix("/head"))
            .and_then(|n| n.parse().ok())
            .ok_or_else(|| anyhow!("GitHub holds no {src}"))?;
        if self.0.headless.borrow().contains(&n) {
            return Ok(None);
        }
        let Some(pull) = self.0.forge.pull(&repo(), n)? else {
            return Ok(None);
        };
        let head = Rev::new(pull.head);
        self.0.own.borrow_mut().insert(dst.into(), head.clone());
        Ok(Some(head))
    }

    fn push_branch(&self, rev: &Rev, branch: &str) -> Result<()> {
        self.0
            .pushed
            .borrow_mut()
            .push((branch.into(), rev.clone()));
        self.0.forge.pushed(branch, &rev.0)
    }
}

fn repo() -> GitHubRepo {
    GitHubRepo::from_url(ADDRESS).unwrap()
}

/// The driver seam as an instance binds it: a pull request's place,
/// `pull/<n>`, to the pull request driver, and every other to the fakes'
/// (C.5).
struct Drivers<'a> {
    sessions: &'a dyn Driver,
    pulls: &'a Pulls,
}

impl Driver for Drivers<'_> {
    fn session(&self, worktree: &str) -> Result<Session> {
        match pulls::number(worktree) {
            Some(_) => self.pulls.session(worktree),
            None => self.sessions.session(worktree),
        }
    }

    fn recipient(&self, folder: &str) -> Result<Option<Submitter>> {
        self.sessions.recipient(folder)
    }

    fn named(&self, branch: &str) -> Result<Submitter> {
        self.sessions.named(branch)
    }

    fn notify(&self, n: &Notification) -> Result<Delivery> {
        match pulls::number(&n.to.worktree) {
            Some(_) => self.pulls.notify(n),
            None => self.sessions.notify(n),
        }
    }
}

struct Rounds(Cell<usize>);

impl Pause for Rounds {
    fn pause(&self) -> bool {
        self.0.set(self.0.get() + 1);
        assert!(self.0.get() < 50, "waited too long");
        true
    }
}

/// `main` on GitHub, taking pull requests with `conditions`, the lines of
/// its `pull_requests` table. Its policy names signers `a` and `b`, so its
/// records are on its remote, and this host signs as `a`, so it keeps
/// `main` and takes its requests in (C.6; 137; design D28).
struct OnGitHub {
    f: Fakes,
    forge: Rc<FileForge>,
    refs: Rc<FakeRefs>,
    pulls: Pulls,
    _dir: Scratch,
}

impl OnGitHub {
    fn new(conditions: &str) -> OnGitHub {
        OnGitHub::with_lines(conditions, "")
    }

    /// `main` testing only its longest combination (C.7 `batch`).
    fn batched(conditions: &str) -> OnGitHub {
        OnGitHub::with_lines(conditions, "batch = true\n")
    }

    /// `main` with `lines` in its table beside its remote and time limit.
    fn with_lines(conditions: &str, lines: &str) -> OnGitHub {
        let f = Fakes::new();
        f.world.init(
            "main",
            &[(
                POLICY_PATH,
                &format!(
                    "[tests]\nrunner = \"fake\"\n\n\
                     [signers.a]\nssh = \"a ssh-ed25519 AAAA\"\n\n\
                     [signers.b]\nssh = \"b ssh-ed25519 BBBB\"\n\n\
                     [branches.main]\nrequired = [\"#gate\"]\ntrust = [\"a\", \"b\"]\n\
                     remote = \"origin\"\ntimeout = \"1h\"\n{lines}\n\
                     [branches.main.pull_requests]\n{conditions}\n"
                ),
            )],
        );
        // `main`'s one gate task reads every file, and fails on a tree
        // holding one named `broken`.
        f.world.task("app:test", "");
        f.world.fails_when("app:test", |t| t.contains_key("broken"));
        f.world.on_remote("main");
        f.world.be_host("a");
        f.world.signs_as("a");
        let dir = Scratch::new("requests");
        let forge = Rc::new(FileForge::new(&dir.path().join("github.json"), &repo()));
        let refs = Rc::new(FakeRefs {
            world: f.world.clone(),
            forge: forge.clone(),
            own: RefCell::default(),
            pushed: RefCell::default(),
            headless: RefCell::default(),
        });
        forge.stamp_at(f.world.now());
        let pulls = Pulls::new(
            refs.clone(),
            dir.path(),
            Some(forge.clone()),
            Kept::new(&dir.path().join("state")),
            DISCOVERY,
            peer(),
        );
        // This host keeps main: a round of it takes the lead (72, 137).
        f.sluice().as_peer(peer()).step("main").unwrap();
        OnGitHub {
            f,
            forge,
            refs,
            pulls,
            _dir: dir,
        }
    }

    /// GitHub, each change to it stamped at this test's time.
    fn gh(&self) -> &FileForge {
        self.forge.stamp_at(self.f.world.now());
        &self.forge
    }

    /// The submissions from pull request `n`, after `steps` more rounds.
    fn of_after(&self, n: u64, steps: usize) -> Vec<Value> {
        for _ in 0..steps {
            self.step();
        }
        self.of(n)
    }

    /// Step until `done` holds, twenty rounds at most.
    fn step_until(&self, what: &str, done: impl Fn(&OnGitHub) -> bool) {
        for _ in 0..20 {
            if done(self) {
                return;
            }
            self.step();
        }
        assert!(done(self), "never {what}: {:?}", self.subs());
    }

    /// The comments on pull request `n`.
    fn comments(&self, n: u64) -> Vec<String> {
        self.forge
            .held()
            .unwrap()
            .comments
            .get(&n)
            .into_iter()
            .flatten()
            .map(|c| c.body.clone())
            .collect()
    }

    /// The report comment on pull request `n`.
    fn report(&self, n: u64) -> Option<String> {
        self.comments(n)
            .into_iter()
            .find(|c| c.starts_with("<!-- sluice-report: main -->"))
    }

    /// sluice's check named `name` on `head`: its status and title.
    fn check(&self, head: &Rev, name: &str) -> Option<(Status, String)> {
        let held = self.forge.held().unwrap();
        let p = held.posted.get(&head.0)?.iter().find(|p| p.name == name)?;
        Some((p.posting.status, p.posting.title.clone()))
    }

    /// The text of sluice's check named `name` on `head`.
    fn text(&self, head: &Rev, name: &str) -> Option<String> {
        let held = self.forge.held().unwrap();
        let p = held.posted.get(&head.0)?.iter().find(|p| p.name == name)?;
        p.posting.text.clone()
    }

    /// How many times sluice's check runs were posted.
    fn posts(&self) -> usize {
        self.forge.held().unwrap().posts.len()
    }

    /// The host's interval passes, so what it last read of a request it
    /// follows is read again (136, design D4).
    fn interval_passes(&self) {
        self.f
            .world
            .advance(chrono::Duration::from_std(DISCOVERY).unwrap());
    }

    /// A second passes, so what is done next is later, to the second
    /// GitHub and the records keep, than what was done before.
    fn a_second_passes(&self) {
        self.f.world.advance(chrono::Duration::seconds(2));
    }

    /// `sluice <args>` on this host: its exit and what it said.
    fn sluice(&self, args: &[&str]) -> (i32, String) {
        self.forge.stamp_at(self.f.world.now());
        let cli =
            Cli::try_parse_from(std::iter::once("sluice").chain(args.iter().copied())).unwrap();
        let mut host = support::FakeHost::new(self.f.world.clone(), 0);
        host.peer = peer();
        let seams = self.f.seams();
        let drivers = Drivers {
            sessions: seams.driver,
            pulls: &self.pulls,
        };
        let mut out = Vec::new();
        let code = run(
            cli,
            sluice_core::Seams {
                driver: &drivers,
                ..seams
            },
            &host,
            &(),
            &self.pulls,
            &self.f.clock,
            &Rounds(Cell::new(0)),
            &mut out,
        )
        .unwrap_or_else(|e| panic!("sluice {args:?}: {e:#}"));
        (code, String::from_utf8(out).unwrap())
    }

    /// One round of `main`, its pull requests taken in first, the host's
    /// interval past since it last asked GitHub, as the host process runs
    /// a step: each cause its intake met, which a step says as a warning
    /// (128; design D26).
    fn round(&self) -> Vec<String> {
        self.f
            .world
            .advance(chrono::Duration::from_std(DISCOVERY).unwrap());
        self.forge.stamp_at(self.f.world.now());
        let seams = self.f.seams();
        let drivers = Drivers {
            sessions: seams.driver,
            pulls: &self.pulls,
        };
        let s = sluice_core::Sluice::new(
            sluice_core::Seams {
                driver: &drivers,
                ..seams
            },
            &self.f.clock,
        )
        .as_peer(peer());
        let stepped = sluice::host_process::pass::step(&s, &self.pulls, &self.f.clock, "main")
            .unwrap_or_else(|e| panic!("a round of main: {e:#}"));
        stepped["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|w| w.as_str().map(str::to_string))
            .collect()
    }

    /// One round of `main`, which meets nothing.
    fn step(&self) {
        let warned = self.round();
        assert!(warned.is_empty(), "{warned:?}");
    }

    /// A session's worktree on `unit/<name>` off `main`, with one commit
    /// of `changes`, its message `Change <the first path>`.
    fn session(&self, name: &str, changes: &[(&str, &str)]) -> String {
        let w = self.worktree(name);
        self.commit_in(&w, changes);
        w
    }

    /// A session's worktree on `unit/<name>` off `main`, with no commit.
    fn worktree(&self, name: &str) -> String {
        self.worktree_on(&format!("unit/{name}"))
    }

    /// A session's worktree on the new branch `branch` off `main`, with no
    /// commit; its agent is named by its branch.
    fn worktree_on(&self, branch: &str) -> String {
        self.f
            .world
            .session(branch, branch, &self.f.world.tip("main"));
        branch.to_string()
    }

    /// A commit in the worktree `w` of `changes`; returns the new head.
    fn commit_in(&self, w: &str, changes: &[(&str, &str)]) -> Rev {
        let changes: Vec<(&str, Option<&str>)> =
            changes.iter().map(|(p, c)| (*p, Some(*c))).collect();
        self.commit(w, &changes, &format!("Change {}", changes[0].0))
    }

    /// A commit in the worktree `w` writing `path`, its message `message`;
    /// returns the new head.
    fn commit_message(&self, w: &str, path: &str, message: &str) -> Rev {
        self.commit(w, &[(path, Some(&format!("{path}\n")))], message)
    }

    fn commit(&self, w: &str, changes: &[(&str, Option<&str>)], message: &str) -> Rev {
        let at = self.f.seams().driver.session(w).unwrap().head;
        let head = self.f.world.work_saying(&at, changes, message);
        self.f.world.session(w, w, &head);
        head
    }

    /// `sluice submit main <args>` from the worktree `w`: its exit and
    /// what it said.
    fn submit(&self, w: &str, args: &[&str]) -> (i32, String) {
        let mut all = vec!["submit", "main", "--worktree", w];
        all.extend(args);
        self.sluice(&all)
    }

    /// [`OnGitHub::submit`], which must take the work.
    fn submitted(&self, w: &str, args: &[&str]) -> String {
        let (code, said) = self.submit(w, args);
        assert_eq!(code, 0, "{said}");
        said
    }

    /// The pull request from `head_ref`, as GitHub holds it now.
    fn pull_from(&self, head_ref: &str) -> sluice::profiles::github::forge::Pull {
        self.forge
            .by_ref(head_ref)
            .unwrap()
            .unwrap_or_else(|| panic!("no pull request from {head_ref}"))
    }

    /// A commit of `changes` in the worktree `w`, pushed to GitHub by git
    /// alone, as a fix another submission lands on its branch would be: no
    /// submit pushes it. Returns the new head.
    fn push_from(&self, w: &str, changes: &[(&str, &str)]) -> Rev {
        let head = self.commit_in(w, changes);
        self.gh().pushed(w, &head.0).unwrap();
        head
    }

    /// The revision last pushed to GitHub's `branch`.
    fn pushed(&self, branch: &str) -> Option<Rev> {
        self.refs
            .pushed
            .borrow()
            .iter()
            .rev()
            .find(|(b, _)| b == branch)
            .map(|(_, r)| r.clone())
    }

    /// GitHub serves no head for pull request `n`, its branch gone, or, with
    /// `false`, serves it again, its branch pushed again.
    fn headless(&self, n: u64, gone: bool) {
        let mut headless = self.refs.headless.borrow_mut();
        match gone {
            true => headless.insert(n),
            false => headless.remove(&n),
        };
    }

    /// `main`'s submissions.
    fn subs(&self) -> Vec<Value> {
        let (code, out) = self.sluice(&["status", "main", "--json", "--all"]);
        assert_eq!(code, 0, "{out}");
        let v: Value = serde_json::from_str(&out).unwrap();
        v["submissions"].as_array().cloned().unwrap_or_default()
    }

    /// The submissions from pull request `n`.
    fn of(&self, n: u64) -> Vec<Value> {
        let place = format!("pull/{n}");
        self.subs()
            .into_iter()
            .filter(|s| s["from"]["worktree"] == place.as_str())
            .collect()
    }

    /// A branch of the repository off `main` with one commit of `changes`;
    /// returns its head.
    fn branch(&self, changes: &[(&str, Option<&str>)]) -> Rev {
        self.f.world.work(&self.f.world.tip("main"), changes)
    }

    /// A person opens a pull request into `main` from `name`, a branch of
    /// the repository with one commit of `changes`; returns its number and
    /// head.
    fn by_hand(&self, name: &str, changes: &[(&str, Option<&str>)], draft: bool) -> (u64, Rev) {
        let head = self.branch(changes);
        let n = self
            .forge
            .open_by("person", "main", name, &head.0, draft, None)
            .unwrap();
        (n, head)
    }

    /// A commit on top of pull request `n`'s head, pushed to its branch;
    /// returns the new head.
    fn push_more(&self, n: u64, path: &str) -> Rev {
        let at = Rev::new(self.forge.pull(&repo(), n).unwrap().unwrap().head);
        let head = self.f.world.work(&at, &[(path, Some("more\n"))]);
        self.gh().push(n, &head.0).unwrap();
        head
    }

    /// How many of `main`'s combinations, as its records hold them, hold
    /// submission `id`.
    fn holding(&self, id: u64) -> usize {
        let snap = self.f.sluice().seams().state.read("main").unwrap();
        let doc = sluice_core::line::Line::load("main", snap).unwrap().doc;
        doc.candidates
            .iter()
            .filter(|c| c.submissions.contains(&id))
            .count()
    }

    /// What sluice's check on `head` reads: its status, title and
    /// summary.
    fn posted(&self, head: &Rev) -> Option<(Status, String, String)> {
        let held = self.forge.held().unwrap();
        let p = held
            .posted
            .get(&head.0)?
            .iter()
            .find(|p| p.name == "sluice")?;
        Some((
            p.posting.status,
            p.posting.title.clone(),
            p.posting.summary.clone(),
        ))
    }
}

// ---------------------------------------------------------------------------
// pull-request-intake
// ---------------------------------------------------------------------------

/// S21, waiting for QA: approved and out of draft, it waits while
/// `qa/approved` has not passed on its head; once it passes, the next
/// round submits it, its agent the request's author where its body names
/// none.
#[test]
fn waiting_for_qa() {
    let g = OnGitHub::new("reviews = 1\nchecks = [\"qa/approved\"]");
    let (n, head) = g.by_hand("unit/qa", &[("qa.txt", Some("qa\n"))], false);
    g.gh().review(n, "reviewer", "APPROVED", &head.0).unwrap();
    g.gh()
        .check(&head.0, "qa/approved", Standing::Pending)
        .unwrap();
    g.step();
    assert!(g.of(n).is_empty(), "{:?}", g.subs());
    g.gh()
        .check(&head.0, "qa/approved", Standing::Passed)
        .unwrap();
    g.step();
    let subs = g.of(n);
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0]["head"], head.0.as_str());
    assert_eq!(subs[0]["from"]["agent"], "person");
}

/// A draft waits, every condition holding, until it is marked ready.
#[test]
fn a_draft_waits() {
    let g = OnGitHub::new("");
    let (n, head) = g.by_hand("unit/draft", &[("d.txt", Some("d\n"))], true);
    g.step();
    g.step();
    assert!(g.of(n).is_empty(), "{:?}", g.subs());
    assert_eq!(g.posted(&head).unwrap().1, "waiting: a draft");
    g.gh().set_draft(n, false).unwrap();
    g.step();
    assert_eq!(g.of(n).len(), 1, "{:?}", g.subs());
}

/// S23: a pull request opened by hand, ready, is submitted by the next
/// round with no `sluice submit` run for it, its agent the one its body
/// names, from the place `pull/<n>` and the branch it was opened from
/// (ADR 0025), with no path.
#[test]
fn a_pull_request_opened_by_hand() {
    let g = OnGitHub::new("reviews = 0");
    let (n, head) = g.by_hand("unit/hand", &[("h.txt", Some("h\n"))], false);
    g.forge
        .edit(
            &repo(),
            n,
            None,
            "Hand work.\n\n<!-- sluice: agent=crw-1-unit-1 -->",
        )
        .unwrap();
    g.step();
    let subs = g.of(n);
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0]["head"], head.0.as_str());
    assert_eq!(subs[0]["from"]["agent"], "crw-1-unit-1");
    assert_eq!(subs[0]["from"]["worktree"], format!("pull/{n}"));
    assert_eq!(subs[0]["from"]["head"], "unit/hand");
    assert!(subs[0]["from"].get("path").is_none());
}

/// C.7 `reviews`: an approval given on an earlier head does not count.
#[test]
fn reviews_on_an_older_head_do_not_count() {
    let g = OnGitHub::new("reviews = 1");
    let (n, old) = g.by_hand("unit/stale", &[("s.txt", Some("s\n"))], false);
    g.gh().review(n, "reviewer", "APPROVED", &old.0).unwrap();
    let new = g.push_more(n, "s2.txt");
    g.step();
    assert!(g.of(n).is_empty(), "{:?}", g.subs());
    assert_eq!(g.posted(&new).unwrap().1, "waiting: needs 1 review");
}

/// A fork's pull request is never submitted, whatever its conditions, and
/// nothing is posted on it.
#[test]
fn a_forks_pull_request_is_never_submitted() {
    let g = OnGitHub::new("reviews = 0");
    let head = g.branch(&[("f.txt", Some("f\n"))]);
    let n = g
        .gh()
        .open_by(
            "stranger",
            "main",
            "fork",
            &head.0,
            false,
            Some("stranger/r"),
        )
        .unwrap();
    g.step();
    g.step();
    assert!(g.of(n).is_empty(), "{:?}", g.subs());
    assert_eq!(g.posted(&head), None);
}

/// S22: a commit pushed to a submitted pull request withdraws its
/// submission; it waits for its conditions on the new head, and is
/// submitted again once they hold.
#[test]
fn a_new_commit_while_queued() {
    let g = OnGitHub::new("reviews = 1");
    g.f.world.hold_runs(true);
    let (n, old) = g.by_hand("unit/moving", &[("m.txt", Some("x\n"))], false);
    g.gh().review(n, "reviewer", "APPROVED", &old.0).unwrap();
    g.step();
    assert_eq!(g.of(n).len(), 1);
    let new = g.push_more(n, "m2.txt");
    g.step();
    let subs = g.of(n);
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0]["state"], "withdrawn");
    assert_eq!(g.posted(&new).unwrap().1, "waiting: needs 1 review");
    g.gh().review(n, "reviewer", "APPROVED", &new.0).unwrap();
    g.step();
    let subs = g.of(n);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], new.0.as_str());
    assert_ne!(subs[1]["state"], "withdrawn");
}

/// S24: a submitted pull request closed is withdrawn.
#[test]
fn closed_while_queued() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let (n, _) = g.by_hand("unit/closed", &[("c.txt", Some("x\n"))], false);
    g.step();
    assert_eq!(g.of(n).len(), 1);
    g.gh().close(n).unwrap();
    g.step();
    assert_eq!(g.of(n)[0]["state"], "withdrawn", "{:?}", g.subs());
}

/// obsolete-combinations, "Closed while under test" (24, 67; design D24):
/// the host keeps main and is testing a pull request's combination, and
/// the request is closed; the round whose intake withdraws it drops its
/// combination before reading any run, so main's records hold no
/// combination with it, and its run is ended.
#[test]
fn a_request_closed_under_test_drops_its_combination() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let (n, _) = g.by_hand("unit/closing", &[("c.txt", Some("x\n"))], false);
    g.step();
    let id = g.of(n)[0]["id"].clone();
    assert_eq!(g.of(n)[0]["state"], "testing", "{:?}", g.subs());
    assert_eq!(g.holding(id.as_u64().unwrap()), 1, "under test");
    assert_eq!(g.f.world.live_runs(), 1, "under test");
    g.gh().close(n).unwrap();
    g.step();
    assert_eq!(g.of(n)[0]["state"], "withdrawn", "{:?}", g.subs());
    assert_eq!(
        g.holding(id.as_u64().unwrap()),
        0,
        "its combination dropped"
    );
    assert!(g.f.world.live_runs() == 0, "its run is ended");
}

/// A blamed pull request whose author marks it a draft and then ready
/// again, with no new commits, is submitted again on the same head.
#[test]
fn blamed_then_retried() {
    let g = OnGitHub::new("");
    let (n, head) = g.by_hand("unit/retried", &[("broken", Some("x\n"))], false);
    g.step_until("blamed", |g| {
        g.of(n).first().is_some_and(|s| s["state"] == "failed")
    });
    g.a_second_passes();
    g.gh().set_draft(n, true).unwrap();
    g.gh().set_draft(n, false).unwrap();
    g.step();
    let subs = g.of(n);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], head.0.as_str());
}

/// A blamed pull request left alone is never submitted again. Its check
/// says it failed; one comment names the failed check, where its log is
/// and how to submit again, pointing at the report; the report is one
/// comment of its own (ADR 0044), which the `sluice` check carries, and
/// the failed check has its own check. Output is posted only where the
/// policy asks for it.
#[test]
fn blamed_and_left_alone() {
    let g = OnGitHub::new("");
    let (n, head) = g.by_hand("unit/alone", &[("broken", Some("x\n"))], false);
    g.step_until("blamed", |g| {
        g.of(n).first().is_some_and(|s| s["state"] == "failed")
    });
    for _ in 0..3 {
        g.step();
    }
    assert_eq!(g.of(n).len(), 1, "{:?}", g.subs());
    let (status, title, _) = g.posted(&head).unwrap();
    assert_eq!(
        (status, title.as_str()),
        (Status::Failure, "failed: app:test")
    );
    let comments = g.comments(n);
    assert_eq!(comments.len(), 2, "{comments:?}");
    let c = comments.iter().find(|c| c.contains("told=")).unwrap();
    assert!(c.contains("`app:test` failed"), "{c}");
    assert!(c.contains("The logs, on the host"), "{c}");
    assert!(
        c.contains("Push a fix") && c.contains("draft and then ready"),
        "{c}"
    );
    assert!(c.contains("[sluice's report]("), "{c}");
    let r = g.report(n).unwrap();
    assert!(
        r.contains("## sluice: failed on main, #1 is to blame"),
        "{r}"
    );
    assert!(r.contains("| 🟥 | `app:test` |"), "{r}");
    assert_eq!(g.text(&head, "sluice").as_deref(), Some(r.as_str()));
    let own = g.check(&head, "sluice / app:test").unwrap();
    assert_eq!(own.0, Status::Failure);
    assert!(!r.contains("it broke"), "{r}");
}

/// A pull request opened by hand, approved and submitted, whose
/// submission the operator withdraws with `sluice withdraw`; returns its
/// number and head.
fn withdrawn_by_hand(g: &OnGitHub, name: &str) -> (u64, Rev) {
    g.f.world.hold_runs(true);
    let (n, head) = g.by_hand(name, &[("w.txt", Some("x\n"))], false);
    g.gh().review(n, "reviewer", "APPROVED", &head.0).unwrap();
    g.step();
    let id = g.of(n)[0]["id"].as_u64().unwrap().to_string();
    let (code, said) = g.sluice(&["withdraw", &id, "--as", "operator"]);
    assert_eq!(code, 0, "{said}");
    (n, head)
}

/// Withdrawn by the operator, opened by hand (120, C.5): its conditions
/// holding, no later round submits it; its check names the operator and
/// how to bring it back, and no comment is left.
#[test]
fn withdrawn_by_the_operator_opened_by_hand() {
    let g = OnGitHub::new("reviews = 1");
    let (n, head) = withdrawn_by_hand(&g, "unit/rests");
    let subs = g.of_after(n, 4);
    assert_eq!(subs.len(), 1, "never submitted again: {subs:?}");
    assert_eq!(subs[0]["state"], "withdrawn");
    assert_eq!(subs[0]["withdrawn"]["by"], "operator");
    assert!(!g.forge.pull(&repo(), n).unwrap().unwrap().draft);
    let (status, title, summary) = g.posted(&head).unwrap();
    assert_eq!(
        (status, title.as_str()),
        (Status::Queued, "waiting: withdrawn by the operator")
    );
    assert!(
        summary.contains("by the operator")
            && summary.contains("New commits, or marking it a draft and then ready for review")
            && summary.contains("Forecast:"),
        "{summary}"
    );
    assert!(
        !g.comments(n).iter().any(|c| c.contains("told=")),
        "{:?}",
        g.comments(n)
    );
}

/// Withdrawn by the operator, then retried by its author: marked draft and
/// then ready, with no new commits, the same head is submitted again; a
/// draft waits.
#[test]
fn withdrawn_by_the_operator_then_retried_by_its_author() {
    let g = OnGitHub::new("reviews = 1");
    let (n, head) = withdrawn_by_hand(&g, "unit/rests-retried");
    assert_eq!(g.of_after(n, 2).len(), 1, "{:?}", g.subs());
    g.a_second_passes();
    g.gh().set_draft(n, true).unwrap();
    g.step();
    assert_eq!(g.of(n).len(), 1, "a draft waits: {:?}", g.subs());
    g.gh().set_draft(n, false).unwrap();
    let subs = g.of_after(n, 1);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], head.0.as_str());
    assert_ne!(subs[1]["state"], "withdrawn", "{subs:?}");
}

/// Withdrawn by the operator, then a new commit: the new head is
/// submitted once its conditions hold on it.
#[test]
fn withdrawn_by_the_operator_then_a_new_commit() {
    let g = OnGitHub::new("reviews = 1");
    let (n, _) = withdrawn_by_hand(&g, "unit/rests-moved");
    assert_eq!(g.of_after(n, 2).len(), 1, "{:?}", g.subs());
    let new = g.push_more(n, "m.txt");
    g.step();
    assert_eq!(g.of(n).len(), 1, "waits for its review: {:?}", g.subs());
    assert_eq!(g.posted(&new).unwrap().1, "waiting: needs 1 review");
    g.gh().review(n, "reviewer", "APPROVED", &new.0).unwrap();
    let subs = g.of_after(n, 1);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], new.0.as_str());
    assert_ne!(subs[1]["state"], "withdrawn", "{subs:?}");
}

/// pull-request-status, edited not repeated (ADR 0044): one report
/// comment, edited from testing to landed, its check passed; a further
/// round, nothing reading differently, posts and edits nothing.
#[test]
fn one_report_comment_edited_until_landed() {
    let g = OnGitHub::new("");
    let (n, head) = g.by_hand("unit/fine", &[("fine", Some("x\n"))], false);
    g.step_until("landed", |g| {
        g.of(n).first().is_some_and(|s| s["state"] == "landed")
    });
    for _ in 0..2 {
        g.step();
    }
    let reports: Vec<String> = g
        .comments(n)
        .into_iter()
        .filter(|c| c.starts_with("<!-- sluice-report: main -->"))
        .collect();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(
        reports[0].contains("## sluice: landed on main as "),
        "{}",
        reports[0]
    );
    assert!(reports[0].contains("| 🟩 | `app:test` |"), "{}", reports[0]);
    let own = g.check(&head, "sluice / app:test").unwrap();
    assert_eq!(own.0, Status::Success);
    let before = g.posts();
    let edits = g.forge.held().unwrap().edits.len();
    g.step();
    assert_eq!(g.posts(), before);
    assert_eq!(g.forge.held().unwrap().edits.len(), edits);
}

/// S50 (118, 20): behind a blamed pull request, nothing of the failed
/// combination is shown, and it lands.
#[test]
fn behind_a_blamed_pull_request_no_failure_is_shown() {
    let g = OnGitHub::new("");
    let (one, _) = g.by_hand("unit/broken", &[("broken", Some("x\n"))], false);
    let (two, head) = g.by_hand("unit/fine", &[("fine", Some("x\n"))], false);
    g.step_until("blamed", |g| {
        g.of(one).first().is_some_and(|s| s["state"] == "failed")
    });
    let shown = g.report(two).unwrap_or_default();
    assert!(!shown.contains("#1") && !shown.contains("🟥"), "{shown}");
    if let Some((status, _, _)) = g.posted(&head) {
        assert_ne!(status, Status::Failure);
    }
    if let Some(own) = g.check(&head, "sluice / app:test") {
        assert_ne!(own.0, Status::Failure, "{own:?}");
    }
    g.step_until("landed", |g| {
        g.of(two).iter().any(|s| s["state"] == "landed")
    });
}

/// Whether a round's warnings name pull request `n` as having no head.
fn names_no_head(warned: &[String], n: u64) -> bool {
    warned.iter().any(|w| {
        w.starts_with(&format!("taking in main's pull requests: pull/{n}: "))
            && w.contains(&format!("pull/{n} has no head"))
    })
}

/// S63 (128): GitHub serves no head for one of two pull requests opened by
/// hand, the one it lists first, whose branch is gone. Each round names it
/// and that it has no head, and the other is submitted; once its branch is
/// pushed again, the next round submits it. Closed, one with no head is
/// named no more.
#[test]
fn s63_a_request_sluice_cannot_fetch_holds_no_other() {
    let g = OnGitHub::new("reviews = 0");
    g.f.world.hold_runs(true);
    let (gone, gone_head) = g.by_hand("unit/gone", &[("gone.txt", Some("g\n"))], false);
    let (fine, fine_head) = g.by_hand("unit/fine", &[("fine.txt", Some("x\n"))], false);
    g.headless(gone, true);
    let warned = g.round();
    assert!(names_no_head(&warned, gone), "{warned:?}");
    assert!(g.of(gone).is_empty(), "{:?}", g.subs());
    assert_eq!(g.of(fine).len(), 1, "{:?}", g.subs());
    let (_, title, _) = g.posted(&fine_head).unwrap();
    assert!(
        title.starts_with("queued") || title.starts_with("testing"),
        "{title}"
    );
    for _ in 0..2 {
        let warned = g.round();
        assert!(names_no_head(&warned, gone), "{warned:?}");
        assert!(g.of(gone).is_empty(), "{:?}", g.subs());
    }
    g.headless(gone, false);
    g.step();
    let subs = g.of(gone);
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0]["head"], gone_head.0.as_str());
    let (third, _) = g.by_hand("unit/third", &[("third.txt", Some("t\n"))], false);
    g.headless(third, true);
    assert!(names_no_head(&g.round(), third));
    g.gh().close(third).unwrap();
    g.step();
    assert!(g.of(third).is_empty(), "{:?}", g.subs());
}

/// 128: GitHub answers an error for one pull request's reviews. The round
/// names it and its reviews, and submits the other, whose review holds;
/// once its reviews can be read, the next round submits it.
#[test]
fn reviews_that_cannot_be_read_hold_no_other() {
    let g = OnGitHub::new("reviews = 1");
    g.f.world.hold_runs(true);
    let (one, one_head) = g.by_hand("unit/one", &[("one.txt", Some("1\n"))], false);
    let (two, two_head) = g.by_hand("unit/two", &[("two.txt", Some("2\n"))], false);
    g.gh()
        .review(one, "reviewer", "APPROVED", &one_head.0)
        .unwrap();
    g.gh()
        .review(two, "reviewer", "APPROVED", &two_head.0)
        .unwrap();
    g.forge.fail_reviews(one, true);
    let warned = g.round();
    assert!(
        warned
            .iter()
            .any(|w| w.contains(&format!("pull/{one}: reading pull/{one}'s reviews"))),
        "{warned:?}"
    );
    assert!(g.of(one).is_empty(), "{:?}", g.subs());
    assert_eq!(g.of(two).len(), 1, "{:?}", g.subs());
    g.forge.fail_reviews(one, false);
    g.step();
    let subs = g.of(one);
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0]["head"], one_head.0.as_str());
}

/// What is missing: a pull request out of draft needing a review and
/// `qa/approved` shows both, queued, with its forecast.
#[test]
fn the_check_names_what_is_missing() {
    let g = OnGitHub::new("reviews = 1\nchecks = [\"qa/approved\"]");
    let (_, head) = g.by_hand("unit/missing", &[("m.txt", Some("m\n"))], false);
    g.step();
    let (status, title, summary) = g.posted(&head).unwrap();
    assert_eq!(status, Status::Queued);
    assert_eq!(title, "waiting: needs 1 review, qa/approved");
    assert!(
        summary.contains("1 review") && summary.contains("`qa/approved`"),
        "{summary}"
    );
    assert!(
        summary.contains("Forecast: merges onto main without conflict"),
        "{summary}"
    );
}

/// Two rounds with nothing changed between them post nothing the second
/// time.
#[test]
fn no_repeated_posts() {
    let g = OnGitHub::new("reviews = 1");
    g.by_hand("unit/quiet", &[("q.txt", Some("q\n"))], true);
    g.by_hand("unit/still", &[("r.txt", Some("r\n"))], false);
    g.step();
    let posts = g.posts();
    assert_eq!(posts, 2);
    g.step();
    assert_eq!(g.posts(), posts);
}

/// A conflict seen early: a waiting pull request changing a file a queued
/// one changes otherwise names that one and the file, and runs nothing.
#[test]
fn a_conflict_seen_early() {
    let g = OnGitHub::new("reviews = 1");
    g.f.world.hold_runs(true);
    let (a, a_head) = g.by_hand("unit/a", &[("plan.py", Some("a\n"))], false);
    g.gh().review(a, "reviewer", "APPROVED", &a_head.0).unwrap();
    let (b, b_head) = g.by_hand("unit/b", &[("plan.py", Some("b\n"))], false);
    g.step();
    assert_eq!(g.of(a).len(), 1);
    assert!(g.of(b).is_empty());
    let (_, _, summary) = g.posted(&b_head).unwrap();
    assert!(
        summary.contains(&format!("conflicts with #{a} in plan.py")),
        "{summary}"
    );
}

/// 128: a draft whose head GitHub serves none of is only a draft. The
/// round names it, submits the pull request that is ready, and posts the
/// draft's check as a draft's, with no forecast.
#[test]
fn a_draft_whose_head_cannot_be_fetched_is_only_a_draft() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let (draft, draft_head) = g.by_hand("unit/drafted", &[("d.txt", Some("d\n"))], true);
    let (ready, _) = g.by_hand("unit/ready", &[("r.txt", Some("r\n"))], false);
    g.headless(draft, true);
    let warned = g.round();
    assert!(
        warned
            .iter()
            .any(|w| w.contains(&format!("pull/{draft}: pull/{draft} has no head"))),
        "{warned:?}"
    );
    assert!(g.of(draft).is_empty(), "{:?}", g.subs());
    assert_eq!(g.of(ready).len(), 1, "{:?}", g.subs());
    let (status, title, summary) = g.posted(&draft_head).unwrap();
    assert_eq!(
        (status, title.as_str()),
        (Status::Queued, "waiting: a draft")
    );
    assert!(!summary.contains("Forecast:"), "{summary}");
}

// ---------------------------------------------------------------------------
// pull-request-submit
// ---------------------------------------------------------------------------

/// First submit: the agent's branch is on GitHub, a pull request from it
/// into `main` is open, titled by the branch with no subject given, and
/// the output names it and what it waits for. Submit again: the same pull
/// request has the new head, and no second is opened.
#[test]
fn submit_opens_then_updates_a_pull_request() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.session("review", &[("r.txt", "r\n")]);
    let said = g.submitted(&w, &["--no-wait"]);
    let pull = g.pull_from("unit/review");
    assert!(pull.open && !pull.draft && pull.base == "main");
    assert_eq!(Some(Rev::new(pull.head.clone())), g.pushed("unit/review"));
    assert!(
        pull.body.contains("<!-- sluice: agent=unit/review -->"),
        "{}",
        pull.body
    );
    assert_eq!(pull.title, "unit/review");
    assert!(
        said.contains(&format!(
            "pushed unit/review · opened #{}: unit/review",
            pull.number
        )),
        "{said}"
    );
    assert!(said.contains("waiting: needs 1 review"), "{said}");
    assert!(g.of(pull.number).is_empty());
    let again = g.commit_in(&w, &[("r2.txt", "r2\n")]);
    let said = g.submitted(&w, &["--no-wait"]);
    assert!(
        said.contains(&format!("updated #{}", pull.number)),
        "{said}"
    );
    let held = g.forge.held().unwrap();
    assert_eq!(held.pulls.len(), 1);
    assert_eq!(held.pulls[0].head, again.0);
}

/// C.5 pull request: a bolt's branch submitted with no subject is titled
/// by its name, never by its first commit, its proposal's.
#[test]
fn a_bolt_submitted_with_no_subject_is_titled_by_its_branch() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.worktree_on("bolt/review-cards");
    g.commit_message(&w, "proposal.md", "docs(openspec): propose review-cards");
    g.commit_message(&w, "cards.txt", "feat(cards): batch review cards");
    let said = g.submitted(&w, &["--no-wait"]);
    let pull = g.pull_from("bolt/review-cards");
    assert_eq!(pull.title, "bolt/review-cards");
    assert!(
        said.contains(&format!("opened #{}: bolt/review-cards", pull.number)),
        "{said}"
    );
}

/// C.5 pull request: a session submitted with a subject is titled by it,
/// as written.
#[test]
fn a_session_submitted_with_a_subject_is_titled_by_it() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.session("review", &[("r.txt", "r\n")]);
    let said = g.submitted(&w, &["--no-wait", "--subject", "Batch review cards"]);
    let pull = g.pull_from("unit/review");
    assert_eq!(pull.title, "Batch review cards");
    assert!(
        said.contains(&format!(
            "pushed unit/review · opened #{}: Batch review cards",
            pull.number
        )),
        "{said}"
    );
}

/// C.5 pull request: whatever titles it, the description holds every
/// commit's message whole, oldest first, and the title is neither's.
#[test]
fn the_description_still_carries_the_commits() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.worktree("cards");
    g.commit_message(&w, "one.txt", "Add the first card\n\nWith its reasons.");
    g.commit_message(&w, "two.txt", "Add the second card");
    g.submitted(&w, &["--no-wait", "--subject", "Batch review cards"]);
    let pull = g.pull_from("unit/cards");
    assert_eq!(
        pull.body,
        "Add the first card\n\nWith its reasons.\n\nAdd the second card\n\n\
         <!-- sluice: agent=unit/cards -->"
    );
    assert_eq!(pull.title, "Batch review cards");
}

/// C.5 pull request: sluice opened the request titled by its branch; a
/// new head submitted with a subject retitles it.
#[test]
fn a_new_head_with_a_subject_retitles_it() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.session("review", &[("r.txt", "r\n")]);
    g.submitted(&w, &["--no-wait"]);
    let n = g.pull_from("unit/review").number;
    assert_eq!(g.pull_from("unit/review").title, "unit/review");
    let head = g.commit_in(&w, &[("r2.txt", "r2\n")]);
    let said = g.submitted(&w, &["--no-wait", "--subject", "Batch review cards"]);
    let pull = g.pull_from("unit/review");
    assert_eq!((pull.number, pull.head.as_str()), (n, head.0.as_str()));
    assert_eq!(pull.title, "Batch review cards");
    assert!(
        said.contains(&format!("updated #{n}: Batch review cards")),
        "{said}"
    );
    assert_eq!(g.forge.held().unwrap().pulls.len(), 1);
}

/// C.5 pull request: a new head submitted with no subject leaves the
/// title as it is, and asks nothing of who opened it.
#[test]
fn a_new_head_with_no_subject_keeps_the_title() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.session("review", &[("r.txt", "r\n")]);
    g.submitted(&w, &["--no-wait", "--subject", "Batch review cards"]);
    let logins = g.forge.logins();
    let head = g.commit_in(&w, &[("r2.txt", "r2\n")]);
    let said = g.submitted(&w, &["--no-wait"]);
    let pull = g.pull_from("unit/review");
    assert_eq!(pull.head, head.0);
    assert_eq!(pull.title, "Batch review cards");
    assert!(
        said.contains(&format!("updated #{}: Batch review cards", pull.number)),
        "{said}"
    );
    assert_eq!(g.forge.logins(), logins);
}

/// A worktree on `unit/<name>` with one commit of `message`, pushed to
/// GitHub, and a pull request from it opened by `author` and titled
/// `title`, a draft; returns the worktree and the request's number.
fn opened_by(g: &OnGitHub, author: &str, name: &str, message: &str, title: &str) -> (String, u64) {
    let w = g.worktree(name);
    let head = g.commit_message(&w, &format!("{name}.txt"), message);
    let n = g
        .gh()
        .open_by(author, "main", &w, &head.0, true, None)
        .unwrap();
    g.gh().edit(&repo(), n, Some(title), "").unwrap();
    (w, n)
}

/// C.5 pull request: a request sluice opened while it titled them by the
/// first commit is retitled by the next subject, which asks who opened it.
#[test]
fn a_request_opened_before_titles_changed_is_retitled() {
    let g = OnGitHub::new("reviews = 1");
    let proposal = "docs(openspec): propose review";
    let (w, n) = opened_by(&g, "sluice", "review", proposal, proposal);
    let logins = g.forge.logins();
    g.submitted(&w, &["--no-wait", "--subject", "Batch review cards"]);
    let pull = g.pull_from("unit/review");
    assert_eq!(pull.number, n);
    assert_eq!(pull.title, "Batch review cards");
    assert!(pull.body.starts_with(proposal), "{}", pull.body);
    assert_eq!(g.forge.logins(), logins + 1);
}

/// 120: a request a person opened keeps their title, whatever the subject;
/// the submit still writes its description and marks it ready at the head
/// submitted.
#[test]
fn a_subject_on_a_request_opened_by_hand_leaves_its_title() {
    let g = OnGitHub::new("reviews = 1");
    let (w, n) = opened_by(
        &g,
        "person",
        "review",
        "Change r.txt",
        "Review cards, first pass",
    );
    let head = g.pull_from("unit/review").head;
    let said = g.submitted(&w, &["--no-wait", "--subject", "Batch review cards"]);
    let pull = g.pull_from("unit/review");
    assert_eq!(pull.number, n);
    assert_eq!(pull.title, "Review cards, first pass");
    assert!(
        pull.body.starts_with("Change r.txt\n\n<!-- sluice: agent="),
        "{}",
        pull.body
    );
    assert!(!pull.draft && pull.head == head, "{pull:?}");
    assert!(
        said.contains(&format!("updated #{n}: Review cards, first pass")),
        "{said}"
    );
}

/// Step until pull request `n`'s submission lands; returns it.
fn landed(g: &OnGitHub, n: u64) -> Value {
    g.step_until("landed", |g| g.of(n).iter().any(|s| s["state"] == "landed"));
    g.of(n)
        .into_iter()
        .find(|s| s["state"] == "landed")
        .unwrap()
}

/// The message of the commit `sub`, a submission of `main`, landed as.
fn landing_message(g: &OnGitHub, sub: &Value) -> String {
    g.f.world
        .message(&Rev::new(sub["landed_as"].as_str().expect("landed")))
}

/// 23, 118: a request a submit opened with a subject is taken with its
/// title as the submission's subject, so its landing commit names the
/// work by it, its trailers still naming the request's place.
#[test]
fn a_request_sluice_opened_with_a_subject_lands_named_by_it() {
    let g = OnGitHub::new("");
    let w = g.session("review", &[("r.txt", "r\n")]);
    g.submitted(&w, &["--no-wait", "--subject", "Batch review cards"]);
    let n = g.pull_from("unit/review").number;
    let sub = landed(&g, n);
    let m = landing_message(&g, &sub);
    let id = &sub["id"];
    assert_eq!(
        m.lines().next(),
        Some(format!("chore(land): #{id} Batch review cards").as_str()),
        "{m}"
    );
    let from = format!(
        "\nSluice-From: {} (pull/{n})\n",
        sub["from"]["agent"].as_str().unwrap()
    );
    assert!(m.contains(&from), "{m}");
}

/// 23, 120: a request a person opened lands named by the title they
/// gave it.
#[test]
fn a_request_a_person_opened_lands_named_by_its_title() {
    let g = OnGitHub::new("");
    let (n, _) = g.by_hand("unit/hand", &[("h.txt", Some("h\n"))], false);
    g.gh()
        .edit(&repo(), n, Some("Review cards, first pass"), "Hand work.")
        .unwrap();
    let sub = landed(&g, n);
    let m = landing_message(&g, &sub);
    assert_eq!(
        m.lines().next(),
        Some(format!("chore(land): #{} Review cards, first pass", sub["id"]).as_str()),
        "{m}"
    );
}

/// A message of `chars` characters: `subject`, a blank line, a body.
fn long_message(subject: &str, chars: usize) -> String {
    let body = "x".repeat(chars - subject.chars().count() - 2);
    format!("{subject}\n\n{body}")
}

/// The line a description gives the commits it holds by subject alone.
fn more_commits(r: usize, name: &str) -> String {
    let s = if r == 1 { "" } else { "s" };
    format!(
        "{r} more commit{s}, each by its subject; all of them in full: \
         https://github.com/o/r/compare/main...unit/{name}"
    )
}

/// A bolt too long for one description: its description holds the first
/// two messages whole, the line counting the third with the compare view,
/// the third's subject and the agent's mark, no longer than GitHub holds.
#[test]
fn a_bolt_too_long_for_one_description() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.worktree("long");
    let ms: Vec<String> = (1..=3)
        .map(|k| long_message(&format!("Add part {k}"), 30_000))
        .collect();
    for (k, m) in ms.iter().enumerate() {
        g.commit_message(&w, &format!("part-{k}.txt"), m);
    }
    g.submitted(&w, &["--no-wait"]);
    let pull = g.pull_from("unit/long");
    assert!(pull.open, "{pull:?}");
    assert_eq!(
        pull.body,
        format!(
            "{}\n\n{}\n\n{}\n\n- Add part 3\n\n<!-- sluice: agent=unit/long -->",
            ms[0],
            ms[1],
            more_commits(1, "long")
        )
    );
    assert!(pull.body.chars().count() <= 65_536);
}

/// One message longer than GitHub holds: no part of its body, the line
/// counting it with the compare view, its subject and the agent's mark.
#[test]
fn one_message_longer_than_github_holds() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.worktree("huge");
    g.commit_message(&w, "huge.txt", &long_message("Add the huge file", 70_000));
    g.submitted(&w, &["--no-wait"]);
    let pull = g.pull_from("unit/huge");
    assert!(pull.open, "{pull:?}");
    assert_eq!(
        pull.body,
        format!(
            "{}\n\n- Add the huge file\n\n<!-- sluice: agent=unit/huge -->",
            more_commits(1, "huge")
        )
    );
}

/// A new commit, submitted again: the description holds both messages,
/// oldest first, then the agent's mark.
#[test]
fn a_new_commit_submitted_again_is_described() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.worktree("again");
    g.commit_message(&w, "first.txt", "Add the first file");
    g.submitted(&w, &["--no-wait"]);
    assert_eq!(
        g.pull_from("unit/again").body,
        "Add the first file\n\n<!-- sluice: agent=unit/again -->"
    );
    g.commit_message(&w, "second.txt", "Add the second file");
    g.submitted(&w, &["--no-wait"]);
    assert_eq!(
        g.pull_from("unit/again").body,
        "Add the first file\n\nAdd the second file\n\n<!-- sluice: agent=unit/again -->"
    );
}

/// Submitted again at the same head: the second submit does not edit the
/// pull request; the request stands there, so the submit answers with it
/// (record 0065).
#[test]
fn submitted_again_at_the_same_head_edits_nothing() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.session("same", &[("same.txt", "same\n")]);
    g.submitted(&w, &["--no-wait"]);
    let before = g.pull_from("unit/same");
    g.a_second_passes();
    let said = g.submitted(&w, &["--no-wait"]);
    assert!(said.contains("waiting: needs 1 review"), "{said}");
    let after = g.pull_from("unit/same");
    assert_eq!(after.updated_at, before.updated_at, "edited");
    assert_eq!(after.body, before.body);
}

/// The crew #127/#128 case (120, S56): a pull request a submit opened is
/// queued, and a commit no submit pushed moves its head. Its submission is
/// withdrawn, sluice marks it a draft, and no round submits it in its
/// agent's name, whatever its conditions.
#[test]
fn a_new_head_no_submit_pushed() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let w = g.session("held", &[("h.txt", "x\n")]);
    g.submitted(&w, &["--no-wait"]);
    let n = g.pull_from("unit/held").number;
    assert_eq!(g.of(n).len(), 1, "queued: {:?}", g.subs());
    let main = g.f.world.tip("main");
    let new = g.push_from(&w, &[("fix.txt", "a fix\n")]);
    let subs = g.of_after(n, 4);
    assert_eq!(subs.len(), 1, "never submitted again: {subs:?}");
    assert_eq!(subs[0]["state"], "withdrawn", "{subs:?}");
    let pull = g.pull_from("unit/held");
    assert!(pull.open && pull.draft, "{pull:?}");
    assert_eq!(pull.head, new.0);
    assert_eq!(g.f.world.tip("main"), main, "nothing landed");
    assert_eq!(
        g.posted(&new).map(|p| (p.0, p.1)),
        Some((Status::Queued, "waiting: the next submit".into()))
    );
}

/// Withdrawn by the operator: a pull request a submit opened, its
/// submission withdrawn with `sluice withdraw`, is marked a draft, and no
/// later round submits it.
#[test]
fn withdrawn_by_the_operator() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let w = g.session("pulled", &[("p.txt", "x\n")]);
    g.submitted(&w, &["--no-wait"]);
    let pull = g.pull_from("unit/pulled");
    let id = g.of(pull.number)[0]["id"].as_u64().unwrap().to_string();
    let (code, said) = g.sluice(&["withdraw", &id, "--as", "operator"]);
    assert_eq!(code, 0, "{said}");
    let subs = g.of_after(pull.number, 3);
    assert_eq!(subs.len(), 1, "never submitted again: {subs:?}");
    assert_eq!(subs[0]["state"], "withdrawn");
    assert!(g.pull_from("unit/pulled").draft);
    assert_eq!(
        g.posted(&Rev::new(pull.head.clone())).unwrap().1,
        "waiting: the next submit"
    );
}

/// Marked ready by a person: a pull request a submit opened, held and
/// marked a draft after its head moved, is submitted at its new head once
/// a person marks it ready for review on GitHub, later to the second than
/// the withdrawal.
#[test]
fn held_then_marked_ready_by_a_person() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let w = g.session("readied", &[("r.txt", "x\n")]);
    g.submitted(&w, &["--no-wait"]);
    let n = g.pull_from("unit/readied").number;
    let new = g.push_from(&w, &[("fix.txt", "a fix\n")]);
    assert_eq!(g.of_after(n, 2).len(), 1);
    assert!(g.pull_from("unit/readied").draft);
    g.a_second_passes();
    g.gh().set_draft(n, false).unwrap();
    let subs = g.of_after(n, 1);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], new.0.as_str());
    assert_ne!(subs[1]["state"], "withdrawn", "{subs:?}");
}

/// Held when it cannot be marked a draft: GitHub refuses the draft, and no
/// round submits the pull request; each round says it waits for the next
/// submit and that GitHub would not mark it a draft, in GitHub's words
/// (128, design D26), its check says it waits for the next submit, and
/// each asking tries the draft again, until one is taken.
#[test]
fn held_when_github_refuses_the_draft() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let w = g.session("refused", &[("r.txt", "x\n")]);
    g.submitted(&w, &["--no-wait"]);
    let n = g.pull_from("unit/refused").number;
    g.forge.refuse_drafts(true);
    let new = g.push_from(&w, &[("fix.txt", "a fix\n")]);
    for _ in 0..3 {
        let warned = g.round();
        assert!(
            warned.iter().any(|w| w.contains(&format!(
                "taking in main's pull requests: pull/{n}: it waits for the next submit, and \
                 GitHub would not mark it a draft"
            )) && w.contains(sluice::profiles::github::fake::NOT_ACCESSIBLE)),
            "{warned:?}"
        );
    }
    let subs = g.of(n);
    assert_eq!(subs.len(), 1, "never submitted again: {subs:?}");
    assert_eq!(subs[0]["state"], "withdrawn");
    assert!(!g.pull_from("unit/refused").draft);
    assert_eq!(g.posted(&new).unwrap().1, "waiting: the next submit");
    g.forge.refuse_drafts(false);
    g.step();
    assert!(g.pull_from("unit/refused").draft);
    assert_eq!(g.of(n).len(), 1, "{:?}", g.subs());
}

/// A worktree holding more than its commits is refused, naming what it
/// holds, and nothing is pushed or opened.
#[test]
fn submit_refuses_a_dirty_worktree() {
    let g = OnGitHub::new("");
    let w = g.session("dirty", &[("d.txt", "d\n")]);
    g.f.world.dirty(&w, &["d.txt"]);
    let (code, said) = g.submit(&w, &[]);
    assert_eq!(code, 2, "{said}");
    assert!(said.contains("d.txt"), "{said}");
    assert_eq!(g.pushed("unit/dirty"), None);
    assert!(g.forge.held().unwrap().pulls.is_empty());
}

/// `--no-wait` returns once the pull request is open; `--status` later
/// reports its state.
#[test]
fn no_wait_then_status() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.session("later", &[("l.txt", "l\n")]);
    g.submitted(&w, &["--no-wait"]);
    let said = g.submitted(&w, &["--status"]);
    assert!(said.contains("waiting: needs 1 review"), "{said}");
}

/// Waiting for a review: `--status --json` names the pull request's head,
/// that it is no draft, what a submit from the worktree would send and the
/// wait that follows it (record 0130), beside its number, branch and state.
#[test]
fn status_names_the_head_waiting_for_a_review() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.session("headed", &[("h.txt", "h\n")]);
    g.submitted(&w, &["--no-wait"]);
    let pull = g.pull_from("unit/headed");
    let doc: Value = serde_json::from_str(&g.submitted(&w, &["--status", "--json"])).unwrap();
    assert_eq!(
        doc,
        serde_json::json!({
            "pull_request": pull.number, "branch": "main",
            "state": "waiting: needs 1 review", "head": pull.head, "draft": false,
            "sends": pull.head,
            "wait": format!("sluice wait main --pull-request {}", pull.number),
        })
    );
}

/// A draft: `--status --json` names its head and that it is a draft.
#[test]
fn status_names_a_draft() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.session("drafted", &[("d.txt", "d\n")]);
    g.submitted(&w, &["--no-wait"]);
    let pull = g.pull_from("unit/drafted");
    g.gh().set_draft(pull.number, true).unwrap();
    g.interval_passes();
    let doc: Value = serde_json::from_str(&g.submitted(&w, &["--status", "--json"])).unwrap();
    assert_eq!(doc["pull_request"], pull.number, "{doc}");
    assert_eq!(doc["head"], pull.head.as_str(), "{doc}");
    assert_eq!(doc["draft"], true, "{doc}");
}

/// Record 0065: a pull request sent back as a draft stands no more, so a
/// submit again with nothing committed marks the request ready for review
/// at that head, and intake takes that head again.
#[test]
fn run_again_marks_a_draft_ready() {
    let g = OnGitHub::new("");
    let w = g.session("drafted", &[("broken", "x\n")]);
    g.submitted(&w, &["--no-wait"]);
    let pull = g.pull_from("unit/drafted");
    g.step_until("sent back", |g| g.pull_from("unit/drafted").draft);
    let said = g.submitted(&w, &["--no-wait"]);
    assert!(
        said.contains(&format!("updated #{}", pull.number))
            && said.contains("marked ready for review"),
        "{said}"
    );
    let subs = g.of(pull.number);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], pull.head.as_str());
}

/// No wait, its conditions hold: the submit answers with the submission
/// sluice took the pull request in as, from its checkout `pull/<n>` at the
/// head pushed, with the pull request and the wait that follows it beside
/// it (record 0130); `sluice wait <id>` follows that submission to its
/// landing.
#[test]
fn no_wait_answers_with_its_submission() {
    let g = OnGitHub::new("");
    let w = g.session("ready", &[("ready.txt", "ready\n")]);
    let doc: Value = serde_json::from_str(&g.submitted(&w, &["--no-wait", "--json"])).unwrap();
    let n = g.pull_from("unit/ready").number;
    assert_eq!(doc["pull_request"], n, "{doc}");
    assert_eq!(doc["branch"], "main", "{doc}");
    assert_eq!(doc["ahead"], serde_json::json!([]), "{doc}");
    assert_eq!(
        doc["wait"],
        format!("sluice wait main --pull-request {n}").as_str(),
        "{doc}"
    );
    let id = doc["id"]
        .as_u64()
        .unwrap_or_else(|| panic!("{doc}"))
        .to_string();
    let (code, out) = g.sluice(&["status", &id, "--json"]);
    assert_eq!(code, 0, "{out}");
    let record: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(record["from"]["worktree"], format!("pull/{n}").as_str());
    assert_eq!(
        Some(Rev::new(record["head"].as_str().unwrap())),
        g.pushed("unit/ready")
    );
    let (code, out) = g.sluice(&["wait", &id, "--json"]);
    assert_eq!(code, sluice::LANDED, "{out}");
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["state"], "landed");
}

/// No wait, waiting for a review: the submit answers with the pull request,
/// what it waits for and the wait that follows it (record 0130), and
/// nothing of it is queued.
#[test]
fn no_wait_waiting_for_a_review_answers_with_the_pull_request() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.session("unreviewed", &[("u.txt", "u\n")]);
    let doc: Value = serde_json::from_str(&g.submitted(&w, &["--no-wait", "--json"])).unwrap();
    let n = g.pull_from("unit/unreviewed").number;
    assert_eq!(
        doc,
        serde_json::json!({
            "pull_request": n, "branch": "main", "state": "waiting: needs 1 review",
            "wait": format!("sluice wait main --pull-request {n}"),
        })
    );
    assert!(g.of(n).is_empty(), "{:?}", g.subs());
}

/// pull-request-submit, "No wait, in words, waiting for a review": after
/// the pull request's state, the submit says the wait that follows it, on
/// the last line, and exits 0 (record 0130 D4).
#[test]
fn no_wait_waiting_for_a_review_names_its_wait_in_words() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.session("review", &[("r.txt", "r\n")]);
    let said = g.submitted(&w, &["--no-wait"]);
    let n = g.pull_from("unit/review").number;
    let lines: Vec<&str> = said.lines().filter(|l| !l.trim().is_empty()).collect();
    let state = lines
        .iter()
        .position(|l| *l == "waiting: needs 1 review")
        .expect(&said);
    let wait = format!("wait: sluice wait main --pull-request {n}");
    assert_eq!(lines.last().copied(), Some(wait.as_str()), "{said}");
    assert!(state < lines.len() - 1, "{said}");
}

/// No wait, in words: after the line naming the pull request opened, the
/// submission it was taken in as and what is queued ahead of it, then the
/// wait that follows the pull request (record 0130).
#[test]
fn no_wait_says_its_submission() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let w = g.session("worded", &[("worded.txt", "w\n")]);
    let said = g.submitted(&w, &["--no-wait"]);
    let n = g.pull_from("unit/worded").number;
    let id = g.of(n)[0]["id"].as_u64().unwrap();
    let lines: Vec<&str> = said.lines().collect();
    let opened = format!("pushed unit/worded · opened #{n}: unit/worded");
    let at = lines.iter().position(|l| *l == opened).expect(&said);
    assert_eq!(
        lines.get(at + 1).copied(),
        Some(format!("submission {id} to main from #{n} · nothing queued ahead").as_str()),
        "{said}"
    );
    assert_eq!(
        lines.get(at + 2).copied(),
        Some(format!("wait: sluice wait main --pull-request {n}").as_str()),
        "{said}"
    );
}

/// ADR 0041: `sluice wait <branch>` follows this worktree's pull request,
/// out of time with the document `--status` prints; `sluice wait <n>`
/// takes the pull requests in before each round, and lands it.
#[test]
fn wait_follows_a_pull_request() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.session("waits", &[("v.txt", "v\n")]);
    g.submitted(&w, &["--no-wait"]);
    let (code, out) = g.sluice(&[
        "wait",
        "main",
        "--worktree",
        &w,
        "--timeout",
        "2s",
        "--json",
    ]);
    assert_eq!(code, sluice::WAITING, "{out}");
    let pull = g.pull_from("unit/waits");
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["pull_request"], pull.number);
    assert_eq!(doc["state"], "waiting: needs 1 review");
    g.gh()
        .review(pull.number, "reviewer", "APPROVED", &pull.head)
        .unwrap();
    g.step();
    let id = g.of(pull.number)[0]["id"].as_u64().unwrap();
    let (code, out) = g.sluice(&["wait", &id.to_string(), "--json"]);
    assert_eq!(code, sluice::LANDED, "{out}");
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["state"], "landed");
}

/// submission-wait, "Out of time, waiting for a review, in words": the
/// agent's pull request waits for a review, and `sluice wait main
/// --timeout 2s` for its worktree exits 5, its last line naming the wait
/// that follows the request by its number, never one that reads the
/// folder it runs in (record 0130).
#[test]
fn out_of_time_waiting_for_a_review_names_its_wait() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.session("reviewed", &[("r.txt", "r\n")]);
    g.submitted(&w, &["--no-wait"]);
    let n = g.pull_from("unit/reviewed").number;
    let (code, out) = g.sluice(&["wait", "main", "--worktree", &w, "--timeout", "2s"]);
    assert_eq!(code, sluice::WAITING, "{out}");
    let last = out.lines().rfind(|l| !l.trim().is_empty());
    assert_eq!(
        last,
        Some(format!("still open: #{n}; sluice wait main --pull-request {n}").as_str()),
        "{out}"
    );
}

// ---------------------------------------------------------------------------
// group-submit (125)
// ---------------------------------------------------------------------------

impl OnGitHub {
    /// A session's worktree for each of `names`, each with one commit.
    fn sessions(&self, names: &[&str]) -> Vec<String> {
        names
            .iter()
            .map(|n| self.session(n, &[(&format!("{n}.txt"), "x\n")]))
            .collect()
    }

    /// `sluice submit main` naming each of `ws`, then `rest`.
    fn submit_group(&self, ws: &[String], rest: &[&str]) -> (i32, String) {
        let mut args = vec!["submit", "main"];
        for w in ws {
            args.extend(["--worktree", w.as_str()]);
        }
        args.extend(rest);
        self.sluice(&args)
    }

    /// The open pull request from `unit/<name>`.
    fn pull_of(&self, name: &str) -> sluice::profiles::github::forge::Pull {
        self.pull_from(&format!("unit/{name}"))
    }

    /// Whether pull request `n` is a draft, as GitHub holds it now.
    fn is_draft(&self, n: u64) -> bool {
        self.forge.pull(&repo(), n).unwrap().unwrap().draft
    }

    /// A person approves pull request `n` at its head.
    fn approve(&self, n: u64) {
        let head = self.forge.pull(&repo(), n).unwrap().unwrap().head;
        self.gh().review(n, "reviewer", "APPROVED", &head).unwrap();
    }

    /// What sluice's check on pull request `n`'s head reads, its title.
    fn shows(&self, n: u64) -> String {
        let head = self.forge.pull(&repo(), n).unwrap().unwrap().head;
        self.posted(&Rev::new(head))
            .map(|p| p.1)
            .unwrap_or_default()
    }
}

/// The group mark of a pull request's body: its token and the rest.
fn group_mark(body: &str) -> Option<(String, String)> {
    body.lines().find_map(|l| {
        let (token, rest) = l.strip_prefix("<!-- sluice: group=")?.split_once(' ')?;
        Some((token.to_string(), rest.to_string()))
    })
}

/// Three worktrees submitted as one act, none of their pull requests
/// reviewed: each branch is on GitHub, each has an open pull request into
/// `main`, a draft marked 1, 2 and 3 of 3 of one group at its head, and
/// the refusal names each with the review it needs; nothing is queued.
/// One of them submitted alone afterwards is marked one of no group.
#[test]
fn a_group_submit_opens_every_pull_request_marked() {
    let g = OnGitHub::new("reviews = 1");
    let names = ["g1", "g2", "g3"];
    let ws = g.sessions(&names);
    let (code, said) = g.submit_group(&ws, &["--no-wait"]);
    assert_eq!(code, 2, "{said}");
    let mut token = None;
    for (k, (name, w)) in names.iter().zip(&ws).enumerate() {
        let pull = g.pull_of(name);
        assert!(pull.open && pull.draft && pull.base == "main", "{pull:?}");
        let head = g.pushed(&format!("unit/{name}")).unwrap();
        assert_eq!(pull.head, head.0);
        assert!(pull
            .body
            .contains(&format!("<!-- sluice: agent=unit/{name} -->")));
        let (t, rest) = group_mark(&pull.body).unwrap_or_else(|| panic!("{}", pull.body));
        assert_eq!(rest, format!("{}/3 head={head} -->", k + 1));
        assert_eq!(*token.get_or_insert(t.clone()), t, "one group");
        let n = pull.number;
        assert!(
            said.contains(&format!("pushed unit/{name} · opened #{n}: unit/{name}")),
            "{said}"
        );
        assert!(
            said.contains(&format!("refused: #{n} from {w} needs 1 review")),
            "{said}"
        );
        assert!(g.of(n).is_empty());
    }
    g.submitted(&ws[0], &["--no-wait"]);
    let pull = g.pull_of("g1");
    assert_eq!(group_mark(&pull.body), None, "{}", pull.body);
    assert!(pull.body.contains("<!-- sluice: agent=unit/g1 -->"));
}

/// 125: two worktrees submitted with a subject each: each pull request is
/// opened titled by its own, in the worktrees' order; submitted again
/// with the subjects the other way round, each sluice opened takes its
/// new one.
#[test]
fn two_requests_two_subjects() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let ws = g.sessions(&["a", "b"]);
    let subjects = [
        "--subject",
        "Batch review cards",
        "--subject",
        "Card filters",
    ];
    let (code, said) = g.submit_group(&ws, &[&["--no-wait"][..], &subjects].concat());
    assert_eq!(code, 0, "{said}");
    for (name, title) in [("a", "Batch review cards"), ("b", "Card filters")] {
        let pull = g.pull_of(name);
        assert_eq!(pull.title, title);
        assert!(
            said.contains(&format!(
                "pushed unit/{name} · opened #{}: {title}",
                pull.number
            )),
            "{said}"
        );
    }
    let swapped = [
        "--subject",
        "Card filters",
        "--subject",
        "Batch review cards",
    ];
    let (code, said) = g.submit_group(&ws, &[&["--no-wait"][..], &swapped].concat());
    assert_eq!(code, 0, "{said}");
    assert_eq!(g.pull_of("a").title, "Card filters");
    assert_eq!(g.pull_of("b").title, "Batch review cards");
}

/// 125: two worktrees submitted with no subject: each pull request is
/// titled by its own branch.
#[test]
fn two_requests_no_subjects() {
    let g = OnGitHub::new("");
    let ws = g.sessions(&["a", "b"]);
    let (code, said) = g.submit_group(&ws, &["--no-wait"]);
    assert_eq!(code, 0, "{said}");
    assert_eq!(g.pull_of("a").title, "unit/a");
    assert_eq!(g.pull_of("b").title, "unit/b");
}

/// Three pull requests, every condition holding: the act leaves each ready
/// for review at its head, marked 1, 2 and 3 of 3 of one group, and queues
/// a submission from each, from its checkout `pull/<n>` at that head, in
/// the order given and in one write, each said after its pull request;
/// they are tested as one combination and land.
#[test]
fn a_group_submit_queues_three_pull_requests_in_order() {
    let g = OnGitHub::batched("");
    g.f.world.hold_runs(true);
    let names = ["q1", "q2", "q3"];
    let ws = g.sessions(&names);
    let (code, said) = g.submit_group(&ws, &["--no-wait"]);
    assert_eq!(code, 0, "{said}");
    let mut token = None;
    let mut ns = Vec::new();
    let mut ids = Vec::new();
    for (k, name) in names.iter().enumerate() {
        let pull = g.pull_of(name);
        assert!(pull.open && !pull.draft && pull.base == "main", "{pull:?}");
        let head = g.pushed(&format!("unit/{name}")).unwrap();
        assert_eq!(pull.head, head.0);
        let (t, rest) = group_mark(&pull.body).unwrap_or_else(|| panic!("{}", pull.body));
        assert_eq!(rest, format!("{}/3 head={head} -->", k + 1));
        assert_eq!(*token.get_or_insert(t.clone()), t, "one group");
        let subs = g.of(pull.number);
        assert_eq!(subs.len(), 1, "{:?}", g.subs());
        assert_eq!(subs[0]["head"], head.0.as_str());
        assert_eq!(
            subs[0]["submitted"],
            g.of(g.pull_of(names[0]).number)[0]["submitted"]
        );
        ns.push(pull.number);
        ids.push(subs[0]["id"].as_u64().unwrap());
    }
    assert!(
        ids.windows(2).all(|w| w[0] < w[1]),
        "in the order given: {ids:?}"
    );
    let lines: Vec<&str> = said.lines().collect();
    for (k, name) in names.iter().enumerate() {
        let opened = format!(
            "pushed unit/{name} · opened #{}: unit/{name} · marked ready for review",
            ns[k]
        );
        let at = lines
            .iter()
            .position(|l| *l == opened)
            .unwrap_or_else(|| panic!("{opened:?} in {said}"));
        let ahead = match k {
            0 => "nothing queued ahead".to_string(),
            _ => format!(
                "{k} queued ahead ({})",
                ns[..k]
                    .iter()
                    .map(|n| format!("#{n}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        assert_eq!(
            lines.get(at + 1).copied(),
            Some(format!("submission {} to main from #{} · {ahead}", ids[k], ns[k]).as_str()),
            "{said}"
        );
    }
    g.f.world.hold_runs(false);
    g.f.world.complete_all();
    g.step_until("landed", |g| {
        ns.iter()
            .all(|n| g.of(*n).first().is_some_and(|s| s["state"] == "landed"))
    });
    for n in &ns {
        assert_eq!(
            g.of(*n)[0]["tested"]["submissions"],
            serde_json::json!(ids),
            "one combination of all three"
        );
    }
}

/// Under `--json` with `--no-wait`, the act answers with each submission
/// beside its worktree and pull request, and what is queued ahead of it.
#[test]
fn a_group_submit_answers_with_each_submission_in_json() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let names = ["j1", "j2"];
    let ws = g.sessions(&names);
    let (code, out) = g.submit_group(&ws, &["--no-wait", "--json"]);
    assert_eq!(code, 0, "{out}");
    let doc: Value = serde_json::from_str(&out).unwrap_or_else(|_| panic!("{out}"));
    assert_eq!(doc["branch"], "main", "{doc}");
    let items = doc["submissions"].as_array().expect("submissions");
    assert_eq!(items.len(), 2, "{doc}");
    let mut ahead = Vec::new();
    for ((item, w), name) in items.iter().zip(&ws).zip(names) {
        let n = g.pull_of(name).number;
        let id = g.of(n)[0]["id"].as_u64().unwrap();
        assert_eq!(item["id"], id, "{doc}");
        assert_eq!(item["worktree"], w.as_str(), "{doc}");
        assert_eq!(item["pull_request"], n, "{doc}");
        assert_eq!(item["ahead"], serde_json::json!(ahead), "{doc}");
        ahead.push(id);
    }
}

/// group-submit, "No wait, every condition holding" and "No wait, in
/// words": under `--json`, each item of `submissions` holds its `id`, its
/// pull request and the wait that follows that pull request; in words,
/// the act says each wait on a line of its own (record 0130 D4).
#[test]
fn a_group_act_names_each_pull_requests_wait() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let ws = g.sessions(&["n1", "n2"]);
    let (code, out) = g.submit_group(&ws, &["--no-wait", "--json"]);
    assert_eq!(code, 0, "{out}");
    let doc: Value = serde_json::from_str(&out).unwrap_or_else(|_| panic!("{out}"));
    let items = doc["submissions"].as_array().expect("submissions");
    assert_eq!(items.len(), 2, "{doc}");
    for (item, name) in items.iter().zip(["n1", "n2"]) {
        let n = g.pull_of(name).number;
        assert!(item["id"].is_u64(), "{doc}");
        assert_eq!(item["pull_request"], n, "{doc}");
        assert_eq!(
            item["wait"],
            format!("sluice wait main --pull-request {n}").as_str(),
            "{doc}"
        );
    }
    let ws = g.sessions(&["w1", "w2"]);
    let (code, said) = g.submit_group(&ws, &["--no-wait"]);
    assert_eq!(code, 0, "{said}");
    let lines: Vec<&str> = said.lines().collect();
    for name in ["w1", "w2"] {
        let n = g.pull_of(name).number;
        let wait = format!("wait: sluice wait main --pull-request {n}");
        assert!(lines.contains(&wait.as_str()), "{wait:?} in {said}");
    }
}

/// One short of a review: the pull requests of `s1` and `s3` are open at
/// the heads their worktrees hold, ready and approved there, and `s2`'s is
/// not open. The act queues nothing and exits 2, naming `s2`'s pull
/// request and that it needs 1 review, and saying all three are drafts;
/// each is a draft at its head, marked one of the group, and no round
/// takes any of them. Under `--json` the refusal is the error's document.
#[test]
fn a_group_submit_one_short_of_a_review_is_refused() {
    let g = OnGitHub::new("reviews = 1");
    let names = ["s1", "s2", "s3"];
    let ws = g.sessions(&names);
    let mut by_hand = Vec::new();
    for w in [&ws[0], &ws[2]] {
        let head = g.f.seams().driver.session(w).unwrap().head;
        g.gh().pushed(w, &head.0).unwrap();
        let n = g
            .gh()
            .open_by("person", "main", w, &head.0, false, None)
            .unwrap();
        g.gh().review(n, "reviewer", "APPROVED", &head.0).unwrap();
        by_hand.push(n);
    }
    let (code, said) = g.submit_group(&ws, &["--no-wait"]);
    assert_eq!(code, 2, "{said}");
    let ns: Vec<u64> = names.iter().map(|n| g.pull_of(n).number).collect();
    assert_eq!([ns[0], ns[2]], [by_hand[0], by_hand[1]]);
    assert!(
        said.contains(&format!(
            "refused: #{} from {} needs 1 review",
            ns[1], ws[1]
        )),
        "{said}"
    );
    for n in [ns[0], ns[2]] {
        assert!(!said.contains(&format!("refused: #{n}")), "{said}");
    }
    assert!(
        said.contains(&format!(
            "nothing submitted: #{}, #{}, #{} are drafts at their heads",
            ns[0], ns[1], ns[2]
        )),
        "{said}"
    );
    let again = format!(
        "sluice submit main {}",
        ws.iter()
            .map(|w| format!("--worktree {w}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    assert!(
        said.contains(&format!(
            "once each meets its conditions, the same submit takes them all: {again}"
        )),
        "{said}"
    );
    let mut token = None;
    for (k, name) in names.iter().enumerate() {
        let pull = g.pull_of(name);
        assert!(pull.open && pull.draft, "{pull:?}");
        let head = g.f.seams().driver.session(&ws[k]).unwrap().head;
        assert_eq!(pull.head, head.0);
        let (t, rest) = group_mark(&pull.body).unwrap_or_else(|| panic!("{}", pull.body));
        assert_eq!(rest, format!("{}/3 head={head} -->", k + 1));
        assert_eq!(*token.get_or_insert(t.clone()), t, "one group");
        assert!(g.of(pull.number).is_empty(), "{:?}", g.subs());
    }
    g.step();
    g.step();
    for n in &ns {
        assert!(g.of(*n).is_empty(), "{:?}", g.subs());
        assert!(g.is_draft(*n));
    }
    let (code, out) = g.submit_group(&ws, &["--no-wait", "--json"]);
    assert_eq!(code, 2, "{out}");
    let doc: Value = serde_json::from_str(&out).unwrap_or_else(|_| panic!("{out}"));
    assert_eq!(
        doc,
        serde_json::json!({ "error": {
            "kind": "refused",
            "message": format!("#{} from {} needs 1 review", ns[1], ws[1]),
            "next": [again],
            "worktrees": [ws[1]],
        } })
    );
    for n in &ns {
        assert!(g.is_draft(*n) && g.of(*n).is_empty(), "{:?}", g.subs());
    }
}

/// Refused, reviewed, submitted again: an act over two worktrees refused
/// for want of reviews leaves two drafts; once a reviewer approves each at
/// its head, the same act leaves both ready and queues both, in the order
/// given and in one write.
#[test]
fn a_group_submit_refused_then_reviewed_is_taken() {
    let g = OnGitHub::new("reviews = 1");
    g.f.world.hold_runs(true);
    let names = ["r1", "r2"];
    let ws = g.sessions(&names);
    let (code, said) = g.submit_group(&ws, &["--no-wait"]);
    assert_eq!(code, 2, "{said}");
    let ns: Vec<u64> = names.iter().map(|n| g.pull_of(n).number).collect();
    for n in &ns {
        assert!(g.is_draft(*n));
        assert!(g.of(*n).is_empty(), "{:?}", g.subs());
        g.approve(*n);
    }
    let (code, said) = g.submit_group(&ws, &["--no-wait"]);
    assert_eq!(code, 0, "{said}");
    let subs: Vec<Value> = ns
        .iter()
        .map(|n| {
            assert!(!g.is_draft(*n));
            let mut of = g.of(*n);
            assert_eq!(of.len(), 1, "{:?}", g.subs());
            of.remove(0)
        })
        .collect();
    assert!(subs[0]["id"].as_u64() < subs[1]["id"].as_u64(), "{subs:?}");
    assert_eq!(subs[0]["submitted"], subs[1]["submitted"], "one write");
}

/// A pull request sluice held as a draft: one of an earlier act is blamed
/// and sluice marks it a draft. Fixed, its worktree is named again in a
/// new act whose conditions hold: it is ready for review at the head
/// pushed, and queued with the rest of the new act.
#[test]
fn a_group_submit_takes_a_pull_request_held_as_a_draft() {
    let g = OnGitHub::new("");
    let w1 = g.session("k1", &[("k1.txt", "x\n"), ("broken", "x\n")]);
    let w2 = g.session("k2", &[("k2.txt", "x\n")]);
    let (code, said) = g.submit_group(&[w1.clone(), w2], &["--no-wait"]);
    assert_eq!(code, 0, "{said}");
    let n1 = g.pull_of("k1").number;
    g.step_until("blamed and held", |g| {
        g.of(n1).first().is_some_and(|s| s["state"] == "failed") && g.is_draft(n1)
    });
    g.f.world.hold_runs(true);
    let fixed = g.commit(
        &w1,
        &[("broken", None), ("k1.txt", Some("fixed\n"))],
        "Fix k1",
    );
    let w3 = g.session("k3", &[("k3.txt", "x\n")]);
    let (code, said) = g.submit_group(&[w1, w3], &["--no-wait"]);
    assert_eq!(code, 0, "{said}");
    let p1 = g.pull_of("k1");
    assert!(!p1.draft && p1.head == fixed.0, "{p1:?}");
    let again: Vec<Value> = g
        .of(n1)
        .into_iter()
        .filter(|s| s["head"] == fixed.0.as_str())
        .collect();
    assert_eq!(again.len(), 1, "{:?}", g.subs());
    assert!(!again[0]["state"].as_str().is_some_and(|s| s == "withdrawn"));
    let n3 = g.pull_of("k3").number;
    let with = g.of(n3);
    assert_eq!(with.len(), 1, "{:?}", g.subs());
    assert_eq!(with[0]["submitted"], again[0]["submitted"], "one write");
    assert!(again[0]["id"].as_u64() < with[0]["id"].as_u64());
}

/// A fork's pull request on a branch of the same name: a fork has a pull
/// request into `main` from its own `unit/f1`, and the act names the
/// worktree on the repository's `unit/f1`. The fork's is not edited,
/// marked or queued; the act's pull request is the repository's own.
#[test]
fn a_group_submit_leaves_a_forks_pull_request_alone() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let ws = g.sessions(&["f1", "f2"]);
    let fork_head = g.branch(&[("fork.txt", Some("f\n"))]);
    let fork = g
        .gh()
        .open_by(
            "forker",
            "main",
            "unit/f1",
            &fork_head.0,
            false,
            Some("forker/r"),
        )
        .unwrap();
    let before = g.forge.pull(&repo(), fork).unwrap().unwrap();
    let (code, said) = g.submit_group(&ws, &["--no-wait"]);
    assert_eq!(code, 0, "{said}");
    assert_eq!(g.forge.pull(&repo(), fork).unwrap().unwrap(), before);
    assert!(g.forge.readied(&repo(), fork).unwrap().is_empty());
    assert!(g.of(fork).is_empty(), "{:?}", g.subs());
    let ours = g.pull_of("f1");
    assert_ne!(ours.number, fork);
    assert_eq!(ours.head_repo, "o/r");
    assert!(!ours.draft, "{ours:?}");
    assert_eq!(g.of(ours.number).len(), 1, "{:?}", g.subs());
}

/// A group whose second worktree holds more than its commits is refused
/// whole: it names the file, exits 2, and pushes and opens nothing for
/// either.
#[test]
fn a_group_submit_with_a_dirty_worktree_pushes_nothing() {
    let g = OnGitHub::new("");
    let ws = g.sessions(&["d1", "d2"]);
    g.f.world.dirty(&ws[1], &["d2.txt"]);
    let (code, said) = g.submit_group(&ws, &[]);
    assert_eq!(code, 2, "{said}");
    assert!(said.contains("d2.txt"), "{said}");
    for name in ["d1", "d2"] {
        assert_eq!(g.pushed(&format!("unit/{name}")), None);
    }
    assert!(g.forge.held().unwrap().pulls.is_empty());
}

/// A waiting act follows each pull request: it says each one's state as
/// it reaches it, then each one's outcome in the order given, and exits 0
/// once all three land.
#[test]
fn a_waiting_group_submit_follows_each_pull_request() {
    let g = OnGitHub::new("");
    let names = ["w1", "w2", "w3"];
    let ws = g.sessions(&names);
    let (code, said) = g.submit_group(&ws, &[]);
    assert_eq!(code, 0, "{said}");
    let lines: Vec<&str> = said.lines().collect();
    let mut at = Vec::new();
    for (name, w) in names.iter().zip(&ws) {
        let n = g.pull_of(name).number;
        assert!(
            g.of(n).iter().any(|s| s["state"] == "landed"),
            "{:?}",
            g.subs()
        );
        let header = format!("#{n} from {w}:");
        let i = lines
            .iter()
            .position(|l| *l == header)
            .unwrap_or_else(|| panic!("{header:?} in {said}"));
        assert!(
            lines[..i]
                .iter()
                .any(|l| l.starts_with(&format!("#{n} queued"))
                    || l.starts_with(&format!("#{n} testing"))),
            "#{n}'s states before its outcome: {said}"
        );
        at.push(i);
    }
    assert!(
        at.windows(2).all(|w| w[0] < w[1]),
        "in the order given: {said}"
    );
    for (k, i) in at.iter().enumerate() {
        let end = at.get(k + 1).copied().unwrap_or(lines.len());
        assert!(
            lines[*i..end]
                .iter()
                .any(|l| l.starts_with("passed · landed as ")),
            "{said}"
        );
    }
}

/// Two of three are ready: a group submit of three refused for want of
/// reviews leaves three drafts marked as one group; a reviewer approves
/// the first two, and a person marks all three ready for review. None is
/// submitted, and each of the two says it waits for the third; once the
/// third is approved, one round submits all three in the group's order,
/// and the first combination formed holds all three.
#[test]
fn two_of_a_group_ready_submits_none() {
    let g = OnGitHub::batched("reviews = 1");
    let names = ["t1", "t2", "t3"];
    let (code, said) = g.submit_group(&g.sessions(&names), &["--no-wait"]);
    assert_eq!(code, 2, "{said}");
    let ns: Vec<u64> = names.iter().map(|n| g.pull_of(n).number).collect();
    g.approve(ns[0]);
    g.approve(ns[1]);
    for n in &ns {
        assert!(g.is_draft(*n));
        g.gh().set_draft(*n, false).unwrap();
    }
    g.step();
    g.step();
    for n in &ns {
        assert!(g.of(*n).is_empty(), "{:?}", g.subs());
    }
    for n in &ns[..2] {
        assert_eq!(g.shows(*n), format!("waiting for its group: #{}", ns[2]));
    }
    assert_eq!(g.shows(ns[2]), "waiting: needs 1 review");
    g.approve(ns[2]);
    g.f.world.hold_runs(true);
    g.step();
    let ids: Vec<u64> = ns
        .iter()
        .map(|n| g.of(*n)[0]["id"].as_u64().unwrap())
        .collect();
    assert!(
        ids.windows(2).all(|w| w[0] < w[1]),
        "in the group's order: {ids:?}"
    );
    g.f.world.hold_runs(false);
    g.f.world.complete_all();
    g.step_until("landed", |g| {
        ns.iter()
            .all(|n| g.of(*n).first().is_some_and(|s| s["state"] == "landed"))
    });
    for n in &ns {
        assert_eq!(
            g.of(*n)[0]["tested"]["submissions"],
            serde_json::json!(ids),
            "one combination of all three"
        );
    }
}

/// A round between the opens: with two of a group of three open, no round
/// submits either; once the third is open, the next round submits all
/// three, and the first combination formed holds all three.
#[test]
fn a_round_between_a_groups_opens_submits_none() {
    let g = OnGitHub::batched("");
    let mut ns = Vec::new();
    let mark = |k: usize, n: u64, head: &Rev| {
        g.gh()
            .edit(
                &repo(),
                n,
                None,
                &format!("<!-- sluice: agent=ops -->\n<!-- sluice: group=t {k}/3 head={head} -->"),
            )
            .unwrap();
    };
    for k in 1..=2 {
        let (n, head) = g.by_hand(
            &format!("unit/o{k}"),
            &[(&format!("o{k}.txt"), Some("o\n"))],
            false,
        );
        mark(k, n, &head);
        ns.push(n);
    }
    g.step();
    g.step();
    for n in &ns {
        assert!(g.of(*n).is_empty(), "{:?}", g.subs());
        assert_eq!(
            g.shows(*n),
            "waiting for its group: the request marked 3/3, not open"
        );
    }
    let (n, head) = g.by_hand("unit/o3", &[("o3.txt", Some("o\n"))], false);
    mark(3, n, &head);
    ns.push(n);
    g.f.world.hold_runs(true);
    g.step();
    let ids: Vec<u64> = ns
        .iter()
        .map(|n| g.of(*n)[0]["id"].as_u64().unwrap())
        .collect();
    g.f.world.hold_runs(false);
    g.f.world.complete_all();
    g.step_until("landed", |g| {
        ns.iter()
            .all(|n| g.of(*n).first().is_some_and(|s| s["state"] == "landed"))
    });
    for n in &ns {
        assert_eq!(g.of(*n)[0]["tested"]["submissions"], serde_json::json!(ids));
    }
}

/// One of a group closed before it is taken: a group submit refused for
/// want of reviews leaves three drafts; the second is closed, and the
/// others approved and marked ready for review by a person. They are not
/// submitted and say they wait for it, until a submit names one of them
/// again, alone.
#[test]
fn one_of_a_group_closed_holds_the_rest() {
    let g = OnGitHub::new("reviews = 1");
    g.f.world.hold_runs(true);
    let names = ["c1", "c2", "c3"];
    let ws = g.sessions(&names);
    let (code, said) = g.submit_group(&ws, &["--no-wait"]);
    assert_eq!(code, 2, "{said}");
    let ns: Vec<u64> = names.iter().map(|n| g.pull_of(n).number).collect();
    g.gh().close(ns[1]).unwrap();
    for n in [ns[0], ns[2]] {
        g.approve(n);
        g.gh().set_draft(n, false).unwrap();
    }
    g.step();
    g.step();
    for n in [ns[0], ns[2]] {
        assert!(g.of(n).is_empty(), "{:?}", g.subs());
        assert_eq!(
            g.shows(n),
            "waiting for its group: the request marked 2/3, not open"
        );
    }
    g.submitted(&ws[0], &["--no-wait"]);
    g.approve(ns[0]);
    g.step();
    assert_eq!(g.of(ns[0]).len(), 1, "{:?}", g.subs());
    assert!(g.of(ns[2]).is_empty(), "{:?}", g.subs());
}

/// Each its own once taken: a commit no submit made, pushed to the second
/// of a group already submitted, withdraws only the second's submission,
/// and sluice marks the second a draft; the first and third are tested
/// without it and land, and no round submits the second again. Its
/// worktree's `sluice submit main` marks it ready at its new head, one of
/// no group, and it is submitted again alone and lands (120, 125).
#[test]
fn one_of_a_group_on_a_new_head_is_withdrawn_alone() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let names = ["h1", "h2", "h3"];
    let ws = g.sessions(&names);
    let (code, said) = g.submit_group(&ws, &["--no-wait"]);
    assert_eq!(code, 0, "{said}");
    let ns: Vec<u64> = names.iter().map(|n| g.pull_of(n).number).collect();
    g.step();
    let first: Vec<u64> = ns
        .iter()
        .map(|n| g.of(*n)[0]["id"].as_u64().expect("submitted together"))
        .collect();
    let moved = g.push_from(&ws[1], &[("h2-more.txt", "more\n")]);
    g.step();
    assert_eq!(g.of(ns[1])[0]["state"], "withdrawn", "{:?}", g.subs());
    assert!(g.is_draft(ns[1]));
    for n in [ns[0], ns[2]] {
        assert_ne!(g.of(n)[0]["state"], "withdrawn", "{:?}", g.subs());
    }
    g.f.world.hold_runs(false);
    g.f.world.complete_all();
    g.step_until("the first and third landed", |g| {
        [ns[0], ns[2]]
            .iter()
            .all(|n| g.of(*n).iter().any(|s| s["state"] == "landed"))
    });
    for n in [ns[0], ns[2]] {
        let landed = g
            .of(n)
            .into_iter()
            .find(|s| s["state"] == "landed")
            .unwrap();
        let tested = landed["tested"]["submissions"].as_array().unwrap().clone();
        assert!(!tested.contains(&serde_json::json!(first[1])), "{landed}");
    }
    let held = g.of_after(ns[1], 2);
    assert_eq!(held.len(), 1, "never submitted again by a round: {held:?}");
    assert!(g.is_draft(ns[1]));
    g.submitted(&ws[1], &["--no-wait"]);
    let pull = g.pull_of("h2");
    assert!(!pull.draft && pull.head == moved.0, "{pull:?}");
    assert_eq!(group_mark(&pull.body), None, "{}", pull.body);
    g.step_until("the second submitted again and landed", |g| {
        g.of(ns[1])
            .iter()
            .any(|s| s["head"] == moved.0.as_str() && s["state"] == "landed")
    });
    let again: Vec<Value> = g
        .of(ns[1])
        .into_iter()
        .filter(|s| s["head"] == moved.0.as_str())
        .collect();
    assert_eq!(again.len(), 1, "{:?}", g.subs());
    assert_eq!(
        again[0]["tested"]["submissions"],
        serde_json::json!([again[0]["id"]]),
        "alone"
    );
}
