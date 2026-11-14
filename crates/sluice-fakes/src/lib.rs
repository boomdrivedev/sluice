//! In-memory profiles of all five seams, scriptable per test.
//!
//! One [`World`] holds everything a host would: a repository of commits
//! and refs, the places made on it and the runs started in them, the
//! sessions' places, and the records. Each seam's fake is a view of the
//! world implementing that seam's trait and nothing else (rule core.2).
//!
//! The records fake stores the real bytes the core writes (53), so a
//! file store need only put them on disk.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Duration as Span, TimeZone, Utc};
use sluice_core::home::{HomeStore, Owed};
use sluice_core::policy::POLICY_PATH;
use sluice_core::records::{HomeDoc, HomeState, HERE, HOME_FILE};
use sluice_core::{Clock, Seams, Sluice};
use sluice_seams::{
    Adopted, AtBound, Branch, Carried, Changes, CheckedOut, Checkout, Delivery, Driver, Enforced,
    Equip, EquipStep, Equipped, Formed, GoneRelease, Guard, HostCopy, HostRun, KeptOutTask,
    LandingCommit, LastResult, Moved, Network, NotRun, Notification, Outcome, Part, Place,
    PreviewPlace, Progress, Proof, ReadRefused, Reading, Reads, ReleaseFailure, ReleasePolicy,
    Released, ResultAsk, Rev, RunHandle, RunKind, RunReport, RunRequest, Running, Session,
    SessionPlace, Settings, Snapshot, Source, State, Submitter, TaskLog, TaskProgress, TaskResult,
    Tests, Unanswered, Written,
};

/// A tree: path to content.
pub type Tree = BTreeMap<String, String>;

/// A fault a test schedules for the next move of a branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crash {
    /// The process stops before the branch moves.
    BeforeMove,
    /// The process stops after the branch moved, before it is recorded.
    AfterMove,
}

#[derive(Clone, Debug)]
struct Commit {
    tree: Tree,
    parents: Vec<Rev>,
    message: String,
    generation: u64,
}

#[derive(Clone, Debug)]
pub struct MoveRecord {
    pub branch: String,
    pub from: Rev,
    pub to: Rev,
}

type Predicate = Rc<dyn Fn(&Tree) -> bool>;

/// 149: what a run made of one task, as a check or as a task a check
/// depends on.
#[derive(Clone)]
enum Seen {
    Ran(TaskResult),
    /// It could not run, with why (114).
    NotRun(String),
    /// A task it depends on did not pass, so the runner skipped it.
    Skipped,
}

#[derive(Clone)]
struct TaskDef {
    name: String,
    /// The paths the task reads, by prefix; also its project.
    prefix: String,
    tags: BTreeSet<String>,
}

#[derive(Clone, Debug)]
struct RunState {
    request: RunRequest,
    /// The host that started it, whose count it is in (71).
    host: String,
    /// The system its host ran as it started, where Nix manages the host
    /// (84, record 0097).
    nix_system: Option<String>,
    complete: bool,
    lost: bool,
    killed: bool,
    report: Option<RunReport>,
}

impl RunState {
    fn live(&self) -> bool {
        !self.killed && !self.lost && self.report.is_none()
    }

    /// A submission's run, as the host counts it: neither a session's nor
    /// a preview's, whose runs `sluice exec` and `sluice preview` record
    /// beside the session runs.
    fn queue(&self) -> bool {
        let place = &self.request.place.name;
        !place.starts_with("session/") && !place.starts_with("preview/")
    }
}

#[derive(Default)]
struct Line {
    version: u64,
    files: BTreeMap<String, Vec<u8>>,
}

/// Record 0122 (design D2): the name a host's own files go by among the
/// fakes' homes; a remote's home goes by the remote's name.
fn own_home(host: &str) -> String {
    format!("here@{host}")
}

#[derive(Default)]
struct Inner {
    // the repository
    commits: BTreeMap<Rev, Commit>,
    refs: BTreeMap<String, Rev>,
    /// The repository's HEAD: the first branch made, as `git init` names it.
    head: Option<String>,
    moves: Vec<MoveRecord>,
    /// Each branch kept on a remote, and the value every host's copy of it
    /// starts at (124). A branch absent here is kept in the host's own
    /// refs, which are the branch.
    remote: BTreeMap<String, Option<Rev>>,
    /// Each host's copy of a branch kept on a remote, by host and branch,
    /// once it differs from where it started: its value, `None` when the
    /// host has none.
    copies: BTreeMap<(String, String), Option<Rev>>,
    /// The next levellings fail with this error, as many as the count.
    level_fails: Option<(usize, String)>,
    /// How many levellings set a copy.
    levels: usize,
    /// How many merges were formed.
    forms: usize,
    /// Revisions whose objects the host lacks, as one another host made
    /// and none fetched here (64, 124): [`Branch::holds`] reads it.
    lacking: BTreeSet<Rev>,
    /// Each host's revision of a branch kept on a remote as last fetched,
    /// by host and branch, once a read fetched it: until then, the value
    /// its copy starts at (129).
    fetched: BTreeMap<(String, String), Option<Rev>>,
    /// How many reads fetched a branch kept on a remote (129).
    fetches: usize,
    /// How many times a branch's revision was read, fetched or as held
    /// (129).
    revision_reads: usize,
    /// How many times each file of a revision was read, by path (129).
    file_reads: BTreeMap<String, usize>,
    /// The words the remote refuses each host's reads of a branch with, by
    /// host and branch (133).
    refused_branch: BTreeMap<(String, String), String>,
    /// The words the remote refuses each host's reads of a branch's records
    /// with, by host and branch: a read answers what is held, stale, and a
    /// write fails (80, 133).
    refused_state: BTreeMap<(String, String), String>,
    /// The words each branch's records fail to be read with, by branch: no
    /// answer at all, as when git cannot read them on a loaded host (129).
    failed_state: BTreeMap<String, String>,
    /// The words the list of the repository's records fails with: no
    /// answer at all, as when git cannot read them on a loaded host (26,
    /// 129).
    failed_list: Option<String>,
    /// The repository's git directory is gone from the host (129).
    repository_gone: bool,
    /// Each remote's tracking branch the host holds, by name as
    /// `origin/main`, as last fetched: read only by [`World::holds_path`]
    /// (129).
    tracking: BTreeMap<String, Rev>,
    /// The words the list of the repository's refs fails with (129).
    failed_refs: Option<String>,
    /// Each host's refusal document of a branch, by host and branch (133).
    refusals: BTreeMap<(String, String), Vec<u8>>,
    /// This host's refusal of each branch's intake (design D27).
    intake_refusals: BTreeMap<(String, String), Vec<u8>>,
    crash: Option<Crash>,
    // the check
    tasks: Vec<TaskDef>,
    fail_rules: Vec<(String, Predicate)>,
    flaky: BTreeMap<String, VecDeque<Outcome>>,
    hang: BTreeSet<String>,
    skip: BTreeSet<String>,
    kept_out: BTreeSet<String>,
    /// Asking the runner which required checks it keeps out of CI fails,
    /// with why (ADR 0050).
    kept_out_fails: Option<String>,
    /// Every worktree the runner was asked in, in order (ADR 0050).
    asked_kept_out: Vec<PathBuf>,
    /// 140: the runner reads every check as affected, as the command
    /// runner does.
    reads_every: bool,
    /// 140: asking the runner's affected reading fails, with why.
    reads_fail: Option<String>,
    /// 140: every change the runner's affected reading was asked of.
    read_pairs: Vec<(Rev, Rev)>,
    /// 147: every working copy the check seam was asked to equip, in
    /// order.
    equips: Vec<Equip>,
    /// 147: the steps an equip answers.
    equip_steps: Vec<EquipStep>,
    /// Every head published for its submission, in order (82).
    published: Vec<Rev>,
    /// The candidates open to claims on the remote, by branch and id,
    /// each at its revision (82, C.6).
    candidates: BTreeMap<(String, String), Rev>,
    /// Every fetch of one candidate by its own ref, by the host that
    /// fetched it, its branch and its id, in order (record 0118).
    candidate_fetches: Vec<(String, String, String)>,
    took: BTreeMap<String, (u64, u32)>,
    /// Checks a run leaves not run, with why (could-not-run-1 114).
    cannot_run: BTreeMap<String, String>,
    /// 149: the tasks each task depends on, which a run runs first.
    depends: BTreeMap<String, Vec<String>>,
    /// What each host lacks of what a run asks of it, by host, as the
    /// check seam's look at the host says it (84; record 0074).
    unmet: BTreeMap<String, String>,
    /// Every look at what a host lacks, by the host that looked, in order.
    looks: Vec<String>,
    /// The system each host runs, by host, as the link Nix keeps for the
    /// active system names it; a host with none is one Nix doesn't manage
    /// (84; record 0097).
    nix_systems: BTreeMap<String, String>,
    /// Every ask whether a host's system was switched during a run, by the
    /// host that asked, in order.
    system_looks: Vec<String>,
    /// The grant each host's executor records with its runs, by host; a
    /// host with none records none (138; design D9).
    grants: BTreeMap<String, String>,
    /// What each host's executor reports refusing a finished run, by host,
    /// in the profile's words (139; design D9).
    refuses: BTreeMap<String, String>,
    /// Why each host's executor cannot read its report of what it refused
    /// a run, by host (139; design D8, D9).
    refusals_unread: BTreeMap<String, String>,
    /// Every ask of what a host's executor refused a run, by the host that
    /// asked, in order.
    refusal_looks: Vec<String>,
    /// Each start's place, with the places live as it started, in order.
    live_at_start: Vec<(String, Vec<String>)>,
    /// Why each host's executor cannot hold a run's memory limit, by host,
    /// as the check seam's look says it (84, 87).
    memory_unmet: BTreeMap<String, String>,
    /// The hosts whose executor holds a run's memory limit, which the
    /// fake's open executor does not.
    holds_memory: BTreeSet<String>,
    cache: BTreeMap<(String, String), String>,
    runs: Vec<RunState>,
    /// What each run says of its tasks while it goes, by run.
    run_tasks: BTreeMap<usize, Vec<TaskProgress>>,
    /// Asking after a run's tasks fails from now.
    tasks_fail: bool,
    hold: bool,
    crash_start: bool,
    /// Starts from now fail after the host's count admits them, as a
    /// toolchain that fails to enter does, with these words (114).
    start_fails: Option<String>,
    /// Checkouts from now cannot be made, with these words.
    checkout_fails: Option<String>,
    /// How many starts from now the forge a pool starts runs on answers
    /// 5xx.
    unanswered_starts: usize,
    /// The next start finds the host at its bound (71).
    refuse_start: bool,
    /// How many times the records were read afresh (design D16).
    state_reads: Cell<usize>,
    /// The next hold finds the host at its bound, as when another
    /// repository's run took the last one (71).
    refuse_hold: bool,
    /// Every part held, by kind, in order (record 0087).
    holds: Vec<RunKind>,
    /// Whether each release of a place and each outcome pushed asks, as an
    /// agent's `sluice exec` asks meanwhile, for a session's room.
    probe_holds: bool,
    /// What each such ask was answered: whether the room was there.
    probed: Vec<bool>,
    host_limit: Option<usize>,
    /// The hosts with no room for a run of their own, whatever the limit
    /// (design D8 of `a-queues-home-is-written-in-its-records`).
    full: BTreeSet<String>,
    /// Other repositories' runs going on each host, by host (71).
    other_runs: BTreeMap<String, usize>,
    /// Parts of each host's bound held and not yet started, by host and
    /// kind (71; record 0087).
    parts: BTreeMap<(String, RunKind), usize>,
    /// The system the host's executor is, as `(os, arch)` (87).
    platform: (String, String),
    /// Tasks whose reported keys the candidate's tree does not bear out
    /// (design D9).
    forged: BTreeSet<String>,
    /// Where the run's record of a pass is looked for and not found:
    /// while set, every ran pass is unkept (114; record 0105).
    no_proof: Option<String>,
    /// What each host last saw of each branch, by host and branch (73).
    seen: BTreeMap<(String, String), (u64, String)>,
    /// The host that made each candidate's place (81).
    place_host: BTreeMap<String, String>,
    /// Each host's last sign of life: a count of its beats, and the beat.
    beats: BTreeMap<String, (u64, Vec<u8>)>,
    /// Each host's view of every other host's beats (77).
    seen_beats: BTreeMap<(String, String), (String, String)>,
    /// Every run started on a pool, in order (88).
    launches: Vec<sluice_seams::Launch>,
    /// How the next start on a pool fails: `true` as a dispatch whose
    /// answer was not read (134), `false` as one the forge refused.
    launch_fails: Option<bool>,
    /// The pool jobs that ended, by claim (77, 88).
    jobs_ended: BTreeSet<String>,
    /// Every branch a pool was woken for, in order (111).
    wakes: Vec<String>,
    /// The signer the host's results are recorded as signed by (78).
    signer: Option<String>,
    /// The hosts whose signing key no branch's policy lists for the signer
    /// they sign as: what they sign verifies for no gate (78, 87).
    unlisted: BTreeSet<String>,
    /// Session runs on the host, by worktree: whether each still runs, and
    /// what its `sluice exec` kept of it (ADR 0035).
    sessions: BTreeMap<String, (bool, HostRun)>,
    observed: Vec<(String, String, Outcome)>,
    host: String,
    // the place
    places: BTreeMap<String, Rev>,
    made: Vec<String>,
    released: Vec<String>,
    /// Session places adopted and not yet released.
    adopted: Vec<String>,
    /// The branch each session place was adopted for.
    adopted_for: BTreeMap<String, String>,
    /// When each session place's last run started.
    adopted_at: BTreeMap<String, String>,
    /// How each session place's last run ended (97).
    last_results: BTreeMap<String, LastResult>,
    /// How long the results log was when each session place's last result
    /// was kept.
    results_when_ran: BTreeMap<String, usize>,
    /// Keeping a session run's result fails.
    keeping_fails: bool,
    /// Each session place released, and the `[tests] release` it was
    /// released with, or `None` for its last run's.
    session_releases: Vec<(String, Option<String>)>,
    /// The next release of a candidate's place fails, its place kept.
    fail_release: bool,
    /// Previews made and not yet released, by name.
    previews: BTreeMap<String, PreviewPlace>,
    /// Each preview released, and whether it was kept as failed.
    preview_releases: Vec<(String, bool)>,
    /// Preview commands started from now fail.
    preview_fails: bool,
    /// Every release from now fails, with this (123).
    release_fails: Option<String>,
    /// The release of these places fails, each with what it says.
    place_release_fails: BTreeMap<String, String>,
    /// Every release asked of the place seam, by place, in order: what
    /// would run a release command.
    release_attempts: Vec<String>,
    /// Places freed with nothing run (123).
    freed: Vec<String>,
    /// Each place whose working copy was gone and whose stored release
    /// ran, in order (123).
    gone_releases: Vec<String>,
    /// Places whose runs stored no release command.
    stored_none: BTreeSet<String>,
    /// Every removal of scratch no record names fails from now, with this.
    unnamed_scratch_fails: Option<String>,
    /// Each removal of scratch no record names asked of the place seam,
    /// with how many places were held then.
    unnamed_scratch_asks: Vec<usize>,
    /// Candidates' places whose checkout was deleted by hand.
    deleted: BTreeSet<String>,
    /// How each session place's release at its asking failed.
    session_release_failed: BTreeMap<String, ReleaseFailure>,
    // the records' kept logs, by where
    logs: BTreeMap<String, String>,
    // the driver
    worktrees: BTreeMap<String, Session>,
    /// The agent the session's harness names for a submit by name (124),
    /// `SLUICE_AGENT` to the worktrunk driver; `None` names the branch.
    harness_agent: Option<String>,
    /// Worktrees with a branch checked out: the branch and the revision.
    checked_out: BTreeMap<String, (String, Rev)>,
    delivered: Vec<Notification>,
    /// Agents whose sessions take no outcome (51), and every notification
    /// pushed to them, kept apart from those delivered.
    undelivering: BTreeSet<String>,
    undelivered: Vec<Notification>,
    crash_notify: bool,
    // the records
    /// Each branch's records in each of its homes, by home and branch: a
    /// host's own files as [`own_home`] names them, a remote's by its name
    /// (record 0122; design D1).
    lines: BTreeMap<(String, String), Line>,
    /// The branches whose lock is held, one lock a branch on the host
    /// (rule core.14), whichever home its records are in.
    locks: BTreeSet<String>,
    /// How many carries wrote anything (record 0122; design D3).
    carries: usize,
    /// The next carry stops after its target's write, before it closes
    /// the home it carried from (59).
    stop_carry: bool,
    /// Each branch's candidates' tasks, kept beside its records.
    progress: BTreeMap<String, BTreeMap<String, Vec<TaskProgress>>>,
    results: Vec<u8>,
    set_asides: Vec<u8>,
    next_id: u64,
    crash_writes: usize,
    interfere: usize,
    /// What another process does on the host once an id is allocated.
    after_allocating: Option<Rc<dyn Fn(&World, u64)>>,
    // the clock
    now: DateTime<Utc>,
}

/// Everything a host holds, shared by the five fakes.
#[derive(Clone)]
pub struct World(Rc<RefCell<Inner>>);

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

/// The five fakes and a clock, ready to make a [`Sluice`].
pub struct Fakes {
    pub world: World,
    pub checkout: FakeCheckout,
    pub tests: FakeTests,
    pub branch: FakeBranch,
    pub driver: FakeDriver,
    pub state: FakeState,
    pub clock: FakeClock,
}

impl Default for Fakes {
    fn default() -> Self {
        Self::new()
    }
}

impl Fakes {
    pub fn new() -> Self {
        Fakes::of(World::new())
    }

    /// Fakes over `world`: another process on the same host, seeing what
    /// the first sees.
    pub fn of(world: World) -> Self {
        Fakes {
            checkout: FakeCheckout(world.clone()),
            tests: FakeTests(world.clone()),
            branch: FakeBranch(world.clone()),
            driver: FakeDriver(world.clone()),
            state: FakeState(world.clone()),
            clock: FakeClock(world.clone()),
            world,
        }
    }

    /// A sluice over these fakes. Making a new one is starting sluice
    /// again: it holds nothing between calls but what the records hold.
    pub fn sluice(&self) -> Sluice<'_> {
        Sluice::new(self.seams(), &self.clock)
    }

    pub fn seams(&self) -> Seams<'_> {
        Seams {
            checkout: &self.checkout,
            tests: &self.tests,
            branch: &self.branch,
            driver: &self.driver,
            state: &self.state,
        }
    }
}

fn fnv(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ seed;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn hex_id(bytes: &[u8]) -> String {
    let s = format!(
        "{:016x}{:016x}{:016x}",
        fnv(1, bytes),
        fnv(2, bytes),
        fnv(3, bytes)
    );
    s[..40].to_string()
}

impl World {
    pub fn new() -> Self {
        World(Rc::new(RefCell::new(Inner {
            next_id: 1,
            host: "fake-host".into(),
            platform: ("linux".into(), "x86_64".into()),
            now: Utc.with_ymd_and_hms(2026, 10, 11, 9, 0, 0).unwrap(),
            ..Default::default()
        })))
    }

    // -- the repository ----------------------------------------------------

    fn put_commit(inner: &mut Inner, tree: Tree, parents: Vec<Rev>, message: &str) -> Rev {
        let mut bytes = Vec::new();
        for (p, c) in &tree {
            bytes.extend(p.as_bytes());
            bytes.push(0);
            bytes.extend(c.as_bytes());
            bytes.push(0);
        }
        for p in &parents {
            bytes.extend(p.0.as_bytes());
        }
        bytes.extend(message.as_bytes());
        let id = Rev(hex_id(&bytes));
        let generation = parents
            .iter()
            .map(|p| inner.commits[p].generation + 1)
            .max()
            .unwrap_or(0);
        inner.commits.entry(id.clone()).or_insert(Commit {
            tree,
            parents,
            message: message.into(),
            generation,
        });
        id
    }

    /// A branch holding `files`.
    pub fn init(&self, branch: &str, files: &[(&str, &str)]) -> Rev {
        let mut inner = self.0.borrow_mut();
        let tree: Tree = files
            .iter()
            .map(|(p, c)| (p.to_string(), c.to_string()))
            .collect();
        let rev = Self::put_commit(&mut inner, tree, vec![], &format!("init {branch}"));
        inner.refs.insert(branch.into(), rev.clone());
        inner.head.get_or_insert_with(|| branch.into());
        rev
    }

    /// The repository's default branch, its HEAD, and its revision.
    pub fn head(&self) -> Option<(String, Option<Rev>)> {
        let inner = self.0.borrow();
        let name = inner.head.clone()?;
        let rev = inner.refs.get(&name).cloned();
        Some((name, rev))
    }

    /// Name `branch` the repository's HEAD.
    pub fn set_head(&self, branch: &str) {
        self.0.borrow_mut().head = Some(branch.into());
    }

    /// A session's work: a commit on `on` changing `changes` (`None`
    /// deletes).
    pub fn work(&self, on: &Rev, changes: &[(&str, Option<&str>)]) -> Rev {
        let mut inner = self.0.borrow_mut();
        let mut tree = inner.commits[on].tree.clone();
        for (p, c) in changes {
            match c {
                Some(c) => tree.insert(p.to_string(), c.to_string()),
                None => tree.remove(*p),
            };
        }
        let msg = format!("work {changes:?}");
        Self::put_commit(&mut inner, tree, vec![on.clone()], &msg)
    }

    /// Someone moves the branch by hand with a commit of their own.
    /// The branch is deleted, as by hand.
    pub fn delete_branch(&self, branch: &str) {
        self.0.borrow_mut().refs.remove(branch);
    }

    pub fn hand_commit(&self, branch: &str, changes: &[(&str, Option<&str>)]) -> Rev {
        let tip = self.tip(branch);
        let rev = self.work(&tip, changes);
        self.0.borrow_mut().refs.insert(branch.into(), rev.clone());
        rev
    }

    pub fn tip(&self, branch: &str) -> Rev {
        self.0.borrow().refs[branch].clone()
    }

    pub fn tree(&self, rev: &Rev) -> Tree {
        self.0.borrow().commits[rev].tree.clone()
    }

    pub fn parents(&self, rev: &Rev) -> Vec<Rev> {
        self.0.borrow().commits[rev].parents.clone()
    }

    pub fn message(&self, rev: &Rev) -> String {
        self.0.borrow().commits[rev].message.clone()
    }

    /// The first-parent history of a branch, newest first.
    pub fn first_parents(&self, branch: &str) -> Vec<Rev> {
        let inner = self.0.borrow();
        let mut out = Vec::new();
        let mut at = inner.refs.get(branch).cloned();
        while let Some(r) = at {
            at = inner.commits[&r].parents.first().cloned();
            out.push(r);
        }
        out
    }

    pub fn is_ancestor(&self, ancestor: &Rev, of: &Rev) -> bool {
        Self::ancestors(&self.0.borrow(), of).contains(ancestor)
    }

    fn ancestors(inner: &Inner, rev: &Rev) -> BTreeSet<Rev> {
        let mut seen = BTreeSet::new();
        let mut todo = vec![rev.clone()];
        while let Some(r) = todo.pop() {
            if seen.insert(r.clone()) {
                todo.extend(inner.commits[&r].parents.iter().cloned());
            }
        }
        seen
    }

    fn merge_base(inner: &Inner, a: &Rev, b: &Rev) -> Option<Rev> {
        let ours = Self::ancestors(inner, a);
        Self::ancestors(inner, b)
            .into_iter()
            .filter(|r| ours.contains(r))
            .max_by_key(|r| (inner.commits[r].generation, r.clone()))
    }

    pub fn moves(&self) -> Vec<MoveRecord> {
        self.0.borrow().moves.clone()
    }

    /// How many merges have been formed, conflicts included.
    pub fn forms(&self) -> usize {
        self.0.borrow().forms
    }

    /// How many reads fetched a branch kept on a remote (129).
    pub fn fetches(&self) -> usize {
        self.0.borrow().fetches
    }

    /// How many times a branch's records were read afresh, as a round
    /// reads them, rather than as the host holds them (129; design D16).
    pub fn state_reads(&self) -> usize {
        self.0.borrow().state_reads.get()
    }

    /// How many times a branch's revision was read, fetched or as held
    /// (129).
    pub fn revision_reads(&self) -> usize {
        self.0.borrow().revision_reads
    }

    /// How many times a file of a revision was read, a policy among them
    /// (129).
    pub fn file_reads(&self) -> usize {
        self.0.borrow().file_reads.values().sum()
    }

    /// How many times the file `path` of a revision was read (129).
    pub fn reads_of(&self, path: &str) -> usize {
        self.0.borrow().file_reads.get(path).copied().unwrap_or(0)
    }

    /// `branch` at `at`, as a bolt's is kept, with no place of it.
    pub fn set_branch(&self, branch: &str, at: &Rev) {
        self.0.borrow_mut().refs.insert(branch.into(), at.clone());
    }

    /// `branch` is kept on a remote, `refs` holding the remote's revision;
    /// every host's copy of it starts at that revision (124).
    pub fn on_remote(&self, branch: &str) {
        let mut inner = self.0.borrow_mut();
        let at = inner.refs.get(branch).cloned();
        inner.remote.insert(branch.into(), at);
    }

    /// Whether `branch` is kept on a remote.
    pub fn is_on_remote(&self, branch: &str) -> bool {
        self.0.borrow().remote.contains_key(branch)
    }

    /// This host's copy of `branch`, set by hand.
    pub fn set_copy(&self, branch: &str, at: Option<&Rev>) {
        let mut inner = self.0.borrow_mut();
        assert!(
            inner.remote.contains_key(branch),
            "{branch} is on no remote"
        );
        let key = (inner.host.clone(), branch.to_string());
        inner.copies.insert(key, at.cloned());
    }

    /// This host's copy of `branch`, `None` when it has none.
    pub fn copy_of(&self, branch: &str) -> Option<Rev> {
        World::copy_here(&self.0.borrow(), branch).flatten()
    }

    /// This host's copy of `branch`: `None` for a branch on no remote.
    fn copy_here(inner: &Inner, branch: &str) -> Option<Option<Rev>> {
        let start = inner.remote.get(branch)?;
        let key = (inner.host.clone(), branch.to_string());
        Some(inner.copies.get(&key).unwrap_or(start).clone())
    }

    /// The next `n` levellings of any copy fail with `why`, as on git's lock.
    pub fn fail_levels(&self, n: usize, why: &str) {
        self.0.borrow_mut().level_fails = Some((n, why.into()));
    }

    /// How many levellings set a copy.
    pub fn levels(&self) -> usize {
        self.0.borrow().levels
    }

    pub fn crash(&self, crash: Crash) {
        self.0.borrow_mut().crash = Some(crash);
    }

    // -- the check ---------------------------------------------------------

    /// A check reading the paths under `prefix`, tagged `gate`.
    pub fn task(&self, name: &str, prefix: &str) {
        self.0.borrow_mut().tasks.push(TaskDef {
            name: name.into(),
            prefix: prefix.into(),
            tags: BTreeSet::from(["gate".to_string()]),
        });
    }

    /// `task` fails on any revision whose tree satisfies `when`.
    pub fn fails_when(&self, task: &str, when: impl Fn(&Tree) -> bool + 'static) {
        self.0
            .borrow_mut()
            .fail_rules
            .push((task.into(), Rc::new(when)));
    }

    /// `task` fails on any revision holding `path` with `content`.
    pub fn fails_with(&self, task: &str, path: &str, content: &str) {
        let (p, c) = (path.to_string(), content.to_string());
        self.fails_when(task, move |t| t.get(&p) == Some(&c));
    }

    /// The next observed runs of `task` end in these outcomes, whatever
    /// the tree, and are never replayed.
    pub fn flaky(&self, task: &str, outcomes: &[Outcome]) {
        self.0
            .borrow_mut()
            .flaky
            .insert(task.into(), outcomes.iter().copied().collect());
    }

    /// Keeping a session run's result on its place fails from now.
    pub fn keeping_fails(&self, fails: bool) {
        self.0.borrow_mut().keeping_fails = fails;
    }

    /// `task` runs past any time limit, which cuts it off: it has no
    /// result and is named missing (time-limit-1 66).
    pub fn hangs(&self, task: &str) {
        self.0.borrow_mut().hang.insert(task.into());
    }

    /// `task` ends in time again, as on a host that keeps up once more.
    pub fn ends_in_time(&self, task: &str) {
        self.0.borrow_mut().hang.remove(task);
    }

    /// Runs leave `task` without a result, as a runner that skips it.
    pub fn skips(&self, task: &str) {
        self.0.borrow_mut().skip.insert(task.into());
    }

    /// The runner keeps `task` out of its CI mode: never run, missing, and
    /// named kept out (ADR 0044); and a submission whose gate requires it
    /// is refused (ADR 0050).
    pub fn keeps_out(&self, task: &str) {
        self.0.borrow_mut().kept_out.insert(task.into());
    }

    /// The runner lets `task` run in its CI mode again, as a submission
    /// that changes its setting does.
    pub fn lets_run(&self, task: &str) {
        self.0.borrow_mut().kept_out.remove(task);
    }

    /// Asking the runner which required checks it keeps out of CI fails
    /// from now, for `why`; `None` lets it answer again (ADR 0050).
    pub fn kept_out_fails(&self, why: Option<&str>) {
        self.0.borrow_mut().kept_out_fails = why.map(String::from);
    }

    /// Every worktree the runner was asked in which required checks it
    /// keeps out of CI, in order.
    pub fn asked_kept_out(&self) -> Vec<PathBuf> {
        self.0.borrow().asked_kept_out.clone()
    }

    /// 140: the runner reads every check as affected from now, as the
    /// command runner does; `false` lets it narrow again.
    pub fn reads_every(&self, every: bool) {
        self.0.borrow_mut().reads_every = every;
    }

    /// 140: asking the runner's affected reading fails from now, for
    /// `why`; `None` lets it answer again.
    pub fn reads_fail(&self, why: Option<&str>) {
        self.0.borrow_mut().reads_fail = why.map(String::from);
    }

    /// 140: every change the runner's affected reading was asked of, in
    /// order.
    pub fn read_pairs(&self) -> Vec<(Rev, Rev)> {
        self.0.borrow().read_pairs.clone()
    }

    /// Every head published for its submission, in order (82).
    pub fn published(&self) -> Vec<Rev> {
        self.0.borrow().published.clone()
    }

    /// Every fetch of one candidate by its own ref, by the host that
    /// fetched it, its branch and its id, in order (record 0118).
    pub fn candidate_fetches(&self) -> Vec<(String, String, String)> {
        self.0.borrow().candidate_fetches.clone()
    }

    /// The runner says `task` took `ms` in `attempts` tries, and the run
    /// took the sum against an estimate of twice it (119).
    pub fn takes(&self, task: &str, ms: u64, attempts: u32) {
        self.0.borrow_mut().took.insert(task.into(), (ms, attempts));
    }

    /// Runs leave `task` not run, for `reason` (could-not-run-1 114);
    /// `None` lets it run again.
    pub fn cannot_run(&self, task: &str, reason: Option<&str>) {
        let mut inner = self.0.borrow_mut();
        match reason {
            Some(r) => inner.cannot_run.insert(task.into(), r.into()),
            None => inner.cannot_run.remove(task),
        };
    }

    /// From now, this world's host lacks what a run asks of it for `why`,
    /// a container engine that answers, as its look says; with `None`, it
    /// meets it again (84; record 0074).
    pub fn engine_unmet(&self, why: Option<&str>) {
        let mut inner = self.0.borrow_mut();
        let host = inner.host.clone();
        match why {
            Some(w) => inner.unmet.insert(host, w.into()),
            None => inner.unmet.remove(&host),
        };
    }

    /// From now, this world's host's executor cannot hold a run's memory
    /// limit, for `why`, as its look says of a policy that states one; with
    /// `None`, it holds one again (84, 87; design D2).
    pub fn memory_unmet(&self, why: Option<&str>) {
        let mut inner = self.0.borrow_mut();
        let host = inner.host.clone();
        match why {
            Some(w) => inner.memory_unmet.insert(host, w.into()),
            None => inner.memory_unmet.remove(&host),
        };
    }

    /// This world's host's executor holds a run's memory limit from now,
    /// as an executor the fake does not stand for would.
    pub fn holds_memory(&self) {
        let mut inner = self.0.borrow_mut();
        let host = inner.host.clone();
        inner.holds_memory.insert(host);
    }

    /// Each look at the container engine, by the host that looked, in
    /// order.
    pub fn engine_looks(&self) -> Vec<String> {
        self.0.borrow().looks.clone()
    }

    /// From now, this world's host runs the system Nix names `system`, a
    /// store path; with `None`, it is a host Nix doesn't manage, as every
    /// host is until this is asked (84; record 0097, design D8). A run
    /// notes its host's system as it starts.
    pub fn nix_system(&self, system: Option<&str>) {
        let mut inner = self.0.borrow_mut();
        let host = inner.host.clone();
        match system {
            Some(s) => inner.nix_systems.insert(host, s.into()),
            None => inner.nix_systems.remove(&host),
        };
    }

    /// From now, this world's host's executor records `grant` with every
    /// run it reports; with `None`, none, as every host does until this is
    /// asked (138; design D9).
    pub fn grant(&self, grant: Option<&str>) {
        let mut inner = self.0.borrow_mut();
        let host = inner.host.clone();
        match grant {
            Some(g) => inner.grants.insert(host, g.into()),
            None => inner.grants.remove(&host),
        };
    }

    /// From now, this world's host's executor reports refusing every run
    /// it is asked of `words`; with `None`, nothing (139; design D9).
    pub fn refuses(&self, words: Option<&str>) {
        let mut inner = self.0.borrow_mut();
        let host = inner.host.clone();
        match words {
            Some(w) => inner.refuses.insert(host, w.into()),
            None => inner.refuses.remove(&host),
        };
    }

    /// From now, this world's host's executor cannot read its report of
    /// what it refused a run, for `words`; with `None`, it reads it again
    /// (139; design D8, D9). A report it cannot read outweighs what it
    /// would report.
    pub fn refusals_unread(&self, words: Option<&str>) {
        let mut inner = self.0.borrow_mut();
        let host = inner.host.clone();
        match words {
            Some(w) => inner.refusals_unread.insert(host, w.into()),
            None => inner.refusals_unread.remove(&host),
        };
    }

    /// Each ask of what a host's executor refused a run, by the host that
    /// asked, in order.
    pub fn refusal_looks(&self) -> Vec<String> {
        self.0.borrow().refusal_looks.clone()
    }

    /// Each ask whether the host's system was switched during a run, by
    /// the host that asked, in order.
    pub fn system_looks(&self) -> Vec<String> {
        self.0.borrow().system_looks.clone()
    }

    /// Each start's place, with the places live as it started, in order of
    /// starting.
    pub fn live_at_each_start(&self) -> Vec<(String, Vec<String>)> {
        self.0.borrow().live_at_start.clone()
    }

    /// The next start finds its place made and stops before starting.
    pub fn crash_next_start(&self) {
        self.0.borrow_mut().crash_start = true;
    }

    /// Starts from now fail with `words` once the host's count admits
    /// them, before any check runs, as a `[tests] toolchain` that fails to
    /// enter does (114); `None` lets them start again.
    pub fn start_fails(&self, words: Option<&str>) {
        self.0.borrow_mut().start_fails = words.map(str::to_string);
    }

    /// Checkouts of combinations from now cannot be made, failing with
    /// `words`; `None` lets them be made again.
    pub fn checkout_fails(&self, words: Option<&str>) {
        self.0.borrow_mut().checkout_fails = words.map(str::to_string);
    }

    /// The next `n` starts fail as a forge's 5xx answer, as a pool's
    /// would whose forge could not answer, before anything starts.
    pub fn unanswer_starts(&self, n: usize) {
        self.0.borrow_mut().unanswered_starts = n;
    }

    /// How many of those are still to fail.
    pub fn unanswered_starts_left(&self) -> usize {
        self.0.borrow().unanswered_starts
    }

    /// The next start is refused at the host's bound, as when another
    /// repository's run took the last one after the round counted (71).
    pub fn refuse_next_start(&self) {
        self.0.borrow_mut().refuse_start = true;
    }

    /// The next hold of a part finds the host at its bound, as when
    /// another repository's run took the last one (71).
    pub fn refuse_next_hold(&self) {
        self.0.borrow_mut().refuse_hold = true;
    }

    /// From now, each release of a place and each outcome pushed asks for
    /// a session's room as an agent's `sluice exec` would meanwhile, and
    /// gives it back at once: what a round holds between its decision and
    /// its starts (rule core.49).
    pub fn probe_holds(&self, probe: bool) {
        self.0.borrow_mut().probe_holds = probe;
    }

    /// What each ask [`World::probe_holds`] made was answered: whether a
    /// session's room was there.
    pub fn probed(&self) -> Vec<bool> {
        self.0.borrow().probed.clone()
    }

    /// An agent's ask for a session's room meanwhile, where asked for.
    fn probe(inner: &mut Inner) {
        if inner.probe_holds {
            let free = FakeTests::count(inner).total() < inner.host_limit.unwrap_or(usize::MAX);
            inner.probed.push(free);
        }
    }

    /// Every part of the host's bound held so far, by kind, in order
    /// (record 0087).
    pub fn holds(&self) -> Vec<RunKind> {
        self.0.borrow().holds.clone()
    }

    /// The parts of this world's host's bound held and not yet started
    /// (record 0087).
    pub fn parts_held(&self) -> usize {
        let inner = self.0.borrow();
        inner
            .parts
            .iter()
            .filter(|((h, _), _)| *h == inner.host)
            .map(|(_, n)| n)
            .sum()
    }

    /// Runs started from now stay running until completed.
    pub fn hold_runs(&self, hold: bool) {
        self.0.borrow_mut().hold = hold;
    }

    /// Complete the held run on `revision`.
    pub fn complete(&self, revision: &Rev) {
        let mut inner = self.0.borrow_mut();
        let mut any = false;
        for r in inner.runs.iter_mut() {
            if r.request.revision == *revision && r.live() {
                r.complete = true;
                any = true;
            }
        }
        assert!(any, "no run on {revision}");
    }

    /// The live run on `revision` says this of its tasks until it ends.
    pub fn says_tasks(&self, revision: &Rev, tasks: &[TaskProgress]) {
        let mut inner = self.0.borrow_mut();
        let n = inner
            .runs
            .iter()
            .rposition(|r| r.request.revision == *revision && r.live())
            .unwrap_or_else(|| panic!("no live run on {revision}"));
        inner.run_tasks.insert(n, tasks.to_vec());
    }

    /// Asking after a run's tasks fails from now.
    pub fn tasks_fail(&self, fail: bool) {
        self.0.borrow_mut().tasks_fail = fail;
    }

    pub fn complete_all(&self) {
        for r in self.0.borrow_mut().runs.iter_mut() {
            r.complete = true;
        }
    }

    /// The executor running on `revision` stops before reporting.
    pub fn lose(&self, revision: &Rev) {
        for r in self.0.borrow_mut().runs.iter_mut() {
            if r.request.revision == *revision {
                r.lost = true;
            }
        }
    }

    pub fn host_limit(&self, limit: usize) {
        self.0.borrow_mut().host_limit = Some(limit);
    }

    /// From now, this world's host has no room for a run: its bound is 0,
    /// whatever another host's is, as a lead that leaves its branch's runs
    /// to other hosts has.
    pub fn no_room(&self) {
        let mut inner = self.0.borrow_mut();
        let host = inner.host.clone();
        inner.full.insert(host);
    }

    /// From now, `n` runs of other repositories go on this world's host,
    /// counted with its own against its bound (71).
    pub fn other_runs(&self, n: usize) {
        let mut inner = self.0.borrow_mut();
        let host = inner.host.clone();
        inner.other_runs.insert(host, n);
    }

    /// From now, the host's results are recorded signed by `signer`, as
    /// on a branch whose records are on its remote (78).
    pub fn signs_as(&self, signer: &str) {
        self.0.borrow_mut().signer = Some(signer.into());
    }

    /// From now, this world's host signs with a key no branch's policy
    /// lists for its signer, or, with `false`, with one they list: the
    /// results it records are never read back, and its records answer that
    /// its runs count for no branch (78, 87).
    pub fn key_unlisted(&self, unlisted: bool) {
        let mut inner = self.0.borrow_mut();
        let host = inner.host.clone();
        if unlisted {
            inner.unlisted.insert(host);
        } else {
            inner.unlisted.remove(&host);
        }
    }

    /// From now, this world's host is `host`: what it observes is its own
    /// (73).
    pub fn be_host(&self, host: &str) {
        self.0.borrow_mut().host = host.into();
    }

    /// From now, the host lacks `rev`'s objects, or, with `false`, holds
    /// them again (64, 124).
    pub fn lack(&self, rev: &Rev, lacking: bool) {
        let mut inner = self.0.borrow_mut();
        match lacking {
            true => inner.lacking.insert(rev.clone()),
            false => inner.lacking.remove(rev),
        };
    }

    /// From now, `origin` refuses this host's reads of `branch` with
    /// `said`; with `None`, it answers them again (133).
    pub fn refuse_reads(&self, branch: &str, said: Option<&str>) {
        let mut inner = self.0.borrow_mut();
        let key = (inner.host.clone(), branch.to_string());
        match said {
            Some(said) => inner.refused_branch.insert(key, said.into()),
            None => inner.refused_branch.remove(&key),
        };
    }

    /// From now, `origin` refuses this host's reads of `branch`'s records
    /// with `said`: a read answers the records as last read, stale, and a
    /// write fails; with `None`, it answers them again (80, 133).
    pub fn refuse_state_reads(&self, branch: &str, said: Option<&str>) {
        let mut inner = self.0.borrow_mut();
        let key = (inner.host.clone(), branch.to_string());
        match said {
            Some(said) => inner.refused_state.insert(key, said.into()),
            None => inner.refused_state.remove(&key),
        };
    }

    /// From now, every read of `branch`'s records fails with `said`, with
    /// nothing answered from what was last read; with `None`, they are
    /// read again (129).
    pub fn fail_state_reads(&self, branch: &str, said: Option<&str>) {
        let mut inner = self.0.borrow_mut();
        match said {
            Some(said) => inner.failed_state.insert(branch.into(), said.into()),
            None => inner.failed_state.remove(branch),
        };
    }

    /// From now, every list of the repository's records fails with `said`;
    /// with `None`, they are listed again (26, 129).
    pub fn fail_state_list(&self, said: Option<&str>) {
        self.0.borrow_mut().failed_list = said.map(Into::into);
    }

    /// From now, the repository's git directory is gone from the host, or
    /// with `false` is there again (129).
    pub fn repository_gone(&self, gone: bool) {
        self.0.borrow_mut().repository_gone = gone;
    }

    /// Whether the repository's git directory is gone from the host.
    pub fn is_repository_gone(&self) -> bool {
        self.0.borrow().repository_gone
    }

    /// `name`, a remote's tracking branch such as `origin/main`, as last
    /// fetched to the host: at `at`, or with `None` held no more (129).
    pub fn set_tracking(&self, name: &str, at: Option<&Rev>) {
        let mut inner = self.0.borrow_mut();
        match at {
            Some(at) => inner.tracking.insert(name.into(), at.clone()),
            None => inner.tracking.remove(name),
        };
    }

    /// From now, every list of the repository's refs fails with `said`;
    /// with `None`, they are listed again (129).
    pub fn fail_ref_list(&self, said: Option<&str>) {
        self.0.borrow_mut().failed_refs = said.map(Into::into);
    }

    /// Every revision a ref the host holds names, a branch, its copy of a
    /// branch kept on a remote, or a remote's tracking branch, each once,
    /// with nothing fetched (129).
    pub fn tips(&self) -> Result<Vec<Rev>> {
        let inner = self.0.borrow();
        if let Some(said) = &inner.failed_refs {
            bail!("{said}");
        }
        let host = &inner.host;
        let copies = inner
            .copies
            .iter()
            .filter(|((h, _), _)| h == host)
            .filter_map(|(_, at)| at.as_ref());
        let mut tips: Vec<Rev> = Vec::new();
        for rev in inner
            .refs
            .values()
            .chain(copies)
            .chain(inner.tracking.values())
        {
            if !tips.contains(rev) {
                tips.push(rev.clone());
            }
        }
        Ok(tips)
    }

    /// Whether any ref the host holds, a branch, its copy of a branch kept
    /// on a remote, or a remote's tracking branch, has `path` in its tree,
    /// with nothing fetched (129).
    pub fn holds_path(&self, path: &str) -> Result<bool> {
        let inner = self.0.borrow();
        if let Some(said) = &inner.failed_refs {
            bail!("{said}");
        }
        let host = &inner.host;
        let copies = inner
            .copies
            .iter()
            .filter(|((h, _), _)| h == host)
            .filter_map(|(_, at)| at.as_ref());
        Ok(inner
            .refs
            .values()
            .chain(copies)
            .chain(inner.tracking.values())
            .any(|rev| inner.commits[rev].tree.contains_key(path)))
    }

    /// This host's refusal document of `branch`, as kept (133).
    pub fn refusal(&self, branch: &str) -> Option<String> {
        let inner = self.0.borrow();
        let key = (inner.host.clone(), branch.to_string());
        let doc = inner.refusals.get(&key)?;
        Some(String::from_utf8(doc.clone()).expect("a refusal is JSON"))
    }

    /// The refused read of `asked` from `origin` with `said`.
    fn refused(asked: String, said: &str) -> anyhow::Error {
        ReadRefused {
            remote: "origin".into(),
            asked,
            said: said.into(),
            note: None,
        }
        .into()
    }

    /// The words this host's reads of `branch`'s records are refused with.
    fn state_refused(inner: &Inner, branch: &str) -> Option<String> {
        let key = (inner.host.clone(), branch.to_string());
        inner.refused_state.get(&key).cloned()
    }

    /// Every run started on a pool.
    pub fn launches(&self) -> Vec<sluice_seams::Launch> {
        self.0.borrow().launches.clone()
    }

    /// The next start on a pool fails: `unanswered`, as a dispatch whose
    /// answer was not read, which the forge may have taken and which is
    /// kept among the launches (134); otherwise as one the forge refused,
    /// which starts nothing.
    pub fn fail_next_launch(&self, unanswered: bool) {
        self.0.borrow_mut().launch_fails = Some(unanswered);
    }

    /// The pool's job under `claim` has ended: lost, cancelled, or done.
    pub fn end_job(&self, claim: &str) {
        self.0.borrow_mut().jobs_ended.insert(claim.into());
    }

    /// Every branch a pool was woken for.
    pub fn wakes(&self) -> Vec<String> {
        self.0.borrow().wakes.clone()
    }

    /// From now, `task`'s reported key is one the candidate's tree does
    /// not bear out, as a check that lies about its key reports it: the
    /// profile answers it refused, saying `<task>: its record names an
    /// input the tree does not hold` (C.6, 20; design D6 of
    /// `a-signed-run-without-a-sandbox-proves-its-passes`).
    pub fn forges(&self, task: &str) {
        self.0.borrow_mut().forged.insert(task.into());
    }

    /// Design D8 of the change `a-pass-nothing-proves-blames-no-one` (114;
    /// record 0105): with `Some(store)`, from now on each ran pass of a run
    /// is unkept, its record not in `store`, naming its own key as the
    /// missing record's (design D6 of
    /// `a-signed-run-without-a-sandbox-proves-its-passes`), and no longer
    /// proven; fails,
    /// the tasks a check was failed by and replays are proven as before.
    /// A run that keeps no proof keeps no pass to replay either, as a
    /// store a run could not write holds neither. `None` keeps proof
    /// again.
    pub fn keeps_no_proof(&self, store: Option<&str>) {
        self.0.borrow_mut().no_proof = store.map(str::to_string);
    }

    /// The system the host's executor is, as `(os, arch)`: `linux` on
    /// `x86_64` unless set.
    pub fn platform(&self, os: &str, arch: &str) {
        self.0.borrow_mut().platform = (os.into(), arch.into());
    }

    /// Every run request, in order of starting.
    pub fn requests(&self) -> Vec<RunRequest> {
        self.0
            .borrow()
            .runs
            .iter()
            .map(|r| r.request.clone())
            .collect()
    }

    /// The revisions runs were started on, in order of starting.
    pub fn started(&self) -> Vec<Rev> {
        self.requests().into_iter().map(|r| r.revision).collect()
    }

    /// Runs neither finished nor ended: a process group still running.
    pub fn live_runs(&self) -> usize {
        self.0.borrow().runs.iter().filter(|r| r.live()).count()
    }

    /// Every check an executor observed, in order: `(task, hash, outcome)`.
    pub fn observed(&self) -> Vec<(String, String, Outcome)> {
        self.0.borrow().observed.clone()
    }

    fn task_hash(task: &TaskDef, tree: &Tree) -> String {
        let mut bytes = task.name.as_bytes().to_vec();
        for (p, c) in tree.iter().filter(|(p, _)| p.starts_with(&task.prefix)) {
            bytes.extend(p.as_bytes());
            bytes.extend(c.as_bytes());
        }
        hex_id(&bytes)[..10].to_string()
    }

    fn evaluate(inner: &mut Inner, request: &RunRequest) -> RunReport {
        // Design D9: the grant of the host the run reports as its own.
        let grant = inner.grants.get(&inner.host).cloned();
        if request.place.name.starts_with("preview/") {
            // A preview's command: one exit, never a check (103).
            return RunReport {
                detail: None,
                kept_out: Vec::new(),
                tasks: vec![TaskResult {
                    took_ms: None,
                    attempts: None,
                    task: "preview".into(),
                    hash: request.revision.to_string(),
                    outcome: if inner.preview_fails {
                        Outcome::Fail
                    } else {
                        Outcome::Pass
                    },
                    source: Source::Ran {
                        host: inner.host.clone(),
                    },
                }],
                executor: Enforced {
                    kind: "open".into(),
                    confined: false,
                    processors: None,
                    memory: None,
                    network: None,
                    docker: false,
                    grant: grant.clone(),
                },
                missing: Vec::new(),
                not_run: Vec::new(),
                dependencies: Vec::new(),
                failed_by: Vec::new(),
                timed_out: false,
                refusals_unread: None,
            };
        }
        let tree = inner.commits[&request.revision].tree.clone();
        // A session's or a preview's command (session-runs-1 clause 89,
        // previews-1 clause 98): its one check is the command, which fails
        // where a rule names it.
        if let Some(command) = request.command.clone() {
            let outcome = match inner
                .fail_rules
                .iter()
                .any(|(n, f)| *n == command && f(&tree))
            {
                true => Outcome::Fail,
                false => Outcome::Pass,
            };
            return RunReport {
                detail: None,
                kept_out: Vec::new(),
                tasks: vec![TaskResult {
                    took_ms: None,
                    attempts: None,
                    task: command,
                    hash: request.revision.to_string(),
                    outcome,
                    source: Source::Ran {
                        host: inner.host.clone(),
                    },
                }],
                executor: Enforced {
                    kind: "open".into(),
                    confined: false,
                    processors: None,
                    memory: None,
                    network: None,
                    docker: request.limits.docker,
                    grant: grant.clone(),
                },
                missing: Vec::new(),
                not_run: Vec::new(),
                dependencies: Vec::new(),
                failed_by: Vec::new(),
                timed_out: false,
                refusals_unread: None,
            };
        }
        let since = request
            .since
            .as_ref()
            .map(|r| inner.commits[r].tree.clone());
        let wanted: Vec<TaskDef> = inner
            .tasks
            .iter()
            .filter(|t| {
                request.required.iter().any(|q| match q.strip_prefix('#') {
                    Some(tag) => t.tags.contains(tag),
                    None => *q == t.name,
                })
            })
            .cloned()
            .collect();
        let mut report = RunReport {
            detail: None,
            kept_out: Vec::new(),
            tasks: Vec::new(),
            // What the run was held to: the request's container engine, as
            // an executor that holds it reports it (83, 84).
            executor: Enforced {
                kind: "open".into(),
                confined: false,
                processors: None,
                memory: None,
                network: None,
                docker: request.limits.docker,
                grant,
            },
            missing: Vec::new(),
            not_run: Vec::new(),
            dependencies: Vec::new(),
            failed_by: Vec::new(),
            timed_out: false,
            refusals_unread: None,
        };
        let names: Vec<String> = wanted.iter().map(|t| t.name.clone()).collect();
        let mut seen = BTreeMap::new();
        for t in wanted {
            if let Some(old) = &since {
                let prefixes = Self::read_prefixes(inner, &t);
                let changed = tree
                    .keys()
                    .chain(old.keys())
                    .filter(|p| prefixes.iter().any(|x| p.starts_with(x)))
                    .any(|p| tree.get(p) != old.get(p));
                if !changed {
                    continue;
                }
            }
            if inner.skip.contains(&t.name) {
                report.missing.push(t.name.clone());
                continue;
            }
            if inner.kept_out.contains(&t.name) {
                report.missing.push(t.name.clone());
                report.kept_out.push(t.name.clone());
                continue;
            }
            if let Some(reason) = inner.cannot_run.get(&t.name) {
                report.not_run.push(NotRun {
                    task: t.name.clone(),
                    reason: reason.clone(),
                    key: None,
                });
                continue;
            }
            if inner.hang.contains(&t.name) && request.limit.is_some() {
                report.missing.push(t.name.clone());
                report.timed_out = true;
                break;
            }
            // 149: what it depends on runs first, and a failure among
            // them skips it: failed by those that ran and failed, or else
            // not run where one could not run.
            let mut by = Vec::new();
            let mut could = None;
            for d in Self::depends_closure(inner, &t.name) {
                match Self::observe(inner, request, &tree, &d, &mut seen) {
                    Seen::Ran(r) if r.outcome == Outcome::Fail => by.push(r),
                    Seen::NotRun(why) if could.is_none() => could = Some((d, why)),
                    _ => {}
                }
            }
            if !by.is_empty() {
                report.failed_by.push(sluice_seams::FailedBy {
                    task: t.name.clone(),
                    by: by.iter().map(|r| r.task.clone()).collect(),
                });
                for r in by {
                    if !names.contains(&r.task)
                        && !report.dependencies.iter().any(|x| x.task == r.task)
                    {
                        report.dependencies.push(r);
                    }
                }
                continue;
            }
            if let Some((d, why)) = could {
                report.not_run.push(NotRun {
                    task: t.name.clone(),
                    reason: format!("its dependency {d} could not run: {why}"),
                    key: None,
                });
                continue;
            }
            match Self::observe(inner, request, &tree, &t.name, &mut seen) {
                Seen::Ran(r) => report.tasks.push(r),
                Seen::NotRun(reason) => report.not_run.push(NotRun {
                    task: t.name.clone(),
                    reason,
                    key: None,
                }),
                Seen::Skipped => report.missing.push(t.name.clone()),
            }
        }
        let total: u64 = report.tasks.iter().filter_map(|t| t.took_ms).sum();
        if total > 0 {
            report.detail = Some(sluice_seams::RunDetail {
                took_ms: Some(total),
                estimate_ms: Some(total * 2),
                changed: Vec::new(),
            });
        }
        report
    }

    /// 149: every task `task` depends on, at any depth, each once,
    /// sorted.
    fn depends_closure(inner: &Inner, task: &str) -> Vec<String> {
        let mut out = BTreeSet::new();
        let mut next = vec![task.to_string()];
        while let Some(t) = next.pop() {
            for d in inner.depends.get(&t).into_iter().flatten() {
                if d != task && out.insert(d.clone()) {
                    next.push(d.clone());
                }
            }
        }
        out.into_iter().collect()
    }

    /// 149: the paths a check reads: its own and those of every task it
    /// depends on.
    fn read_prefixes(inner: &Inner, t: &TaskDef) -> Vec<String> {
        let deps = Self::depends_closure(inner, &t.name);
        std::iter::once(t.prefix.clone())
            .chain(
                inner
                    .tasks
                    .iter()
                    .filter(|d| deps.contains(&d.name))
                    .map(|d| d.prefix.clone()),
            )
            .collect()
    }

    /// 149: what a run makes of the task `name`, once a run however many
    /// checks depend on it: skipped where a task it depends on did not
    /// pass, not run where it cannot run, and otherwise run, or replayed
    /// where the run may and a pass of its key is kept.
    fn observe(
        inner: &mut Inner,
        request: &RunRequest,
        tree: &Tree,
        name: &str,
        seen: &mut BTreeMap<String, Seen>,
    ) -> Seen {
        if let Some(s) = seen.get(name) {
            return s.clone();
        }
        // A cycle ends here, as the runner refuses one.
        seen.insert(name.into(), Seen::Skipped);
        let deps = inner.depends.get(name).cloned().unwrap_or_default();
        let mut passed = true;
        for d in &deps {
            let s = Self::observe(inner, request, tree, d, seen);
            passed &= matches!(s, Seen::Ran(ref r) if r.outcome == Outcome::Pass);
        }
        let s = if !passed {
            Seen::Skipped
        } else if let Some(why) = inner.cannot_run.get(name) {
            Seen::NotRun(why.clone())
        } else {
            Seen::Ran(Self::run_task(inner, request, tree, name))
        };
        seen.insert(name.into(), s.clone());
        s
    }

    /// The task `name` run on `tree`: replayed where the run may and a pass
    /// of its key is kept, else its `flaky` script's next outcome, else a
    /// fail where a rule names it, else a pass.
    fn run_task(inner: &mut Inner, request: &RunRequest, tree: &Tree, name: &str) -> TaskResult {
        let t = inner
            .tasks
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("{name} is no task the world defines"))
            .clone();
        let hash = Self::task_hash(&t, tree);
        let key = (t.name.clone(), hash.clone());
        let scripted = inner.flaky.get_mut(&t.name).and_then(|q| q.pop_front());
        if scripted.is_none() && request.replay {
            if let Some(at) = inner.cache.get(&key) {
                return TaskResult {
                    took_ms: None,
                    attempts: None,
                    task: t.name.clone(),
                    hash,
                    outcome: Outcome::Pass,
                    source: Source::Replayed {
                        from_run_at: Some(at.clone()),
                    },
                };
            }
        }
        let outcome = match scripted {
            Some(o) => o,
            None if inner
                .fail_rules
                .iter()
                .any(|(n, f)| *n == t.name && f(tree)) =>
            {
                Outcome::Fail
            }
            None => Outcome::Pass,
        };
        // Like a content-keyed cache: only a pass is kept, and nothing by
        // a run that keeps no proof, whose store holds neither (record
        // 0105).
        if outcome == Outcome::Pass && inner.no_proof.is_none() {
            let at = inner.now.to_rfc3339();
            inner.cache.insert(key, at);
        }
        inner.observed.push((t.name.clone(), hash.clone(), outcome));
        let took = inner.took.get(&t.name).copied();
        TaskResult {
            took_ms: took.map(|t| t.0),
            attempts: took.map(|t| t.1),
            task: t.name.clone(),
            hash,
            outcome,
            source: Source::Ran {
                host: inner.host.clone(),
            },
        }
    }

    // -- the place ---------------------------------------------------------

    pub fn live_places(&self) -> Vec<String> {
        self.0.borrow().places.keys().cloned().collect()
    }

    pub fn made_places(&self) -> Vec<String> {
        self.0.borrow().made.clone()
    }

    /// Each session place released, with the `[tests] release` text of the
    /// policy it was released under, or `None` for its last run's.
    pub fn session_releases(&self) -> Vec<(String, Option<String>)> {
        self.0.borrow().session_releases.clone()
    }

    /// Session places adopted and not yet released.
    pub fn adopted_places(&self) -> Vec<String> {
        self.0.borrow().adopted.clone()
    }

    pub fn released_places(&self) -> Vec<String> {
        self.0.borrow().released.clone()
    }

    /// The next release of a candidate's place fails and keeps the place,
    /// as a release command that fails does.
    pub fn fail_next_release(&self) {
        self.0.borrow_mut().fail_release = true;
    }

    /// A kept log's text, by where the records seam said it is (20).
    pub fn kept_log(&self, at: &str) -> Option<String> {
        self.0.borrow().logs.get(at).cloned()
    }

    /// Previews made and not yet released, as the place seam lists them.
    pub fn live_previews(&self) -> Vec<PreviewPlace> {
        self.0.borrow().previews.values().cloned().collect()
    }

    /// Each preview released, and whether it was kept as failed.
    pub fn preview_releases(&self) -> Vec<(String, bool)> {
        self.0.borrow().preview_releases.clone()
    }

    /// Preview commands started from now exit non-zero, or zero again.
    pub fn preview_fails(&self, fails: bool) {
        self.0.borrow_mut().preview_fails = fails;
    }

    /// A preview's checkout is removed by hand, with nothing released.
    pub fn remove_preview_checkout(&self, name: &str) {
        if let Some(p) = self.0.borrow_mut().previews.get_mut(name) {
            p.gone = true;
        }
    }

    /// Every release from now fails, saying `message`; `None` lets them
    /// succeed again.
    pub fn release_fails(&self, message: Option<&str>) {
        self.0.borrow_mut().release_fails = message.map(str::to_string);
    }

    /// Every removal of scratch no record names fails from now, saying
    /// `message`; `None` lets it succeed again.
    pub fn unnamed_scratch_fails(&self, message: Option<&str>) {
        self.0.borrow_mut().unnamed_scratch_fails = message.map(str::to_string);
    }

    /// Each removal of scratch no record names asked, with how many places
    /// were held then.
    pub fn unnamed_scratch_asks(&self) -> Vec<usize> {
        self.0.borrow().unnamed_scratch_asks.clone()
    }

    /// The release of the place `name` fails from now, saying `message`;
    /// `None` lets it succeed again.
    pub fn place_release_fails(&self, name: &str, message: Option<&str>) {
        let mut inner = self.0.borrow_mut();
        match message {
            Some(m) => inner.place_release_fails.insert(name.into(), m.into()),
            None => inner.place_release_fails.remove(name),
        };
    }

    /// A candidate's place's checkout is deleted by hand, with nothing
    /// released: its record stays.
    pub fn delete_checkout(&self, name: &str) {
        self.0.borrow_mut().deleted.insert(name.into());
    }

    /// Every release asked of the place seam, by place, in order.
    pub fn release_attempts(&self) -> Vec<String> {
        self.0.borrow().release_attempts.clone()
    }

    /// The places freed with nothing run, in order.
    pub fn freed_places(&self) -> Vec<String> {
        self.0.borrow().freed.clone()
    }

    /// Each place whose working copy was gone and whose stored release
    /// ran, in order: a release command run from what sluice held (123).
    pub fn gone_releases(&self) -> Vec<String> {
        self.0.borrow().gone_releases.clone()
    }

    /// The runs of the place `name` stored no release command.
    pub fn stores_no_release(&self, name: &str) {
        self.0.borrow_mut().stored_none.insert(name.into());
    }

    // -- the driver --------------------------------------------------------

    /// A session's place holding `head`.
    pub fn session(&self, worktree: &str, agent: &str, head: &Rev) {
        self.0.borrow_mut().worktrees.insert(
            worktree.into(),
            Session {
                head: head.clone(),
                from: Submitter {
                    agent: agent.into(),
                    worktree: worktree.into(),
                    path: None,
                    head: None,
                    host: None,
                    session: None,
                },
                dirty: Vec::new(),
                detached: false,
            },
        );
    }

    /// 124: from now, a submit by name runs in a session whose harness
    /// names `agent`, as `SLUICE_AGENT` does; `None` names the branch.
    pub fn harness_names(&self, agent: Option<&str>) {
        self.0.borrow_mut().harness_agent = agent.map(str::to_string);
    }

    /// A worktree has `branch` checked out, at the branch's tip; `None`
    /// detaches it.
    pub fn check_out(&self, worktree: &str, branch: Option<&str>) {
        let mut inner = self.0.borrow_mut();
        match branch {
            Some(b) => {
                let head = inner.refs.get(b).cloned().expect("a branch");
                inner.checked_out.insert(worktree.into(), (b.into(), head));
            }
            None => {
                inner.checked_out.remove(worktree);
            }
        }
    }

    /// How long the results log was when the session place `name`'s last
    /// result was kept.
    pub fn results_when_ran(&self, name: &str) -> Option<usize> {
        self.0.borrow().results_when_ran.get(name).copied()
    }

    /// The clock moves on by `by`.
    pub fn advance(&self, by: Span) {
        self.0.borrow_mut().now += by;
    }

    /// The session's worktree is removed, as its driver or a person
    /// removes it, with nothing released first.
    pub fn remove_worktree(&self, worktree: &str) {
        self.0.borrow_mut().worktrees.remove(worktree);
    }

    /// The session's place holds `paths` beyond its commits.
    pub fn dirty(&self, worktree: &str, paths: &[&str]) {
        self.0
            .borrow_mut()
            .worktrees
            .get_mut(worktree)
            .expect("a session's place")
            .dirty = paths.iter().map(|p| p.to_string()).collect();
    }

    /// The session's place is at `path`, the path its submissions name.
    pub fn at_path(&self, worktree: &str, path: &str) {
        self.0
            .borrow_mut()
            .worktrees
            .get_mut(worktree)
            .expect("a session's place")
            .from
            .path = Some(path.into());
    }

    /// The session's place at `worktree` is a pull request opened from
    /// `head`, as the pull-request driver records it: no path of its own.
    pub fn opened_from(&self, worktree: &str, head: &str) {
        self.0
            .borrow_mut()
            .worktrees
            .get_mut(worktree)
            .expect("a session's place")
            .from
            .head = Some(head.into());
    }

    /// The session's place records `session`, the name its driver gives
    /// the session that submits from it (51).
    pub fn in_session(&self, worktree: &str, session: &str) {
        self.0
            .borrow_mut()
            .worktrees
            .get_mut(worktree)
            .expect("a session's place")
            .from
            .session = Some(session.into());
    }

    /// 51: `agent`'s sessions take no outcome from now: each notify is
    /// answered undelivered, and kept with [`World::undelivered`].
    pub fn not_delivering(&self, agent: &str) {
        self.0.borrow_mut().undelivering.insert(agent.into());
    }

    /// Every notification not delivered, in order.
    pub fn undelivered(&self) -> Vec<Notification> {
        self.0.borrow().undelivered.clone()
    }

    /// The next notification is delivered and the process stops before
    /// it is confirmed.
    pub fn crash_after_next_notify(&self) {
        self.0.borrow_mut().crash_notify = true;
    }

    /// Every notification delivered, in order.
    pub fn delivered(&self) -> Vec<Notification> {
        self.0.borrow().delivered.clone()
    }

    /// The notifications delivered to `agent`.
    pub fn delivered_to(&self, agent: &str) -> Vec<Notification> {
        self.delivered()
            .into_iter()
            .filter(|n| n.to.agent == agent)
            .collect()
    }

    // -- the records -------------------------------------------------------

    /// The next records write fails as if the process stopped.
    pub fn crash_next_write(&self) {
        self.0.borrow_mut().crash_writes += 1;
    }

    /// Another writer writes the records just before the next write.
    pub fn interfere_next_write(&self) {
        self.0.borrow_mut().interfere += 1;
    }

    /// Another process does `then` with the world each time an id is
    /// allocated, before the allocating call goes on: a round run between
    /// a submit's numbering and its write.
    pub fn after_allocating(&self, then: impl Fn(&World, u64) + 'static) {
        self.0.borrow_mut().after_allocating = Some(Rc::new(then));
    }

    /// The next number the records give is `n`: a branch whose records are
    /// on its remote numbers from the remote's count and one in the host's
    /// own files from the host's, so two branches may hold one number (26;
    /// record 0114).
    pub fn numbers_from(&self, n: u64) {
        self.0.borrow_mut().next_id = n;
    }

    /// A branch's record file as written, in the home this host's reads
    /// of it are routed to (record 0122; design D2).
    pub fn file(&self, branch: &str, path: &str) -> Option<String> {
        self.files(branch)
            .remove(path)
            .map(|b| String::from_utf8(b).expect("records are text"))
    }

    /// Every file of a branch's records, as written, in the home this
    /// host's reads of it are routed to.
    pub fn files(&self, branch: &str) -> BTreeMap<String, Vec<u8>> {
        let inner = self.0.borrow();
        let Ok(home) = World::route(&inner, branch) else {
            return BTreeMap::new();
        };
        inner
            .lines
            .get(&(home, branch.to_string()))
            .map(|l| l.files.clone())
            .unwrap_or_default()
    }

    pub fn version(&self, branch: &str) -> u64 {
        let inner = self.0.borrow();
        World::route(&inner, branch)
            .ok()
            .and_then(|home| inner.lines.get(&(home, branch.to_string())))
            .map_or(0, |l| l.version)
    }

    /// Record 0122 (design D1): a branch's record file as written in the
    /// home `home`, `here` for this host's own files or a remote's name.
    pub fn file_in(&self, home: &str, branch: &str, path: &str) -> Option<String> {
        let inner = self.0.borrow();
        let key = (World::home_key(&inner, home), branch.to_string());
        inner
            .lines
            .get(&key)
            .and_then(|l| l.files.get(path))
            .map(|b| String::from_utf8(b.clone()).expect("records are text"))
    }

    /// Record 0122 (design D1): `branch`'s home document in `home`, as
    /// written; `None` where that home holds none.
    pub fn home_in(&self, home: &str, branch: &str) -> Option<HomeDoc> {
        let text = self.file_in(home, branch, HOME_FILE)?;
        Some(serde_json::from_str(&text).expect("a home document reads"))
    }

    /// Record 0122 (design D1): put `doc` as `branch`'s home document in
    /// `home`, as a host that wrote it would have, moving no version.
    pub fn put_home(&self, home: &str, branch: &str, doc: &HomeDoc) {
        let mut inner = self.0.borrow_mut();
        let key = (World::home_key(&inner, home), branch.to_string());
        let line = inner.lines.entry(key).or_default();
        line.files
            .insert(HOME_FILE.into(), sluice_core::records::to_document(doc));
    }

    /// Record 0122 (design D3): how many carries wrote anything.
    pub fn carries(&self) -> usize {
        self.0.borrow().carries
    }

    /// Record 0122 (59): the next carry stops after its target's write,
    /// before it closes the home it carried from, as a process stopped
    /// there would.
    pub fn stop_next_carry(&self) {
        self.0.borrow_mut().stop_carry = true;
    }

    /// 51, record 0122 (design D4): the session in `worktree` submits as
    /// made on `host`, as a session's driver names the host it ran on.
    pub fn from_host(&self, worktree: &str, host: &str) {
        let mut inner = self.0.borrow_mut();
        let session = inner
            .worktrees
            .get_mut(worktree)
            .unwrap_or_else(|| panic!("no session in {worktree}"));
        session.from.host = Some(host.into());
    }

    /// Design D2: the home document the first write into the home keyed
    /// `key` writes, where it holds none and the write puts none: open,
    /// naming the home and the branch's head, whose policy chose it.
    fn opened_home(inner: &Inner, key: &str, branch: &str, changes: &Changes) -> Option<Vec<u8>> {
        let holds = inner
            .lines
            .get(&(key.to_string(), branch.to_string()))
            .is_some_and(|l| l.files.contains_key(HOME_FILE));
        if holds || changes.put.contains_key(HOME_FILE) {
            return None;
        }
        let at = match key.starts_with("here@") {
            true => HERE,
            false => key,
        };
        let doc = HomeDoc::open(at, inner.refs.get(branch).cloned());
        Some(sluice_core::records::to_document(&doc))
    }

    /// The key of the home `home` names, as this host reads it.
    fn home_key(inner: &Inner, home: &str) -> String {
        match home {
            HERE => own_home(&inner.host),
            remote => remote.to_string(),
        }
    }

    /// `branch`'s home document in the home keyed `key`, as written.
    fn home_at(inner: &Inner, key: &str, branch: &str) -> Option<HomeDoc> {
        let bytes = inner
            .lines
            .get(&(key.to_string(), branch.to_string()))?
            .files
            .get(HOME_FILE)?;
        serde_json::from_slice(bytes).ok()
    }

    /// Record 0122 (design D2): the home `branch`'s records are read and
    /// written in, by this host: its own `home.json`, open or moving, then
    /// closed to the home its pointer names; where it has none, a remote's
    /// that is open or moving; where none holds a home, the policy at the
    /// branch's head, a remote it names with signers, its own files
    /// otherwise. A remote home closed toward the hosts' own files routes
    /// to this host's own. This host's own home open while a remote's that
    /// came from this host's own files is live refuses the branch, naming
    /// both (26).
    fn route(inner: &Inner, branch: &str) -> Result<String> {
        let own = own_home(&inner.host);
        let remotes: BTreeSet<String> = inner
            .lines
            .keys()
            .filter(|(h, b)| b == branch && !h.starts_with("here@"))
            .map(|(h, _)| h.clone())
            .collect();
        let remote_live = |r: &str| World::home_at(inner, r, branch).filter(HomeDoc::is_live);
        if let Some(home) = World::home_at(inner, &own, branch) {
            match home.state {
                HomeState::Open => {
                    for r in &remotes {
                        let split = remote_live(r).is_some_and(|h| {
                            h.came_from.as_ref().is_some_and(|c| {
                                c.home == HERE && c.host.as_deref() == Some(inner.host.as_str())
                            })
                        });
                        if split {
                            return Err(sluice_core::home::two_homes(branch, HERE, r));
                        }
                    }
                    return Ok(own);
                }
                HomeState::Moving => return Ok(own),
                HomeState::Closed => {
                    let to = home.closed_to().unwrap_or(HERE).to_string();
                    if to == HERE {
                        return Ok(own);
                    }
                    return Ok(match World::home_at(inner, &to, branch) {
                        Some(r) if r.closed_to() == Some(HERE) => own,
                        _ => to,
                    });
                }
            }
        }
        for r in &remotes {
            match World::home_at(inner, r, branch) {
                Some(h) if h.is_live() => return Ok(r.clone()),
                Some(h) if h.closed_to() == Some(HERE) => return Ok(own),
                _ => {}
            }
        }
        Ok(World::named_at_head(inner, branch).unwrap_or(own))
    }

    /// Design D2: the remote the policy at `branch`'s head keeps its
    /// records on, signers and a `remote` named for it; `None` for the
    /// host's own files, or where no policy there reads.
    fn named_at_head(inner: &Inner, branch: &str) -> Option<String> {
        let tip = inner.refs.get(branch)?;
        let text = inner.commits.get(tip)?.tree.get(POLICY_PATH)?;
        let policy = sluice_core::policy::Policy::parse(text).ok()?;
        if policy.signers.is_empty() {
            return None;
        }
        policy.remote_of(branch).ok().flatten()
    }

    pub fn results_log(&self) -> String {
        String::from_utf8(self.0.borrow().results.clone()).expect("records are text")
    }

    /// A session run on `worktree`, running or ended with its record
    /// left.
    pub fn session_run(&self, worktree: &str, running: bool) {
        let run = HostRun {
            worktree: worktree.into(),
            branch: String::new(),
            started: String::new(),
            command: None,
            tasks: Vec::new(),
        };
        self.0
            .borrow_mut()
            .sessions
            .insert(worktree.into(), (running, run));
    }

    /// A session run started on the host, as `sluice exec` records it.
    pub fn begin_host_run(&self, run: HostRun) {
        self.0
            .borrow_mut()
            .sessions
            .insert(run.worktree.clone(), (true, run));
    }

    /// `worktree`'s session run's tasks, as its `sluice exec` keeps them.
    pub fn session_tasks(&self, worktree: &str, tasks: &[TaskProgress]) {
        if let Some((_, r)) = self.0.borrow_mut().sessions.get_mut(worktree) {
            r.tasks = tasks.to_vec();
        }
    }

    /// `worktree`'s session run has ended; its record is left behind.
    pub fn kill_session(&self, worktree: &str) {
        if let Some((r, _)) = self.0.borrow_mut().sessions.get_mut(worktree) {
            *r = false;
        }
    }

    pub fn hold_lock(&self, branch: &str, held: bool) {
        let mut inner = self.0.borrow_mut();
        match held {
            true => inner.locks.insert(branch.into()),
            false => inner.locks.remove(branch),
        };
    }
}

// ---------------------------------------------------------------------------
// The five seams
// ---------------------------------------------------------------------------

pub struct FakeBranch(World);
pub struct FakeTests(World);
pub struct FakeCheckout(World);
pub struct FakeDriver(World);
pub struct FakeState(World);
pub struct FakeClock(World);

impl Branch for FakeBranch {
    /// A branch kept on a remote is fetched: this host holds it as read
    /// from then (129).
    fn read(&self, branch: &str) -> Result<Option<Rev>> {
        let mut inner = self.0 .0.borrow_mut();
        inner.revision_reads += 1;
        let key = (inner.host.clone(), branch.to_string());
        let on_remote = inner.remote.contains_key(branch);
        if on_remote {
            inner.fetches += 1;
        }
        if let Some(said) = inner.refused_branch.get(&key) {
            return Err(World::refused(format!("refs/heads/{branch}"), said));
        }
        let at = inner.refs.get(branch).cloned();
        if on_remote {
            inner.fetched.insert(key, at.clone());
        }
        Ok(at)
    }

    /// 129: a branch kept on a remote as this host last fetched it, with
    /// no fetch; any other as [`FakeBranch::read`] reads it.
    fn held(&self, branch: &str) -> Result<Option<Rev>> {
        let mut inner = self.0 .0.borrow_mut();
        inner.revision_reads += 1;
        let Some(start) = inner.remote.get(branch) else {
            return Ok(inner.refs.get(branch).cloned());
        };
        let key = (inner.host.clone(), branch.to_string());
        Ok(inner.fetched.get(&key).unwrap_or(start).clone())
    }

    /// A revision of the world the host was not made to lack.
    fn holds(&self, rev: &Rev) -> Result<bool> {
        let inner = self.0 .0.borrow();
        Ok(inner.commits.contains_key(rev) && !inner.lacking.contains(rev))
    }

    fn copy(&self, branch: &str) -> Result<HostCopy> {
        Ok(match World::copy_here(&self.0 .0.borrow(), branch) {
            Some(at) => HostCopy::At(at),
            None => HostCopy::Itself,
        })
    }

    fn level(&self, branch: &str, expected: Option<&Rev>, to: &Rev) -> Result<Moved> {
        let mut inner = self.0 .0.borrow_mut();
        let Some(found) = World::copy_here(&inner, branch) else {
            bail!("{branch} is kept in this host's own refs, which have no copy of it to level");
        };
        if let Some((n, why)) = inner.level_fails.take() {
            if n > 1 {
                inner.level_fails = Some((n - 1, why.clone()));
            }
            if n > 0 {
                bail!("{why}");
            }
        }
        if found.as_ref() != expected {
            return Ok(Moved::Elsewhere(found));
        }
        let key = (inner.host.clone(), branch.to_string());
        inner.copies.insert(key, Some(to.clone()));
        inner.levels += 1;
        Ok(Moved::Moved)
    }

    fn read_file(&self, rev: &Rev, path: &str) -> Result<Option<Vec<u8>>> {
        *self
            .0
             .0
            .borrow_mut()
            .file_reads
            .entry(path.to_string())
            .or_default() += 1;
        let inner = self.0 .0.borrow();
        let c = inner
            .commits
            .get(rev)
            .ok_or_else(|| anyhow!("no revision {rev}"))?;
        Ok(c.tree.get(path).map(|s| s.as_bytes().to_vec()))
    }

    /// A three-way merge per path, on the merge base of the two.
    fn form(&self, onto: &Rev, head: &Rev) -> Result<Formed> {
        let mut inner = self.0 .0.borrow_mut();
        inner.forms += 1;
        let base = World::merge_base(&inner, onto, head);
        let b = base
            .as_ref()
            .map(|r| inner.commits[r].tree.clone())
            .unwrap_or_default();
        let ours = inner.commits[onto].tree.clone();
        let theirs = inner.commits[head].tree.clone();
        let mut tree = ours.clone();
        let mut conflicts = Vec::new();
        let paths: BTreeSet<&String> = b.keys().chain(theirs.keys()).collect();
        for p in paths {
            if b.get(p) == theirs.get(p) {
                continue;
            }
            if ours.get(p) == b.get(p) || ours.get(p) == theirs.get(p) {
                match theirs.get(p) {
                    Some(c) => tree.insert(p.clone(), c.clone()),
                    None => tree.remove(p),
                };
            } else {
                conflicts.push(p.clone());
            }
        }
        if !conflicts.is_empty() {
            return Ok(Formed::Conflict { paths: conflicts });
        }
        let rev = World::put_commit(
            &mut inner,
            tree,
            vec![onto.clone(), head.clone()],
            "candidate",
        );
        Ok(Formed::Revision(rev))
    }

    fn commit(&self, c: &LandingCommit) -> Result<Rev> {
        let mut inner = self.0 .0.borrow_mut();
        let tree = inner.commits[&c.tree_of].tree.clone();
        Ok(World::put_commit(
            &mut inner,
            tree,
            vec![c.parent.clone(), c.head.clone()],
            &c.message,
        ))
    }

    fn contains(&self, tip: &Rev, rev: &Rev) -> Result<bool> {
        Ok(World::ancestors(&self.0 .0.borrow(), tip).contains(rev))
    }

    /// 122: down `tip`'s first parents to the oldest that holds `rev`.
    fn landed_by(&self, tip: &Rev, rev: &Rev) -> Result<Option<Rev>> {
        let inner = self.0 .0.borrow();
        if !World::ancestors(&inner, tip).contains(rev) {
            return Ok(None);
        }
        let mut at = tip.clone();
        while at != *rev {
            match inner.commits[&at].parents.first() {
                Some(p) if World::ancestors(&inner, p).contains(rev) => at = p.clone(),
                _ => break,
            }
        }
        Ok(Some(at))
    }

    fn move_to(&self, branch: &str, expected: &Rev, to: &Rev) -> Result<Moved> {
        let mut inner = self.0 .0.borrow_mut();
        if inner.crash == Some(Crash::BeforeMove) {
            inner.crash = None;
            bail!("crashed before moving {branch}");
        }
        let found = inner.refs.get(branch).cloned();
        if found.as_ref() != Some(expected) {
            return Ok(Moved::Elsewhere(found));
        }
        inner.refs.insert(branch.into(), to.clone());
        inner.moves.push(MoveRecord {
            branch: branch.into(),
            from: expected.clone(),
            to: to.clone(),
        });
        if inner.crash == Some(Crash::AfterMove) {
            inner.crash = None;
            bail!("crashed after moving {branch}");
        }
        Ok(Moved::Moved)
    }

    fn make(&self, branch: &str, at: &Rev) -> Result<Moved> {
        let mut inner = self.0 .0.borrow_mut();
        if let Some(found) = inner.refs.get(branch) {
            return Ok(Moved::Elsewhere(Some(found.clone())));
        }
        inner.refs.insert(branch.into(), at.clone());
        Ok(Moved::Moved)
    }

    fn remove(&self, branch: &str, expected: &Rev) -> Result<Moved> {
        let mut inner = self.0 .0.borrow_mut();
        let found = inner.refs.get(branch).cloned();
        if found.as_ref() != Some(expected) {
            return Ok(Moved::Elsewhere(found));
        }
        inner.refs.remove(branch);
        Ok(Moved::Moved)
    }
}

impl FakeTests {
    /// 149: `task` depends on `dependency`, a task the world defines
    /// (`World::task`), which a run runs first, as moon does: under its own
    /// key, fail rules, `flaky` script and `cannot_run`. Where it ran and
    /// failed, the run skips `task`, which is failed by it, and reports the
    /// dependency's result in `dependencies` unless it is itself a check
    /// of the run; where it could not run, `task` is not run, saying so in
    /// the dependency's words. Its log is its own, as any task's is. A
    /// check reads the paths of the tasks it depends on as its own.
    pub fn depends_on(&self, task: &str, dependency: &str) {
        let mut inner = self.0 .0.borrow_mut();
        let deps = inner.depends.entry(task.into()).or_default();
        if !deps.iter().any(|d| d == dependency) {
            deps.push(dependency.into());
        }
    }

    /// 147: every equip the check seam was asked, in order.
    pub fn equips(&self) -> Vec<Equip> {
        self.0 .0.borrow().equips.clone()
    }

    /// 147: each equip from now answers `steps`, as they ran; none by
    /// default.
    pub fn equip_answers(&self, steps: &[EquipStep]) {
        self.0 .0.borrow_mut().equip_steps = steps.to_vec();
    }

    /// The host's count: the submissions' runs it started that still go,
    /// other repositories' runs on it, and the session runs.
    fn count(inner: &Inner) -> Running {
        let own = inner
            .runs
            .iter()
            .filter(|r| r.live() && r.queue() && r.host == inner.host)
            .count();
        let parts = |kind| {
            inner
                .parts
                .get(&(inner.host.clone(), kind))
                .copied()
                .unwrap_or(0)
        };
        Running {
            queue: own
                + inner.other_runs.get(&inner.host).copied().unwrap_or(0)
                + parts(RunKind::Queue),
            sessions: inner.sessions.values().filter(|(r, _)| *r).count() + parts(RunKind::Session),
        }
    }
}

/// A part of the fakes' host's bound (71; record 0087): counted while it
/// is held, and given back when it is dropped unstarted. A start that
/// takes it moves its room to the run it starts.
struct FakePart {
    world: std::rc::Weak<RefCell<Inner>>,
    host: String,
    kind: RunKind,
    /// Taken by a start, which counted it out itself.
    spent: bool,
}

impl FakePart {
    fn give_back(inner: &mut Inner, host: &str, kind: RunKind) {
        if let Some(n) = inner.parts.get_mut(&(host.to_string(), kind)) {
            *n = n.saturating_sub(1);
        }
    }
}

impl Drop for FakePart {
    fn drop(&mut self) {
        if self.spent {
            return;
        }
        if let Some(world) = self.world.upgrade() {
            if let Ok(mut inner) = world.try_borrow_mut() {
                FakePart::give_back(&mut inner, &self.host, self.kind);
            }
        }
    }
}

impl Tests for FakeTests {
    fn start(&self, request: &RunRequest, part: Part) -> Result<RunHandle> {
        // The part's room moves to the run, or back to the host where the
        // start fails: either way it is held no longer.
        let mut part = part.take::<FakePart>().ok();
        let mut inner = self.0 .0.borrow_mut();
        if let Some(p) = part.as_mut() {
            FakePart::give_back(&mut inner, &p.host, p.kind);
            p.spent = true;
        }
        let held = inner
            .places
            .get(&request.place.name)
            .ok_or_else(|| anyhow!("no place {}", request.place.name))?;
        // 35: the place holds exactly the revision the run is for.
        if *held != request.revision {
            bail!(
                "place {} holds {held} not {}",
                request.place.name,
                request.revision
            );
        }
        // 84: the fake is an open executor, which holds no memory or
        // network limit.
        if let Some(k) = request.limits.executor.as_deref().filter(|k| *k != "open") {
            bail!("the fake executor is open, not {k}");
        }
        if request.limits.memory.is_some() && !inner.holds_memory.contains(&inner.host) {
            bail!("the open executor cannot hold a memory limit");
        }
        if matches!(
            request.limits.network,
            Some(Network::Denied | Network::Localhost)
        ) {
            bail!("the open executor cannot keep a run off the network");
        }
        // 71: its room was held by its part; a part the host no longer
        // honours is refused before anything of the run runs.
        if std::mem::take(&mut inner.refuse_start) {
            return Err(AtBound.into());
        }
        if std::mem::take(&mut inner.crash_start) {
            bail!("crashed before starting a run in {}", request.place.name);
        }
        if let Some(words) = &inner.start_fails {
            bail!("{words}");
        }
        if inner.unanswered_starts > 0 {
            inner.unanswered_starts -= 1;
            return Err(Unanswered(format!(
                "starting a run in {}: 503 Service Unavailable",
                request.place.name
            ))
            .into());
        }
        let complete = !inner.hold;
        let host = inner.host.clone();
        let nix_system = inner.nix_systems.get(&host).cloned();
        let live = inner.places.keys().cloned().collect();
        inner.live_at_start.push((request.place.name.clone(), live));
        inner.runs.push(RunState {
            request: request.clone(),
            host,
            nix_system,
            complete,
            lost: false,
            killed: false,
            report: None,
        });
        Ok(RunHandle(format!("run-{}", inner.runs.len() - 1)))
    }

    fn progress(&self, run: &RunHandle) -> Result<Progress> {
        let mut inner = self.0 .0.borrow_mut();
        let n: usize = run
            .0
            .strip_prefix("run-")
            .and_then(|n| n.parse().ok())
            .ok_or_else(|| anyhow!("no run {}", run.0))?;
        let r = inner
            .runs
            .get(n)
            .ok_or_else(|| anyhow!("no run {}", run.0))?
            .clone();
        if r.lost || r.killed {
            return Ok(Progress::Lost);
        }
        if let Some(report) = r.report {
            return Ok(Progress::Finished(report));
        }
        if !r.complete {
            return Ok(Progress::Running);
        }
        let report = World::evaluate(&mut inner, &r.request);
        inner.runs[n].report = Some(report.clone());
        Ok(Progress::Finished(report))
    }

    fn unmet(&self, limits: &sluice_seams::Limits) -> Result<Option<String>> {
        let mut inner = self.0 .0.borrow_mut();
        let host = inner.host.clone();
        // Design D2: a limit the executor cannot hold, before any engine.
        if limits.memory.is_some() {
            if let Some(why) = inner.memory_unmet.get(&host) {
                return Ok(Some(why.clone()));
            }
        }
        if !limits.docker {
            return Ok(None);
        }
        inner.looks.push(host.clone());
        Ok(inner.unmet.get(&host).cloned())
    }

    /// 84, 114, record 0097 (design D4, D5): as the check profile reads
    /// the link Nix keeps, the system the run noted as it started against
    /// the one its host runs now, in the profile's words.
    fn switched(&self, run: &RunHandle) -> Result<Option<String>> {
        let mut inner = self.0 .0.borrow_mut();
        let host = inner.host.clone();
        inner.system_looks.push(host.clone());
        let began = run
            .0
            .strip_prefix("run-")
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|n| inner.runs.get(n))
            .ok_or_else(|| anyhow!("no run {}", run.0))?
            .nix_system
            .clone();
        Ok(match (began, inner.nix_systems.get(&host)) {
            (Some(from), Some(to)) if from != *to => Some(format!(
                "the host's system was switched during the run, from {from} to {to}"
            )),
            _ => None,
        })
    }

    /// 139 (design D9): what the host's executor reports refusing the run,
    /// as the world says, each ask counted.
    fn refused(&self, _: &RunHandle) -> Result<sluice_seams::Refusals> {
        let mut inner = self.0 .0.borrow_mut();
        let host = inner.host.clone();
        inner.refusal_looks.push(host.clone());
        Ok(
            match (inner.refusals_unread.get(&host), inner.refuses.get(&host)) {
                (Some(why), _) => sluice_seams::Refusals::Unread(why.clone()),
                (None, Some(words)) => sluice_seams::Refusals::Refused(words.clone()),
                (None, None) => sluice_seams::Refusals::None,
            },
        )
    }

    fn bound(&self) -> Result<usize> {
        let inner = self.0 .0.borrow();
        if inner.full.contains(&inner.host) {
            return Ok(0);
        }
        Ok(inner.host_limit.unwrap_or(usize::MAX))
    }

    /// 71 (record 0087): a part while the parts held and the runs going
    /// on the host, other repositories' and sessions' among them, are
    /// under its bound.
    fn hold(&self, kind: RunKind) -> Result<Option<Part>> {
        let mut inner = self.0 .0.borrow_mut();
        if std::mem::take(&mut inner.refuse_hold) {
            return Ok(None);
        }
        let running = FakeTests::count(&inner);
        let bound = match inner.full.contains(&inner.host) {
            true => 0,
            false => inner.host_limit.unwrap_or(usize::MAX),
        };
        if running.total() >= bound {
            return Ok(None);
        }
        let host = inner.host.clone();
        *inner.parts.entry((host.clone(), kind)).or_default() += 1;
        inner.holds.push(kind);
        Ok(Some(Part::new(
            kind,
            FakePart {
                world: Rc::downgrade(&self.0 .0),
                host,
                kind,
                spent: false,
            },
        )))
    }

    fn launch(&self, launch: &sluice_seams::Launch) -> Result<()> {
        let mut inner = self.0 .0.borrow_mut();
        let path = format!("dispatching {} for {}", launch.pool, launch.claim);
        match inner.launch_fails.take() {
            Some(false) => bail!("{path}: 404 Not Found"),
            Some(true) => {
                inner.launches.push(launch.clone());
                Err(Unanswered(format!("{path}: its answer was not read: 502 Bad Gateway")).into())
            }
            None => {
                inner.launches.push(launch.clone());
                Ok(())
            }
        }
    }

    fn job_alive(
        &self,
        _kind: &(String, sluice_seams::Setting),
        claim: &str,
        _remote: Option<&str>,
    ) -> Result<bool> {
        Ok(!self.0 .0.borrow().jobs_ended.contains(claim))
    }

    fn wake(
        &self,
        _kind: &(String, sluice_seams::Setting),
        branch: &str,
        _remote: Option<&str>,
    ) -> Result<()> {
        self.0 .0.borrow_mut().wakes.push(branch.into());
        Ok(())
    }

    fn platform(&self) -> Result<sluice_seams::Platform> {
        let inner = self.0 .0.borrow();
        Ok(sluice_seams::Platform {
            os: inner.platform.0.clone(),
            arch: inner.platform.1.clone(),
        })
    }

    fn running(&self) -> Result<Running> {
        Ok(FakeTests::count(&self.0 .0.borrow()))
    }

    fn session_runs(&self) -> Result<Vec<HostRun>> {
        Ok(self
            .0
             .0
            .borrow()
            .sessions
            .values()
            .filter(|(r, _)| *r)
            .map(|(_, run)| run.clone())
            .collect())
    }

    fn proven(&self, _run: &RunHandle, report: &RunReport) -> Result<Proof> {
        let inner = self.0 .0.borrow();
        let unkept = |t: &TaskResult| {
            inner
                .no_proof
                .as_ref()
                .filter(|_| matches!(t.source, Source::Ran { .. }) && t.outcome == Outcome::Pass)
        };
        // Design D6 of `a-signed-run-without-a-sandbox-proves-its-passes`
        // (C.6, 20): a forged key is refused with the profile's words.
        let refuse = |t: &TaskResult| sluice_seams::Refused {
            task: t.task.clone(),
            key: t.hash.clone(),
            why: format!(
                "{}: its record names an input the tree does not hold",
                t.task
            ),
        };
        let mut proof = Proof::default();
        for t in &report.tasks {
            if inner.forged.contains(&t.task) {
                proof.refused.push(refuse(t));
                continue;
            }
            match unkept(t) {
                Some(store) => proof.unkept.push(NotRun {
                    task: t.task.clone(),
                    reason: format!(
                        "it passed, but nothing the run kept proves it ran: \
                         its record is not in {store}"
                    ),
                    key: Some(t.hash.clone()),
                }),
                None => proof.proven.push(t.task.clone()),
            }
        }
        for t in &report.dependencies {
            if inner.forged.contains(&t.task) {
                proof.refused.push(refuse(t));
            } else {
                proof.proven.push(t.task.clone());
            }
        }
        Ok(proof)
    }

    fn logs(&self, run: &RunHandle, tasks: &[String]) -> Result<Vec<TaskLog>> {
        Ok(tasks
            .iter()
            .map(|t| TaskLog {
                task: t.clone(),
                text: format!("{t} failed in {}\n", run.0),
            })
            .collect())
    }

    fn tasks(&self, run: &RunHandle) -> Result<Vec<TaskProgress>> {
        let inner = self.0 .0.borrow();
        if inner.tasks_fail {
            bail!("reading {}'s tasks failed", run.0);
        }
        let n: usize = run
            .0
            .strip_prefix("run-")
            .and_then(|n| n.parse().ok())
            .ok_or_else(|| anyhow!("no run {}", run.0))?;
        Ok(inner.run_tasks.get(&n).cloned().unwrap_or_default())
    }

    fn kept_out(
        &self,
        worktree: &Path,
        _tests: &Settings,
        required: &[String],
    ) -> Result<Vec<KeptOutTask>> {
        let mut inner = self.0 .0.borrow_mut();
        inner.asked_kept_out.push(worktree.to_path_buf());
        if let Some(why) = &inner.kept_out_fails {
            bail!("{why}");
        }
        // The entry naming the task itself, else the first tag it carries.
        Ok(inner
            .tasks
            .iter()
            .filter(|t| inner.kept_out.contains(&t.name))
            .filter_map(|t| {
                let entry = required.iter().find(|q| **q == t.name).or_else(|| {
                    required
                        .iter()
                        .find(|q| q.strip_prefix('#').is_some_and(|tag| t.tags.contains(tag)))
                })?;
                Some(KeptOutTask {
                    task: t.name.clone(),
                    entry: entry.clone(),
                })
            })
            .collect())
    }

    /// 147: the steps [`FakeTests::equip_answers`] set, the equip kept
    /// for [`FakeTests::equips`]; it takes no place and counts nothing.
    fn equip(&self, equip: &Equip) -> Result<Equipped> {
        let mut inner = self.0 .0.borrow_mut();
        inner.equips.push(equip.clone());
        Ok(Equipped {
            steps: inner.equip_steps.clone(),
        })
    }

    /// 140: each pair's required checks whose prefix holds a path the
    /// change alters, as a run's narrowing reads them.
    fn reads(&self, reading: &Reading) -> Result<Vec<Reads>> {
        let mut inner = self.0 .0.borrow_mut();
        inner.read_pairs.extend(reading.pairs.iter().cloned());
        if let Some(why) = &inner.reads_fail {
            bail!("{why}");
        }
        if inner.reads_every {
            return Ok(vec![Reads::Every; reading.pairs.len()]);
        }
        reading
            .pairs
            .iter()
            .map(|(from, to)| {
                let tree = |r: &Rev| {
                    inner
                        .commits
                        .get(r)
                        .map(|c| c.tree.clone())
                        .ok_or_else(|| anyhow!("no commit {r}"))
                };
                let (old, new) = (tree(from)?, tree(to)?);
                let read = inner
                    .tasks
                    .iter()
                    .filter(|t| {
                        reading.required.iter().any(|q| match q.strip_prefix('#') {
                            Some(tag) => t.tags.contains(tag),
                            None => *q == t.name,
                        })
                    })
                    .filter(|t| {
                        // A project's `reads` file, one prefix a line, names
                        // more paths its check reads in that tree, as a
                        // submission that widens a check's inputs does; and
                        // a check reads what the tasks it depends on read
                        // (149).
                        let more = new
                            .get(&format!("{}reads", t.prefix))
                            .map(|r| r.lines().map(String::from).collect::<Vec<_>>())
                            .unwrap_or_default();
                        let prefixes = World::read_prefixes(&inner, t);
                        new.keys()
                            .chain(old.keys())
                            .filter(|p| {
                                prefixes.iter().any(|x| p.starts_with(x))
                                    || more.iter().any(|m| p.starts_with(m))
                            })
                            .any(|p| new.get(p) != old.get(p))
                    })
                    .map(|t| t.name.clone())
                    .collect();
                Ok(Reads::Checks(read))
            })
            .collect()
    }
}

impl FakeCheckout {
    /// A place's removal, with everything started in it ended.
    fn drop_place(&self, name: &str) {
        self.drop_place_as(name, false);
    }

    /// A place's removal, released or, when `freed`, freed with nothing
    /// run.
    fn drop_place_as(&self, name: &str, freed: bool) {
        let mut inner = self.0 .0.borrow_mut();
        inner.adopted.retain(|a| a != name);
        inner.adopted_for.remove(name);
        inner.adopted_at.remove(name);
        inner.last_results.remove(name);
        inner.session_release_failed.remove(name);
        inner.deleted.remove(name);
        if inner.places.remove(name).is_some() {
            if freed {
                inner.freed.push(name.into());
            } else {
                inner.released.push(name.into());
            }
            // 34: everything started in it is ended.
            for r in inner.runs.iter_mut() {
                if r.request.place.name == name && r.report.is_none() {
                    r.killed = true;
                }
            }
        }
    }

    /// A release asked of the place `name`: counted, and refused with
    /// what the world says its release fails with.
    fn attempt(&self, name: &str) -> Result<()> {
        let mut inner = self.0 .0.borrow_mut();
        inner.release_attempts.push(name.into());
        match inner
            .place_release_fails
            .get(name)
            .or(inner.release_fails.as_ref())
        {
            Some(m) => bail!("the release command of {name} ended exit status: 2: {m}"),
            None => Ok(()),
        }
    }

    /// When the world's clock says it is.
    fn now(&self) -> String {
        self.0 .0.borrow().now.to_rfc3339()
    }
}

/// `failure` with one more try, failing now with `e`, held when `stop`.
fn failed_again(
    failure: Option<&ReleaseFailure>,
    e: &anyhow::Error,
    at: String,
    stop: impl Fn(u32) -> bool,
) -> ReleaseFailure {
    let tries = failure.map_or(0, |f| f.tries) + 1;
    ReleaseFailure {
        failed: format!("{e:#}"),
        tries,
        at,
        stopped: failure.is_some_and(|f| f.stopped) || stop(tries),
    }
}

/// Each `{name.<port>}` and `{port.<port>}` of `template` filled in.
fn fake_fill(
    label: &str,
    template: &str,
    names: &BTreeMap<String, String>,
    ports: &BTreeMap<String, String>,
) -> Result<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let close = rest[i..]
            .find('}')
            .ok_or_else(|| anyhow!("{label} leaves a {{ open"))?;
        let inner = &rest[i + 1..i + close];
        let value = match inner.split_once('.') {
            Some(("name", p)) => names.get(p),
            Some(("port", p)) => ports.get(p),
            _ => None,
        };
        let Some(value) = value else {
            bail!("[preview] addresses {label} names {{{inner}}}, a port [checkout] ports does not list");
        };
        out.push_str(value);
        rest = &rest[i + close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

impl Checkout for FakeCheckout {
    fn materialize(&self, name: &str, rev: &Rev, _: &Settings) -> Result<Place> {
        let mut inner = self.0 .0.borrow_mut();
        if let Some(words) = &inner.checkout_fails {
            bail!("{words}");
        }
        // 32: two places on one host never collide.
        if inner.places.contains_key(name) || inner.worktrees.contains_key(name) {
            bail!("place {name} already exists");
        }
        if !inner.commits.contains_key(rev) {
            bail!("no revision {rev}");
        }
        // git's worktree, at a revision whose objects the host lacks.
        if inner.lacking.contains(rev) {
            bail!("git worktree add /fake/places/{name}: fatal: invalid reference: {rev}");
        }
        inner.places.insert(name.into(), rev.clone());
        inner.made.push(name.into());
        let host = inner.host.clone();
        inner.place_host.insert(name.into(), host);
        Ok(Place {
            name: name.into(),
            path: format!("/fake/places/{name}").into(),
            tmp: format!("/fake/tmp/{name}").into(),
            env: Default::default(),
        })
    }

    fn adopt(&self, worktree: &str, branch: &str, _: &Settings, at: &str) -> Result<Place> {
        let mut inner = self.0 .0.borrow_mut();
        let head = inner
            .worktrees
            .get(worktree)
            .ok_or_else(|| anyhow!("no worktree {worktree}"))?
            .head
            .clone();
        let name = format!("session/{worktree}");
        // The place holds what the worktree holds, for the run's check.
        inner.places.insert(name.clone(), head);
        if !inner.adopted.contains(&name) {
            inner.adopted.push(name.clone());
            inner.adopted_for.insert(name.clone(), branch.into());
        }
        inner.adopted_at.insert(name.clone(), at.into());
        // 97: a run under way has no result.
        inner.last_results.remove(&name);
        Ok(Place {
            name: name.clone(),
            path: worktree.into(),
            tmp: format!("/fake/tmp/{name}").into(),
            env: Default::default(),
        })
    }

    fn adopted(&self, worktree: &str) -> Result<Option<Adopted>> {
        let name = format!("session/{worktree}");
        let inner = self.0 .0.borrow();
        Ok(inner.adopted.contains(&name).then(|| Adopted {
            branch: inner.adopted_for.get(&name).cloned().unwrap_or_default(),
            name,
        }))
    }

    fn release_session(&self, name: &str, policy: Option<&ReleasePolicy>) -> Result<Released> {
        let release = policy.and_then(|p| match p.tests.get("release") {
            Some(sluice_seams::Setting::Text(t)) => Some(t.clone()),
            _ => None,
        });
        if !self.0 .0.borrow().adopted.iter().any(|a| a == name) {
            // Record 0120: a worktree of the world's it holds nothing for
            // is released by the policy's release command all the same,
            // and nothing is kept for it, passed or failed.
            let worktree = name.strip_prefix("session/").unwrap_or(name);
            let Some(release) =
                release.filter(|_| self.0 .0.borrow().worktrees.contains_key(worktree))
            else {
                return Ok(Released::Nothing);
            };
            self.attempt(name)?;
            self.0
                 .0
                .borrow_mut()
                .session_releases
                .push((name.into(), Some(release)));
            return Ok(Released::Policy);
        }
        if let Err(e) = self.attempt(name) {
            // 123: kept, held: no round releases a session's place.
            let at = self.now();
            let mut inner = self.0 .0.borrow_mut();
            let failure = failed_again(inner.session_release_failed.get(name), &e, at, |_| true);
            inner.session_release_failed.insert(name.into(), failure);
            return Err(e);
        }
        self.drop_place(name);
        self.0
             .0
            .borrow_mut()
            .session_releases
            .push((name.into(), release));
        Ok(if policy.is_some() {
            Released::Policy
        } else {
            Released::LastRun
        })
    }

    fn session_places(&self) -> Result<Vec<SessionPlace>> {
        let inner = self.0 .0.borrow();
        Ok(inner
            .adopted
            .iter()
            .map(|name| {
                let worktree = name.strip_prefix("session/").unwrap_or(name);
                SessionPlace {
                    name: name.clone(),
                    worktree: worktree.into(),
                    branch: inner.adopted_for.get(name).cloned().unwrap_or_default(),
                    ports: BTreeMap::new(),
                    last_run: inner.adopted_at.get(name).cloned(),
                    gone: !inner.worktrees.contains_key(worktree),
                    last_result: inner.last_results.get(name).cloned(),
                    release_failed: inner.session_release_failed.get(name).cloned(),
                }
            })
            .collect())
    }

    fn ran(&self, name: &str, last: &LastResult) -> Result<()> {
        let mut inner = self.0 .0.borrow_mut();
        if inner.keeping_fails {
            bail!("the place's record could not be written");
        }
        if !inner.adopted.iter().any(|a| a == name) {
            if inner.places.contains_key(name) {
                bail!("place {name} is no session's");
            }
            // Released meanwhile: nothing to keep it on (95).
            return Ok(());
        }
        inner.last_results.insert(name.into(), last.clone());
        let len = inner.results.len();
        inner.results_when_ran.insert(name.into(), len);
        Ok(())
    }

    fn checked_out(&self) -> Result<Vec<CheckedOut>> {
        let inner = self.0 .0.borrow();
        Ok(inner
            .checked_out
            .iter()
            .map(|(w, (branch, head))| CheckedOut {
                worktree: w.into(),
                branch: branch.clone(),
                head: head.clone(),
            })
            .collect())
    }

    fn made(&self) -> Result<Vec<String>> {
        let inner = self.0 .0.borrow();
        Ok(inner
            .places
            .keys()
            .filter(|p| inner.place_host.get(*p) == Some(&inner.host))
            .cloned()
            .collect())
    }

    fn release(&self, name: &str) -> Result<()> {
        // Rule core.16: the core releases only places it made, never a
        // session's; a session's place goes only through `release_session`.
        assert!(
            !self.0 .0.borrow().worktrees.contains_key(name) && !name.starts_with("session/"),
            "sluice released a session's place {name}"
        );
        World::probe(&mut self.0 .0.borrow_mut());
        if self.0 .0.borrow().places.contains_key(name) {
            self.attempt(name)?;
        }
        if std::mem::take(&mut self.0 .0.borrow_mut().fail_release) {
            bail!("the release command of {name} failed");
        }
        self.drop_place(name);
        Ok(())
    }

    fn gone(&self, name: &str) -> Result<bool> {
        let inner = self.0 .0.borrow();
        if let Some(p) = inner.previews.get(name) {
            return Ok(p.gone);
        }
        if let Some(worktree) = name.strip_prefix("session/") {
            return Ok(
                inner.adopted.iter().any(|a| a == name) && !inner.worktrees.contains_key(worktree)
            );
        }
        Ok(inner.places.contains_key(name) && inner.deleted.contains(name))
    }

    fn free(&self, name: &str) -> Result<()> {
        // Rule core.16: the worktree of a session's place is left as it is.
        self.drop_place_as(name, true);
        let mut inner = self.0 .0.borrow_mut();
        inner.previews.remove(name);
        inner.stored_none.remove(name);
        Ok(())
    }

    fn release_gone(&self, name: &str) -> Result<GoneRelease> {
        let mut gone = GoneRelease::default();
        {
            let mut inner = self.0 .0.borrow_mut();
            let held = inner.places.contains_key(name)
                || inner.adopted.iter().any(|a| a == name)
                || inner.previews.contains_key(name);
            if held && !inner.stored_none.contains(name) {
                // 123: the release its runs stored, once, failing as the
                // world says its release fails.
                inner.gone_releases.push(name.into());
                gone.ran = true;
                gone.failed = inner
                    .place_release_fails
                    .get(name)
                    .or(inner.release_fails.as_ref())
                    .map(|m| format!("the release command of {name} ended exit status: 2: {m}"));
            }
        }
        self.free(name)?;
        Ok(gone)
    }

    /// The fake keeps no scratch directories: there is none to remove,
    /// unless the world says the removal fails.
    fn remove_unnamed_scratch(&self) -> Result<Vec<PathBuf>> {
        let mut inner = self.0 .0.borrow_mut();
        let held = inner.places.len();
        inner.unnamed_scratch_asks.push(held);
        match &inner.unnamed_scratch_fails {
            Some(m) => bail!("{m}"),
            None => Ok(Vec::new()),
        }
    }

    fn preview(
        &self,
        name: &str,
        rev: &Rev,
        branch: &str,
        checkout: &Settings,
        addresses: &BTreeMap<String, String>,
        at: &str,
    ) -> Result<(Place, PreviewPlace)> {
        let mut inner = self.0 .0.borrow_mut();
        let place = |p: &PreviewPlace| Place {
            name: p.name.clone(),
            path: p.path.clone(),
            tmp: format!("/fake/tmp/{}", p.name).into(),
            env: p.ports.clone().into_iter().chain(p.names.clone()).collect(),
        };
        if let Some(p) = inner.previews.get(name) {
            return Ok((place(p), p.clone()));
        }
        if !inner.commits.contains_key(rev) {
            bail!("no revision {rev}");
        }
        let ports: Vec<String> = match checkout.get("ports") {
            Some(sluice_seams::Setting::List(xs)) => xs.clone(),
            _ => Vec::new(),
        };
        // Two previews never share a port or a name.
        let block = inner.previews.len() + inner.preview_releases.len();
        let (mut names, mut numbers) = (BTreeMap::new(), BTreeMap::new());
        for (i, p) in ports.iter().enumerate() {
            names.insert(p.clone(), format!("{p}-{}--{branch}", rev.short()));
            numbers.insert(p.clone(), (30000 + block * 10 + i).to_string());
        }
        let mut filled = BTreeMap::new();
        for (label, template) in addresses {
            filled.insert(label.clone(), fake_fill(label, template, &names, &numbers)?);
        }
        let upper = |prefix: &str, m: &BTreeMap<String, String>| -> BTreeMap<String, String> {
            m.iter()
                .map(|(k, v)| (format!("{prefix}{}", k.to_ascii_uppercase()), v.clone()))
                .collect()
        };
        let preview = PreviewPlace {
            name: name.into(),
            branch: branch.into(),
            revision: rev.clone(),
            path: format!("/fake/previews/{name}").into(),
            ports: upper("SLUICE_PORT_", &numbers),
            names: upper("SLUICE_NAME_", &names),
            addresses: filled,
            made: at.into(),
            run: None,
            gone: false,
            failed: None,
            release_failed: None,
        };
        inner.places.insert(name.into(), rev.clone());
        inner.made.push(name.into());
        inner.previews.insert(name.into(), preview.clone());
        Ok((place(&preview), preview))
    }

    fn preview_run(&self, name: &str, run: &RunHandle) -> Result<()> {
        let mut inner = self.0 .0.borrow_mut();
        let p = inner
            .previews
            .get_mut(name)
            .ok_or_else(|| anyhow!("no preview {name}"))?;
        p.run = Some(run.clone());
        Ok(())
    }

    fn previews(&self) -> Result<Vec<PreviewPlace>> {
        Ok(self.0 .0.borrow().previews.values().cloned().collect())
    }

    fn release_preview(&self, name: &str, failed: Option<&str>, retries: u32) -> Result<bool> {
        let Some(p) = self.0 .0.borrow().previews.get(name).cloned() else {
            return Ok(false);
        };
        if let Err(e) = self.attempt(name) {
            // 123: kept with what failed, held once its tries are spent.
            let failure = failed_again(p.release_failed.as_ref(), &e, self.now(), |t| t > retries);
            if let Some(q) = self.0 .0.borrow_mut().previews.get_mut(name) {
                q.release_failed = Some(failure);
            }
            return Err(e);
        }
        self.drop_place(name);
        let mut inner = self.0 .0.borrow_mut();
        inner.preview_releases.push((name.into(), failed.is_some()));
        match failed {
            Some(output) if p.failed.is_none() => {
                inner.previews.insert(
                    name.into(),
                    PreviewPlace {
                        ports: BTreeMap::new(),
                        run: None,
                        gone: false,
                        failed: Some(output.into()),
                        release_failed: None,
                        ..p
                    },
                );
            }
            _ => {
                inner.previews.remove(name);
            }
        }
        Ok(true)
    }
}

impl Driver for FakeDriver {
    fn session(&self, worktree: &str) -> Result<Session> {
        self.0
             .0
            .borrow()
            .worktrees
            .get(worktree)
            .cloned()
            .ok_or_else(|| anyhow!("no place {worktree}"))
    }

    /// 124: the agent the harness names, else the branch; the branch as
    /// its work and its head; this world's host; no path and no session,
    /// whatever places the world holds (record 0090).
    fn named(&self, branch: &str) -> Result<Submitter> {
        let inner = self.0 .0.borrow();
        Ok(Submitter {
            agent: inner
                .harness_agent
                .clone()
                .unwrap_or_else(|| branch.to_string()),
            worktree: branch.into(),
            path: None,
            head: Some(branch.into()),
            host: Some(inner.host.clone()),
            session: None,
        })
    }

    /// 51: delivered to the session its submitter recorded, or to its
    /// agent where it recorded none, unless the agent's sessions take
    /// nothing.
    fn notify(&self, notification: &Notification) -> Result<Delivery> {
        let mut inner = self.0 .0.borrow_mut();
        World::probe(&mut inner);
        let to = &notification.to;
        if inner.undelivering.contains(&to.agent) {
            inner.undelivered.push(notification.clone());
            return Ok(Delivery::Undelivered {
                why: format!("{} takes nothing", to.agent),
            });
        }
        inner.delivered.push(notification.clone());
        if std::mem::take(&mut inner.crash_notify) {
            bail!("crashed after delivering {}", notification.id);
        }
        Ok(Delivery::Handed {
            way: "notice".into(),
            to: to.session.clone().unwrap_or_else(|| to.agent.clone()),
            confirms: false,
        })
    }
}

struct FakeGuard<'a> {
    world: &'a World,
    branch: String,
}

impl Guard for FakeGuard<'_> {}

impl Drop for FakeGuard<'_> {
    fn drop(&mut self) {
        self.world.hold_lock(&self.branch, false);
    }
}

/// Record 0122 (design D3): one home of a branch in the fakes' records,
/// keyed as [`World::route`] keys it, for the core's carry.
struct FakeHome<'a> {
    world: &'a World,
    key: String,
    branch: String,
}

impl HomeStore for FakeHome<'_> {
    fn name(&self) -> String {
        match self.key.starts_with("here@") {
            true => HERE.into(),
            false => self.key.clone(),
        }
    }

    fn read(&self) -> Result<Snapshot> {
        let inner = self.world.0.borrow();
        Ok(
            match inner.lines.get(&(self.key.clone(), self.branch.clone())) {
                Some(l) => Snapshot {
                    version: l.version,
                    files: l
                        .files
                        .iter()
                        .filter(|(p, _)| *p != "version" && finished_name(p).is_none())
                        .map(|(p, b)| (p.clone(), b.clone()))
                        .collect(),
                },
                None => Snapshot::default(),
            },
        )
    }

    fn read_finished(&self, name: &str) -> Result<Option<Vec<u8>>> {
        let inner = self.world.0.borrow();
        Ok(inner
            .lines
            .get(&(self.key.clone(), self.branch.clone()))
            .and_then(|l| {
                l.files
                    .get(&format!("{}/{name}", sluice_seams::FINISHED))
                    .cloned()
            }))
    }

    fn write(&self, read_at: u64, changes: Changes) -> Result<Written> {
        let mut inner = self.world.0.borrow_mut();
        let line = inner
            .lines
            .entry((self.key.clone(), self.branch.clone()))
            .or_default();
        if line.version != read_at {
            return Ok(Written::Refused);
        }
        for (path, bytes) in changes.put {
            line.files.insert(path, bytes);
        }
        for (path, bytes) in changes.append {
            line.files.entry(path).or_default().extend(bytes);
        }
        for path in changes.remove {
            line.files.remove(&path);
        }
        line.version += 1;
        let v = line.version;
        line.files
            .insert("version".into(), format!("{v}\n").into_bytes());
        Ok(Written::Written(v))
    }

    /// The fakes number every home from one count.
    fn raise_past(&self, number: u64) -> Result<()> {
        let mut inner = self.world.0.borrow_mut();
        inner.next_id = inner.next_id.max(number + 1);
        Ok(())
    }

    fn next_number(&self) -> Result<u64> {
        let mut inner = self.world.0.borrow_mut();
        let id = inner.next_id;
        inner.next_id += 1;
        Ok(id)
    }
}

/// The name of `path` under the finished records, if it is one of them.
fn finished_name(path: &str) -> Option<&str> {
    path.strip_prefix(sluice_seams::FINISHED)?.strip_prefix('/')
}

impl State for FakeState {
    fn beat(&self, host: &str, beat: &[u8]) -> Result<()> {
        let mut inner = self.0 .0.borrow_mut();
        let n = inner.beats.get(host).map_or(0, |(n, _)| n + 1);
        inner.beats.insert(host.into(), (n, beat.to_vec()));
        Ok(())
    }

    fn beats(&self) -> Result<BTreeMap<String, (String, Vec<u8>)>> {
        Ok(self
            .0
             .0
            .borrow()
            .beats
            .iter()
            .map(|(h, (n, b))| (h.clone(), (n.to_string(), b.clone())))
            .collect())
    }

    fn observed_beat(&self, host: &str) -> Result<Option<(String, String)>> {
        let inner = self.0 .0.borrow();
        Ok(inner
            .seen_beats
            .get(&(inner.host.clone(), host.to_string()))
            .cloned())
    }

    fn observe_beat(&self, host: &str, marker: &str, at: &str) -> Result<()> {
        let mut inner = self.0 .0.borrow_mut();
        let key = (inner.host.clone(), host.to_string());
        inner.seen_beats.insert(key, (marker.into(), at.into()));
        Ok(())
    }

    fn observed(&self, branch: &str) -> Result<Option<(u64, String)>> {
        let inner = self.0 .0.borrow();
        Ok(inner
            .seen
            .get(&(inner.host.clone(), branch.to_string()))
            .cloned())
    }

    fn observe(&self, branch: &str, version: u64, at: &str) -> Result<()> {
        let mut inner = self.0 .0.borrow_mut();
        let key = (inner.host.clone(), branch.to_string());
        inner.seen.insert(key, (version, at.to_string()));
        Ok(())
    }

    fn lock<'a>(&'a self, branch: &str) -> Result<Option<Box<dyn Guard + 'a>>> {
        let mut inner = self.0 .0.borrow_mut();
        World::route(&inner, branch)?;
        if !inner.locks.insert(branch.into()) {
            return Ok(None);
        }
        Ok(Some(Box::new(FakeGuard {
            world: &self.0,
            branch: branch.into(),
        })))
    }

    fn wait_lock<'a>(&'a self, branch: &str) -> Result<Box<dyn Guard + 'a>> {
        // One thread: a held lock would never be let go.
        self.lock(branch)?
            .ok_or_else(|| anyhow!("{branch}'s lock is held, and nothing here lets it go"))
    }

    /// The fakes' records are the host's own: nothing to ask.
    fn refresh(&self) -> Result<()> {
        Ok(())
    }

    /// As the host holds them: no read afresh is counted (design D16).
    fn read_held(&self, branch: &str) -> Result<Snapshot> {
        let read = self.read(branch);
        let inner = self.0 .0.borrow();
        inner
            .state_reads
            .set(inner.state_reads.get().saturating_sub(1));
        read
    }

    fn read(&self, branch: &str) -> Result<Snapshot> {
        let inner = self.0 .0.borrow();
        inner.state_reads.set(inner.state_reads.get() + 1);
        if let Some(said) = inner.failed_state.get(branch) {
            bail!("{said}");
        }
        let key = (World::route(&inner, branch)?, branch.to_string());
        if let Some(said) = World::state_refused(&inner, branch) {
            // 80: answered from what was last read, when there is any.
            if !inner.lines.get(&key).is_some_and(|l| l.version > 0) {
                return Err(World::refused(format!("refs/sluice/state/{branch}"), &said));
            }
        }
        Ok(match inner.lines.get(&key) {
            Some(l) => Snapshot {
                version: l.version,
                files: l
                    .files
                    .iter()
                    .filter(|(p, _)| *p != "version" && finished_name(p).is_none())
                    .map(|(p, b)| (p.clone(), b.clone()))
                    .collect(),
            },
            None => Snapshot::default(),
        })
    }

    fn read_finished(&self, branch: &str, name: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.read_all_finished(branch)?.remove(name))
    }

    fn read_all_finished(&self, branch: &str) -> Result<BTreeMap<String, Vec<u8>>> {
        let inner = self.0 .0.borrow();
        if let Some(said) = inner.failed_state.get(branch) {
            bail!("{said}");
        }
        let key = (World::route(&inner, branch)?, branch.to_string());
        Ok(inner
            .lines
            .get(&key)
            .map(|l| {
                l.files
                    .iter()
                    .filter_map(|(p, b)| Some((finished_name(p)?.to_string(), b.clone())))
                    .collect()
            })
            .unwrap_or_default())
    }

    fn write(&self, branch: &str, read_at: u64, changes: Changes) -> Result<Written> {
        let mut inner = self.0 .0.borrow_mut();
        if let Some(said) = World::state_refused(&inner, branch) {
            bail!("pushing refs/sluice/state/{branch} to origin failed: {said}");
        }
        if inner.crash_writes > 0 {
            inner.crash_writes -= 1;
            bail!("crashed before writing {branch}'s records");
        }
        let interfere = inner.interfere > 0;
        if interfere {
            inner.interfere -= 1;
        }
        // As the real records: a write needs the branch's lock.
        if !inner.locks.contains(branch) {
            bail!("writing {branch}'s records without holding its lock");
        }
        let home = World::route(&inner, branch)?;
        let opened = World::opened_home(&inner, &home, branch, &changes);
        let line = inner.lines.entry((home, branch.into())).or_default();
        if interfere {
            line.version += 1;
        }
        if line.version != read_at {
            return Ok(Written::Refused);
        }
        // Design D2: the first write into a home that holds no home
        // document writes it, open, naming the home and the revision whose
        // policy chose it.
        if let Some(doc) = opened {
            line.files.insert(HOME_FILE.into(), doc);
        }
        for (path, bytes) in changes.put {
            line.files.insert(path, bytes);
        }
        for (path, bytes) in changes.append {
            line.files.entry(path).or_default().extend(bytes);
        }
        for path in changes.remove {
            line.files.remove(&path);
        }
        line.version += 1;
        let v = line.version;
        line.files
            .insert("version".into(), format!("{v}\n").into_bytes());
        Ok(Written::Written(v))
    }

    fn branches(&self) -> Result<Vec<String>> {
        let inner = self.0 .0.borrow();
        if let Some(said) = &inner.failed_list {
            bail!("{said}");
        }
        let own = own_home(&inner.host);
        let mut all: Vec<String> = inner
            .lines
            .iter()
            .filter(|((h, _), l)| l.version > 0 && (*h == own || !h.starts_with("here@")))
            .map(|((_, b), _)| b.clone())
            .collect();
        all.sort();
        all.dedup();
        Ok(all)
    }

    fn publish_head(&self, _branch: &str, head: &Rev) -> Result<()> {
        self.0 .0.borrow_mut().published.push(head.clone());
        Ok(())
    }

    fn fetch_heads(&self, branch: &str) -> Result<()> {
        match World::state_refused(&self.0 .0.borrow(), branch) {
            Some(said) => Err(World::refused("refs/sluice/heads/*".into(), &said)),
            None => Ok(()),
        }
    }

    /// Every candidate the remote holds of the branch comes to the host.
    fn fetch_candidates(&self, branch: &str) -> Result<()> {
        let mut inner = self.0 .0.borrow_mut();
        if let Some(said) = World::state_refused(&inner, branch) {
            return Err(World::refused(
                format!("refs/sluice/combinations/{branch}/*"),
                &said,
            ));
        }
        let fetched: Vec<Rev> = inner
            .candidates
            .iter()
            .filter(|((b, _), _)| b == branch)
            .map(|(_, rev)| rev.clone())
            .collect();
        for rev in fetched {
            inner.lacking.remove(&rev);
        }
        Ok(())
    }

    fn publish_candidate(&self, branch: &str, id: &str, rev: &Rev) -> Result<()> {
        self.0
             .0
            .borrow_mut()
            .candidates
            .insert((branch.into(), id.into()), rev.clone());
        Ok(())
    }

    fn unpublish_candidate(&self, branch: &str, id: &str) -> Result<()> {
        self.0
             .0
            .borrow_mut()
            .candidates
            .remove(&(branch.to_string(), id.to_string()));
        Ok(())
    }

    /// The candidate comes to the host where the remote holds it.
    fn fetch_candidate(&self, branch: &str, id: &str) -> Result<()> {
        let mut inner = self.0 .0.borrow_mut();
        if let Some(said) = World::state_refused(&inner, branch) {
            return Err(World::refused(
                format!("refs/sluice/combinations/{branch}/{id}"),
                &said,
            ));
        }
        let host = inner.host.clone();
        inner
            .candidate_fetches
            .push((host, branch.into(), id.into()));
        if let Some(rev) = inner.candidates.get(&(branch.to_string(), id.to_string())) {
            let rev = rev.clone();
            inner.lacking.remove(&rev);
        }
        Ok(())
    }

    fn stale(&self, branch: &str) -> Option<ReadRefused> {
        let inner = self.0 .0.borrow();
        let said = World::state_refused(&inner, branch)?;
        Some(ReadRefused {
            remote: "origin".into(),
            asked: format!("refs/sluice/state/{branch}"),
            said,
            note: None,
        })
    }

    fn refusal(&self, branch: &str) -> Result<Option<Vec<u8>>> {
        let inner = self.0 .0.borrow();
        let key = (inner.host.clone(), branch.to_string());
        Ok(inner.refusals.get(&key).cloned())
    }

    fn keep_refusal(&self, branch: &str, doc: Option<&[u8]>) -> Result<()> {
        let mut inner = self.0 .0.borrow_mut();
        let key = (inner.host.clone(), branch.to_string());
        match doc {
            Some(doc) => inner.refusals.insert(key, doc.to_vec()),
            None => inner.refusals.remove(&key),
        };
        Ok(())
    }

    fn intake_refusal(&self, branch: &str) -> Result<Option<Vec<u8>>> {
        let inner = self.0 .0.borrow();
        let key = (inner.host.clone(), branch.to_string());
        Ok(inner.intake_refusals.get(&key).cloned())
    }

    fn keep_intake_refusal(&self, branch: &str, doc: Option<&[u8]>) -> Result<()> {
        let mut inner = self.0 .0.borrow_mut();
        let key = (inner.host.clone(), branch.to_string());
        match doc {
            Some(doc) => inner.intake_refusals.insert(key, doc.to_vec()),
            None => inner.intake_refusals.remove(&key),
        };
        Ok(())
    }

    /// Record 0122 (design D3, D4): as the records binding carries, by the
    /// core's one carry, between this host's own home and the remote's.
    fn carry(&self, branch: &str) -> Result<Carried> {
        let inner = self.0 .0.borrow();
        if !inner.locks.contains(branch) {
            bail!("carrying {branch}'s records without holding its lock");
        }
        let me = inner.host.clone();
        let own = own_home(&me);
        let routed = World::route(&inner, branch)?;
        let home = World::home_at(&inner, &routed, branch);
        let (from, to, owed) = match home.as_ref().and_then(HomeDoc::moving_to) {
            Some(to) => {
                let to = World::home_key(&inner, to);
                let owed = match to == own {
                    true => Owed::Mine,
                    false => Owed::All,
                };
                (routed.clone(), to, owed)
            }
            None => {
                // Not moving: what this host made, left in a remote home
                // closed toward the hosts' own files.
                let remote = inner
                    .lines
                    .keys()
                    .filter(|(h, b)| b == branch && !h.starts_with("here@"))
                    .map(|(h, _)| h.clone())
                    .find(|r| {
                        World::home_at(&inner, r, branch)
                            .is_some_and(|h| h.closed_to() == Some(HERE))
                    });
                match remote {
                    Some(r) if routed == own => (r, own.clone(), Owed::Mine),
                    _ => return Ok(Carried::default()),
                }
            }
        };
        let chosen_at = home
            .as_ref()
            .and_then(|h| h.moving.as_ref())
            .and_then(|m| m.chosen_at.clone());
        let now = inner.now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        drop(inner);
        let source = FakeHome {
            world: &self.0,
            key: from,
            branch: branch.into(),
        };
        let target = FakeHome {
            world: &self.0,
            key: to,
            branch: branch.into(),
        };
        let stop = || -> Result<()> {
            let mut inner = self.0 .0.borrow_mut();
            inner.carries += 1;
            if std::mem::take(&mut inner.stop_carry) {
                bail!("stopped after carrying {branch}'s queue, before closing its home");
            }
            Ok(())
        };
        sluice_core::home::carry(branch, &source, &target, &me, owed, chosen_at, &now, &stop)
    }

    fn closed_homes(&self, branch: &str) -> Result<Vec<String>> {
        let inner = self.0 .0.borrow();
        let own = own_home(&inner.host);
        Ok(inner
            .lines
            .keys()
            .filter(|(h, b)| b == branch && (*h == own || !h.starts_with("here@")))
            .filter(|(h, _)| {
                World::home_at(&inner, h, branch).is_some_and(|d| d.state == HomeState::Closed)
            })
            .map(|(h, _)| match h.starts_with("here@") {
                true => HERE.to_string(),
                false => h.clone(),
            })
            .collect())
    }

    fn read_in(&self, branch: &str, home: &str) -> Result<Snapshot> {
        let key = World::home_key(&self.0 .0.borrow(), home);
        FakeHome {
            world: &self.0,
            key,
            branch: branch.into(),
        }
        .read()
    }

    fn read_finished_in(&self, branch: &str, home: &str, name: &str) -> Result<Option<Vec<u8>>> {
        let key = World::home_key(&self.0 .0.borrow(), home);
        FakeHome {
            world: &self.0,
            key,
            branch: branch.into(),
        }
        .read_finished(name)
    }

    fn allocate_id(&self, _branch: &str) -> Result<u64> {
        let mut inner = self.0 .0.borrow_mut();
        let id = inner.next_id;
        inner.next_id += 1;
        let then = inner.after_allocating.clone();
        drop(inner);
        if let Some(then) = then {
            then(&self.0, id);
        }
        Ok(id)
    }

    /// From their own log, the lines `ask` names (design D17).
    fn results_for(&self, _: &str, ask: &ResultAsk) -> Result<Vec<u8>> {
        Ok(ask.answer(&self.0 .0.borrow().results))
    }

    fn verifies_own(&self, _: &str, _: &Rev) -> Result<bool> {
        let inner = self.0 .0.borrow();
        Ok(!inner.unlisted.contains(&inner.host))
    }

    fn record_results(&self, lines: &[u8]) -> Result<()> {
        let mut inner = self.0 .0.borrow_mut();
        let Some(signer) = inner.signer.clone() else {
            inner.results.extend(lines);
            return Ok(());
        };
        // Signed with a key no policy lists, it verifies for no gate.
        if inner.unlisted.contains(&inner.host) {
            return Ok(());
        }
        // As the remote refs profile reads a run back: signed by the host's
        // signer, a replay as a replay (core.9).
        for mut l in sluice_core::records::parse_lines::<sluice_core::records::ResultLine>(lines)? {
            l.signer = Some(signer.clone());
            inner.results.extend(sluice_core::records::to_line(&l));
        }
        Ok(())
    }

    fn set_asides(&self) -> Result<Vec<u8>> {
        Ok(self.0 .0.borrow().set_asides.clone())
    }

    fn set_aside(&self, line: &[u8]) -> Result<()> {
        self.0 .0.borrow_mut().set_asides.extend(line);
        Ok(())
    }

    fn keep_logs(&self, branch: &str, candidate: &str, logs: &[TaskLog]) -> Result<Vec<TaskLog>> {
        let mut inner = self.0 .0.borrow_mut();
        Ok(logs
            .iter()
            .map(|l| {
                let at = format!("logs/{branch}/{candidate}/{}.log", l.task);
                inner.logs.insert(at.clone(), l.text.clone());
                TaskLog {
                    task: l.task.clone(),
                    text: at,
                }
            })
            .collect())
    }

    fn progress(&self, branch: &str) -> Result<BTreeMap<String, Vec<TaskProgress>>> {
        Ok(self
            .0
             .0
            .borrow()
            .progress
            .get(branch)
            .cloned()
            .unwrap_or_default())
    }

    fn put_progress(
        &self,
        branch: &str,
        candidate: &str,
        tasks: Option<&[TaskProgress]>,
    ) -> Result<()> {
        let mut inner = self.0 .0.borrow_mut();
        // As a round's: kept under the branch's lock (rule core.14).
        if !inner.locks.contains(branch) {
            bail!("keeping {branch}'s tasks without holding its lock");
        }
        let kept = inner.progress.entry(branch.into()).or_default();
        match tasks {
            Some(t) => {
                kept.insert(candidate.into(), t.to_vec());
            }
            None => {
                kept.remove(candidate);
            }
        }
        Ok(())
    }
}

impl Clock for FakeClock {
    fn now(&self) -> DateTime<Utc> {
        let mut inner = self.0 .0.borrow_mut();
        inner.now += Span::seconds(1);
        inner.now
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sluice_seams::Decided;

    const SIGNED: &str = "[tests]\nrunner = \"fake\"\n\n[checkout]\n\n\
        [branches.\"bolt/*\"]\nrequired = [\"#gate\"]\nremote = \"origin\"\n\
        trust = [\"box\"]\n\n[signers.box]\nssh = \"box ssh-ed25519 AAAA\"\n";
    const UNSIGNED: &str = "[tests]\nrunner = \"fake\"\n\n[checkout]\n\n\
        [branches.\"bolt/*\"]\nrequired = [\"#gate\"]\ntrust = [\"open\"]\n";

    /// A submission's document, `id` made on `host` from `worktree`.
    fn submission(id: u64, worktree: &str, host: &str) -> Vec<u8> {
        format!(
            "{{\"schema\":\"sluice/submission@1\",\"id\":{id},\"branch\":\"bolt/x\",\
             \"head\":\"h{worktree}\",\"from\":{{\"agent\":\"a\",\"worktree\":\"{worktree}\",\
             \"host\":\"{host}\"}},\"submitted\":\"t\",\"after\":[],\"priority\":0,\
             \"attempt\":1,\"state\":\"queued\"}}"
        )
        .into_bytes()
    }

    /// Write `files` into `bolt/x`'s records wherever they are routed.
    fn put(f: &Fakes, files: &[(String, Vec<u8>)]) -> Written {
        let _lock = f.state.lock("bolt/x").unwrap().unwrap();
        let read = f.state.read("bolt/x").unwrap();
        let changes = Changes {
            put: files.iter().cloned().collect(),
            ..Changes::default()
        };
        f.state.write("bolt/x", read.version, changes).unwrap()
    }

    fn line(f: &Fakes) -> sluice_core::line::Line {
        sluice_core::line::Line::load("bolt/x", f.state.read("bolt/x").unwrap()).unwrap()
    }

    /// Record 0122 (design D1, D2): a branch with no home anywhere is
    /// routed by the policy at its head, and its first write writes the
    /// home that policy chose; once it has a home, the records are read
    /// where its `home.json` says, this host's own first, a closed one by
    /// its pointer, whatever the policy at the head names; and this host's
    /// own home open beside a remote's that came from it refuses the branch,
    /// naming both.
    #[test]
    fn a_fake_branch_is_read_in_the_home_its_records_name() {
        let f = Fakes::new();
        let made = f.world.init("bolt/x", &[(POLICY_PATH, UNSIGNED)]);
        put(
            &f,
            &[(
                "submissions/1.json".into(),
                submission(1, "unit/a", "fake-host"),
            )],
        );
        let own = f
            .world
            .home_in(HERE, "bolt/x")
            .expect("its first write wrote it");
        assert_eq!((own.at.as_str(), own.state), (HERE, HomeState::Open));
        assert_eq!(own.chosen_at, Some(made));
        assert!(f.world.home_in("origin", "bolt/x").is_none());

        // A policy naming the remote moves no reading.
        f.world
            .hand_commit("bolt/x", &[(POLICY_PATH, Some(SIGNED))]);
        assert!(line(&f).subs.contains_key(&1));
        put(
            &f,
            &[(
                "submissions/2.json".into(),
                submission(2, "unit/b", "fake-host"),
            )],
        );
        assert!(f
            .world
            .file_in("origin", "bolt/x", "submissions/2.json")
            .is_none());
        assert_eq!(line(&f).subs.len(), 2);

        // Closed toward origin, it is read on origin.
        let mut closed = own.clone();
        closed.state = HomeState::Closed;
        closed.closed = Some(sluice_core::records::Closed {
            to: "origin".into(),
            carried: BTreeMap::new(),
        });
        f.world.put_home(HERE, "bolt/x", &closed);
        f.world
            .put_home("origin", "bolt/x", &HomeDoc::open("origin", None));
        assert!(line(&f).subs.is_empty());
        put(
            &f,
            &[(
                "submissions/3.json".into(),
                submission(3, "unit/c", "fake-host"),
            )],
        );
        assert!(f
            .world
            .file_in("origin", "bolt/x", "submissions/3.json")
            .is_some());

        // Another host with no records of it finds it on origin.
        f.world.be_host("other");
        assert_eq!(line(&f).subs.keys().copied().collect::<Vec<_>>(), [3]);
        // Origin closed toward the hosts' own files: each host's own.
        let mut home = HomeDoc::open("origin", None);
        home.state = HomeState::Closed;
        home.closed = Some(sluice_core::records::Closed {
            to: HERE.into(),
            carried: BTreeMap::new(),
        });
        f.world.put_home("origin", "bolt/x", &home);
        assert!(line(&f).subs.is_empty());
        f.world.be_host("fake-host");
        assert_eq!(line(&f).subs.len(), 2, "this host's own home again");

        // Own open, beside a live origin home that came from it.
        let mut split = HomeDoc::open("origin", None);
        split.came_from = Some(sluice_core::records::CameFrom {
            home: HERE.into(),
            host: Some("fake-host".into()),
            carried: BTreeMap::new(),
        });
        f.world.put_home("origin", "bolt/x", &split);
        f.world.put_home(HERE, "bolt/x", &own);
        let refused = f.state.read("bolt/x").unwrap_err().to_string();
        assert!(
            refused.contains("this host's own files") && refused.contains("origin"),
            "{refused}"
        );
        assert!(f.state.lock("bolt/x").is_err());

        // A branch with no home anywhere, signers and a remote at its head.
        let f = Fakes::new();
        f.world.init("bolt/x", &[(POLICY_PATH, SIGNED)]);
        put(
            &f,
            &[(
                "submissions/1.json".into(),
                submission(1, "unit/a", "fake-host"),
            )],
        );
        assert_eq!(
            f.world.home_in("origin", "bolt/x").map(|h| h.at),
            Some("origin".to_string())
        );
        assert!(f.world.home_in(HERE, "bolt/x").is_none());
    }

    /// Record 0122 (design D3 to D5): a carry moves a moving home's live
    /// submissions to the home it names under their numbers, the next where
    /// the target holds one, opens it naming where it came from and closes
    /// the old one with its pointer; a carry stopped after its first write
    /// is closed by the next, which carries nothing twice; and a branch
    /// coming home carries only this host's own.
    #[test]
    fn a_fake_carry_moves_the_live_records_once() {
        let f = Fakes::new();
        f.world.init("bolt/x", &[(POLICY_PATH, UNSIGNED)]);
        put(
            &f,
            &[
                (
                    "submissions/52.json".into(),
                    submission(52, "unit/a", "fake-host"),
                ),
                (
                    "submissions/53.json".into(),
                    submission(53, "unit/b", "fake-host"),
                ),
            ],
        );
        // Origin's branch already holds a #53, another host's.
        f.world
            .put_home("origin", "bolt/x", &HomeDoc::open("origin", None));
        {
            let mut inner = f.world.0.borrow_mut();
            let line = inner
                .lines
                .entry(("origin".into(), "bolt/x".into()))
                .or_default();
            line.files.insert(
                "submissions/53.json".into(),
                submission(53, "unit/z", "other"),
            );
            line.version = 1;
            inner.next_id = 60;
        }
        let mut moving = f.world.home_in(HERE, "bolt/x").unwrap();
        moving.state = HomeState::Moving;
        moving.moving = Some(sluice_core::records::Moving {
            to: "origin".into(),
            chosen_at: None,
            since: "t".into(),
        });
        f.world.put_home(HERE, "bolt/x", &moving);
        f.world.stop_next_carry();
        {
            let _lock = f.state.lock("bolt/x").unwrap().unwrap();
            assert!(
                f.state.carry("bolt/x").is_err(),
                "stopped after its first write"
            );
        }
        assert_eq!(f.world.carries(), 1);
        assert_eq!(
            f.world.home_in(HERE, "bolt/x").unwrap().state,
            HomeState::Moving
        );
        let carried = {
            let _lock = f.state.lock("bolt/x").unwrap().unwrap();
            f.state.carry("bolt/x").unwrap()
        };
        assert_eq!(carried.to.as_deref(), Some("origin"));
        assert_eq!(carried.numbers, BTreeMap::from([(52, 52), (53, 60)]));
        let origin: Vec<u64> = line(&f).subs.keys().copied().collect();
        assert_eq!(origin, [52, 53, 60], "each carried once");
        let sixty = line(&f).subs[&60].clone();
        assert_eq!(
            (sixty.was, sixty.carried_from.as_deref()),
            (Some(53), Some(HERE))
        );
        let own = f.world.home_in(HERE, "bolt/x").unwrap();
        assert_eq!(own.closed_to(), Some("origin"));
        assert_eq!(own.closed.unwrap().carried[&53].number, 60);
        assert!(f
            .world
            .file_in(HERE, "bolt/x", "submissions/52.json")
            .is_none());
        // Origin was open: its own home stands as it was.
        assert_eq!(f.world.home_in("origin", "bolt/x").unwrap().came_from, None);
        // A carry made writes nothing more.
        let again = {
            let _lock = f.state.lock("bolt/x").unwrap().unwrap();
            f.state.carry("bolt/x").unwrap()
        };
        assert_eq!(again, Carried::default());
        assert_eq!(f.world.carries(), 2);

        // Coming home: origin moving to the hosts' own files, as its lead
        // writes it; the lead takes its own, another host its own after.
        let mut home = f.world.home_in("origin", "bolt/x").unwrap();
        home.state = HomeState::Moving;
        home.moving = Some(sluice_core::records::Moving {
            to: HERE.into(),
            chosen_at: None,
            since: "t".into(),
        });
        f.world.put_home("origin", "bolt/x", &home);
        f.world.be_host("other");
        let lead = {
            let _lock = f.state.lock("bolt/x").unwrap().unwrap();
            f.state.carry("bolt/x").unwrap()
        };
        assert_eq!(lead.numbers, BTreeMap::from([(53, 53)]));
        let here = f.world.home_in(HERE, "bolt/x").unwrap();
        assert_eq!(here.came_from.unwrap().home, "origin");
        assert_eq!(line(&f).subs.keys().copied().collect::<Vec<_>>(), [53]);
        f.world.be_host("fake-host");
        let mine = {
            let _lock = f.state.lock("bolt/x").unwrap().unwrap();
            f.state.carry("bolt/x").unwrap()
        };
        assert_eq!(mine.numbers, BTreeMap::from([(52, 52), (60, 60)]));
        assert_eq!(line(&f).subs.keys().copied().collect::<Vec<_>>(), [52, 60]);
        let origin = f.world.home_in("origin", "bolt/x").unwrap();
        let carried = origin.closed.unwrap().carried;
        assert_eq!(carried[&53].host.as_deref(), Some("other"));
        assert_eq!(carried[&60].host.as_deref(), Some("fake-host"));
        assert_eq!(f.world.carries(), 4);
    }

    /// Record 0114: the records give the number `numbers_from` sets next,
    /// and count on from it, so a test can make two branches hold one.
    #[test]
    fn numbers_from_gives_the_next_number() {
        let f = Fakes::new();
        assert_eq!(f.state.allocate_id("main").unwrap(), 1);
        assert_eq!(f.state.allocate_id("main").unwrap(), 2);
        f.world.numbers_from(1);
        assert_eq!(f.state.allocate_id("main").unwrap(), 1);
        assert_eq!(f.state.allocate_id("main").unwrap(), 2);
        f.world.numbers_from(26);
        assert_eq!(f.state.allocate_id("main").unwrap(), 26);
    }

    /// 51: the driver fake delivers to the session its submitter recorded,
    /// or its agent where it recorded none, and an agent whose sessions
    /// take nothing is answered undelivered, kept apart from those delivered.
    #[test]
    fn the_driver_delivers_to_the_session_or_answers_undelivered() {
        let f = Fakes::new();
        let head = Rev::new("h");
        f.world.session("unit/a", "agent-a", &head);
        f.world.session("unit/b", "agent-b", &head);
        f.world.in_session("unit/a", "pane-1");
        let to = |worktree: &str| Notification {
            id: format!("{worktree}:landed"),
            submission: 1,
            branch: "main".into(),
            to: f.driver.session(worktree).unwrap().from,
            decided: Decided::Landed {
                landed_as: head.clone(),
                beside: false,
                covered_by: None,
            },
            report: None,
        };
        let delivered = |to: &str| Delivery::Handed {
            way: "notice".into(),
            to: to.into(),
            confirms: false,
        };
        assert_eq!(f.driver.notify(&to("unit/a")).unwrap(), delivered("pane-1"));
        assert_eq!(
            f.driver.notify(&to("unit/b")).unwrap(),
            delivered("agent-b")
        );
        f.world.not_delivering("agent-b");
        assert!(matches!(
            f.driver.notify(&to("unit/b")).unwrap(),
            Delivery::Undelivered { .. }
        ));
        assert_eq!(f.world.delivered().len(), 2);
        assert_eq!(f.world.undelivered(), [to("unit/b")]);
    }

    /// 138, design D9: a host's grant is on the executor of every report
    /// its runs make, a check's run, a command's and a preview's; another
    /// host's, and a host's once it records none, carry none.
    #[test]
    fn a_hosts_grant_is_on_its_reports() {
        let f = Fakes::new();
        let rev = f.world.init("main", &[("a/x", "1")]);
        f.world.task("a:test", "a/");
        let n = std::cell::Cell::new(0);
        let grant = |place: &str, command: Option<&str>| -> Option<String> {
            n.set(n.get() + 1);
            let request = RunRequest {
                place: f
                    .checkout
                    .materialize(&format!("{place}/{}", n.get()), &rev, &Settings::new())
                    .unwrap(),
                revision: rev.clone(),
                required: vec!["a:test".into()],
                since: None,
                limit: None,
                limits: Default::default(),
                tests: Settings::new(),
                command: command.map(str::to_string),
                replay: false,
                system: None,
            };
            let h = f.tests.start(&request, queue_part(&f)).unwrap();
            match f.tests.progress(&h).unwrap() {
                Progress::Finished(r) => r.executor.grant,
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(grant("sluice/try/main", None), None);
        f.world.grant(Some("sb1:g1"));
        for (place, command) in [
            ("sluice/try/main", None),
            ("session/w", Some("make check")),
            ("preview/main", None),
        ] {
            assert_eq!(grant(place, command).as_deref(), Some("sb1:g1"), "{place}");
        }
        f.world.be_host("b");
        assert_eq!(grant("sluice/try/main", None), None);
        f.world.grant(Some("open:1"));
        assert_eq!(grant("sluice/try/main", None).as_deref(), Some("open:1"));
        f.world.be_host("fake-host");
        assert_eq!(grant("sluice/try/main", None).as_deref(), Some("sb1:g1"));
        f.world.grant(None);
        assert_eq!(grant("sluice/try/main", None), None);
    }

    /// 139, design D9: a host's executor answers what the world says it
    /// refused or could not read, nothing for another host, and counts
    /// each ask by the host that asked.
    #[test]
    fn a_hosts_refusals_are_answered_and_counted() {
        use sluice_seams::Refusals;
        let f = Fakes::new();
        let run = RunHandle("run-0".into());
        assert_eq!(f.tests.refused(&run).unwrap(), Refusals::None);
        f.world.refuses(Some("refused x"));
        assert_eq!(
            f.tests.refused(&run).unwrap(),
            Refusals::Refused("refused x".into())
        );
        f.world.refusals_unread(Some("log: exited 1"));
        assert_eq!(
            f.tests.refused(&run).unwrap(),
            Refusals::Unread("log: exited 1".into())
        );
        f.world.be_host("b");
        assert_eq!(f.tests.refused(&run).unwrap(), Refusals::None);
        f.world.be_host("fake-host");
        f.world.refusals_unread(None);
        f.world.refuses(None);
        assert_eq!(f.tests.refused(&run).unwrap(), Refusals::None);
        assert_eq!(
            f.world.refusal_looks(),
            ["fake-host", "fake-host", "fake-host", "b", "fake-host"]
        );
    }

    /// The check fake counts the host's session runs while they run, and
    /// never one that has ended.
    #[test]
    fn session_runs_are_counted_while_they_run() {
        let f = Fakes::new();
        let sessions = || f.tests.running().unwrap().sessions;
        assert_eq!(sessions(), 0);
        f.world.session_run("/w/a", true);
        f.world.session_run("/w/b", true);
        f.world.session_run("/w/gone", false);
        assert_eq!(sessions(), 2);
        f.world.kill_session("/w/a");
        assert_eq!(sessions(), 1);
        assert_eq!(f.tests.running().unwrap().queue, 0);
    }

    /// 149: a task a check depends on runs first, under its own key; where
    /// it fails, the check is failed by it, its result reported unless it
    /// is itself a check of the run, and its log is its own; where it
    /// cannot run, the check is not run in its words; a check two steps up
    /// is failed by the task that ran.
    #[test]
    fn a_check_is_failed_by_the_task_it_depends_on() {
        let f = Fakes::new();
        let rev = f
            .world
            .init("main", &[("a/lib", "1"), ("b/lib", "1"), ("r/lib", "1")]);
        f.world.task("api:integration", "a/");
        f.world.task("api:build", "b/");
        f.world.task("root:integration", "r/");
        f.world.task("root:all", "r/");
        f.tests.depends_on("root:integration", "api:integration");
        f.tests.depends_on("root:all", "api:build");
        f.tests.depends_on("api:build", "api:integration");
        f.world.fails_with("api:integration", "a/lib", "1");
        let n = std::cell::Cell::new(0);
        let run = |required: &[&str]| -> (RunHandle, RunReport) {
            n.set(n.get() + 1);
            let request = RunRequest {
                place: f
                    .checkout
                    .materialize(
                        &format!("sluice/try/main/{}", n.get()),
                        &rev,
                        &Settings::new(),
                    )
                    .unwrap(),
                revision: rev.clone(),
                required: required.iter().map(|r| r.to_string()).collect(),
                since: None,
                limit: None,
                limits: Default::default(),
                tests: Settings::new(),
                command: None,
                replay: true,
                system: None,
            };
            let h = f.tests.start(&request, queue_part(&f)).unwrap();
            match f.tests.progress(&h).unwrap() {
                Progress::Finished(r) => (h, r),
                other => panic!("{other:?}"),
            }
        };
        let (h, r) = run(&["root:integration", "root:all"]);
        assert!(r.tasks.is_empty() && r.missing.is_empty() && r.not_run.is_empty());
        let by = |task: &str| sluice_seams::FailedBy {
            task: task.into(),
            by: vec!["api:integration".into()],
        };
        assert_eq!(r.failed_by, [by("root:integration"), by("root:all")]);
        assert_eq!(r.dependencies.len(), 1, "once, however many it failed");
        let dep = &r.dependencies[0];
        assert_eq!(
            (dep.task.as_str(), dep.outcome),
            ("api:integration", Outcome::Fail)
        );
        assert!(matches!(dep.source, Source::Ran { .. }));
        assert_eq!(
            f.world
                .observed()
                .iter()
                .filter(|(t, _, _)| t == "api:integration")
                .count(),
            1,
            "run once"
        );
        let logs = f.tests.logs(&h, &["api:integration".into()]).unwrap();
        assert!(logs[0].text.starts_with("api:integration failed"));
        assert_eq!(f.tests.proven(&h, &r).unwrap().proven, ["api:integration"]);

        // A dependency that is itself a check is reported as one.
        let (_, r) = run(&["api:integration", "root:integration"]);
        assert_eq!(r.tasks.len(), 1);
        assert_eq!(r.tasks[0].task, "api:integration");
        assert!(r.dependencies.is_empty());
        assert_eq!(r.failed_by, [by("root:integration")]);

        // One that cannot run leaves the check not run, in its words.
        f.world.cannot_run("api:integration", Some("exited 77"));
        let (_, r) = run(&["root:integration"]);
        assert!(r.failed_by.is_empty() && r.dependencies.is_empty());
        assert_eq!(
            r.not_run,
            [NotRun {
                task: "root:integration".into(),
                reason: "its dependency api:integration could not run: exited 77".into(),
                key: None,
            }]
        );

        // One that passes, on a tree its rule does not name, lets the
        // check run.
        f.world.cannot_run("api:integration", None);
        let rev2 = f.world.work(&rev, &[("a/lib", Some("2"))]);
        let request = RunRequest {
            place: f
                .checkout
                .materialize("sluice/try/main/9", &rev2, &Settings::new())
                .unwrap(),
            revision: rev2,
            required: vec!["root:integration".into()],
            since: None,
            limit: None,
            limits: Default::default(),
            tests: Settings::new(),
            command: None,
            replay: true,
            system: None,
        };
        let h = f.tests.start(&request, queue_part(&f)).unwrap();
        let Progress::Finished(r) = f.tests.progress(&h).unwrap() else {
            panic!("finished")
        };
        assert!(r.failed_by.is_empty() && r.dependencies.is_empty());
        assert_eq!(r.tasks[0].task, "root:integration");
        assert_eq!(r.tasks[0].outcome, Outcome::Pass);
    }

    /// Design D8 (114; record 0105): a world that keeps no proof answers
    /// each ran pass unkept, with why, and a fail proven as before; once
    /// it keeps proof again, both are proven.
    #[test]
    fn a_world_that_keeps_no_proof_answers_each_ran_pass_unkept() {
        let f = Fakes::new();
        let rev = f.world.init("main", &[("a/x", "1"), ("b/x", "1")]);
        f.world.task("a:test", "a/");
        f.world.task("b:test", "b/");
        f.world.fails_with("a:test", "a/x", "1");
        let n = std::cell::Cell::new(0);
        let run = || -> (RunHandle, RunReport) {
            n.set(n.get() + 1);
            let request = RunRequest {
                place: f
                    .checkout
                    .materialize(
                        &format!("sluice/try/main/{}", n.get()),
                        &rev,
                        &Settings::new(),
                    )
                    .unwrap(),
                revision: rev.clone(),
                required: vec!["a:test".into(), "b:test".into()],
                since: None,
                limit: None,
                limits: Default::default(),
                tests: Settings::new(),
                command: None,
                replay: false,
                system: None,
            };
            let h = f.tests.start(&request, queue_part(&f)).unwrap();
            match f.tests.progress(&h).unwrap() {
                Progress::Finished(r) => (h, r),
                other => panic!("{other:?}"),
            }
        };
        f.world.keeps_no_proof(Some("/the/store"));
        let (h, r) = run();
        let outcome = |task: &str| r.tasks.iter().find(|t| t.task == task).unwrap().outcome;
        assert_eq!(
            (outcome("a:test"), outcome("b:test")),
            (Outcome::Fail, Outcome::Pass)
        );
        let proof = f.tests.proven(&h, &r).unwrap();
        assert_eq!(proof.proven, ["a:test"]);
        let b = r.tasks.iter().find(|t| t.task == "b:test").unwrap();
        assert_eq!(
            proof.unkept,
            [NotRun {
                task: "b:test".into(),
                reason: "it passed, but nothing the run kept proves it ran: \
                         its record is not in /the/store"
                    .into(),
                key: Some(b.hash.clone()),
            }]
        );
        f.world.keeps_no_proof(None);
        let (h, r) = run();
        let mut proven = f.tests.proven(&h, &r).unwrap().proven;
        proven.sort();
        assert_eq!(proven, ["a:test", "b:test"]);
        assert!(f.tests.proven(&h, &r).unwrap().unkept.is_empty());
    }

    /// Design D6 of `a-signed-run-without-a-sandbox-proves-its-passes`
    /// (C.6, 20): a task the world forges is neither proven nor unkept,
    /// and is answered refused with its key and the profile's words; the
    /// rest are proven as before.
    #[test]
    fn a_forged_key_is_refused_with_why() {
        let f = Fakes::new();
        let rev = f.world.init("main", &[("a/x", "1"), ("b/x", "1")]);
        f.world.task("a:test", "a/");
        f.world.task("b:test", "b/");
        f.world.forges("a:test");
        let request = RunRequest {
            place: f
                .checkout
                .materialize("sluice/try/main/1", &rev, &Settings::new())
                .unwrap(),
            revision: rev.clone(),
            required: vec!["a:test".into(), "b:test".into()],
            since: None,
            limit: None,
            limits: Default::default(),
            tests: Settings::new(),
            command: None,
            replay: false,
            system: None,
        };
        let h = f.tests.start(&request, queue_part(&f)).unwrap();
        let Progress::Finished(r) = f.tests.progress(&h).unwrap() else {
            panic!("finished")
        };
        let a = r.tasks.iter().find(|t| t.task == "a:test").unwrap();
        let proof = f.tests.proven(&h, &r).unwrap();
        assert_eq!(proof.proven, ["b:test"]);
        assert!(proof.unkept.is_empty(), "{proof:?}");
        assert_eq!(
            proof.refused,
            [sluice_seams::Refused {
                task: "a:test".into(),
                key: a.hash.clone(),
                why: "a:test: its record names an input the tree does not hold".into(),
            }]
        );
    }

    /// A part of the fakes' host's bound for a combination's run.
    fn queue_part(f: &Fakes) -> Part {
        f.tests
            .hold(RunKind::Queue)
            .unwrap()
            .expect("room on the host")
    }

    fn a_request(f: &Fakes, name: &str, rev: &Rev) -> RunRequest {
        RunRequest {
            place: f.checkout.materialize(name, rev, &Settings::new()).unwrap(),
            revision: rev.clone(),
            required: Vec::new(),
            since: None,
            limit: None,
            limits: Default::default(),
            tests: Settings::new(),
            command: None,
            replay: true,
            system: None,
        }
    }

    /// 71 (record 0087): a part is held while the parts held and the runs
    /// going are under the bound, other repositories' and sessions' among
    /// them, and none past it.
    #[test]
    fn a_part_is_refused_at_the_bound() {
        let f = Fakes::new();
        let rev = f.world.init("main", &[("x", "1")]);
        f.world.host_limit(3);
        f.world.hold_runs(true);
        f.world.other_runs(1);
        let first = f.tests.hold(RunKind::Queue).unwrap().expect("room");
        assert_eq!(first.kind(), RunKind::Queue);
        f.tests
            .start(&a_request(&f, "sluice/try/main/1", &rev), first)
            .unwrap();
        let second = f.tests.hold(RunKind::Session).unwrap().expect("room");
        assert_eq!(
            f.tests.running().unwrap(),
            Running {
                queue: 2,
                sessions: 1
            },
            "the part counts while it is held"
        );
        assert!(f.tests.hold(RunKind::Queue).unwrap().is_none());
        assert!(f.tests.hold(RunKind::Session).unwrap().is_none());
        drop(second);
        f.world.complete(&rev);
        assert!(f.tests.hold(RunKind::Queue).unwrap().is_some());
    }

    /// 71 (record 0087): a part dropped unstarted, or one whose start
    /// failed, gives its room back at once.
    #[test]
    fn a_dropped_part_frees_its_room() {
        let f = Fakes::new();
        let rev = f.world.init("main", &[("x", "1")]);
        f.world.host_limit(1);
        f.world.hold_runs(true);
        let part = f.tests.hold(RunKind::Queue).unwrap().expect("room");
        assert_eq!(f.world.parts_held(), 1);
        assert!(f.tests.hold(RunKind::Session).unwrap().is_none());
        drop(part);
        assert_eq!(f.world.parts_held(), 0);
        assert_eq!(f.tests.running().unwrap(), Running::default());
        // A start that fails gives its part back.
        let part = f.tests.hold(RunKind::Queue).unwrap().expect("room");
        f.world.start_fails(Some("the toolchain did not enter"));
        let request = a_request(&f, "sluice/try/main/1", &rev);
        assert!(f.tests.start(&request, part).is_err());
        assert_eq!(f.tests.running().unwrap(), Running::default());
        assert!(f.tests.hold(RunKind::Queue).unwrap().is_some());
    }

    /// 71: the host's count holds other repositories' runs beside its own,
    /// and no part is held at the bound, nothing of a run started.
    #[test]
    fn the_host_count_holds_other_repositories_runs() {
        let f = Fakes::new();
        let rev = f.world.init("main", &[("x", "1")]);
        f.world.host_limit(2);
        f.world.hold_runs(true);
        f.world.other_runs(1);
        assert_eq!(
            f.tests.running().unwrap(),
            Running {
                queue: 1,
                sessions: 0
            }
        );
        let request = |name: &str| RunRequest {
            place: f
                .checkout
                .materialize(name, &rev, &Settings::new())
                .unwrap(),
            revision: rev.clone(),
            required: Vec::new(),
            since: None,
            limit: None,
            limits: Default::default(),
            tests: Settings::new(),
            command: None,
            replay: true,
            system: None,
        };
        f.tests
            .start(&request("sluice/try/main/1"), queue_part(&f))
            .unwrap();
        assert_eq!(f.tests.running().unwrap().queue, 2);
        let second = request("sluice/try/main/2");
        assert!(f.tests.hold(RunKind::Queue).unwrap().is_none());
        assert_eq!(f.world.requests().len(), 1, "nothing of it ran");
        // Another host's count is its own.
        f.world.be_host("other");
        assert_eq!(f.tests.running().unwrap().queue, 0);
        f.world.be_host("fake-host");
        f.world.other_runs(0);
        f.tests.start(&second, queue_part(&f)).unwrap();
        assert_eq!(f.tests.running().unwrap().queue, 2);
        // A part the host no longer honours is refused at its start.
        f.world.host_limit(3);
        let third = request("sluice/try/main/3");
        f.world.refuse_next_start();
        let err = f.tests.start(&third, queue_part(&f)).unwrap_err();
        assert!(err.downcast_ref::<AtBound>().is_some(), "{err:#}");
        assert_eq!(f.world.parts_held(), 0);
    }

    /// 124: a branch on a remote has a host copy, levelled only from the
    /// value it holds, and failing as many times as it is told; any other
    /// branch is its own copy and is never levelled.
    #[test]
    fn a_branch_on_a_remote_has_a_copy_levelled_from_its_value() {
        let f = Fakes::new();
        let base = f.world.init("main", &[("a", "1")]);
        let bolt = f.world.init("bolt/x", &[("b", "1")]);
        f.world.on_remote("bolt/x");
        assert_eq!(f.branch.copy("main").unwrap(), HostCopy::Itself);
        assert!(f.branch.level("main", Some(&base), &base).is_err());
        assert_eq!(
            f.branch.copy("bolt/x").unwrap(),
            HostCopy::At(Some(bolt.clone()))
        );
        let landed = f.world.hand_commit("bolt/x", &[("b", Some("2"))]);
        assert_eq!(f.world.copy_of("bolt/x"), Some(bolt.clone()));
        assert_eq!(
            f.branch.level("bolt/x", Some(&base), &landed).unwrap(),
            Moved::Elsewhere(Some(bolt.clone()))
        );
        f.world
            .fail_levels(2, "git holds the lock on refs/heads/bolt/x");
        for _ in 0..2 {
            let err = f.branch.level("bolt/x", Some(&bolt), &landed).unwrap_err();
            assert_eq!(err.to_string(), "git holds the lock on refs/heads/bolt/x");
        }
        assert_eq!(f.world.copy_of("bolt/x"), Some(bolt.clone()));
        assert_eq!(f.world.levels(), 0);
        assert_eq!(
            f.branch.level("bolt/x", Some(&bolt), &landed).unwrap(),
            Moved::Moved
        );
        assert_eq!(
            f.branch.copy("bolt/x").unwrap(),
            HostCopy::At(Some(landed.clone()))
        );
        assert_eq!(f.world.levels(), 1);
        f.world.set_copy("bolt/x", None);
        assert_eq!(
            f.branch.level("bolt/x", None, &landed).unwrap(),
            Moved::Moved
        );
    }

    /// 133: a host's reads of a branch, or of its records, are refused with
    /// the words given until answered again; a refused read of the records
    /// answers them as held, stale, and a write fails. Each host keeps its
    /// own refusal document.
    #[test]
    fn a_host_s_reads_are_refused_with_the_words_given() {
        let f = Fakes::new();
        let tip = f.world.init("main", &[("a", "1")]);
        f.world
            .refuse_reads("main", Some("remote: Repository not found."));
        let err = f.branch.read("main").unwrap_err();
        assert_eq!(
            sluice_seams::read_refused(&err),
            Some(&ReadRefused {
                remote: "origin".into(),
                asked: "refs/heads/main".into(),
                said: "remote: Repository not found.".into(),
                note: None,
            })
        );
        f.world.be_host("other");
        assert_eq!(f.branch.read("main").unwrap(), Some(tip.clone()));
        f.world.be_host("fake-host");
        f.world.refuse_reads("main", None);
        assert_eq!(f.branch.read("main").unwrap(), Some(tip));

        f.world.refuse_state_reads("main", Some("HTTP 503"));
        let err = f.state.read("main").unwrap_err();
        let refused = sluice_seams::read_refused(&err).unwrap();
        assert_eq!(refused.asked, "refs/sluice/state/main");
        f.world.refuse_state_reads("main", None);
        {
            let _g = f.state.lock("main").unwrap().unwrap();
            f.state.write("main", 0, Changes::default()).unwrap();
            f.world.refuse_state_reads("main", Some("HTTP 503"));
            assert_eq!(f.state.read("main").unwrap().version, 1);
            assert_eq!(f.state.stale("main").unwrap().said, "HTTP 503");
            assert!(f.state.write("main", 1, Changes::default()).is_err());
            let err = f.state.fetch_heads("main").unwrap_err();
            assert_eq!(sluice_seams::read_refused(&err).unwrap().said, "HTTP 503");
            assert!(f.state.fetch_candidates("main").is_err());
        }
        f.world.refuse_state_reads("main", None);
        assert_eq!(f.state.stale("main"), None);

        f.state
            .keep_refusal("main", Some(b"{\"rounds\":1}"))
            .unwrap();
        assert_eq!(f.world.refusal("main").as_deref(), Some("{\"rounds\":1}"));
        f.world.be_host("other");
        assert_eq!(f.state.refusal("main").unwrap(), None);
        f.world.be_host("fake-host");
        f.state.keep_refusal("main", None).unwrap();
        assert_eq!(f.world.refusal("main"), None);
    }

    /// 129: a branch's records fail to be read with the words given, read
    /// before or not, until told otherwise; another branch's are read.
    #[test]
    fn a_read_of_the_records_fails_until_cleared() {
        let f = Fakes::new();
        f.world.init("main", &[("x", "1")]);
        {
            let _g = f.state.lock("main").unwrap().unwrap();
            f.state.write("main", 0, Changes::default()).unwrap();
        }
        let said = "fatal: unable to read 4b825dc6: Resource temporarily unavailable";
        f.world.fail_state_reads("main", Some(said));
        let err = f.state.read("main").unwrap_err();
        assert_eq!(format!("{err:#}"), said);
        assert!(f.state.read_finished("main", "1").is_err());
        assert_eq!(f.state.read("other").unwrap().version, 0);
        f.world.fail_state_reads("main", None);
        assert_eq!(f.state.read("main").unwrap().version, 1);
    }

    fn last(outcome: sluice_seams::RunOutcome) -> LastResult {
        LastResult {
            at: "2026-10-16T09:00:00Z".into(),
            head: Rev("a".into()),
            kind: sluice_seams::SessionRunKind::Checks,
            outcome,
            recorded: true,
            tasks: Vec::new(),
            reused: None,
        }
    }

    /// 97: a session place keeps its last run's result, the next replaces
    /// it, its release removes it and a result after the release is kept
    /// nowhere, and a candidate's place takes none.
    #[test]
    fn a_session_place_keeps_its_last_result_until_released() {
        use sluice_seams::RunOutcome::{Fail, Pass};
        let f = Fakes::new();
        f.world.session("/w/a", "a", &Rev("a".into()));
        let place = f
            .checkout
            .adopt("/w/a", "main", &Settings::new(), "2026-10-16T08:59:00Z")
            .unwrap();
        assert_eq!(f.checkout.session_places().unwrap()[0].last_result, None);
        f.checkout.ran(&place.name, &last(Pass)).unwrap();
        assert_eq!(
            f.checkout.session_places().unwrap()[0].last_result,
            Some(last(Pass))
        );
        f.checkout.ran(&place.name, &last(Fail)).unwrap();
        assert_eq!(
            f.checkout.session_places().unwrap()[0].last_result,
            Some(last(Fail))
        );
        f.checkout.release_session(&place.name, None).unwrap();
        assert!(f.checkout.session_places().unwrap().is_empty());
        f.checkout.ran(&place.name, &last(Pass)).unwrap();
        assert!(f.checkout.session_places().unwrap().is_empty());
        f.world.init("main", &[("x", "1")]);
        f.checkout
            .materialize("candidate-1", &f.world.tip("main"), &Settings::new())
            .unwrap();
        assert!(f.checkout.ran("candidate-1", &last(Pass)).is_err());
    }
}
