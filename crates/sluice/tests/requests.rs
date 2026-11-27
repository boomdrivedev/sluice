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
use std::io::Write as _;
use std::rc::Rc;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use serde_json::Value;
use sluice::profiles::github::fake::FileForge;
use sluice::profiles::github::forge::{
    Check, Comment, Forge, Listed, Posted, Posting, Pull, Request, Review, Standing, Status,
};
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

/// Host `name` as its peers name it, signing as the signer of its name:
/// `a` and `b` are the signers `main`'s policy names, any other none it
/// names (C.6; 137; design D28).
fn peer(name: &str) -> sluice_core::Peer {
    sluice_core::Peer {
        name: name.into(),
        repository: "this-repository".into(),
        signer: Some(name.into()),
        ..sluice_core::Peer::default()
    }
}

/// One host over the test's repository and GitHub: its peer, the refs it
/// fetched and pushed, and its pull request driver, with the state it
/// keeps of GitHub.
struct Host {
    peer: sluice_core::Peer,
    refs: Rc<FakeRefs>,
    pulls: Pulls,
    dir: Rc<Scratch>,
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

/// GitHub as a host's pull request driver reaches it: the file standing
/// for it, each call stamped at the fakes' time as it is made, so what the
/// driver changes on GitHub and what it records are on one clock.
struct Stamped {
    world: World,
    forge: Rc<FileForge>,
}

impl Stamped {
    fn now(&self) -> &FileForge {
        self.forge.stamp_at(self.world.now());
        &self.forge
    }
}

impl Forge for Stamped {
    fn login(&self, repo: &GitHubRepo) -> Result<String> {
        self.now().login(repo)
    }
    fn open_pulls(&self, repo: &GitHubRepo) -> Result<Vec<Pull>> {
        self.now().open_pulls(repo)
    }
    fn pulls(&self, repo: &GitHubRepo, base: &str) -> Result<Vec<Pull>> {
        self.now().pulls(repo, base)
    }
    fn listed(&self, repo: &GitHubRepo) -> Result<Listed> {
        self.now().listed(repo)
    }
    fn requests(&self, repo: &GitHubRepo, base: &str) -> Result<Vec<Request>> {
        self.now().requests(repo, base)
    }
    fn pull(&self, repo: &GitHubRepo, number: u64) -> Result<Option<Pull>> {
        self.now().pull(repo, number)
    }
    fn reviews(&self, repo: &GitHubRepo, number: u64) -> Result<Vec<Review>> {
        self.now().reviews(repo, number)
    }
    fn checks(&self, repo: &GitHubRepo, rev: &str) -> Result<Vec<Check>> {
        self.now().checks(repo, rev)
    }
    fn readied(&self, repo: &GitHubRepo, number: u64) -> Result<Vec<DateTime<Utc>>> {
        self.now().readied(repo, number)
    }
    fn posted(&self, repo: &GitHubRepo, rev: &str) -> Result<Option<Posted>> {
        self.now().posted(repo, rev)
    }
    fn posted_all(&self, repo: &GitHubRepo, rev: &str) -> Result<Vec<Posted>> {
        self.now().posted_all(repo, rev)
    }
    fn post(
        &self,
        repo: &GitHubRepo,
        rev: &str,
        existing: Option<u64>,
        posting: &Posting,
    ) -> Result<u64> {
        self.now().post(repo, rev, existing, posting)
    }
    fn post_named(
        &self,
        repo: &GitHubRepo,
        rev: &str,
        name: &str,
        existing: Option<u64>,
        posting: &Posting,
    ) -> Result<u64> {
        self.now().post_named(repo, rev, name, existing, posting)
    }
    fn comments(&self, repo: &GitHubRepo, number: u64) -> Result<Vec<String>> {
        self.now().comments(repo, number)
    }
    fn comment_list(&self, repo: &GitHubRepo, number: u64) -> Result<Vec<Comment>> {
        self.now().comment_list(repo, number)
    }
    fn comment(&self, repo: &GitHubRepo, number: u64, body: &str) -> Result<u64> {
        self.now().comment(repo, number, body)
    }
    fn edit_comment(&self, repo: &GitHubRepo, id: u64, body: &str) -> Result<()> {
        self.now().edit_comment(repo, id, body)
    }
    fn open(
        &self,
        repo: &GitHubRepo,
        base: &str,
        head_ref: &str,
        title: &str,
        body: &str,
        draft: bool,
    ) -> Result<Pull> {
        self.now().open(repo, base, head_ref, title, body, draft)
    }
    fn edit(&self, repo: &GitHubRepo, number: u64, title: Option<&str>, body: &str) -> Result<()> {
        self.now().edit(repo, number, title, body)
    }
    fn set_draft(&self, repo: &GitHubRepo, number: u64, draft: bool) -> Result<()> {
        Forge::set_draft(self.now(), repo, number, draft)
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
        assert!(self.0.get() < 400, "waited too long");
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
    /// Each host that has run a command, by name.
    hosts: RefCell<BTreeMap<String, Rc<Host>>>,
    /// The host the next command runs on.
    here: RefCell<String>,
    /// The hosts' `[forge] discovery`.
    discovery: Cell<Duration>,
    /// The `[peers]` lease and liveness each host is set up with, where a
    /// test gives one.
    leases: RefCell<BTreeMap<String, (Duration, Duration)>>,
    /// The signer each host signs as, where a test gives one.
    signers: RefCell<BTreeMap<String, String>>,
    /// Each host's state on disk, which outlives its processes.
    dirs: RefCell<BTreeMap<String, Rc<Scratch>>>,
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
        let g = OnGitHub::unled(conditions, lines);
        // This host keeps main: a round of it takes the lead (72, 137).
        g.f.sluice().as_peer(peer("a")).step("main").unwrap();
        g
    }

    /// [`OnGitHub::with_lines`], main's records naming no host yet as
    /// keeping it.
    fn unled(conditions: &str, lines: &str) -> OnGitHub {
        OnGitHub::built(conditions, lines, "1h", "")
    }

    /// `main` whose runs its time limit `timeout` ends, kept by this host.
    fn timed(timeout: &str) -> OnGitHub {
        let g = OnGitHub::built("", "", timeout, "");
        g.f.sluice().as_peer(peer("a")).step("main").unwrap();
        g
    }

    /// `main`, kept by this host, and `bolt/x` beside it, a branch on
    /// GitHub sluice lands on, made from `main` and taking no pull
    /// requests.
    fn with_bolt() -> OnGitHub {
        let g = OnGitHub::built(
            "",
            "",
            "1h",
            "\n[branches.\"bolt/*\"]\nrequired = [\"#gate\"]\ntrust = [\"a\", \"b\"]\n\
             remote = \"origin\"\n",
        );
        g.f.world.set_branch("bolt/x", &g.f.world.tip("main"));
        g.f.world.on_remote("bolt/x");
        g.f.sluice().as_peer(peer("a")).step("main").unwrap();
        g
    }

    /// `main` under the policy whose `main` table holds `lines` and its
    /// time limit `timeout`, whose `pull_requests` table holds
    /// `conditions`, and `extra` after them.
    fn built(conditions: &str, lines: &str, timeout: &str, extra: &str) -> OnGitHub {
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
                     remote = \"origin\"\ntimeout = \"{timeout}\"\n{lines}\n\
                     [branches.main.pull_requests]\n{conditions}\n{extra}"
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
        forge.stamp_at(f.world.now());
        OnGitHub {
            f,
            forge,
            hosts: RefCell::default(),
            here: RefCell::new("a".into()),
            discovery: Cell::new(DISCOVERY),
            leases: RefCell::default(),
            signers: RefCell::default(),
            dirs: RefCell::default(),
            _dir: dir,
        }
    }

    /// The host the next command runs on, made at its first command.
    fn host(&self) -> Rc<Host> {
        let name = self.here.borrow().clone();
        if let Some(h) = self.hosts.borrow().get(&name) {
            return h.clone();
        }
        let mut peer = peer(&name);
        peer.signer = Some(self.signer(&name));
        if let Some((lease, alive)) = self.leases.borrow().get(&name) {
            (peer.lease, peer.alive) = (*lease, *alive);
        }
        let dir = self
            .dirs
            .borrow_mut()
            .entry(name.clone())
            .or_insert_with(|| Rc::new(Scratch::new(&format!("requests-{name}"))))
            .clone();
        let refs = Rc::new(FakeRefs {
            world: self.f.world.clone(),
            forge: self.forge.clone(),
            own: RefCell::default(),
            pushed: RefCell::default(),
            headless: RefCell::default(),
        });
        let pulls = Pulls::new(
            refs.clone(),
            dir.path(),
            Some(Rc::new(Stamped {
                world: self.f.world.clone(),
                forge: self.forge.clone(),
            })),
            Kept::new(&dir.path().join("state")),
            self.discovery.get(),
            peer.clone(),
        );
        let host = Rc::new(Host {
            peer,
            refs,
            pulls,
            dir,
        });
        self.hosts.borrow_mut().insert(name, host.clone());
        host
    }

    /// This host's process starts afresh: what it held in memory is gone,
    /// and what it keeps on disk, the host's state, stands.
    fn restart(&self) {
        let name = self.here.borrow().clone();
        self.hosts.borrow_mut().remove(&name);
    }

    /// The next commands run on host `name`, signing as its signer.
    fn be(&self, name: &str) {
        *self.here.borrow_mut() = name.into();
        self.f.world.be_host(name);
        self.f.world.signs_as(&self.signer(name));
    }

    /// The signer host `name` signs as: its own name, unless set otherwise.
    fn signer(&self, name: &str) -> String {
        self.signers
            .borrow()
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.into())
    }

    /// Host `name` signs as `signer`, set before its first command.
    fn signs(&self, name: &str, signer: &str) {
        self.signers.borrow_mut().insert(name.into(), signer.into());
    }

    /// Host `name` keeps main: a round of it takes the lead (72, 137). The
    /// next commands run on it.
    fn leads(&self, name: &str) {
        self.be(name);
        let peer = self.host().peer.clone();
        self.f.sluice().as_peer(peer).step("main").unwrap();
        assert_eq!(self.lead().as_deref(), Some(name));
    }

    /// The pull requests into `main` this host follows, by number, as its
    /// forge state keeps them (137); `null` where it keeps none.
    fn follows(&self) -> Value {
        let path = self
            .host()
            .dir
            .path()
            .join("state/forge/follows/o/r/main.json");
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .map_or(Value::Null, |v| v["requests"].clone())
    }

    /// This host's next follow of pull request `n` into `main` reads it
    /// from GitHub, as if no process of the host had read it within a
    /// quarter of `discovery` (D4).
    fn follow_due(&self, n: u64) {
        let path = self
            .host()
            .dir
            .path()
            .join("state/forge/follows/o/r/main.json");
        let mut v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        v["requests"][n.to_string()]["read_at"] = Value::Null;
        std::fs::write(&path, v.to_string()).unwrap();
    }

    /// sluice's check on `head`, pull request `n`'s, as another host's
    /// sluice left it, on GitHub alone; this host's next follow of the
    /// request reads it.
    fn posted_elsewhere(&self, n: u64, head: &str, status: Status, title: &str, summary: &str) {
        let posting = Posting {
            status,
            title: title.into(),
            summary: summary.into(),
            text: None,
        };
        self.posting_elsewhere(n, head, &posting);
    }

    fn posting_elsewhere(&self, n: u64, head: &str, posting: &Posting) {
        let run = self.forge.posted(&repo(), head).unwrap().map(|r| r.id);
        self.gh()
            .post_named(&repo(), head, "sluice", run, posting)
            .unwrap();
        self.follow_due(n);
    }

    /// sluice's check on `head`, pull request `n`'s, as host B posts it
    /// while B holds the request as `submission`, next on main.
    fn held_there(&self, n: u64, head: &str, submission: u64) {
        self.posting_elsewhere(n, head, &queued_on_b(submission));
    }

    /// The hosts' `[forge] discovery` is `every`, set before any host runs a
    /// command.
    fn discovery(&self, every: Duration) {
        self.discovery.set(every);
        self.hosts.borrow_mut().clear();
    }

    /// The host main's records name as keeping it (72, 137; design D25);
    /// `None` where none does.
    fn lead(&self) -> Option<String> {
        let snap = self.f.sluice().seams().state.read("main").unwrap();
        sluice_core::line::Line::load("main", snap)
            .unwrap()
            .doc
            .lead
    }

    /// How many times GitHub was asked `what`, as [`FileForge::calls`]
    /// names it.
    fn calls(&self, what: &str) -> usize {
        self.forge.calls(what)
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
            .advance(chrono::Duration::from_std(self.discovery.get()).unwrap());
    }

    /// A second passes, so what is done next is later, to the second
    /// GitHub and the records keep, than what was done before.
    fn a_second_passes(&self) {
        self.f.world.advance(chrono::Duration::seconds(2));
    }

    /// `sluice <args>` on this host: its exit and what it said.
    fn sluice(&self, args: &[&str]) -> (i32, String) {
        self.sluice_pausing(args, &Rounds(Cell::new(0)))
    }

    /// What GitHub does by itself: an open pull request whose head `main`
    /// holds now reads merged.
    fn github_merges(&self) {
        let main = self.f.world.tip("main");
        for p in self.forge.held().unwrap().pulls {
            if p.open && !p.head.is_empty() && self.f.world.is_ancestor(&Rev::new(p.head), &main) {
                self.gh().merge(p.number).unwrap();
            }
        }
    }

    /// [`OnGitHub::sluice`], each of its pauses `pause`.
    fn sluice_pausing(&self, args: &[&str], pause: &dyn Pause) -> (i32, String) {
        self.github_merges();
        self.forge.stamp_at(self.f.world.now());
        let cli =
            Cli::try_parse_from(std::iter::once("sluice").chain(args.iter().copied())).unwrap();
        let here = self.host();
        let mut host = support::FakeHost::new(self.f.world.clone(), 0);
        host.peer = here.peer.clone();
        let seams = self.f.seams();
        let drivers = Drivers {
            sessions: seams.driver,
            pulls: &here.pulls,
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
            &here.pulls,
            &self.f.clock,
            pause,
            &mut out,
        )
        // An error ends the command with sluice's own exit, its words
        // last, as the binary ends it.
        .unwrap_or_else(|e| {
            writeln!(out, "sluice: {e:#}").unwrap();
            sluice::INTERNAL
        });
        (code, String::from_utf8(out).unwrap())
    }

    /// One round of `main`, its pull requests taken in first, the host's
    /// interval past since it last asked GitHub, as the host process runs
    /// a step: each cause its intake met, which a step says as a warning
    /// (128; design D26).
    fn round(&self) -> Vec<String> {
        self.f
            .world
            .advance(chrono::Duration::from_std(self.discovery.get()).unwrap());
        self.github_merges();
        self.forge.stamp_at(self.f.world.now());
        let here = self.host();
        let seams = self.f.seams();
        let drivers = Drivers {
            sessions: seams.driver,
            pulls: &here.pulls,
        };
        let s = sluice_core::Sluice::new(
            sluice_core::Seams {
                driver: &drivers,
                ..seams
            },
            &self.f.clock,
        )
        .as_peer(here.peer.clone());
        let stepped = sluice::host_process::pass::step(&s, &here.pulls, &self.f.clock, "main")
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
    fn pull_from(&self, head_ref: &str) -> Pull {
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
        self.host()
            .refs
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
        let host = self.host();
        let mut headless = host.refs.headless.borrow_mut();
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
    fn pull_of(&self, name: &str) -> Pull {
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

// ---------------------------------------------------------------------------
// request-intake-host: one host takes a branch's requests in (137)
// ---------------------------------------------------------------------------

/// sluice's check on a pull request's head as host B posts it while B
/// holds the request as `submission`, next on main.
fn queued_on_b(submission: u64) -> Posting {
    let block = serde_json::json!({
        "host": "b", "taken_in": true, "submission": submission, "state": "queued",
        "ahead": [], "waits_for": [],
    });
    Posting {
        status: Status::Queued,
        title: "queued: next on main".into(),
        summary: format!(
            "Submission {submission} on b.\n\n\
             In main's queue; it lands once its combination passes.\n\n\
             <!-- sluice:queue {block} -->"
        ),
        text: None,
    }
}

/// The last document a `--json` command printed.
fn last_document(out: &str) -> Value {
    let last = out.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("");
    serde_json::from_str(last).unwrap_or_else(|e| panic!("{e}: {out}"))
}

impl OnGitHub {
    /// Host B keeps main, and host A opened a pull request from its session
    /// `name` by a submit that does not wait: the session's worktree, and
    /// the pull request. The next commands run on A.
    fn opened_here_held_by_b(&self, name: &str) -> (String, Pull) {
        self.leads("b");
        self.be("a");
        let w = self.session(name, &[(&format!("{name}.txt"), "x\n")]);
        self.submitted(&w, &["--no-wait"]);
        (w, self.pull_from(&format!("unit/{name}")))
    }

    /// Rounds of main on host B until pull request `n`'s submission there
    /// lands; the next commands run on A.
    fn b_lands(&self, n: u64) {
        self.be("b");
        self.step_until("landed by B", |g| {
            g.of(n).first().is_some_and(|s| s["state"] == "landed")
        });
        self.be("a");
    }
}

/// request-intake-host, "Records on the remote": another host keeps main,
/// its lease not passed by this host's observation, and a `sluice wait` on
/// this host asks GitHub nothing about main's open pull requests and runs
/// no round of main.
#[test]
fn records_on_the_remote_only_the_lead_asks() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let (n, _) = g.by_hand("unit/signed", &[("s.txt", Some("s\n"))], false);
    g.step();
    let id = g.of(n)[0]["id"].as_u64().unwrap().to_string();
    g.be("b");
    let lists = g.calls("list");
    let (code, out) = g.sluice(&["wait", &id, "--timeout", "30s"]);
    assert_eq!(code, sluice::WAITING, "{out}");
    assert_eq!(g.calls("list"), lists, "{out}");
    assert_eq!(g.of(n)[0]["state"], "testing", "no round of main");
}

/// request-intake-host, "Two hosts beside the keeper" (120, 137; design
/// D24): host A keeps main and takes `#1` in; hosts B and C each pass main
/// ten times while its combination is tested, and it lands as the one
/// submission A took in, main's records holding no other of it.
#[test]
fn a_request_keeps_one_number_while_other_hosts_pass() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let (n, head) = g.by_hand("unit/one", &[("o.txt", Some("o\n"))], false);
    g.step();
    let id = g.of(n)[0]["id"].clone();
    for host in ["b", "c"] {
        g.be(host);
        let (code, said) = g.sluice(&["run", "--rounds", "10"]);
        assert_eq!(code, 0, "{said}");
    }
    g.be("a");
    g.f.world.hold_runs(false);
    g.f.world.complete_all();
    g.step_until("landed", |g| g.of(n)[0]["state"] == "landed");
    let subs = g.of(n);
    assert_eq!(subs.len(), 1, "one submission of it: {subs:?}");
    assert_eq!(
        (&subs[0]["id"], &subs[0]["head"]),
        (&id, &head.0.as_str().into())
    );
    assert_eq!(g.lead().as_deref(), Some("a"));
}

/// request-intake-host, "Another host keeps it": host B keeps main, and
/// host A's step takes nothing in and asks GitHub nothing of main's open
/// pull requests (137; design D21).
#[test]
fn another_host_keeps_main_and_this_hosts_step_takes_nothing_in() {
    let g = OnGitHub::unled("", "");
    g.leads("b");
    g.be("a");
    let (n, _) = g.by_hand("unit/taken", &[("t.txt", Some("t\n"))], false);
    let lists = g.calls("list");
    g.step();
    assert!(g.of(n).is_empty());
    assert_eq!(
        g.calls("list"),
        lists,
        "A asked GitHub nothing of main's list"
    );
    assert_eq!(g.lead().as_deref(), Some("b"));
}

/// request-intake-host, "Read at its pace": a wait on host A follows its
/// pull request, which host B takes in, for a minute with `discovery` at
/// sixty seconds; it reads the request and its check from GitHub no more
/// than four times each, finds its request by the list once, and runs no
/// intake and no round.
#[test]
fn a_wait_across_hosts_reads_at_its_pace() {
    let g = OnGitHub::unled("reviews = 1", "");
    g.discovery(Duration::from_secs(60));
    let (w, pull) = g.opened_here_held_by_b("paced");
    let n = pull.number;
    let (before, checks, lists) = (
        g.calls(&format!("pull/{n}")),
        g.calls(&format!("posted/{}", pull.head)),
        g.calls("list"),
    );
    let (code, out) = g.sluice(&["wait", "main", "--worktree", &w, "--timeout", "57s"]);
    assert_eq!(code, sluice::WAITING, "{out}");
    let reads = g.calls(&format!("pull/{n}")) - before;
    assert!((1..=4).contains(&reads), "{reads} reads: {out}");
    assert!(
        g.calls(&format!("posted/{}", pull.head)) - checks <= 4,
        "{out}"
    );
    assert_eq!(
        g.calls("list"),
        lists + 1,
        "its finding of its request alone"
    );
    assert!(g.of(n).is_empty(), "no intake, no round");
}

/// request-intake-host, "Submitted here, taken in there": host B keeps
/// main, and a submit on host A that does not wait opens a pull request;
/// A's follows name it at its head, and A's next round asks GitHub nothing
/// of main's open pull requests and takes nothing in.
#[test]
fn submitted_here_taken_in_there() {
    let g = OnGitHub::unled("", "");
    let (_w, pull) = g.opened_here_held_by_b("there");
    let lists = g.calls("list");
    let follows = g.follows();
    let row = &follows[pull.number.to_string()];
    assert_eq!(row["head"], pull.head.as_str(), "{follows}");
    assert_eq!(
        (&row["from"], &row["open"], &row["merged"]),
        (&"unit/there".into(), &true.into(), &false.into()),
        "{follows}"
    );
    g.step();
    assert_eq!(g.calls("list"), lists, "A's round asks nothing of the list");
    assert!(g.of(pull.number).is_empty(), "A took nothing in");
    assert!(g.follows()[pull.number.to_string()].is_object());
}

/// request-intake-host, "Merged" and "Landed on another host": host A
/// follows the pull request its submit opened, host B takes it in and
/// lands it, and A's wait on its worktree reads it merged: it says it
/// landed, exits 0, and A's follows name it no longer.
#[test]
fn landed_on_another_host() {
    let g = OnGitHub::unled("", "");
    let (w, pull) = g.opened_here_held_by_b("landed");
    let n = pull.number;
    assert!(g.follows()[n.to_string()].is_object(), "{}", g.follows());
    g.b_lands(n);
    g.interval_passes();
    let (code, out) = g.sluice(&["wait", "main", "--worktree", &w, "--timeout", "1m"]);
    assert_eq!(code, sluice::LANDED, "{out}");
    assert!(out.contains("passed · landed"), "{out}");
    assert!(g.follows().get(n.to_string()).is_none(), "{}", g.follows());
    assert_eq!(g.of(n).len(), 1, "B's submission alone");
}

/// request-intake-host, "Blamed on another host": B's round blames a pull
/// request a person opened from A's worktree's branch, which no submit
/// holds, so sluice's check on its head concludes as a failure naming the
/// blamed check, and A's wait exits as a failed submission does, naming
/// what the check names.
#[test]
fn blamed_on_another_host() {
    let g = OnGitHub::unled("", "");
    g.leads("b");
    g.be("a");
    let w = g.session("blamed", &[("broken", "b\n")]);
    let head = g.f.seams().driver.session(&w).unwrap().head;
    g.gh().pushed(&w, &head.0).unwrap();
    let n = g
        .gh()
        .open_by("person", "main", &w, &head.0, false, None)
        .unwrap();
    g.be("b");
    g.step_until("blamed by B", |g| {
        g.of(n).first().is_some_and(|s| s["state"] == "failed")
            && g.posted(&head).is_some_and(|p| p.1.starts_with("failed: "))
    });
    g.be("a");
    g.interval_passes();
    let (code, out) = g.sluice(&["wait", "main", "--worktree", &w, "--timeout", "1m"]);
    assert_eq!(code, sluice::FAILED, "{out}");
    let said = g.posted(&head).unwrap().1;
    assert!(out.contains(&said), "{out}");
}

/// request-intake-host, "GitHub keeps a run completed": GitHub shows
/// sluice's run on the head of host A's pull request completed as a
/// failure from an earlier blame, while its block now reads it taken in
/// and under test on B. A's wait reads it under test and does not end
/// (D6), and A takes nothing in.
#[test]
fn a_run_github_keeps_completed_reads_by_its_block() {
    let g = OnGitHub::unled("", "");
    let (w, pull) = g.opened_here_held_by_b("kept");
    let n = pull.number;
    let block = serde_json::json!({
        "host": "b", "taken_in": true, "submission": 1406, "state": "testing",
        "ahead": [], "waits_for": [],
    });
    let testing = format!("testing main + #{n}");
    g.posted_elsewhere(
        n,
        &pull.head,
        Status::Failure,
        &testing,
        &format!(
            "Submission 1406 on b.\n\nUnder test in main + #{n}.\n\n\
             <!-- sluice:queue {block} -->"
        ),
    );
    let (code, out) = g.sluice(&["wait", "main", "--worktree", &w, "--timeout", "3s"]);
    assert_eq!(code, sluice::WAITING, "{out}");
    assert!(out.contains(&testing), "{out}");
    assert!(g.follows()[n.to_string()].is_object(), "{}", g.follows());
    assert!(g.of(n).is_empty(), "A took nothing in");
}

/// request-intake-host, "An older sluice's check": host B's older sluice
/// posted the check on the head of host A's pull request with no block,
/// titled `failed: app:test`, and A's wait reads it sent back by its
/// words, as it did before checks carried their fields (D6).
#[test]
fn an_older_sluices_check_reads_by_its_words() {
    let g = OnGitHub::unled("", "");
    let (w, pull) = g.opened_here_held_by_b("older");
    let n = pull.number;
    g.posted_elsewhere(
        n,
        &pull.head,
        Status::Failure,
        "failed: app:test",
        "app:test failed. Push a fix and submit again.",
    );
    let (code, out) = g.sluice(&["wait", "main", "--worktree", &w, "--timeout", "1m"]);
    assert_eq!(code, sluice::FAILED, "{out}");
    assert!(out.contains("failed: app:test"), "{out}");
}

/// submission-wait, "Out of time, held on another host": host B holds host
/// A's pull request as submission 1406, and A's `sluice wait main
/// --timeout 2s --json` for its worktree exits 5, naming the request, the
/// host that takes it in, the queue B's check carries, and the wait that
/// follows the request by its number, never B's 1406 (record 0130).
#[test]
fn out_of_time_queued_on_the_other_host() {
    let g = OnGitHub::unled("", "");
    let (w, pull) = g.opened_here_held_by_b("timeout");
    let n = pull.number;
    g.held_there(n, &pull.head, 1406);
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
    let doc = last_document(&out);
    assert_eq!(doc["pull_request"], pull.number, "{doc}");
    assert_eq!(
        doc["taken_in_by"],
        serde_json::json!({ "host": "b" }),
        "{doc}"
    );
    assert_eq!(doc["queue"]["submission"], 1406, "{doc}");
    assert_eq!(doc["queue"]["host"], "b", "{doc}");
    assert_eq!(
        doc["wait"],
        format!("sluice wait main --pull-request {}", pull.number).as_str(),
        "{doc}"
    );
    assert!(doc.get("id").is_none(), "{doc}");
    assert!(g.of(pull.number).is_empty(), "A took nothing in");
}

/// submission-wait, "Held on another host, out of time" and "Landed on
/// another host": host B holds host A's pull request as submission 1406,
/// and `sluice wait main --pull-request <n> --timeout 2s --json`, from no
/// worktree, exits 5, naming the request, the host that takes it in, B's
/// queue and the wait itself, with no `id`. Once B lands it and GitHub
/// shows it merged, the same wait prints it landed and exits 0 (record
/// 0130 D2).
#[test]
fn a_wait_by_pull_request_from_the_main_checkout_follows_it_on_another_host() {
    let g = OnGitHub::unled("", "");
    let (_w, pull) = g.opened_here_held_by_b("bynumber");
    let n = pull.number;
    g.held_there(n, &pull.head, 1406);
    let number = n.to_string();
    let wait = ["wait", "main", "--pull-request", &number, "--json"];
    let (code, out) = g.sluice(&[&wait[..], &["--timeout", "2s"]].concat());
    assert_eq!(code, sluice::WAITING, "{out}");
    let doc = last_document(&out);
    assert_eq!(doc["pull_request"], n, "{doc}");
    assert_eq!(
        doc["taken_in_by"],
        serde_json::json!({ "host": "b" }),
        "{doc}"
    );
    assert_eq!(doc["queue"]["submission"], 1406, "{doc}");
    assert_eq!(doc["queue"]["host"], "b", "{doc}");
    assert_eq!(
        doc["wait"],
        format!("sluice wait main --pull-request {n}").as_str(),
        "{doc}"
    );
    assert!(doc.get("id").is_none(), "{doc}");
    g.gh().merge(n).unwrap();
    g.follow_due(n);
    let (code, out) = g.sluice(&[&wait[..], &["--timeout", "1m"]].concat());
    assert_eq!(code, sluice::LANDED, "{out}");
    let doc = last_document(&out);
    assert_eq!(doc["pull_request"], n, "{doc}");
    assert_eq!(doc["state"], "landed", "{doc}");
    assert!(g.of(n).is_empty(), "A took nothing in");
}

/// pull-request-submit, "No wait, another host takes it in": host B keeps
/// main and takes its pull requests in, and `sluice submit main --no-wait
/// --json` on host A answers with the pull request, its state, the host
/// that takes it in and the wait that follows it, with no `id`. That wait
/// follows the request to the landing B gives it (record 0130).
#[test]
fn no_wait_on_a_host_that_does_not_take_it_in_names_its_wait() {
    let g = OnGitHub::unled("", "");
    g.leads("b");
    g.be("a");
    let w = g.session("two-apps", &[("t.txt", "t\n")]);
    let doc = last_document(&g.submitted(&w, &["--no-wait", "--json"]));
    let n = g.pull_from("unit/two-apps").number;
    assert_eq!(doc["pull_request"], n, "{doc}");
    assert_eq!(doc["branch"], "main", "{doc}");
    assert!(doc["state"].is_string(), "{doc}");
    assert_eq!(
        doc["taken_in_by"],
        serde_json::json!({ "host": "b" }),
        "{doc}"
    );
    let wait = format!("sluice wait main --pull-request {n}");
    assert_eq!(doc["wait"], wait.as_str(), "{doc}");
    assert!(doc.get("id").is_none(), "{doc}");
    g.b_lands(n);
    g.interval_passes();
    let args: Vec<&str> = wait.split(' ').skip(1).collect();
    let (code, out) = g.sluice(&[&args[..], &["--timeout", "1m", "--json"]].concat());
    assert_eq!(code, sluice::LANDED, "{out}");
    let doc = last_document(&out);
    assert_eq!(doc["pull_request"], n, "{doc}");
    assert_eq!(doc["state"], "landed", "{doc}");
    assert_eq!(g.of(n).len(), 1, "B's submission alone");
}

/// submission-wait, "The status a submit reads": `sluice submit main
/// --status --json` in the worktree gives the request's head, its draft
/// mark, the host that takes it in and the check's queue beside the
/// request, its branch and its state.
#[test]
fn the_status_a_submit_reads_across_hosts() {
    let g = OnGitHub::unled("", "");
    let (w, pull) = g.opened_here_held_by_b("status");
    let n = pull.number;
    g.held_there(n, &pull.head, 1406);
    let doc = last_document(&g.submitted(&w, &["--status", "--json"]));
    assert_eq!(
        (&doc["pull_request"], &doc["branch"], &doc["state"]),
        (
            &pull.number.into(),
            &"main".into(),
            &"queued: next on main".into()
        ),
        "{doc}"
    );
    assert_eq!(
        (&doc["head"], &doc["draft"]),
        (&pull.head.as_str().into(), &false.into()),
        "{doc}"
    );
    assert_eq!(
        doc["taken_in_by"],
        serde_json::json!({ "host": "b" }),
        "{doc}"
    );
    assert_eq!(
        doc["queue"],
        serde_json::json!({
            "host": "b", "taken_in": true, "submission": 1406, "state": "queued",
            "ahead": [], "waits_for": [],
        }),
        "{doc}"
    );
    assert_eq!(
        doc["wait"],
        format!("sluice wait main --pull-request {}", pull.number).as_str(),
        "{doc}"
    );
}

/// Each pause of a wait runs `then` with the count of pauses so far.
struct Then<F: Fn(usize)>(Cell<usize>, F);

impl<F: Fn(usize)> Pause for Then<F> {
    fn pause(&self) -> bool {
        self.0.set(self.0.get() + 1);
        assert!(self.0.get() < 400, "waited too long");
        (self.1)(self.0.get());
        true
    }
}

/// submission-wait, "Its words": `sluice wait main` on host A follows its
/// pull request while host B holds it as submission 1406, with `discovery`
/// at ten seconds, so each pass reads it again. It says once that the
/// request is submission 1406 on `b`, taken in by `b`, and not again over
/// the reads that find the same; once B holds it as 1407, it says so,
/// once. The check's own first line, which says the same, is not said
/// beside it.
#[test]
fn a_waits_words_across_hosts() {
    let g = OnGitHub::unled("", "");
    let (w, pull) = g.opened_here_held_by_b("words");
    let n = pull.number;
    g.held_there(n, &pull.head, 1406);
    let moved = Cell::new(false);
    let pause = Then(Cell::new(0), |k| {
        if k == 5 && !moved.replace(true) {
            g.posting_elsewhere(n, &pull.head, &queued_on_b(1407));
        }
    });
    let (code, shown) = g.sluice_pausing(
        &["wait", "main", "--worktree", &w, "--timeout", "1m"],
        &pause,
    );
    assert_eq!(code, sluice::WAITING, "{shown}");
    assert!(moved.get(), "the wait paused five times: {shown}");
    let first = format!("#{n} is submission 1406 on b, taken in by b");
    let next = format!("#{n} is submission 1407 on b, taken in by b");
    assert_eq!(shown.matches(&first).count(), 1, "{shown}");
    assert_eq!(shown.matches(&next).count(), 1, "{shown}");
    assert!(!shown.contains("Submission 1406 on b."), "{shown}");
    assert!(shown.contains("queued: next on main"), "{shown}");
}

/// submission-wait, "Not yet taken in": host B keeps main, and B's check on
/// the head of host A's pull request says it needs one review; A's `sluice
/// wait main --timeout 2s --json` exits 5, its queue not taken in and
/// waiting for the review, taken in by B. Without `--json`, it says once
/// that the request waits to be taken in by B.
#[test]
fn not_yet_taken_in_across_hosts() {
    let g = OnGitHub::unled("reviews = 1", "");
    let (w, pull) = g.opened_here_held_by_b("untaken");
    let n = pull.number;
    let block = serde_json::json!({
        "host": "b", "taken_in": false, "submission": null, "state": null,
        "ahead": [], "waits_for": ["1 review"],
    });
    g.posted_elsewhere(
        n,
        &pull.head,
        Status::Queued,
        "waiting: needs 1 review",
        &format!(
            "Waiting to be taken in on b.\n\n\
             Needs 1 review on this commit before sluice takes it.\n\n\
             <!-- sluice:queue {block} -->"
        ),
    );
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
    let doc = last_document(&out);
    assert_eq!(doc["pull_request"], pull.number, "{doc}");
    assert_eq!(doc["queue"]["taken_in"], false, "{doc}");
    assert_eq!(
        doc["queue"]["waits_for"],
        serde_json::json!(["1 review"]),
        "{doc}"
    );
    assert_eq!(
        doc["taken_in_by"],
        serde_json::json!({ "host": "b" }),
        "{doc}"
    );
    let (code, out) = g.sluice(&["wait", "main", "--worktree", &w, "--timeout", "2s"]);
    assert_eq!(code, sluice::WAITING, "{out}");
    let line = format!("#{} waits to be taken in by b", pull.number);
    assert_eq!(out.matches(&line).count(), 1, "{out}");
}

impl OnGitHub {
    /// A pass of this host's host process, which reads the pull requests
    /// it follows from GitHub, as one does before it answers a read (137,
    /// design D5).
    fn pass(&self) {
        let (code, out) = self.sluice(&["run", "--rounds", "1"]);
        assert_eq!(code, 0, "{out}");
    }

    /// `sluice status main --json` on this host, after a pass of its host
    /// process, which succeeds: its document.
    fn status_document(&self) -> Value {
        self.pass();
        let (code, out) = self.sluice(&["status", "main", "--json"]);
        assert_eq!(code, 0, "{out}");
        serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"))
    }

    /// `sluice status main` on this host, after a pass of its host
    /// process, which succeeds: what it says.
    fn status_words(&self) -> String {
        self.pass();
        let (code, out) = self.sluice(&["status", "main"]);
        assert_eq!(code, 0, "{out}");
        out
    }
}

/// The one `pull_requests` row of a status document, pull request `n`'s.
fn row_of(doc: &Value, n: u64) -> &Value {
    let rows = doc["pull_requests"]
        .as_array()
        .unwrap_or_else(|| panic!("{doc}"));
    assert_eq!(rows.len(), 1, "{doc}");
    assert_eq!(rows[0]["pull_request"], n, "{doc}");
    &rows[0]
}

/// request-intake-host, "Queued on the other host": host B holds host A's
/// pull request as submission 1406 behind `#10`'s 1405. `sluice status
/// main --json` on A gives the host that takes it in, and a row for the
/// request whose queue names 1406, queued, behind 1405 and `#10`; its
/// words name B and the submission there.
#[test]
fn queued_on_the_other_host() {
    let g = OnGitHub::unled("", "");
    let (_w, pull) = g.opened_here_held_by_b("queued");
    let n = pull.number;
    let block = serde_json::json!({
        "host": "b", "taken_in": true, "submission": 1406, "state": "queued",
        "ahead": [{ "submission": 1405, "pull_request": 10 }], "waits_for": [],
    });
    g.posted_elsewhere(
        n,
        &pull.head,
        Status::Queued,
        "queued behind #10",
        &format!(
            "Submission 1406 on b.\n\n\
             In main's queue; it lands once its combination passes.\n\n\
             <!-- sluice:queue {block} -->"
        ),
    );
    let doc = g.status_document();
    assert_eq!(
        doc["taken_in_by"],
        serde_json::json!({ "host": "b" }),
        "{doc}"
    );
    let row = row_of(&doc, n);
    assert_eq!(row["queue"], block, "{doc}");
    assert_eq!(
        (&row["title"], &row["from"], &row["head"]),
        (
            &pull.title.as_str().into(),
            &"unit/queued".into(),
            &pull.head.as_str().into()
        ),
        "{doc}"
    );
    assert_eq!(
        (&row["draft"], &row["state"]),
        (&false.into(), &"queued behind #10".into()),
        "{doc}"
    );
    assert!(row["read_at"].is_string(), "{doc}");
    let said = g.status_words();
    assert!(said.contains("pull requests b takes in:"), "{said}");
    let line = format!(
        "#{n} {} · submission 1406 there · queued behind #10",
        pull.title
    );
    assert!(said.contains(&line), "{said}");
    assert!(g.of(n).is_empty(), "A took nothing in");
}

/// request-intake-host, "Waiting for a review there": host B keeps main,
/// and B's check on host A's pull request says it needs one review.
/// `sluice status main` on A names B, and says the request waits to be
/// taken in, needing 1 review.
#[test]
fn waiting_for_a_review_there() {
    let g = OnGitHub::unled("reviews = 1", "");
    let (_w, pull) = g.opened_here_held_by_b("review");
    let n = pull.number;
    let block = serde_json::json!({
        "host": "b", "taken_in": false, "submission": null, "state": null,
        "ahead": [], "waits_for": ["1 review"],
    });
    g.posted_elsewhere(
        n,
        &pull.head,
        Status::Queued,
        "waiting: needs 1 review",
        &format!(
            "Waiting to be taken in on b.\n\n\
             Needs 1 review on this commit before sluice takes it.\n\n\
             <!-- sluice:queue {block} -->"
        ),
    );
    let said = g.status_words();
    assert!(said.contains("pull requests b takes in:"), "{said}");
    let line = format!(
        "#{n} {} · waiting to be taken in by b: needs 1 review",
        pull.title
    );
    assert!(said.contains(&line), "{said}");
    let doc = g.status_document();
    assert_eq!(
        doc["taken_in_by"],
        serde_json::json!({ "host": "b" }),
        "{doc}"
    );
    let row = row_of(&doc, n);
    assert_eq!(row["queue"], block, "{doc}");
    assert_eq!(row["state"], "waiting: needs 1 review", "{doc}");
}

/// request-intake-host, "No check on its head yet": host B keeps main,
/// and host A's submit has just pushed a new head to its pull request, on
/// which no `sluice` check stands. `sluice status main --json` on A gives
/// the request at its new head with a `queue` of `null`, and its words say
/// it waits to be taken in by B.
#[test]
fn no_check_on_its_head_yet() {
    let g = OnGitHub::unled("", "");
    let (w, pull) = g.opened_here_held_by_b("fresh");
    let n = pull.number;
    g.held_there(n, &pull.head, 1406);
    g.commit_in(&w, &[("more.txt", "more\n")]);
    g.submitted(&w, &["--no-wait"]);
    let head = g.pull_from("unit/fresh").head;
    assert_ne!(head, pull.head, "the submit pushed a new head");
    let doc = g.status_document();
    let row = row_of(&doc, n);
    assert_eq!(row["head"], head.as_str(), "{doc}");
    assert_eq!(row["queue"], Value::Null, "{doc}");
    assert_eq!(row["state"], "waiting", "{doc}");
    let said = g.status_words();
    let line = format!("#{n} {} · waiting to be taken in by b", pull.title);
    assert!(said.contains(&line), "{said}");
}

/// request-intake-host, "The keeper and the check name different hosts":
/// host C keeps main, while sluice's check on host A's pull request,
/// posted by host B, still names submission 1406 there. `sluice status
/// main --json` on A gives `taken_in_by` naming C and the request's queue
/// naming B and 1406, and its words say both (design D25). C signs as the
/// policy's signer `b`, so main accepts it (137; record 0122, design D8).
#[test]
fn the_keeper_and_the_check_name_different_hosts() {
    let g = OnGitHub::unled("", "");
    g.signs("c", "b");
    g.leads("c");
    g.be("a");
    let w = g.session("moved", &[("m.txt", "m\n")]);
    g.submitted(&w, &["--no-wait"]);
    let pull = g.pull_from("unit/moved");
    let n = pull.number;
    g.held_there(n, &pull.head, 1406);
    let doc = g.status_document();
    assert_eq!(
        doc["taken_in_by"],
        serde_json::json!({ "host": "c" }),
        "{doc}"
    );
    let row = row_of(&doc, n);
    assert_eq!(
        (&row["queue"]["host"], &row["queue"]["submission"]),
        (&"b".into(), &1406.into()),
        "{doc}"
    );
    let said = g.status_words();
    assert!(said.contains("pull requests c takes in:"), "{said}");
    let line = format!(
        "#{n} {} · submission 1406 on b · queued: next on main",
        pull.title
    );
    assert!(said.contains(&line), "{said}");
}

/// request-intake-host, "Merged", read by status: host A follows its pull
/// request, which host B holds, and GitHub marks it merged. A's `sluice
/// status main` says it landed, once: A's follows name it no longer, and
/// the next status shows no row for it.
#[test]
fn a_status_says_a_merged_request_landed_once() {
    let g = OnGitHub::unled("", "");
    let (_w, pull) = g.opened_here_held_by_b("merged");
    let n = pull.number;
    g.held_there(n, &pull.head, 1406);
    g.gh().merge(n).unwrap();
    g.follow_due(n);
    let said = g.status_words();
    assert!(
        said.contains(&format!("#{n} {} · landed", pull.title)),
        "{said}"
    );
    assert!(g.follows()[n.to_string()].is_null(), "{}", g.follows());
    let said = g.status_words();
    assert!(!said.contains(&format!("#{n} ")), "{said}");
    assert!(g.status_document()["pull_requests"].is_null());
}

// ---------------------------------------------------------------------------
// a submission whose work lands elsewhere (122, 5; record 0100)
// ---------------------------------------------------------------------------

impl OnGitHub {
    /// Another queue of `main` lands pull request `n` at `head`: a merge
    /// commit onto main, as a second copy of the repository on the host
    /// lands it, which GitHub reads as the pull request merged. Returns the
    /// landing.
    fn landed_elsewhere(&self, n: u64, head: &Rev) -> Rev {
        let landing = self
            .f
            .world
            .merge_onto("main", head, "chore(land): elsewhere");
        self.gh().merge(n).unwrap();
        landing
    }

    /// `main`'s log, a document a line, as its records hold it.
    fn log(&self) -> Vec<Value> {
        let snap = self.f.sluice().seams().state.read("main").unwrap();
        sluice_core::line::Line::read_log(&snap)
            .unwrap()
            .iter()
            .map(|l| serde_json::to_value(l).unwrap())
            .collect()
    }
}

/// 122, 5; pull-request-intake, "Landed by another queue": a submitted pull
/// request another queue of `main` lands, under test here, leaves GitHub's
/// open list as merged; its submission here is landed as that landing,
/// never withdrawn, its run ended, and a wait on it exits 0.
#[test]
fn landed_by_another_queue_while_queued() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let (n, head) = g.by_hand("unit/elsewhere", &[("e.txt", Some("x\n"))], false);
    g.step();
    let id = g.of(n)[0]["id"].as_u64().unwrap();
    assert_eq!(g.of(n)[0]["state"], "testing", "{:?}", g.of(n));
    let landing = g.landed_elsewhere(n, &head);
    g.step();
    let a = g.of(n);
    assert_eq!(a.len(), 1, "{a:?}");
    assert_eq!(a[0]["state"], "landed", "{a:?}");
    assert_eq!(a[0]["landed_as"], landing.0.as_str());
    assert_eq!(a[0]["landed_elsewhere"], true);
    let log = g.log();
    assert!(!log.iter().any(|l| l["event"] == "withdrawn"), "{log:?}");
    assert!(
        log.iter()
            .any(|l| l["event"] == "found_landed" && l["to"] == landing.0.as_str()),
        "{log:?}"
    );
    assert_eq!(
        g.f.world.tip("main"),
        landing,
        "nothing more is landed of it"
    );
    assert_eq!(g.f.world.live_runs(), 0, "its run ended");
    let (code, out) = g.sluice(&["wait", &id.to_string(), "--json"]);
    assert_eq!(code, sluice::LANDED, "{out}");
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["state"], "landed");
    assert_eq!(doc["landed_as"], landing.0.as_str());
}

/// 122: a submission withdrawn whose work the branch then took, as a copy
/// whose intake withdrew a request another queue landed left its records,
/// is reported landed as the commit that landed it, by `sluice wait <id>`
/// and by `sluice wait main` for its worktree, never as withdrawn.
#[test]
fn a_withdrawn_submission_whose_work_landed_is_reported_landed() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let w = g.session("withdrawn-landed", &[("w.txt", "x\n")]);
    g.submitted(&w, &["--no-wait"]);
    let pull = g.pull_from("unit/withdrawn-landed");
    let id = g.of(pull.number)[0]["id"].as_u64().unwrap();
    let (code, said) = g.sluice(&["withdraw", &id.to_string()]);
    assert_eq!(code, 0, "{said}");
    let landing = g.landed_elsewhere(pull.number, &Rev::new(pull.head.clone()));
    let (code, out) = g.sluice(&["wait", &id.to_string(), "--json"]);
    assert_eq!(code, sluice::LANDED, "{out}");
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["state"], "landed", "{doc}");
    assert_eq!(doc["landed_as"], landing.0.as_str());
    assert_eq!(
        g.of(pull.number)[0]["state"],
        "withdrawn",
        "its record stands"
    );
    let (code, said) = g.sluice(&["wait", "main", "--worktree", &w]);
    assert_eq!(code, sluice::LANDED, "{said}");
    assert!(
        said.contains(&format!("landed as {}", &landing.0[..7])),
        "{said}"
    );
    assert!(!said.contains("closed without landing"), "{said}");
}

/// 122; record 0100 D5: `sluice wait main` follows a pull request this host
/// took in; another queue then lands it, and GitHub reads it closed, with
/// no fetch of `main` on this host since the wait began. The wait reads the
/// closed request against `main` as a round reads it, from its remote: it
/// says landed as the commit that landed it and exits 0, never closed
/// without landing.
#[test]
fn a_wait_beside_a_driver_reads_a_request_another_queue_landed_from_the_branch() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let w = g.session("beside", &[("b.txt", "x\n")]);
    g.submitted(&w, &["--no-wait"]);
    let pull = g.pull_from("unit/beside");
    assert_eq!(g.of(pull.number).len(), 1, "{:?}", g.subs());
    let landing = RefCell::new(None);
    let pause = Then(Cell::new(0), |k| {
        if k == 2 {
            *landing.borrow_mut() =
                Some(g.landed_elsewhere(pull.number, &Rev::new(pull.head.clone())));
        }
    });
    let (code, said) = g.sluice_pausing(
        &["wait", "main", "--worktree", &w, "--timeout", "60s"],
        &pause,
    );
    assert_eq!(code, sluice::LANDED, "{said}");
    let landing = landing.into_inner().expect("landed while the wait went");
    assert!(
        said.contains(&format!("landed as {}", &landing.0[..7])),
        "{said}"
    );
    assert!(!said.contains("closed without landing"), "{said}");
}

/// 5, 120: the operator's withdrawal of a hand-opened pull request's
/// submission rests it on its head, open and ready: no work of it goes
/// on, so a wait on it exits 4 at once.
#[test]
fn a_wait_on_a_request_the_operator_withdrew_exits_withdrawn() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let (n, _) = g.by_hand("unit/rested", &[("r.txt", Some("x\n"))], false);
    g.step();
    let id = g.of(n)[0]["id"].as_u64().unwrap();
    let (code, said) = g.sluice(&["withdraw", &id.to_string()]);
    assert_eq!(code, 0, "{said}");
    let (code, out) = g.sluice(&["wait", &id.to_string(), "--timeout", "20s", "--json"]);
    assert_eq!(code, sluice::WITHDRAWN, "{out}");
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["state"], "withdrawn", "{doc}");
    assert_eq!(doc["id"], id, "{doc}");
}

/// 67, 122; record 0100 D2, D4: a wait on a pull request's submission its
/// own submit withdrew for a new head follows the request to the
/// submission of that head, and ends with its own submission landed by
/// that one's landing, naming it in `carried_by`.
#[test]
fn a_wait_follows_the_new_head_its_submit_withdrew_it_for() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let w = g.session("follow", &[("f.txt", "x\n")]);
    g.submitted(&w, &["--no-wait"]);
    let n = g.pull_from("unit/follow").number;
    let first = g.of(n)[0]["id"].as_u64().unwrap();
    let new = g.commit_in(&w, &[("more.txt", "more\n")]);
    g.submitted(&w, &["--no-wait"]);
    g.f.world.hold_runs(false);
    g.f.world.complete_all();
    let (code, out) = g.sluice(&["wait", &first.to_string(), "--timeout", "60s", "--json"]);
    assert_eq!(code, sluice::LANDED, "{out}");
    let doc: Value = serde_json::from_str(&out).unwrap();
    let subs = g.of(n);
    let second = subs
        .iter()
        .find(|x| x["head"] == new.0.as_str())
        .unwrap_or_else(|| panic!("{subs:?}"));
    assert_eq!(second["state"], "landed", "{second}");
    assert_eq!(doc["id"], first, "{doc}");
    assert_eq!(doc["state"], "landed", "{doc}");
    assert_eq!(doc["landed_elsewhere"], true, "{doc}");
    assert_eq!(doc["landed_as"], second["landed_as"], "{doc}");
    assert_eq!(
        doc["carried_by"],
        serde_json::json!({ "id": second["id"], "state": "landed" }),
        "{doc}"
    );
    assert_eq!(g.of(n)[0]["state"], "withdrawn");
}

/// 67, 120; record 0100 D2, D3: a pull request closed on the new head that
/// carries a withdrawn submission's work, before that head lands, ends the
/// withdrawn submission's wait withdrawn, exit 4, its own record naming the
/// request closed in `carried_by`. What the submit read of the request
/// stays the wait's for a quarter of the host's interval, here the whole
/// test, and is read again once the request's entry moved (rule core.32):
/// the closing that withdrew the new head decides the wait, never the
/// submit's read of the request open.
#[test]
fn a_wait_exits_withdrawn_when_the_request_carrying_its_work_closes() {
    let g = OnGitHub::unled("", "");
    g.discovery(Duration::from_secs(3600));
    g.f.sluice().as_peer(peer("a")).step("main").unwrap();
    g.f.world.hold_runs(true);
    let w = g.session("follow-closed", &[("c.txt", "x\n")]);
    g.submitted(&w, &["--no-wait"]);
    let n = g.pull_from("unit/follow-closed").number;
    let first = g.of(n)[0]["id"].as_u64().unwrap();
    g.commit_in(&w, &[("more.txt", "more\n")]);
    g.submitted(&w, &["--no-wait"]);
    assert_eq!(g.of(n).len(), 2, "{:?}", g.of(n));
    g.gh().close(n).unwrap();
    g.interval_passes();
    let (code, out) = g.sluice(&["wait", &first.to_string(), "--timeout", "60s", "--json"]);
    assert_eq!(code, sluice::WITHDRAWN, "{out}");
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["id"], first, "{doc}");
    assert_eq!(doc["state"], "withdrawn", "{doc}");
    assert_eq!(
        doc["carried_by"],
        serde_json::json!({ "pull_request": n, "state": "closed" }),
        "{out}"
    );
}

// ---------------------------------------------------------------------------
// pull-request-status: what a request shows of its entry
// ---------------------------------------------------------------------------

impl OnGitHub {
    /// 51, D13: submission `id`'s `delivery`, as `sluice status <id>
    /// --json` gives it; null where it has none.
    fn delivery(&self, id: &Value) -> Value {
        let (code, out) = self.sluice(&["status", &id.to_string(), "--json"]);
        assert_eq!(code, 0, "{out}");
        serde_json::from_str::<Value>(&out).unwrap()["delivery"].clone()
    }

    /// The log's lines of `event` naming submission `id`.
    fn logged(&self, event: &str, id: &Value) -> Vec<Value> {
        self.log()
            .into_iter()
            .filter(|l| l["event"] == event && l["id"] == *id)
            .collect()
    }

    /// How many times sluice's check on `head` was posted.
    fn posts_on(&self, head: &Rev) -> usize {
        let held = self.forge.held().unwrap();
        held.posts
            .iter()
            .filter(|(at, name, _)| *at == head.0 && name == "sluice")
            .count()
    }
}

/// A pull request held for its submitter says so: one a waiting submit
/// opened, blamed, is a draft whose check reads "waiting: the next submit",
/// naming the next submit from its branch and the ready mark; GitHub keeps
/// the check failed once it was, so what sluice sent is queued (record
/// 0096, D3). Its one comment names the failed check, links to the report,
/// and says sluice marked it a draft and what submits it again.
#[test]
fn a_submits_pull_request_sent_back_says_so() {
    let g = OnGitHub::new("");
    let w = g.session("blamed", &[("broken", "x\n")]);
    let (code, said) = g.submit(&w, &[]);
    assert_eq!(code, sluice::FAILED, "{said}");
    let pull = g.pull_from("unit/blamed");
    assert!(pull.draft, "{pull:?}");
    assert_eq!(g.of_after(pull.number, 2).len(), 1, "{:?}", g.subs());
    let head = Rev::new(pull.head.clone());
    let (_, title, summary) = g.posted(&head).unwrap();
    assert_eq!(title, "waiting: the next submit");
    let held = g.forge.held().unwrap();
    let sent = held
        .posts
        .iter()
        .rfind(|(rev, name, _)| *rev == pull.head && name == "sluice")
        .unwrap();
    assert_eq!(
        (sent.2.status, sent.2.title.as_str()),
        (Status::Queued, "waiting: the next submit")
    );
    assert!(
        summary.contains(
            "The next `sluice submit main` from `unit/blamed`, or marking it ready for review, \
             submits it again."
        ),
        "{summary}"
    );
    let delivered: Vec<String> = g
        .comments(pull.number)
        .into_iter()
        .filter(|c| c.contains("told="))
        .collect();
    assert_eq!(delivered.len(), 1, "{delivered:?}");
    let c = &delivered[0];
    assert!(c.contains("`app:test` failed"), "{c}");
    assert!(c.contains("[sluice's report]("), "{c}");
    assert!(
        c.contains(
            "sluice marked it a draft. The next `sluice submit main` from `unit/blamed`, or \
             marking it ready for review, submits it again."
        ),
        "{c}"
    );
    assert!(!c.contains("Push a fix"), "{c}");
}

/// Landed, merged by landing: the check is success and reads landed;
/// `main` is sluice's landing commit, whose second parent is the pull
/// request's head, and GitHub shows it merged. No sent-back comment is
/// left, only the report (ADR 0044).
#[test]
fn landed_and_merged_by_landing() {
    let g = OnGitHub::new("");
    let (n, head) = g.by_hand("unit/lands", &[("l.txt", Some("l\n"))], false);
    g.step_until("landed", |g| {
        g.of(n).first().is_some_and(|s| s["state"] == "landed")
    });
    let landed = g.f.world.tip("main");
    assert_eq!(g.f.world.parents(&landed).get(1), Some(&head));
    let (status, title, _) = g.posted(&head).unwrap();
    assert_eq!(status, Status::Success);
    assert!(title.starts_with("landed as "), "{title}");
    g.step();
    let pull = g.forge.held().unwrap().pulls[0].clone();
    assert!(!pull.open, "GitHub shows it merged");
    let comments = g.comments(n);
    assert!(
        comments.iter().all(|c| !c.contains("told=")),
        "{comments:?}"
    );
    assert!(g
        .report(n)
        .unwrap()
        .contains("## sluice: landed on main as "));
}

/// 51, C.5; outcome-delivery spec, "A request's landing is posted" and "A
/// post that fails": the driver's post of a request's outcome is a
/// hand-over by `pull-request` to `pull/<n>` that confirms on receipt, so a
/// landing posted is done, with its hand-over and its confirmation logged,
/// and no inbox in a worktree on the request's head branch gives it; a
/// blame whose post GitHub refuses stays owed to the request with no
/// hand-over (record 0102, design D5).
#[test]
fn a_requests_post_confirms_its_outcome_on_receipt() {
    let g = OnGitHub::new("");
    let (n, head) = g.by_hand("unit/delivered", &[("t.txt", Some("t\n"))], false);
    g.step_until("landed", |g| {
        g.of(n).first().is_some_and(|s| s["state"] == "landed")
    });
    let id = g.of(n)[0]["id"].clone();
    let key = format!("{id}:landed");
    let reader = format!("pull/{n}");
    g.step_until("posted", |g| !g.logged("handed", &id).is_empty());
    let delivery = g.delivery(&id);
    assert_eq!(delivery["key"], key.as_str(), "{delivery}");
    assert_eq!(delivery["state"], "done", "{delivery}");
    let handed = g.logged("handed", &id);
    assert_eq!(handed.len(), 1, "{handed:?}");
    assert_eq!(handed[0]["by"], "pull-request");
    assert_eq!(handed[0]["to"], reader.as_str());
    let confirmed = g.logged("confirmed", &id);
    assert_eq!(confirmed.len(), 1, "{confirmed:?}");
    assert_eq!(confirmed[0]["reader"], reader.as_str());
    assert_eq!(confirmed[0]["by"], "pull-request");
    g.f.world.session("unit/delivered", "unit/delivered", &head);
    let (code, out) = g.sluice(&["inbox", "--worktree", "unit/delivered", "--json"]);
    assert_eq!(code, 0, "{out}");
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc, serde_json::json!({ "outcomes": [] }));

    let (m, _) = g.by_hand("unit/broken", &[("broken", Some("x\n"))], false);
    g.forge.refuse_posts(true);
    // Each step whose post GitHub refuses fails, as it says.
    let step = || {
        g.interval_passes();
        g.sluice(&["step", "main"])
    };
    for _ in 0..20 {
        if g.of(m).iter().any(|s| s["state"] == "failed") {
            break;
        }
        step();
    }
    for _ in 0..2 {
        step();
    }
    let id = g.of(m)[0]["id"].clone();
    assert_eq!(g.of(m)[0]["state"], "failed", "{:?}", g.subs());
    let delivery = g.delivery(&id);
    assert_eq!(
        delivery["reader"],
        format!("pull/{m}").as_str(),
        "{delivery}"
    );
    assert_eq!(delivery["state"], "owed", "{delivery}");
    assert_eq!(delivery["handed"], serde_json::json!([]), "{delivery}");
    assert!(g.logged("handed", &id).is_empty());
}

/// D1: the queue a check's summary carries in its block, as a program
/// reads it.
fn block(summary: &str) -> Value {
    let q = sluice::profiles::github::pulls::queue_of(summary)
        .unwrap_or_else(|| panic!("no queue block: {summary}"));
    serde_json::to_value(q).unwrap()
}

/// A submission's number on this host.
fn id_of(subs: &[Value]) -> u64 {
    subs.first()
        .and_then(|s| s["id"].as_u64())
        .unwrap_or_else(|| panic!("not taken in: {subs:?}"))
}

/// pull-request-status, "Queued behind another", "A queued submission's
/// block", "Waiting for a review" and "Nothing in it changed": host `b`
/// takes main's pull requests in, one at a time. The check on the second
/// request's head names `b`, its submission there and the one ahead of it
/// with its pull request, in its words and its block; the check of one
/// needing a review says it waits to be taken in on `b`, and for what.
/// Once the second round has posted the first as under test and the
/// forecast behind both, a round where neither moved posts nothing on
/// either (D1; rule core.32).
#[test]
fn the_check_names_its_host_and_queue() {
    let g = OnGitHub::unled("reviews = 1", "parallel = 1\n");
    g.leads("b");
    g.f.world.hold_runs(true);
    let (a, a_head) = g.by_hand("unit/ahead", &[("a.txt", Some("a\n"))], false);
    let (b, b_head) = g.by_hand("unit/behind", &[("b.txt", Some("b\n"))], false);
    let (_, c_head) = g.by_hand("unit/unreviewed", &[("c.txt", Some("c\n"))], false);
    for (n, head) in [(a, &a_head), (b, &b_head)] {
        g.gh().review(n, "reviewer", "APPROVED", &head.0).unwrap();
    }
    g.step();
    let (ahead, behind) = (id_of(&g.of(a)), id_of(&g.of(b)));
    let (status, title, summary) = g.posted(&b_head).unwrap();
    assert_eq!(status, Status::Queued);
    assert_eq!(title, format!("queued behind #{a}"));
    assert!(
        summary.starts_with(&format!("Submission {behind} on b.\n\n")),
        "{summary}"
    );
    assert_eq!(
        block(&summary),
        serde_json::json!({
            "host": "b", "taken_in": true, "submission": behind, "state": "queued",
            "ahead": [{ "submission": ahead, "pull_request": a }], "waits_for": [],
        })
    );
    let (_, title, summary) = g.posted(&c_head).unwrap();
    assert_eq!(title, "waiting: needs 1 review");
    assert!(
        summary.starts_with("Waiting to be taken in on b.\n\n"),
        "{summary}"
    );
    assert_eq!(
        block(&summary),
        serde_json::json!({
            "host": "b", "taken_in": false, "submission": null, "state": null,
            "ahead": [], "waits_for": ["1 review"],
        })
    );
    g.step();
    let (on_b, on_c) = (g.posts_on(&b_head), g.posts_on(&c_head));
    g.step();
    assert_eq!(g.posts_on(&b_head), on_b, "{:?}", g.posted(&b_head));
    assert_eq!(g.posts_on(&c_head), on_c, "{:?}", g.posted(&c_head));
}

/// pull-request-status, "The queue ahead moves": once the submission ahead
/// lands, host `b` posts the check of the one behind it again, the same
/// submission with nothing ahead of it (D1).
#[test]
fn the_queue_block_moves_with_the_queue_ahead() {
    let g = OnGitHub::unled("", "parallel = 1\n");
    g.leads("b");
    g.f.world.hold_runs(true);
    let (a, _) = g.by_hand("unit/first", &[("f.txt", Some("f\n"))], false);
    let (b, b_head) = g.by_hand("unit/second", &[("s.txt", Some("s\n"))], false);
    g.step();
    let (ahead, behind) = (id_of(&g.of(a)), id_of(&g.of(b)));
    let before = block(&g.posted(&b_head).unwrap().2);
    assert_eq!(
        before["ahead"],
        serde_json::json!([{ "submission": ahead, "pull_request": a }])
    );
    g.f.world.complete_all();
    g.step_until("the first landed", |g| {
        g.of(a).first().is_some_and(|s| s["state"] == "landed")
    });
    g.step();
    let (_, title, summary) = g.posted(&b_head).unwrap();
    assert!(
        title == "queued: next on main" || title == format!("testing main + #{b}"),
        "{title}"
    );
    assert!(
        summary.starts_with(&format!("Submission {behind} on b.\n\n")),
        "{summary}"
    );
    let after = block(&summary);
    assert_eq!(after["submission"], behind, "{after}");
    assert_eq!(after["ahead"], serde_json::json!([]), "{after}");
}

/// pull-request-status, "each submission ahead of it": the check's block
/// and title name what is ahead in the order host `b`'s rounds take its
/// queue, not the order of submission. With the first request under test,
/// a planner gives the third priority 5, and the second's check names it
/// ahead; then the third lands after the second, and the second's check
/// names only the first again, the third's both (6, 7; D1).
#[test]
fn the_queue_block_follows_the_queues_order() {
    let g = OnGitHub::unled("", "parallel = 1\n");
    g.leads("b");
    g.f.world.hold_runs(true);
    let (a, _) = g.by_hand("unit/tested", &[("t.txt", Some("t\n"))], false);
    let (b, b_head) = g.by_hand("unit/second", &[("b.txt", Some("b\n"))], false);
    let (c, c_head) = g.by_hand("unit/third", &[("c.txt", Some("c\n"))], false);
    g.step();
    let (first, second, third) = (id_of(&g.of(a)), id_of(&g.of(b)), id_of(&g.of(c)));
    let amend = |args: &[&str]| {
        let (code, said) = g.sluice(&[&["amend", &third.to_string()], args].concat());
        assert_eq!(code, 0, "{said}");
    };
    let shown = |head: &Rev| {
        let (_, title, summary) = g.posted(head).unwrap();
        (title, block(&summary)["ahead"].clone())
    };
    let entry = |submission: u64, n: u64| serde_json::json!({ "submission": submission, "pull_request": n });
    amend(&["--priority", "5"]);
    g.step();
    assert_eq!(
        shown(&b_head),
        (
            format!("queued behind #{a}, #{c}"),
            serde_json::json!([entry(first, a), entry(third, c)])
        )
    );
    assert_eq!(
        shown(&c_head),
        (
            format!("queued behind #{a}"),
            serde_json::json!([entry(first, a)])
        )
    );
    amend(&["--after", &second.to_string()]);
    g.step();
    assert_eq!(
        shown(&b_head),
        (
            format!("queued behind #{a}"),
            serde_json::json!([entry(first, a)])
        )
    );
    assert_eq!(
        shown(&c_head),
        (
            format!("queued behind #{a}, #{b}"),
            serde_json::json!([entry(first, a), entry(second, b)])
        )
    );
}

// ---------------------------------------------------------------------------
// a submission's wait and its submit again (record 0130; 120)
// ---------------------------------------------------------------------------

/// A pull request taken in, out of time: `sluice wait <branch>` follows its
/// submission, and ends with that submission's record, as `sluice wait
/// <id>` does.
#[test]
fn wait_follows_a_pull_requests_submission() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let w = g.session("taken", &[("t.txt", "x\n")]);
    let doc: Value = serde_json::from_str(&g.submitted(&w, &["--no-wait", "--json"])).unwrap();
    let id = doc["id"].as_u64().unwrap_or_else(|| panic!("{doc}"));
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
    let record: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(record["id"], id, "{record}");
    assert!(
        record["state"] == "queued" || record["state"] == "testing",
        "{record}"
    );
    assert!(record.get("pull_request").is_none(), "{record}");
}

/// submission-wait, "Taken in here, out of time": this host took the
/// agent's pull request in as submission `<id>`, under test. `sluice wait
/// main --timeout 2s --json` for the agent's worktree exits 5 with `<id>`'s
/// record, which gains no `wait`; in words it says `still waiting: sluice
/// wait <id>` (record 0130 D5).
#[test]
fn a_wait_on_a_request_taken_in_here_out_of_time_prints_its_record() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let w = g.session("record", &[("r.txt", "x\n")]);
    let doc: Value = serde_json::from_str(&g.submitted(&w, &["--no-wait", "--json"])).unwrap();
    let id = doc["id"].as_u64().unwrap_or_else(|| panic!("{doc}"));
    let wait = |rest: &[&str]| {
        g.sluice(
            &[
                &["wait", "main", "--worktree", &w, "--timeout", "2s"][..],
                rest,
            ]
            .concat(),
        )
    };
    let (code, out) = wait(&["--json"]);
    assert_eq!(code, sluice::WAITING, "{out}");
    let record: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(record["id"], id, "{record}");
    assert!(
        record["state"] == "queued" || record["state"] == "testing",
        "{record}"
    );
    assert!(record.get("wait").is_none(), "{record}");
    let (code, out) = wait(&[]);
    assert_eq!(code, sluice::WAITING, "{out}");
    let last = out.lines().rfind(|l| !l.trim().is_empty());
    assert_eq!(
        last,
        Some(format!("still waiting: sluice wait {id}").as_str()),
        "{out}"
    );
}

/// submission-wait, "Into another branch" and "A branch that takes no
/// pull requests": pull request `#n` is into `bolt/x`, which takes none.
/// `sluice wait main --pull-request <n> --json` prints the not-found
/// document, `no pull request #<n> into main`, and `sluice wait bolt/x
/// --pull-request <n>` says `no pull request #<n> into bolt/x`; each exits
/// 1 and follows nothing (record 0130 D3).
#[test]
fn a_wait_by_pull_request_into_another_branch_is_not_found() {
    let g = OnGitHub::with_bolt();
    let head = g.branch(&[("x.txt", Some("x\n"))]);
    let n = g
        .gh()
        .open_by("person", "bolt/x", "unit/into-bolt", &head.0, false, None)
        .unwrap();
    let number = n.to_string();
    let (code, out) = g.sluice(&["wait", "main", "--pull-request", &number, "--json"]);
    assert_eq!(code, sluice::FAILED, "{out}");
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["error"]["kind"], "not_found", "{doc}");
    assert_eq!(
        doc["error"]["message"],
        format!("no pull request #{n} into main").as_str(),
        "{doc}"
    );
    let (code, out) = g.sluice(&["wait", "bolt/x", "--pull-request", &number]);
    assert_eq!(code, sluice::FAILED, "{out}");
    assert_eq!(out, format!("no pull request #{n} into bolt/x\n"));
    assert!(g.subs().is_empty(), "{:?}", g.subs());
}

/// submission-wait, "Taken in here, out of time", by its number: this host
/// took the agent's pull request in as submission `<id>`, under test, and
/// `sluice wait main --pull-request <n> --timeout 2s --json` from no
/// worktree exits 5 with `<id>`'s record, as `sluice wait <id> --timeout 2s
/// --json` prints it (record 0130 D2, D5).
#[test]
fn a_wait_by_pull_request_taken_in_here_prints_its_record() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let w = g.session("here", &[("h.txt", "x\n")]);
    let doc: Value = serde_json::from_str(&g.submitted(&w, &["--no-wait", "--json"])).unwrap();
    let id = doc["id"].as_u64().unwrap_or_else(|| panic!("{doc}"));
    let n = doc["pull_request"]
        .as_u64()
        .unwrap_or_else(|| panic!("{doc}"));
    g.step();
    let wait = |target: &str, rest: &[&str]| {
        let (code, out) =
            g.sluice(&[&["wait", target, "--timeout", "2s", "--json"][..], rest].concat());
        assert_eq!(code, sluice::WAITING, "{out}");
        serde_json::from_str::<Value>(&out).unwrap()
    };
    let record = wait("main", &["--pull-request", &n.to_string()]);
    assert_eq!(record["id"], id, "{record}");
    assert_eq!(record["state"], "testing", "{record}");
    assert!(record.get("wait").is_none(), "{record}");
    let by_id = wait(&id.to_string(), &[]);
    assert_eq!(record, by_id);
}

/// submission-wait, "Another worktree's pull request": worktree `unit/a`
/// opened one pull request and `unit/b` another, this host took both in,
/// and GitHub refuses every post of their outcomes, so neither is
/// confirmed on the request. `sluice wait main --pull-request <a's>` for
/// `unit/b` reports `unit/a`'s submission landed, never `unit/b`'s, and
/// hands it over to no one; `sluice inbox` in `unit/a` then hands that
/// landing over (51; rule core.52; record 0130 D2).
#[test]
fn a_wait_by_another_worktrees_pull_request_hands_nothing_over() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let submitted = |name: &str| {
        let w = g.session(name, &[(&format!("{name}.txt"), "x\n")]);
        let doc: Value = serde_json::from_str(&g.submitted(&w, &["--no-wait", "--json"])).unwrap();
        let id = doc["id"].as_u64().unwrap_or_else(|| panic!("{doc}"));
        (w, id, doc["pull_request"].clone())
    };
    let (a, a_id, a_n) = submitted("a");
    let (b, b_id, _) = submitted("b");
    g.forge.refuse_posts(true);
    g.f.world.hold_runs(false);
    g.f.world.complete_all();
    let (code, out) = g.sluice(&[
        "wait",
        "main",
        "--worktree",
        &b,
        "--pull-request",
        &a_n.to_string(),
        "--timeout",
        "1m",
        "--json",
    ]);
    assert_eq!(code, sluice::LANDED, "{out}");
    let record: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(record["id"], a_id, "{record}");
    assert_ne!(record["id"], b_id, "{record}");
    assert_eq!(record["state"], "landed", "{record}");
    let id = Value::from(a_id);
    assert_eq!(g.delivery(&id)["state"], "owed", "{}", g.delivery(&id));
    assert_eq!(g.delivery(&id)["handed"], serde_json::json!([]));
    let (code, out) = g.sluice(&["inbox", "--worktree", &a, "--json"]);
    assert_eq!(code, 0, "{out}");
    let doc: Value = serde_json::from_str(&out).unwrap();
    let outcomes = doc["outcomes"]
        .as_array()
        .unwrap_or_else(|| panic!("{doc}"));
    assert_eq!(outcomes.len(), 1, "{doc}");
    assert_eq!(outcomes[0]["id"], a_id, "{doc}");
    assert_eq!(outcomes[0]["state"], "landed", "{doc}");
}

/// 122, 123: a waiting submit whose pull request's submission lands, and
/// whose round then fails to release the combination's checkout, says the
/// failed try, reports it landed and exits 0: the round went on, so
/// nothing failed after the landing.
#[test]
fn a_landing_stands_when_its_release_then_fails() {
    let g = OnGitHub::new("");
    let w = g.session("release", &[("r.txt", "r\n")]);
    g.f.world.fail_next_release();
    let (code, said) = g.submit(&w, &[]);
    assert_eq!(code, sluice::LANDED, "{said}");
    let n = g.pull_from("unit/release").number;
    assert!(said.contains(&format!("testing main + #{n} ·")), "{said}");
    let main = g.f.world.tip("main");
    assert!(
        said.lines()
            .any(|l| l.starts_with("could not release sluice/try/main/")),
        "{said}"
    );
    assert!(
        said.ends_with(&format!("passed · landed as {}\n", &main.0[..7])),
        "{said}"
    );
    assert!(!said.contains("sluice: after the landing: "), "{said}");
}

/// Blamed, then submitted again on the same head (120): the submit marks
/// the pull request ready for review, intake takes the same head, and its
/// checks run again; the submit follows that test to its outcome, names
/// who fixes it, and the request is held for the next submit again.
#[test]
fn blamed_then_submitted_again_on_the_same_head() {
    let g = OnGitHub::new("");
    let w = g.session("again", &[("broken", "x\n")]);
    let (code, said) = g.submit(&w, &[]);
    assert_eq!(code, sluice::FAILED, "{said}");
    let pull = g.pull_from("unit/again");
    let n = pull.number;
    assert!(pull.draft, "{pull:?}");
    let (code, said) = g.submit(&w, &[]);
    assert_eq!(code, sluice::FAILED, "{said}");
    assert!(
        said.contains(&format!("updated #{n}")) && said.contains("marked ready for review"),
        "{said}"
    );
    assert!(!said.contains("nothing was queued or run again"), "{said}");
    assert!(said.contains(&format!("testing main + #{n} ·")), "{said}");
    assert!(said.contains("failed: app:test"), "{said}");
    assert!(
        said.lines().last().is_some_and(
            |l| l == "the agent working in unit/again fixes it there and submits again"
        ),
        "{said}"
    );
    let subs = g.of(n);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], pull.head.as_str());
    assert_eq!(subs[1]["state"], "failed", "{subs:?}");
    assert!(g.pull_from("unit/again").draft);
    assert_eq!(g.of_after(n, 2).len(), 2, "{:?}", g.subs());
}

/// Timed out, then submitted again on the same head: the advice to submit
/// the same head again is one sluice takes, never "nothing was queued or
/// run again".
#[test]
fn timed_out_then_submitted_again_on_the_same_head() {
    let g = OnGitHub::timed("2s");
    g.f.world.hangs("app:test");
    let w = g.session("slowly", &[("s.txt", "x\n")]);
    let (code, said) = g.submit(&w, &[]);
    assert_eq!(code, sluice::FAILED, "{said}");
    assert!(
        said.contains(
            "if the host was slow, the agent working in unit/slowly submits the same head again"
        ),
        "{said}"
    );
    assert!(!said.contains("draft"), "{said}");
    let n = g.pull_from("unit/slowly").number;
    let (code, said) = g.submit(&w, &[]);
    assert_eq!(code, sluice::FAILED, "{said}");
    assert!(!said.contains("nothing was queued or run again"), "{said}");
    assert!(said.contains(&format!("testing main + #{n} ·")), "{said}");
    assert!(said.contains("timed out"), "{said}");
    let subs = g.of(n);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["result"], "timed out", "{subs:?}");
}

/// A submit on a queued pull request replaces its submission: the submit
/// withdraws the old head's submission itself, and the new head is taken
/// with no draft and no hold.
#[test]
fn a_new_head_submitted_while_queued() {
    let g = OnGitHub::new("");
    g.f.world.hold_runs(true);
    let w = g.session("replaced", &[("r.txt", "x\n")]);
    g.submitted(&w, &["--no-wait"]);
    let n = g.pull_from("unit/replaced").number;
    let first = g.of(n);
    assert_eq!(first.len(), 1, "{first:?}");
    let new = g.commit_in(&w, &[("more.txt", "more\n")]);
    let said = g.submitted(&w, &["--no-wait"]);
    assert!(
        said.contains(&format!("withdrew its submission {}", first[0]["id"])),
        "{said}"
    );
    let pull = g.pull_from("unit/replaced");
    assert!(!pull.draft && pull.head == new.0, "{pull:?}");
    let subs = g.of_after(n, 1);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[0]["state"], "withdrawn");
    assert_eq!(subs[0]["withdrawn"]["by"], "unit/replaced");
    assert_eq!(subs[1]["head"], new.0.as_str());
    assert_ne!(subs[1]["state"], "withdrawn", "{subs:?}");
    assert_ne!(
        g.posted(&new).map(|p| p.1),
        Some("waiting: the next submit".into())
    );
}

/// Submitted by the operator's ops: an agent's pull request, sent back, is
/// submitted from a worktree on the same branch with ops's fix and ops as
/// its agent. It is ready for review at the head pushed, names ops as its
/// agent, and lands.
#[test]
fn submitted_by_the_operators_ops() {
    let g = OnGitHub::new("");
    let w = g.session("unit-1", &[("broken", "x\n")]);
    let (code, said) = g.submit(&w, &[]);
    assert_eq!(code, sluice::FAILED, "{said}");
    let n = g.pull_from("unit/unit-1").number;
    assert!(g.pull_from("unit/unit-1").draft);
    let at = g.f.seams().driver.session(&w).unwrap().head;
    let fixed = g.f.world.work_saying(&at, &[("broken", None)], "ops's fix");
    g.f.world.session(&w, "madswan-ops", &fixed);
    let said = g.submitted(&w, &["--no-wait"]);
    assert!(said.contains("marked ready for review"), "{said}");
    let pull = g.pull_from("unit/unit-1");
    assert!(!pull.draft && pull.head == fixed.0, "{pull:?}");
    assert!(
        pull.body.contains("<!-- sluice: agent=madswan-ops -->")
            && !pull.body.contains("agent=unit/unit-1"),
        "{}",
        pull.body
    );
    g.step_until("landed", |g| g.of(n).iter().any(|s| s["state"] == "landed"));
    let subs = g.of(n);
    let landed = subs.iter().find(|s| s["state"] == "landed").unwrap();
    assert_eq!(landed["from"]["agent"], "madswan-ops", "{subs:?}");
    assert_eq!(landed["head"], fixed.0.as_str());
}

impl OnGitHub {
    /// S56 (120): the request from `unit/first`, changing `plan.py`, landed,
    /// so a request made off `main` before it that changes `plan.py` too
    /// conflicts.
    fn s56_land_first(&self) {
        let (first, head) = self.by_hand("unit/first", &[("plan.py", Some("first\n"))], false);
        self.gh()
            .review(first, "reviewer", "APPROVED", &head.0)
            .unwrap();
        self.step_until("the first landed", |g| {
            g.of(first).iter().any(|s| s["state"] == "landed")
        });
    }

    /// S56: a fix of pull request `n`'s branch onto `main`: a merge keeping
    /// the branch's files, which merges onto `main` without conflict,
    /// pushed to GitHub. Returns the new head.
    fn s56_fix_onto_main(&self, n: u64) -> Rev {
        let pull = self.forge.pull(&repo(), n).unwrap().unwrap();
        let fixed = self.f.world.merge_ours(
            &Rev::new(pull.head),
            &self.f.world.tip("main"),
            "Fix onto main",
        );
        self.gh().pushed(&pull.head_ref, &fixed.0).unwrap();
        fixed
    }
}

/// S56 (120): an agent's pull request, refused for a conflict, waits for
/// its submitter. A fix pushed to its branch, standing for one another
/// submission landed there, moves its head; over several rounds it is a
/// draft, and nothing is submitted or landed in the agent's name, though
/// its review holds. The agent's next submit marks it ready at the new
/// head, and it lands.
#[test]
fn s56_a_refused_request_waits_for_its_submitter() {
    let g = OnGitHub::new("reviews = 1");
    let w = g.session("s56", &[("plan.py", "agent\n")]);
    g.s56_land_first();
    g.submitted(&w, &["--no-wait"]);
    let pull = g.pull_from("unit/s56");
    let n = pull.number;
    g.gh()
        .review(n, "reviewer", "APPROVED", &pull.head)
        .unwrap();
    g.step_until("conflict", |g| {
        g.of(n).iter().any(|s| s["state"] == "conflict")
    });
    let main = g.f.world.tip("main");
    let fixed = g.s56_fix_onto_main(n);
    g.f.world.session(&w, &w, &fixed);
    g.gh().review(n, "reviewer", "APPROVED", &fixed.0).unwrap();
    let subs = g.of_after(n, 3);
    assert_eq!(subs.len(), 1, "nothing submitted in its name: {subs:?}");
    assert_eq!(subs[0]["state"], "conflict");
    let pull = g.pull_from("unit/s56");
    assert!(pull.draft && pull.head == fixed.0, "{pull:?}");
    assert_eq!(g.f.world.tip("main"), main, "nothing landed in its name");
    assert_eq!(g.posted(&fixed).unwrap().1, "waiting: the next submit");
    let (code, said) = g.submit(&w, &[]);
    assert_eq!(code, sluice::LANDED, "{said}");
    assert!(said.contains("marked ready for review"), "{said}");
    assert!(said.contains("landed as"), "{said}");
    let subs = g.of(n);
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert_eq!(subs[1]["head"], fixed.0.as_str());
    assert_eq!(subs[1]["state"], "landed");
    assert_eq!(subs[1]["from"]["agent"], "unit/s56");
}

/// S56 (120): a pull request opened by hand, refused for a conflict, whose
/// head moves as an agent's would, is submitted again once its review
/// holds on the new head.
#[test]
fn s56_a_refused_request_opened_by_hand_is_taken_again_once_reviewed() {
    let g = OnGitHub::new("reviews = 1");
    let (hand, hand_head) = g.by_hand("unit/hand", &[("plan.py", Some("hand\n"))], false);
    g.s56_land_first();
    g.gh()
        .review(hand, "reviewer", "APPROVED", &hand_head.0)
        .unwrap();
    g.step_until("the hand-opened conflict", |g| {
        g.of(hand).iter().any(|s| s["state"] == "conflict")
    });
    let moved = g.s56_fix_onto_main(hand);
    let subs = g.of_after(hand, 2);
    assert_eq!(subs.len(), 1, "waits for its review: {subs:?}");
    assert!(!g.pull_from("unit/hand").draft);
    assert_eq!(g.posted(&moved).unwrap().1, "waiting: needs 1 review");
    g.gh()
        .review(hand, "reviewer", "APPROVED", &moved.0)
        .unwrap();
    g.step_until("the hand-opened landed", |g| {
        g.of(hand)
            .iter()
            .any(|s| s["state"] == "landed" && s["head"] == moved.0.as_str())
    });
}

/// S56 (120): a pull request opened by hand and withdrawn by the operator
/// rests, its review holding, until its author pushes a new head.
#[test]
fn s56_a_request_the_operator_withdrew_waits_for_a_new_head() {
    let g = OnGitHub::new("reviews = 1");
    g.f.world.hold_runs(true);
    let (rest, rest_head) = g.by_hand("unit/rest", &[("r.txt", Some("x\n"))], false);
    g.gh()
        .review(rest, "reviewer", "APPROVED", &rest_head.0)
        .unwrap();
    g.step_until("the withdrawn one submitted", |g| !g.of(rest).is_empty());
    let id = g.of(rest)[0]["id"].as_u64().unwrap().to_string();
    let (code, said) = g.sluice(&["withdraw", &id, "--as", "operator"]);
    assert_eq!(code, 0, "{said}");
    let subs = g.of_after(rest, 3);
    assert_eq!(subs.len(), 1, "never submitted again: {subs:?}");
    assert_eq!(subs[0]["state"], "withdrawn");
    assert_eq!(
        g.posted(&rest_head).unwrap().1,
        "waiting: withdrawn by the operator"
    );
    let pushed = g.push_more(rest, "rest.txt");
    g.gh()
        .review(rest, "reviewer", "APPROVED", &pushed.0)
        .unwrap();
    g.step_until("the withdrawn one submitted again", |g| {
        g.of(rest).iter().any(|s| s["head"] == pushed.0.as_str())
    });
}

// ---------------------------------------------------------------------------
// forge-reads: the open pull requests asked for at the host's interval, a
// pull request read again only once it moved, and posted on only when what
// sluice says changed (136). The fakes' clock moves a second at each read,
// so a run of passes takes a span the test cannot set; an interval of an
// hour stands for the host's minute, which those passes fall within.
// ---------------------------------------------------------------------------

/// An hour: an interval no run of passes in a test reaches.
const HOUR: Duration = Duration::from_secs(3600);

impl OnGitHub {
    /// Two rounds of `main`: the first writes the branch's records, which
    /// the second's asking reads as a move; after them nothing on the host
    /// moves.
    fn settle(&self) {
        self.step();
        self.step();
    }

    /// `sluice run --rounds <rounds>` on this host: its host process's
    /// passes.
    fn passes(&self, rounds: usize) {
        let (code, out) = self.sluice(&["run", "--rounds", &rounds.to_string()]);
        assert_eq!(code, 0, "{out}");
    }

    /// `sluice step main` on this host, the clock moved on by nothing.
    fn steps(&self) {
        let (code, out) = self.sluice(&["step", "main"]);
        assert_eq!(code, 0, "{out}");
    }

    /// How many times what pull request `n` at `head` is beside its list
    /// entry was read alone: its reviews, other systems' checks on its
    /// head, its ready marks and its comments.
    fn beside_reads(&self, n: u64, head: &Rev) -> usize {
        [
            format!("reviews/{n}"),
            format!("checks/{head}"),
            format!("readied/{n}"),
            format!("comments/{n}"),
        ]
        .iter()
        .map(|w| self.calls(w))
        .sum()
    }
}

/// forge-reads, "Nothing moved" and "A restart asks nothing new": twenty
/// passes within the interval ask for main's open pull requests once, and
/// steps of a process started again then ask nothing; the one waiting for
/// a review is read again only at that asking, by the one query.
#[test]
fn nothing_moved_asks_once_a_minute() {
    let g = OnGitHub::new("reviews = 1");
    g.discovery(HOUR);
    let (n, head) = g.by_hand("unit/review", &[("r.txt", Some("r\n"))], false);
    g.settle();
    g.interval_passes();
    let (lists, beside, queries) = (
        g.calls("list"),
        g.beside_reads(n, &head),
        g.calls("query/main"),
    );
    g.passes(20);
    assert_eq!(g.calls("list"), lists + 1);
    assert_eq!(
        g.calls("query/main"),
        queries + 1,
        "its reviews, at the asking"
    );
    assert_eq!(g.beside_reads(n, &head), beside, "nothing read alone");
    assert_eq!(g.posted(&head).unwrap().1, "waiting: needs 1 review");
    let posts = g.posts();
    g.restart();
    for _ in 0..3 {
        g.steps();
    }
    assert_eq!(
        g.calls("list"),
        lists + 1,
        "a restart asks nothing within the interval"
    );
    assert_eq!(g.calls("query/main"), queries + 1);
    assert_eq!(g.beside_reads(n, &head), beside);
    assert_eq!(g.posts(), posts);
}

/// forge-reads, "Three branches, one list": main and two bolts take pull
/// requests. Twenty passes within the interval, with nothing on the host
/// moving, ask GitHub for the repository's open pull requests once, in one
/// list that serves all three branches
/// (`one-forge-read-per-repository-per-interval@b3b6757cea2d`).
#[test]
fn a_repositorys_branches_share_one_list_each_interval() {
    let g = OnGitHub::built(
        "",
        "",
        "1h",
        "\n[branches.\"bolt/*\"]\nrequired = [\"#gate\"]\ntrust = [\"a\", \"b\"]\n\
         remote = \"origin\"\n\n[branches.\"bolt/*\".pull_requests]\n",
    );
    g.discovery(HOUR);
    for bolt in ["bolt/x", "bolt/y"] {
        g.f.world.set_branch(bolt, &g.f.world.tip("main"));
        g.f.world.on_remote(bolt);
    }
    for branch in ["main", "bolt/x", "bolt/y"] {
        g.f.sluice().as_peer(peer("a")).step(branch).unwrap();
    }
    g.passes(2);
    g.interval_passes();
    let lists = g.calls("list");
    g.passes(20);
    assert_eq!(g.calls("list"), lists + 1);
}

/// forge-reads, "The host sets the interval": forty passes within the
/// host's interval ask once; the next pass, the interval on, asks again.
/// The interval's setting is `instance.rs`'s
/// `the_host_sets_the_discovery_interval_ten_seconds_at_least`.
#[test]
fn the_host_sets_the_interval() {
    let g = OnGitHub::new("reviews = 1");
    g.discovery(2 * HOUR);
    g.by_hand("unit/slowly", &[("s.txt", Some("s\n"))], false);
    let lists = g.calls("list");
    g.passes(40);
    assert_eq!(g.calls("list"), lists + 1);
    g.f.world.advance(chrono::Duration::hours(2));
    g.passes(1);
    assert_eq!(g.calls("list"), lists + 2, "asked again, the interval on");
}

/// forge-reads, "A submit on the host asks at once": seconds after main's
/// last asking, a submit that does not wait opens a pull request, and its
/// own pass asks for main's pull requests and takes it in.
#[test]
fn a_submit_on_the_host_asks_at_once() {
    let g = OnGitHub::new("");
    g.discovery(HOUR);
    g.f.world.hold_runs(true);
    g.step();
    let asked = g.calls("list");
    let w = g.session("prompt", &[("p.txt", "p\n")]);
    let said = g.submitted(&w, &["--no-wait"]);
    let n = g.pull_from("unit/prompt").number;
    assert!(said.contains(&format!("from #{n}")), "taken in: {said}");
    assert_eq!(g.of(n).len(), 1, "{:?}", g.subs());
    assert_eq!(
        g.calls("list"),
        asked + 2,
        "the open's own list and the pass's"
    );
}

/// forge-reads, "A landing asks at once": the pass after a round lands a
/// submission asks for main's pull requests, and once nothing moves again
/// the next asks nothing.
#[test]
fn a_landing_asks_at_once() {
    let g = OnGitHub::new("");
    g.discovery(HOUR);
    let (n, _) = g.by_hand("unit/lands", &[("l.txt", Some("l\n"))], false);
    g.interval_passes();
    for _ in 0..20 {
        if g.of(n).first().is_some_and(|s| s["state"] == "landed") {
            break;
        }
        g.steps();
    }
    assert_eq!(g.of(n)[0]["state"], "landed", "{:?}", g.subs());
    let asked = g.calls("list");
    g.steps();
    assert_eq!(g.calls("list"), asked + 1, "the landing asks");
    g.steps();
    assert_eq!(g.calls("list"), asked + 1, "then nothing moved");
}

/// forge-reads, "A waiting pull request that stands" and "Nothing new to
/// say": a pull request taken into the queue, asked for three times more
/// with nothing moved, has nothing beside its list entry read again, and
/// its check is neither read back nor posted again.
#[test]
fn a_queued_pull_request_that_stands_is_read_by_the_list_alone() {
    let g = OnGitHub::new("reviews = 1");
    g.discovery(HOUR);
    g.f.world.hold_runs(true);
    let w = g.session("stands", &[("s.txt", "x\n")]);
    g.submitted(&w, &["--no-wait"]);
    let pull = g.pull_from("unit/stands");
    let n = pull.number;
    let head = Rev::new(pull.head.clone());
    g.gh()
        .review(n, "reviewer", "APPROVED", &pull.head)
        .unwrap();
    let status = |g: &OnGitHub| {
        g.interval_passes();
        g.submitted(&w, &["--status"])
    };
    let said = status(&g);
    assert_eq!(g.of(n).len(), 1, "taken in: {said}");
    assert!(said.starts_with("queued"), "{said}");
    let (lists, beside) = (g.calls("list"), g.beside_reads(n, &head));
    let (read_back, posts) = (g.calls(&format!("posted/{head}")), g.posts());
    for _ in 0..3 {
        let said = status(&g);
        assert!(said.starts_with("queued"), "{said}");
    }
    assert_eq!(
        g.calls("list"),
        lists + 6,
        "each finds it in the list, and asks"
    );
    assert_eq!(g.beside_reads(n, &head), beside, "nothing beside the list");
    assert_eq!(
        g.calls(&format!("posted/{head}")),
        read_back,
        "its check not read back"
    );
    assert_eq!(g.posts(), posts, "nor posted again");
}

/// forge-reads, "The state moves": the pull request's submission goes
/// from queued to testing, and its check is posted once, saying testing,
/// with no read of GitHub's copy first.
#[test]
fn the_state_moves_and_is_posted_unread() {
    let g = OnGitHub::new("");
    g.discovery(HOUR);
    g.f.world.hold_runs(true);
    let (n, head) = g.by_hand("unit/moves", &[("m.txt", Some("m\n"))], false);
    g.interval_passes();
    g.steps();
    assert_eq!(g.of(n).len(), 1, "{:?}", g.subs());
    let (read_back, posts) = (g.calls(&format!("posted/{head}")), g.posts());
    g.steps();
    let (status, title, _) = g.posted(&head).unwrap();
    assert_eq!(status, Status::InProgress, "{title}");
    assert!(title.starts_with("testing main + #"), "{title}");
    assert_eq!(g.posts(), posts + 1);
    assert_eq!(g.calls(&format!("posted/{head}")), read_back);
}

/// forge-reads, "A draft": each asking reads a draft from the list alone.
#[test]
fn a_draft_is_read_from_the_list_alone() {
    let g = OnGitHub::new("reviews = 1\nchecks = [\"qa/approved\"]");
    let (n, head) = g.by_hand("unit/drafty", &[("d.txt", Some("d\n"))], true);
    let lists = g.calls("list");
    for _ in 0..3 {
        g.step();
    }
    assert_eq!(g.calls("list"), lists + 3);
    assert_eq!(g.beside_reads(n, &head), 0);
    assert_eq!(g.posted(&head).unwrap().1, "waiting: a draft");
}

/// forge-reads, "A pull request waiting on a check": out of draft, it
/// waits for `qa/approved`, which another system passes on its head,
/// moving nothing GitHub lists; the next asking reads its checks again, by
/// the one query, and submits it. pull-request-intake: "the next asking
/// submits it".
#[test]
fn a_pull_request_waiting_on_a_check_is_read_at_each_asking() {
    let g = OnGitHub::new("checks = [\"qa/approved\"]");
    let (n, head) = g.by_hand("unit/qa", &[("q.txt", Some("q\n"))], false);
    g.step();
    assert!(g.of(n).is_empty(), "{:?}", g.subs());
    let (before, queries) = (g.calls(&format!("checks/{head}")), g.calls("query/main"));
    let updated = g.pull_from("unit/qa").updated_at;
    g.gh()
        .check(&head.0, "qa/approved", Standing::Passed)
        .unwrap();
    assert_eq!(g.pull_from("unit/qa").updated_at, updated);
    g.step();
    assert_eq!(g.calls("query/main"), queries + 1);
    assert_eq!(
        g.calls(&format!("checks/{head}")),
        before,
        "nothing read alone"
    );
    assert_eq!(g.of(n).len(), 1, "{:?}", g.subs());
}

/// forge-reads, "A new head": a pull request's head moves, and its reviews
/// and checks are read again at the next asking, against its new head, by
/// the one query.
#[test]
fn a_new_head_is_read_again() {
    let g = OnGitHub::new("reviews = 1\nchecks = [\"qa/approved\"]");
    let (n, old) = g.by_hand("unit/heads", &[("h.txt", Some("h\n"))], false);
    g.step();
    let reviews = g.calls(&format!("reviews/{n}"));
    let queries = g.calls("query/main");
    let new = g.push_more(n, "h2.txt");
    g.step();
    assert_eq!(g.calls("query/main"), queries + 1);
    assert_eq!(g.calls(&format!("reviews/{n}")), reviews);
    assert_eq!(g.beside_reads(n, &new), 0);
    assert_eq!(
        g.posted(&new).unwrap().1,
        "waiting: needs 1 review, qa/approved"
    );
    assert!(g.posted(&old).is_some());
}
