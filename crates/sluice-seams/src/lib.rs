//! The seams of sluice: exactly five, one trait each (rule core.2,
//! statement Part B, ADR 0010).
//!
//! | seam | trait | statement |
//! |---|---|---|
//! | the place | [`Checkout`] | B.1, 33 to 35 |
//! | the check | [`Tests`] | B.2, 36 to 38 |
//! | the branch | [`Branch`] | B.4, 42 to 46 |
//! | the driver | [`Driver`] | B.5, 47 to 51 |
//! | the records | [`State`] | B.7, 61 to 63 |
//!
//! A profile implements one of these traits and nothing of the core. The
//! core reaches every seam through these traits alone, so the types that
//! cross a seam are defined here and nowhere else.
//!
//! Errors are `anyhow::Error`: a seam that cannot do what it was asked
//! fails the step that asked. A refusal the statement names (a conflict,
//! a branch found elsewhere, a records write refused) is a value, never an
//! error, because the core acts on it. One refusal is an error the core
//! finds by its type: a start at the host's bound, [`AtBound`], refused
//! before the check seam runs anything (71). Another error is told apart,
//! [`Unanswered`]: a remote that could not answer just then, which a
//! waiting command reads again through. A third, [`ReadRefused`], is a read
//! of a branch's remote that could not be made, which a round logs (133).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// A remote's or a forge's 5xx answer to a call, in the profile's words:
/// its server could not answer just then, which says nothing of what it
/// holds.
#[derive(Debug)]
pub struct Unanswered(pub String);

impl fmt::Display for Unanswered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Unanswered {}

/// Whether `e`, or anything that caused it, is an [`Unanswered`].
pub fn unanswered(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.is::<Unanswered>())
}

/// 133: a read of a branch's remote that could not be made: a fetch that
/// failed for any reason but the remote having no such ref.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadRefused {
    /// The remote's name.
    pub remote: String,
    /// The ref fetched: `refs/heads/<branch>`, or the records' ref.
    pub asked: String,
    /// What git printed of the failure, whole, trailing whitespace trimmed.
    pub said: String,
    /// sluice's own words on the credentials the remote refused, when it
    /// tried more than the host's: never the remote's, so never logged.
    pub note: Option<String>,
}

impl fmt::Display for ReadRefused {
    /// `fetching main from origin failed: …`, a branch named as git's
    /// branch is, then the note on the next line.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let asked = self
            .asked
            .strip_prefix("refs/heads/")
            .unwrap_or(&self.asked);
        write!(
            f,
            "fetching {asked} from {} failed: {}",
            self.remote, self.said
        )?;
        if let Some(note) = &self.note {
            write!(f, "\n{note}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ReadRefused {}

/// The [`ReadRefused`] `e` is, or that caused it.
pub fn read_refused(e: &anyhow::Error) -> Option<&ReadRefused> {
    e.chain().find_map(|c| c.downcast_ref::<ReadRefused>())
}

/// A git revision, as the branch seam names it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Rev(pub String);

impl Rev {
    pub fn new(s: impl Into<String>) -> Self {
        Rev(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The first seven characters, as the CLI shows a revision.
    pub fn short(&self) -> &str {
        &self.0[..self.0.len().min(7)]
    }
}

impl fmt::Display for Rev {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// B.1 The place
// ---------------------------------------------------------------------------

/// A place sluice made for a candidate. Its name is chosen by the core
/// before the place exists, so a place can be released by name after a
/// crash (25, 34).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub name: String,
    pub path: PathBuf,
    /// A scratch directory of the place's own, given to the checks as
    /// `SLUICE_TMP`: under a short directory of the host user's own, apart
    /// from every repository, at a path that grows with neither the place's
    /// name nor where its checkout or the repository is, as its record
    /// names it (35 as amended, rule core.30).
    pub tmp: PathBuf,
    /// 31: what the place gives its checks beyond its paths: its short
    /// name, `SLUICE_PLACE`, and a `SLUICE_PORT_<NAME>` for each port the
    /// policy names, held by no other place alive on the host.
    pub env: BTreeMap<String, String>,
}

/// A session's place sluice equips, as the place seam finds it by its
/// worktree (session-runs-1 clause 90).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adopted {
    pub name: String,
    /// The branch whose policy its runs were held to.
    pub branch: String,
}

/// A session's place the repository holds, as the place seam lists it
/// (session-runs-1 clause 97).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionPlace {
    pub name: String,
    /// The worktree it was adopted for.
    pub worktree: PathBuf,
    /// The branch whose policy its runs were held to.
    pub branch: String,
    /// `SLUICE_PORT_<NAME>` for each port it holds.
    pub ports: BTreeMap<String, String>,
    /// When its last run started, as the core gave it to `adopt`.
    pub last_run: Option<String>,
    /// Its worktree no longer exists: the place is no session's (97).
    pub gone: bool,
    /// How its last run ended, once one has and none has started since
    /// (97).
    pub last_result: Option<LastResult>,
    /// Its release at its session's or driver's asking failed, and how
    /// (123).
    pub release_failed: Option<ReleaseFailure>,
}

/// A release of a preview or of a session's place that failed, kept on
/// the place's record until the place is released or freed (123).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseFailure {
    /// What failed, the end of what the release command printed included.
    pub failed: String,
    /// How many times its release was tried, the first included.
    pub tries: u32,
    /// When it last failed.
    pub at: String,
    /// It is tried no more on its own: held until its release is asked
    /// for by name, or its failure accepted.
    #[serde(default)]
    pub stopped: bool,
}

/// How a session's last run ended, kept on its place until the place's
/// release (session-runs-1 clause 97). It decides nothing: no round, gate
/// or replay reads it (rule core.18).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastResult {
    /// When the run ended.
    pub at: String,
    /// The head the run was given.
    pub head: Rev,
    pub kind: SessionRunKind,
    pub outcome: RunOutcome,
    /// Its results are under their keys (92): only a run of the checks in
    /// a place holding exactly its head that did not hit its time limit.
    pub recorded: bool,
    /// Each task of a run of the checks that reported, as it ended:
    /// passed, failed or replayed (ADR 0035).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<TaskProgress>,
    /// Whether its passes are what the branch's gate takes when its head is
    /// submitted: recorded, every check passed, and its executor trusted by
    /// the gate read from the branch's revision (93, 56). Set for a run of
    /// the checks that reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reused: Option<bool>,
}

/// What a session run ran (89).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionRunKind {
    Checks,
    Command,
}

/// How a session run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    /// Every check it named reported and passed, or the command exited 0.
    Pass,
    Fail,
    /// Its time limit ended it (66).
    TimedOut,
    /// Its supervisor stopped before it reported (60).
    Lost,
    /// Its caller ended it.
    Stopped,
    /// Nothing failed and a check could not run (could-not-run-1 114).
    NotRun,
}

/// A worktree of the repository with a branch checked out, as the place
/// seam lists it: none of sluice's places, which are detached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedOut {
    pub worktree: PathBuf,
    pub branch: String,
    /// The revision it holds.
    pub head: Rev,
}

/// The policy a session's place is released under, read from its
/// branch's current revision (56, 95).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleasePolicy {
    /// The branch the policy was read from: the one a held place's runs
    /// were held to, or, for a worktree sluice holds nothing for, the
    /// repository's default branch (record 0120).
    pub branch: String,
    pub tests: Settings,
    pub limits: Limits,
}

/// A preview: a place sluice made at a revision its branch has landed,
/// held until its caller asks for its release (previews-1 clauses 98 to
/// 101).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewPlace {
    pub name: String,
    /// The branch it was first asked for through.
    pub branch: String,
    pub revision: Rev,
    /// Its checkout.
    pub path: PathBuf,
    /// `SLUICE_PORT_<NAME>` for each port it holds (31).
    pub ports: BTreeMap<String, String>,
    /// `SLUICE_NAME_<NAME>` for each port: one DNS label, unique on the
    /// host (99).
    pub names: BTreeMap<String, String>,
    /// Each address the policy names, filled in (99).
    pub addresses: BTreeMap<String, String>,
    /// When it was made.
    pub made: String,
    /// Its command's run, once started.
    pub run: Option<RunHandle>,
    /// Its checkout no longer exists.
    pub gone: bool,
    /// It was released because its command failed: the end of what the
    /// command printed. Such a preview holds nothing (101).
    pub failed: Option<String>,
    /// Its release failed, and how (123).
    pub release_failed: Option<ReleaseFailure>,
}

/// How the release of a place whose working copy was gone went (123).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GoneRelease {
    /// A release command its runs stored was run.
    pub ran: bool,
    /// It failed, and how: the end of what it printed included.
    pub failed: Option<String>,
}

/// How a session's place was released.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Released {
    /// Nothing to release: no such place and no policy given, a folder
    /// that is no worktree of the repository, or a policy that names no
    /// release command for a worktree sluice holds nothing for (record
    /// 0120).
    Nothing,
    /// Under the policy given.
    Policy,
    /// With no policy given: the release command its last run carried.
    LastRun,
}

pub trait Checkout {
    /// 33: a place holding exactly `rev`, with the environment the
    /// repository declares, named `name`, made as the policy's
    /// `[checkout]` table says.
    fn materialize(&self, name: &str, rev: &Rev, checkout: &Settings) -> Result<Place>;

    /// 34: the place's removal, with everything started in it ended, what
    /// its checks handed to another process included (ADR 0016). The
    /// core calls this only with names it chose (rule core.16); releasing
    /// a name that was chosen but never materialized does nothing.
    fn release(&self, name: &str) -> Result<()>;

    /// 123: whether the working copy of the place `name` is gone: its
    /// record names one, and that path is no directory, or, for a place
    /// sluice made, a combination's checkout or a preview, a folder git no
    /// longer lists as a worktree of the repository, whatever is left in
    /// it (record 0115). A session's worktree is gone only once it is no
    /// directory (rule core.16). A place with no record, or one recorded
    /// before its checkout was made, is not gone: `release` handles it. A
    /// reading that fails is an error, never gone. A read: nothing is
    /// written or released.
    fn gone(&self, name: &str) -> Result<bool>;

    /// 123: free what sluice itself holds for the place `name`, running
    /// nothing in its name: its runs' groups ended and their directories
    /// removed, the checkout sluice made removed, by git where git lists
    /// it, and otherwise what is left of its folder by sluice, a directory
    /// its owner cannot write made writable first (record 0115); its
    /// scratch directory, its ports and its record let go, and a preview's
    /// names. No release command runs; a session's worktree is left as it
    /// is (25, rule core.16). Nothing when there is no such place.
    fn free(&self, name: &str) -> Result<()>;

    /// 123: release the place `name`, whose working copy is gone, from
    /// what sluice itself holds for it: the release command its last run
    /// stored, run once with the environment and ports stored with it, in
    /// the place's scratch directory, entering no toolchain and reading no
    /// policy; then free it as [`Checkout::free`] does, whatever the command
    /// did. A place whose gone release has started once is freed with
    /// nothing run, so the command never runs twice. Only a failure to free
    /// is an error; the command's failure is in what is returned.
    fn release_gone(&self, name: &str) -> Result<GoneRelease>;

    /// 35 as amended, 123, rule core.30: remove every scratch directory of
    /// the repository that no place's record names, under the repository's
    /// own directory in the user's and under its former root, and return
    /// the paths removed; another repository's scratch is never weighed. A
    /// directory a record names is left, an older record's, which names its
    /// scratch by the place's name, among them; so is one made while the
    /// removal runs, since a place's record names its scratch before the
    /// directory is made. A directory removed meanwhile is no error.
    fn remove_unnamed_scratch(&self) -> Result<Vec<PathBuf>>;

    /// 81: every candidate's place this host made and has not released,
    /// by name: what a returning host releases before it claims again.
    fn made(&self) -> Result<Vec<String>>;

    /// Session-runs-1 clause 90: a session's own place, `worktree`, taken
    /// as found for a session run held to `branch`'s policy: its ports,
    /// held from its first run until its release and the same on every
    /// run, and a scratch directory kept across its runs. Nothing of the
    /// worktree is written, moved or removed (25). Its name begins
    /// `session/`, so it is never a candidate's. `at` is when the run
    /// starts, kept as the place's last run (97); the result of the run
    /// before is dropped, so a run that never ends shows none.
    fn adopt(&self, worktree: &str, branch: &str, checkout: &Settings, at: &str) -> Result<Place>;

    /// The session's place sluice equips for `worktree`, if any.
    fn adopted(&self, worktree: &str) -> Result<Option<Adopted>>;

    /// Session-runs-1 clause 95: release the session's place `name` at its
    /// session's or driver's asking: end every run in it, run the release
    /// command once, as `policy` states it or, given none, as its last run
    /// carried it, remove its scratch directory and let its ports go. The
    /// worktree is left as it is (25, rule core.16). A release that fails
    /// keeps the place with what failed, held: no round releases it (123).
    ///
    /// A name with no record, given a policy, whose folder is the
    /// repository's main worktree or a linked worktree git lists, detached
    /// or not, is released all the same, by the path a held place's
    /// release takes: a record and a scratch of the release's own, no
    /// ports, the policy's release command run, and the record and scratch
    /// let go once it ends, whether it passed or failed, so nothing is
    /// kept for the worktree (95; rule core.10; record 0120). Given no
    /// policy, or a folder that is no worktree of the repository, such a
    /// name has nothing to release.
    fn release_session(&self, name: &str, policy: Option<&ReleasePolicy>) -> Result<Released>;

    /// Session-runs-1 clause 97: every session's place the repository
    /// holds, each with whether its worktree still exists and how its last
    /// run ended.
    fn session_places(&self) -> Result<Vec<SessionPlace>>;

    /// Session-runs-1 clause 97: keep `last` as how the session's place
    /// `name`'s last run ended, replacing any, until the place's release.
    /// Nothing for a place already released, whose record a release never
    /// sees written again; an error for a candidate's or a preview's place.
    fn ran(&self, name: &str, last: &LastResult) -> Result<()>;

    /// Every worktree of the repository that has a branch checked out,
    /// with the branch and the revision it holds; a detached worktree is
    /// left out. A read: nothing is written, moved or released.
    fn checked_out(&self) -> Result<Vec<CheckedOut>>;

    /// Previews-1 clauses 98, 99: a preview named `name` holding exactly
    /// `rev`, made as a candidate's place is, with its caches and ports, and
    /// a name for each port; each of `addresses` filled in with the names
    /// and ports. An address that names a port `checkout` does not list is
    /// refused before anything is made. A preview of that name that exists
    /// already is given back as it is (100).
    fn preview(
        &self,
        name: &str,
        rev: &Rev,
        branch: &str,
        checkout: &Settings,
        addresses: &BTreeMap<String, String>,
        at: &str,
    ) -> Result<(Place, PreviewPlace)>;

    /// The preview `name`'s command has started as `run`.
    fn preview_run(&self, name: &str, run: &RunHandle) -> Result<()>;

    /// Previews-1 clause 101: every preview the repository holds.
    fn previews(&self) -> Result<Vec<PreviewPlace>>;

    /// Previews-1 clause 101: release the preview `name`: its command's
    /// run ended, the release command run, its checkout removed, its ports
    /// and names let go. With `failed`, a record of it is kept, holding
    /// nothing, with what its command printed. `false` when there is no
    /// such preview. A release that fails keeps the preview with what
    /// failed, and holds it once it has been tried more than `retries`
    /// times, the bound the core read for its branch (123).
    fn release_preview(&self, name: &str, failed: Option<&str>, retries: u32) -> Result<bool>;
}

// ---------------------------------------------------------------------------
// B.2 The check
// ---------------------------------------------------------------------------

/// One value of a policy table the core passes to a profile without
/// reading it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Setting {
    Text(String),
    List(Vec<String>),
    /// An array of tables in a profile's table, such as `[[tests.service]]`,
    /// or one table, such as a pool's `github = { workflow = "…" }`.
    Tables(Vec<Settings>),
}

/// A policy table as the core passes it to a profile: the profile's own
/// settings, read from the branch's revision (56) and given no meaning by
/// the core (52).
pub type Settings = BTreeMap<String, Setting>;

/// 83: how far a run may reach over the network, as a branch's policy
/// states it (`[executor] network = false | "localhost" | true`) and as an
/// executor held it. Read from `false` and `true` as well as by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    /// No connection at all, local sockets included.
    Denied,
    /// Only localhost (127.0.0.1 and ::1): a check may start a server
    /// there and reach it, and reach nothing else.
    Localhost,
    /// Anywhere.
    Allowed,
}

impl<'de> Deserialize<'de> for Network {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Form {
            Flag(bool),
            Word(String),
        }
        match Form::deserialize(d)? {
            Form::Flag(false) => Ok(Network::Denied),
            Form::Flag(true) => Ok(Network::Allowed),
            Form::Word(w) => match w.as_str() {
                "denied" => Ok(Network::Denied),
                "localhost" => Ok(Network::Localhost),
                "allowed" => Ok(Network::Allowed),
                _ => Err(serde::de::Error::custom(format!(
                    "network = {w:?}: one of false, \"localhost\" or true"
                ))),
            },
        }
    }
}

/// 83: what a run may use and reach, as the branch's policy states it.
/// The cores each run is given are the instance's, not the policy's,
/// so they are the check seam's own (71).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Limits {
    /// The executor the policy names; `None` names none, so any executor
    /// that can hold the other limits runs it.
    pub executor: Option<String>,
    /// The most memory the run may use, in bytes.
    pub memory: Option<u64>,
    /// How far the run may reach over the network; `None` states nothing.
    pub network: Option<Network>,
    /// Whether the run may reach the host's container engine (ADR 0016).
    /// A container it starts may mount any path, so such a run is never
    /// confined.
    pub docker: bool,
}

/// 83, 84: what an executor held a run to, recorded with every result it
/// observed (16), so a gate reads confinement from the record and never
/// from the policy (28, 29).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Enforced {
    /// The executor profile: `open`, `sandbox` or `container` (C.2).
    pub kind: String,
    /// No session could reach the run's place or what it wrote.
    pub confined: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processors: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<u64>,
    /// The network the executor held the run to; `None` when it held it
    /// to nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<Network>,
    /// The run could reach the host's container engine (ADR 0016).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub docker: bool,
    /// 83, 138: what the executor let the run reach, as one string a key's
    /// results are weighed under. The sandbox's is `sb1:` and a digest of
    /// the profile it wrote with every path named by its role, so places
    /// and hosts given one profile give one grant; the open executor's is
    /// `open:1`. sluice's version is no part of it. `None` on a line
    /// recorded before the grant, which is weighed only with others that
    /// carry none (rule core.34).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant: Option<String>,
}

/// What the core asks the check seam to run on one candidate (37).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunRequest {
    pub place: Place,
    /// The candidate revision the place holds.
    pub revision: Rev,
    /// The checks the branch's gate requires, as the policy names them.
    pub required: Vec<String>,
    /// The branch's revision the candidate's affected checks are taken
    /// against; `None` when that revision is covered by no verdict, so
    /// every required check is affected (70).
    pub since: Option<Rev>,
    /// The time limit the branch's policy states (66).
    pub limit: Option<Duration>,
    /// 83: the memory and network the branch's policy states, and the
    /// executor it names.
    pub limits: Limits,
    /// The policy's `[tests]` table: which runner, and its settings.
    pub tests: Settings,
    /// A command the run runs in place of the runner's checks (89, 98),
    /// its working copy equipped by the runner `tests` names (147).
    pub command: Option<String>,
    /// Whether a result may be answered from the task runner's cache. A
    /// replay supplies outputs and decides nothing: it counts only through
    /// a trusted pass recorded for its key (108), and a run whose replays
    /// none proved runs again without it.
    pub replay: bool,
    /// The system the run is for, by the policy's name (85, 87); `None`
    /// when the gate names none, or the policy no systems.
    pub system: Option<String>,
}

/// 88, 107: a run started on a pool: one job, on the runner its system
/// names, whose start is its claim. The pool's kind and settings are the
/// policy's `[pools.<name>]` table, which the core does not read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub pool: String,
    pub kind: (String, Setting),
    /// The identity of the signer its jobs sign as: where a forge's job
    /// runs from.
    pub signer: (String, Setting),
    pub runner: String,
    /// The git remote the branch lives on, which the pool's jobs fetch from.
    pub remote: Option<String>,
    pub branch: String,
    pub candidate: String,
    pub combination: Rev,
    pub system: Option<String>,
    /// The claim the job runs under: the host its results are reported as.
    pub claim: String,
}

/// The operating system and processor architecture an executor runs on,
/// as the toolchain names them (85): `linux`, `macos`; `x86_64`,
/// `aarch64`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Platform {
    pub os: String,
    pub arch: String,
}

/// A run in progress, as the check seam names it. The core keeps it in
/// the records and asks after it in a later step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunHandle(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Pass,
    Fail,
}

/// Whether an outcome was observed in this run or answered from an
/// earlier one (37, 38).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// Observed by an executor on `host`.
    Ran { host: String },
    /// Read from an earlier run; never an observation (16, 38).
    Replayed { from_run_at: Option<String> },
}

/// One check's result in a run: its key is `(task, hash)` (glossary
/// "key").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskResult {
    pub task: String,
    pub hash: String,
    pub outcome: Outcome,
    pub source: Source,
    /// Clause 119: how long the runner says the task
    /// took, in milliseconds; a replay's is what restoring it took.
    /// Shown, never read as an outcome, a replay or a flake.
    pub took_ms: Option<u64>,
    /// Clause 119: how many times the runner tried it.
    pub attempts: Option<u32>,
}

/// Clause 119: what a runner says of a whole run beyond
/// its results. Recorded with the run's result and shown; nothing reads a
/// verdict from it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunDetail {
    /// How long the run took, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub took_ms: Option<u64>,
    /// How long the runner estimates it would have taken with nothing
    /// replayed, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate_ms: Option<u64>,
    /// The files the runner took as changed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<String>,
}

impl RunDetail {
    /// The time the runner estimates the run saved, never below zero.
    pub fn saved_ms(&self) -> Option<u64> {
        Some(self.estimate_ms?.saturating_sub(self.took_ms?))
    }

    pub fn is_empty(&self) -> bool {
        self == &RunDetail::default()
    }
}

/// A check a run did not judge: its program could not be started, or it
/// exited as the policy's `not_run` names (could-not-run-1 114). It has
/// no outcome (60): no result under its key, and it is run again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotRun {
    pub task: String,
    /// Why, for the person told of it (20).
    pub reason: String,
    /// Design D3 of the change `a-signed-run-without-a-sandbox-proves-its-
    /// passes` (114; record 0105): for a pass the run kept no proof of, the
    /// key of the task runner's record that is missing, the task's own or,
    /// as its reason names, a task's it depends on. Set only where the
    /// check profile names one; the reason carries no key, so its cause is
    /// logged once for each host and reason (rule core.28).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

/// Design D1 and D3 of the change `a-pass-nothing-proves-blames-no-one`
/// (114, C.6; record 0105): what a finished run's place proves of it.
/// `proven` holds the tasks whose keys the candidate's tree bears out, its
/// checks and the tasks a check was failed by alike (149). `unkept` holds
/// each ran check that passed whose proof the run kept none of, with why:
/// it has no outcome, never a fail. A task in neither list is no pass.
/// `refused` holds, with why, each ran task the check profile read a
/// record of and found its key not borne out by the tree for any cause
/// but a missing record (C.6, 20; design D5 of the change
/// `a-signed-run-without-a-sandbox-proves-its-passes`): it is in neither
/// other list, and the submission blamed for it says why.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Proof {
    pub proven: Vec<String>,
    pub unkept: Vec<NotRun>,
    pub refused: Vec<Refused>,
}

/// C.6, 20: a ran task whose key the candidate's tree does not bear out,
/// for a cause other than a missing record. `why` is the check profile's
/// words for it, naming the task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub task: String,
    pub key: String,
    pub why: String,
}

/// 149: a required check the runner skipped because the tasks `by` it
/// depends on ran and failed; its outcome is theirs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedBy {
    pub task: String,
    pub by: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunReport {
    pub tasks: Vec<TaskResult>,
    /// 84: what the executor held the run to.
    pub executor: Enforced,
    /// 15: required checks the run was to run that it left without a
    /// result, and no task they depend on failed or could not run:
    /// skipped for another reason, never reached, or, in a run its time
    /// limit ended, cut off by it (time-limit-1 66). None of them passes.
    pub missing: Vec<String>,
    /// Checks the run did not judge (could-not-run-1 114): in neither
    /// `tasks` nor `missing`.
    pub not_run: Vec<NotRun>,
    /// 149, 16: the result of each task a check of `failed_by` was failed
    /// by that is no check of the run, recorded under its own key as any
    /// result is; never a replay, since a runner answers no failed task
    /// from its cache.
    pub dependencies: Vec<TaskResult>,
    /// 149: required checks the runner skipped for a task they depend on
    /// that ran and failed, in neither `tasks`, `missing` nor `not_run`.
    pub failed_by: Vec<FailedBy>,
    /// The run hit its time limit and was ended with everything it
    /// started (66).
    pub timed_out: bool,
    /// Clause 119: what the runner says of the run.
    pub detail: Option<RunDetail>,
    /// Required checks the runner keeps out of its CI mode, so never runs
    /// (moon's `runInCI: false`); each is also in `missing`.
    pub kept_out: Vec<String>,
    /// 139 (design D8 of the change
    /// `a-check-its-executor-refused-has-no-outcome`): why the executor
    /// could not read its own report of what it refused the run, in the
    /// words of what failed; the run is read as one refused nothing. `None`
    /// where it read it, and wherever it was not asked.
    pub refusals_unread: Option<String>,
}

/// 139 (design D6 of the change
/// `a-check-its-executor-refused-has-no-outcome`): what the executor
/// reports it refused a finished run, read from its own report alone,
/// never from what a check printed, its exit or how it failed (38; rule
/// core.35).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusals {
    /// It refused nothing, or keeps no report of what it refuses: the open
    /// executor, and a run an older sluice started.
    None,
    /// What it refused, in the profile's words.
    Refused(String),
    /// It could not read its own report, in the words of what failed; the
    /// run is read as one refused nothing, and says so (design D8).
    Unread(String),
}

impl RunReport {
    /// A check failed on its own result, was failed by a task it depends
    /// on, or was left without a result (15, 149); a check not run is
    /// none of them (could-not-run-1 114). Every reader of a run reads
    /// it, so none counts a check failed by a dependency as passed.
    pub fn failed(&self) -> bool {
        !self.missing.is_empty()
            || !self.failed_by.is_empty()
            || self.tasks.iter().any(|t| t.outcome == Outcome::Fail)
    }
}

/// ADR 0050: a task a branch's gate requires that the runner keeps out
/// of its CI mode, so never runs, as the submitting worktree has it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptOutTask {
    /// The task, as the runner names it.
    pub task: String,
    /// The gate's required entry that names it, as the policy writes it:
    /// the task itself, or a tag it carries.
    pub entry: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress {
    Running,
    Finished(RunReport),
    /// The run's executor stopped before reporting: no outcome (60).
    Lost,
}

/// Where one task of a run still going has got, as its runner tells it.
/// No run and no result (16, 38): no verdict, gate, blame or result reads
/// it (rules core.7, core.9; ADR 0028).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskProgress {
    /// The task, as the runner names it.
    pub target: String,
    pub state: TaskState,
    /// When it started, RFC 3339, where the runner says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<String>,
    /// When it ended, RFC 3339, once it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<String>,
    /// Its exit code, once it has ended and was run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskState {
    Waiting,
    Running,
    Passed,
    Failed,
    /// Answered from the runner's cache, as the runner says (38).
    Replayed,
}

impl TaskState {
    /// Whether the task has ended, however.
    pub fn ended(self) -> bool {
        matches!(self, Self::Passed | Self::Failed | Self::Replayed)
    }
}

pub trait Tests {
    /// 36, 37, 83: start the required checks the candidate affects since
    /// `request.since`, in a place, as one process group under the time
    /// limit and the request's limits (32, 66; rule core.10). 84: a limit
    /// the executor cannot hold is an error naming it, and nothing runs.
    ///
    /// 71 (rule core.49; record 0087): the run takes `part`, its room in
    /// the host's bound that [`Tests::hold`] gave, and keeps it until it
    /// ends; a start that fails gives it back as the part is dropped. A
    /// part the host's room no longer honours is refused with [`AtBound`]
    /// and nothing runs.
    fn start(&self, request: &RunRequest, part: Part) -> Result<RunHandle>;

    /// 71, 150 (rule core.49; records 0087, 0123): a run's room in the
    /// host's bound, counted and held in one step against every
    /// repository's runs, before the decision to start the run is written:
    /// a combination's where a run of the bound is free, owed to no waiting
    /// session and to no repository with fewer runs under test; a
    /// session's or a preview's where a run of the bound less the part kept
    /// for combinations is free and owed to no one. `None` where there is
    /// none: the run's wait is recorded for the host, and nothing is held.
    /// The part is held until [`Tests::start`] takes it, or is dropped,
    /// which gives it back at once. The profile names its own repository
    /// to the host's room, so the core names none.
    fn hold(&self, kind: RunKind) -> Result<Option<Part>>;

    /// 37: a started run's progress; once finished, every check's key,
    /// outcome, executor and whether it was replayed.
    fn progress(&self, run: &RunHandle) -> Result<Progress>;

    /// 84, 87, record 0074: what of `limits` this host cannot meet, in
    /// words. First what its executor cannot hold, the executor kind,
    /// memory or network, as [`Tests::start`] would refuse it, which looks
    /// at nothing of the host; then, where `limits.docker` asks for one, a
    /// container engine that answers: the places it looked and what each
    /// said. `None` when it meets them all, with no engine looked for where
    /// none is asked. It reads only what the executor can see on the host
    /// for itself, where the run would reach it, and runs nothing of the
    /// repository's (rules core.37, core.38).
    ///
    /// The core asks it before it claims a run, and claims none while it
    /// answers a reason; before it starts a run claimed without that look
    /// (88); and after a run in which a check did not pass, last of the
    /// three, once [`Tests::switched`] and [`Tests::refused`] answered
    /// nothing, so that an engine gone during the run leaves those checks
    /// with no outcome, never a fail (114, 139).
    fn unmet(&self, limits: &Limits) -> Result<Option<String>>;

    /// 84, 114, 139, record 0097: whether the host's system moved since
    /// `run` began, in the profile's words, else `None`. It reads only what
    /// the executor can see on the host for itself, as the run began and
    /// now, and runs nothing (rules core.37, core.38); a host whose system
    /// it cannot read, or a run that noted none as it began, answers
    /// `None`, read as before.
    ///
    /// The core asks it only after a run in which a check did not pass and
    /// the time limit did not end it, whatever the policy asks of the host,
    /// first of the three, before [`Tests::refused`] and the engine: where
    /// it answers, those checks have no outcome, for the reason it gives,
    /// and neither is asked (139; record 0072).
    fn switched(&self, run: &RunHandle) -> Result<Option<String>>;

    /// 139, record 0072 (design D6 of the change
    /// `a-check-its-executor-refused-has-no-outcome`): what the executor
    /// reports it refused `run`, read from its own report of the run alone,
    /// never from what a check printed, its exit or how it failed (38; rule
    /// core.35). It runs nothing, so reading a run again reads the same
    /// answer.
    ///
    /// The core asks it only after a run in which a check did not pass and
    /// the time limit did not end it, after [`Tests::switched`] and before
    /// it looks at the engine: where it answers `Refused`, those checks
    /// have no outcome, naming what was refused, and the engine is not
    /// looked at; where it answers `Unread`, the run is read as before and
    /// says so.
    fn refused(&self, run: &RunHandle) -> Result<Refusals>;

    /// 71: the most checks running on the host at once, whatever
    /// repositories and branches they serve and whatever their windows.
    /// `running` counts them.
    fn bound(&self) -> Result<usize>;

    /// 87: the system this instance's executor runs on, so a run is taken
    /// only where its system is.
    fn platform(&self) -> Result<Platform>;

    /// 88, 107: start one job on a pool for a run no standing host can
    /// take; its start is its claim (ADR 0034).
    fn launch(&self, launch: &Launch) -> Result<()> {
        anyhow::bail!("no pool is bound for {}", launch.pool)
    }

    /// 77, 88: whether the job under `claim` that pool `kind` was given
    /// still runs or waits to; one its pool finished, cancelled or lost
    /// without a report is a lapsed claim.
    fn job_alive(
        &self,
        _kind: &(String, Setting),
        _claim: &str,
        _remote: Option<&str>,
    ) -> Result<bool> {
        Ok(false)
    }

    /// 111: ask a pool to advance `branch`, when work is written that no
    /// live instance will take. A wake that is lost loses nothing (59).
    fn wake(&self, _kind: &(String, Setting), _branch: &str, _remote: Option<&str>) -> Result<()> {
        Ok(())
    }

    /// 71, session-runs-1 clause 89: the checks running on this host, by
    /// kind, every repository's, each counting against the bound. A
    /// running check is the host's, never a branch's, so the check seam
    /// counts it beside the bound and the records never hold the count.
    fn running(&self) -> Result<Running>;

    /// Design D9 (C.6): what a finished run's place proves of it, read
    /// while its place still exists, running nothing, in one walk of what
    /// the run kept. `proven` names the tasks whose keys the candidate's
    /// tree bears out, its checks and the tasks a check was failed by alike
    /// (149); a signer signs only these, so no check signs a pass under a
    /// key it does not hold. `unkept` names each ran check that passed
    /// whose proof the run kept none of, with why read from that fact, in
    /// the profile's words: it has no outcome (114; record 0105). A fail,
    /// a task a check was failed by and a replay are never unkept, and a
    /// task in neither list is no pass.
    fn proven(&self, run: &RunHandle, report: &RunReport) -> Result<Proof>;

    /// ADR 0035: the session runs running on this host, as `running`
    /// counts them, each with what its `sluice exec` kept of it: for a
    /// person to read, never a run or a result (rule core.18).
    fn session_runs(&self) -> Result<Vec<HostRun>>;

    /// 20: what each of `tasks` printed in a finished run, while its place
    /// still exists: the run's own output for a task with none of its
    /// own. Only the end of a long output is kept.
    fn logs(&self, run: &RunHandle, tasks: &[String]) -> Result<Vec<TaskLog>>;

    /// ADR 0028: where each task of a run still going has got, for a
    /// person to read; empty when the runner says nothing until the run
    /// ends. Never a result (37, 38).
    fn tasks(&self, run: &RunHandle) -> Result<Vec<TaskProgress>>;

    /// ADR 0050: which of `required`, the branch's required checks as its
    /// policy names them, the runner keeps out of its CI mode in
    /// `worktree`, by the runner's settings in `tests`: each such task
    /// with the required entry that names it. A query, run in the
    /// worktree outside any executor; it runs no check. A runner with no
    /// CI mode keeps nothing out.
    fn kept_out(
        &self,
        _worktree: &Path,
        _tests: &Settings,
        _required: &[String],
    ) -> Result<Vec<KeptOutTask>> {
        Ok(Vec::new())
    }

    /// 140, 36: the check's affected reading, asked outside any run: for
    /// each of `reading.pairs`, which of the required checks the change
    /// from its first revision to its second reads, in pairs' order. A
    /// change to what the runner names as its own setting (its
    /// configuration, the toolchain) reads [`Reads::Every`]. A query: it
    /// takes no place, runs no check and counts nothing against the
    /// host's bound (13, 71). A runner that cannot say reads every check,
    /// so nothing it serves lands beside the chain.
    fn reads(&self, reading: &Reading) -> Result<Vec<Reads>> {
        Ok(vec![Reads::Every; reading.pairs.len()])
    }

    /// 147: equip `equip.working_copy` by the runner `equip.tests` names,
    /// through `equip.through`, as the check profile equips every working
    /// copy before it runs anything there: each step run outside any
    /// executor, with a run's environment, and the sequence stopped at
    /// the first that fails. It takes no place, admits nothing against
    /// the host's bound (71), and keeps nothing once it answers. A step
    /// that failed is answered as a step, the last of [`Equipped`]'s,
    /// never as an `Err`, which is sluice's own failure. A check profile
    /// with nothing to equip answers no steps.
    fn equip(&self, _equip: &Equip) -> Result<Equipped> {
        Ok(Equipped::default())
    }
}

/// 147: how far a working copy is equipped. A working copy sluice only
/// reads in, to ask the runner a question, is equipped through
/// `Reading`; one a check or a command runs in, through `Running`, which
/// holds every step of `Reading` and then the runner's setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Through {
    /// Up to the runner's version check.
    Reading,
    /// Through the runner's setup.
    Running,
}

/// 147: what [`Tests::equip`] is asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Equip {
    /// The working copy equipped: the top of its worktree.
    pub working_copy: PathBuf,
    /// The policy's `[tests]` table, whose toolchain and runner equip it.
    pub tests: Settings,
    /// How far it is equipped.
    pub through: Through,
}

/// 147: one step of equipping a working copy, as it ran.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EquipStep {
    /// The step's name, as sluice runs it: `toolchain`, or the runner's.
    pub step: String,
    /// Whether it passed.
    pub passed: bool,
    /// What it printed, its output then its error stream; for one that
    /// failed, sluice's reason before them.
    pub words: String,
}

/// 147: the steps an equip ran, in order; the last failed where any did,
/// and none ran after it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Equipped {
    /// Each step run.
    pub steps: Vec<EquipStep>,
}

impl Equipped {
    /// The step that failed, where one did.
    pub fn failed(&self) -> Option<&EquipStep> {
        self.steps.iter().find(|s| !s.passed)
    }
}

/// 140: what the core asks of the check's affected reading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reading {
    /// The checks the branch's gate requires, as the policy names them,
    /// read at the branch's current revision (56).
    pub required: Vec<String>,
    /// The policy's `[tests]` table at the branch's current revision.
    pub tests: Settings,
    /// Each change asked of: from a revision to that revision extended
    /// by the work.
    pub pairs: Vec<(Rev, Rev)>,
}

/// 140: which required checks one change reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reads {
    /// These, by the runner's names; none when empty.
    Checks(Vec<String>),
    /// Every required check: the runner cannot narrow this change.
    Every,
}

impl Reads {
    /// Whether no required check reads the change.
    pub fn none(&self) -> bool {
        matches!(self, Reads::Checks(c) if c.is_empty())
    }
}

/// The checks running on the host, every repository's (71).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Running {
    /// Submissions' runs: combinations under test.
    pub queue: usize,
    /// Session runs (`sluice exec`) and previews' commands.
    pub sessions: usize,
}

impl Running {
    /// Every check running on the host.
    pub fn total(&self) -> usize {
        self.queue + self.sessions
    }
}

/// What a part of the host's bound is held for (71, 150; record 0087).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunKind {
    /// A submission's combination.
    Queue,
    /// A session's run (`sluice exec`) or a preview's command.
    Session,
}

/// 71 (rule core.49; record 0087): a run's room in the host's bound, held
/// from the count that chose it to its start. It is opaque to the core:
/// the check profile that held it reads what it holds back when the run
/// starts. Dropped unstarted, by a start that failed, a decision whose
/// write was refused or a round that stopped, it gives its room back at
/// once, whatever it holds doing so as it drops.
pub struct Part {
    kind: RunKind,
    held: Box<dyn std::any::Any>,
}

impl Part {
    /// A part of `kind`, holding `held`, whose drop gives the room back.
    pub fn new(kind: RunKind, held: impl std::any::Any) -> Part {
        Part {
            kind,
            held: Box::new(held),
        }
    }

    pub fn kind(&self) -> RunKind {
        self.kind
    }

    /// What the part holds, as the profile that held it made it; the part
    /// back where it holds something else.
    pub fn take<T: 'static>(self) -> std::result::Result<T, Part> {
        match self.held.downcast::<T>() {
            Ok(held) => Ok(*held),
            Err(held) => Err(Part {
                kind: self.kind,
                held,
            }),
        }
    }
}

impl fmt::Debug for Part {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Part").field("kind", &self.kind).finish()
    }
}

/// 71: a start refused because the host's count of running checks has
/// reached its bound; nothing of the run ran. The core finds it by
/// downcasting `start`'s error, and the run waits for a later round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtBound;

impl fmt::Display for AtBound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the host is at its bound")
    }
}

impl std::error::Error for AtBound {}

/// A session run going on the host (session-runs-1 clause 89), as the
/// host's state keeps it beside its count (ADR 0035). It decides nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostRun {
    /// The session's place, as its canonical path.
    pub worktree: String,
    /// The branch whose policy the run is held to.
    pub branch: String,
    /// When it started, RFC 3339.
    pub started: String,
    /// The command of a run of a command; `None` for a run of the checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Where each task has got, as its runner says; empty when it says
    /// nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<TaskProgress>,
}

/// What one task printed in a run, or where it is kept.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskLog {
    pub task: String,
    /// The output itself, from the check seam; where it is kept, from the
    /// records seam.
    pub text: String,
}

/// How many failed candidates of a branch keep their logs.
pub const KEPT_LOGS: usize = 20;

/// The most of one task's output kept: its end.
pub const LOG_LIMIT: usize = 64 * 1024;

/// The end of `bytes`, at most `LOG_LIMIT` of them, as text.
pub fn log_tail(bytes: &[u8]) -> String {
    let start = bytes.len().saturating_sub(LOG_LIMIT);
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

// ---------------------------------------------------------------------------
// B.4 The branch
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Formed {
    Revision(Rev),
    /// 12, 43: the content contested.
    Conflict {
        paths: Vec<String>,
    },
}

/// 44: one landing commit. Its tree is the tree of `tree_of`, the
/// candidate that was tested; its parents are `parent` and `head` (23).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LandingCommit {
    pub tree_of: Rev,
    pub parent: Rev,
    pub head: Rev,
    /// The subject and the `Sluice-*` trailers naming the entry's work
    /// and its verdict (23; ADR 0006).
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Moved {
    Moved,
    /// 45: refused, the branch was elsewhere.
    Elsewhere(Option<Rev>),
}

/// What this host holds of a branch apart from the branch itself (124).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostCopy {
    /// The host's ref is the branch: there is no copy apart from it.
    Itself,
    /// The branch is kept elsewhere; this is the host's ref of it, sluice's
    /// copy, and its value, `None` when the host has none.
    At(Option<Rev>),
}

pub trait Branch {
    /// 42: the branch's current revision, or `None` when it does not exist.
    fn read(&self, branch: &str) -> Result<Option<Rev>>;

    /// 129: the branch's revision as the host's own files hold it, with no
    /// fetch: a branch in the host's own refs as [`Branch::read`] reads it,
    /// and a branch on a remote at its revision as last fetched. What a
    /// reader's tick that decides nothing reads; a round, a submission and
    /// a status read with [`Branch::read`]. A reader names it beside the
    /// records it read, so a forecast forms on what that read found (64,
    /// 124).
    fn held(&self, branch: &str) -> Result<Option<Rev>>;

    /// 64, 124: whether this host has `rev`'s objects, so a merge can be
    /// formed on it here with nothing fetched.
    fn holds(&self, _rev: &Rev) -> Result<bool> {
        Ok(true)
    }

    /// 124: what this host holds of the branch apart from it, read on the
    /// host alone, with no fetch.
    fn copy(&self, branch: &str) -> Result<HostCopy>;

    /// 124: set the host's copy of the branch to `to`, only while it holds
    /// `expected`, or is absent when `expected` is `None`; a copy found
    /// elsewhere is reported, never forced (rule core.5). Called only for a
    /// branch whose copy is [`HostCopy::At`].
    fn level(&self, branch: &str, expected: Option<&Rev>, to: &Rev) -> Result<Moved>;

    /// 56: a file of a revision's tree, for the policy read from the
    /// branch's revision.
    fn read_file(&self, rev: &Rev, path: &str) -> Result<Option<Vec<u8>>>;

    /// 43: `head` merged onto `onto`, or the conflict.
    fn form(&self, onto: &Rev, head: &Rev) -> Result<Formed>;

    /// 44: write one landing commit.
    fn commit(&self, commit: &LandingCommit) -> Result<Rev>;

    /// Whether `rev` is `tip` or in its history: a landing found under a
    /// branch moved again since (24).
    fn contains(&self, tip: &Rev, rev: &Rev) -> Result<bool>;

    /// 122: the commit that landed `rev` on a branch at `tip`: the oldest
    /// of `tip`'s first-parent line that holds it, `rev` itself where it is
    /// on that line; `None` when `tip` does not hold it. A profile that
    /// cannot walk the line answers `tip`, which holds it all the same.
    fn landed_by(&self, tip: &Rev, rev: &Rev) -> Result<Option<Rev>> {
        Ok(self.contains(tip, rev)?.then(|| tip.clone()))
    }

    /// 45, 46: move the branch from exactly `expected` to `to`, all or
    /// nothing.
    fn move_to(&self, branch: &str, expected: &Rev, to: &Rev) -> Result<Moved>;

    /// 45, 46, branches-1 104: make the branch at `at`, only while it is
    /// absent; refused with what it holds when present.
    fn make(&self, branch: &str, at: &Rev) -> Result<Moved>;

    /// 45, 46, branches-1 105: remove the branch, only while it is at
    /// `expected`.
    fn remove(&self, branch: &str, expected: &Rev) -> Result<Moved>;
}

// ---------------------------------------------------------------------------
// B.5 The driver
// ---------------------------------------------------------------------------

/// Who submitted: the agent and its worktree (3; answers 20).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Submitter {
    pub agent: String,
    /// The session's work, as its driver names it: a branch, or a pull
    /// request's `pull/<n>`.
    pub worktree: String,
    /// The absolute path of the worktree it was submitted from, for a
    /// reader to match a submission to a session; `None` where the driver
    /// has no worktree on this host, and in records written before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The branch a pull request was opened from, for a reader to match
    /// it to the worktree that pushed that branch; `None` for a worktree's
    /// own submission, and in records written before it (ADR 0025).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    /// 51: the host the submit ran on, as its peers name it, so a round on
    /// another host pushes nothing into a session it cannot reach; `None`
    /// in records written before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// 51: the session that submitted, as the driver names it, for the
    /// round that decides the outcome to push it there; the core shows it
    /// and never reads into it (rule core.1). `None` for a submit run in no
    /// session the driver can reach, and in records written before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

impl Submitter {
    /// 124: a submission that named a branch sluice lands on as its head,
    /// from no worktree: its head is its work, the branch (record 0090).
    /// A worktree's own submission names no head (ADR 0025), and a pull
    /// request's work is its `pull/<n>`, so nothing else has both equal.
    pub fn by_name(&self) -> bool {
        self.head.as_deref() == Some(self.worktree.as_str())
    }
}

/// A session's place as its driver sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub head: Rev,
    pub from: Submitter,
    /// Everything the place holds beyond its commits (2): a submission
    /// from a place holding anything here is refused.
    pub dirty: Vec<String>,
    /// The place is on no branch, so it names no session's work (3): a
    /// submission from it is refused.
    pub detached: bool,
}

/// One entry contested in a conflict, with what it submitted (65).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contested {
    pub id: u64,
    pub from: Submitter,
}

/// 23, 70, 140: the verdict that covers a branch's revision: the
/// revision of the branch whose landing named it, and its words. A
/// combination that lands is the same combination on the revision it
/// lands as (core.17), so that revision is where the verdict stands on
/// the branch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoveringVerdict {
    /// The branch's revision that landed on it (core.17).
    pub at: Rev,
    /// Its result word, `passed` (22).
    pub result: String,
    pub ran: u64,
    pub replayed: u64,
}

/// 20, 51: a submission's decided outcome, which a notification carries to
/// its session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decided {
    Landed {
        landed_as: Rev,
        /// 140: landed beside the chain, with no check run.
        beside: bool,
        /// 140: for a landing beside, the verdict that covers the
        /// revision it landed on; `None` for a landing through the chain
        /// and in outcomes recorded before it (record 0083).
        covered_by: Option<CoveringVerdict>,
    },
    Failed {
        failed: Vec<String>,
        combination: Rev,
        submissions: Vec<u64>,
        timed_out: bool,
        /// Where each failed task's output is kept (20).
        logs: Vec<TaskLog>,
    },
    Conflict {
        paths: Vec<String>,
        with: Vec<Contested>,
    },
}

impl Decided {
    pub fn word(&self) -> &'static str {
        match self {
            Decided::Landed { .. } => "landed",
            Decided::Failed { .. } => "failed",
            Decided::Conflict { .. } => "conflict",
        }
    }
}

// ---------------------------------------------------------------------------
// A submission's report (49; 118, 119; ADR 0044)
// ---------------------------------------------------------------------------

/// What a submission's report says.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerdictView {
    pub branch: String,
    pub id: u64,
    pub standing: Standing,
    /// The combination its rows are of, and the branch revision it
    /// extends; `None` when it shows none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combination: Option<Rev>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<Rev>,
    /// The submissions tested in it, in chain order (10).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub submissions: Vec<u64>,
    /// What the task runner said of each run, by system (119).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<RunShown>,
    /// One per required check on each system: failed, flaky, could not
    /// run, missing, timed out and set aside first, then by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<Row>,
    /// ADR 0028: while its combination is under test, its tasks as last
    /// kept on the host that runs it; no result (16, 38).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<TaskProgress>,
    /// While its combination is under test, the checks of its runs not
    /// yet finished, each on its system: what a check per check shows in
    /// progress.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<(String, Option<String>)>,
}

/// Where the submission stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Standing {
    /// In the queue, behind `ahead` (live submissions before it).
    Queued {
        ahead: Vec<u64>,
    },
    /// Waiting on an `after` submission (7).
    Waiting,
    /// In a combination under test.
    Testing,
    /// Its combination passed; the combinations ahead have yet to (18).
    Passed,
    Landing,
    Landed {
        landed_as: Rev,
        /// 140: landed beside the chain, with no check run.
        beside: bool,
        /// 140: for a landing beside, the verdict that covers the revision
        /// it landed on, which its report names as its landing commit
        /// does; `None` otherwise, and in records written before it (70;
        /// record 0083).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        covered_by: Option<CoveringVerdict>,
    },
    /// Blamed for the failure its rows show (17, 20).
    Blamed {
        timed_out: bool,
    },
    Conflict {
        paths: Vec<String>,
        with: Vec<Contested>,
    },
    /// Tested with a submission blamed for the failure: its combination is
    /// formed again, and nothing of the failed one is shown (20; proposed
    /// 118).
    FormedAgain,
    Withdrawn,
}

/// What a run's runner said of it, on one system.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunShown {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub detail: RunDetail,
}

/// One required check on one system.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Its key's hash; `None` for a check left with no result, or failed
    /// by a task it depends on (149).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub result: RowResult,
    /// Answered from an earlier pass, not run (38).
    pub replayed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// 78: the signer whose signature counted it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<String>,
    /// 84: what its executor enforced; none for a replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor: Option<Enforced>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub took_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempts: Option<u32>,
    /// 19: the result a flaky reading contradicts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flaky: Option<Contradicted>,
    /// Could-not-run-1 115: why it could not run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_run: Option<String>,
    /// 149: the tasks it depends on that ran and failed, which its runner
    /// skipped it for; each has a row of its own with its key. Empty for a
    /// check that ran, or was left without a result, on its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_by: Vec<String>,
    /// Could-not-run-1 116: why its result was set aside.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_aside: Option<String>,
    /// 20: where what it printed is kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RowResult {
    Failed,
    Flaky,
    NotRun,
    Missing,
    TimedOut,
    SetAside,
    Passed,
}

/// 19: the recorded result a flaky reading contradicts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contradicted {
    pub outcome: Outcome,
    pub at: String,
    pub combination: Rev,
    /// The session's worktree, for a session run (session-runs-1 92).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
}

/// 50, 51: an outcome for one entry's session. `id` names the entry and
/// the outcome, so a repeat after a crash is known as one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub id: String,
    pub submission: u64,
    pub branch: String,
    pub to: Submitter,
    pub decided: Decided,
    /// 49: the entry's report as its records hold it when decided, for a
    /// driver that shows one (ADR 0044); `None` when it could not be read.
    pub report: Option<VerdictView>,
}

/// 51: whether a notify handed its outcome to its reader, which alone is
/// recorded as a hand-over (weir 103; record 0102; design D5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Handed to the reader by `way`, reaching `to`, as the carrier names
    /// it; `confirms` where its reader confirms on receipt (weir 103).
    Handed {
        way: String,
        to: String,
        confirms: bool,
    },
    /// Not handed over, and why, in the carrier's words.
    Undelivered { why: String },
}

pub trait Driver {
    /// 2, 47: the head of a session's place, who holds it, and anything
    /// the place holds beyond its commits.
    fn session(&self, worktree: &str) -> Result<Session>;

    /// 51: who collects outcomes in `folder`: the submitter a submit there
    /// would record, read from neither the folder's files nor its `HEAD`'s
    /// commit, so a folder with no files checked out, or whose branch has
    /// no commit yet, is read as the worktree on the branch its `HEAD`
    /// names with no path; `None` where `HEAD` names no branch, a place that
    /// submitted nothing (2, 3; record 0099; rule core.48). A driver whose
    /// `session` reads no commit keeps this one.
    fn recipient(&self, folder: &str) -> Result<Option<Submitter>> {
        let s = self.session(folder)?;
        Ok((!s.detached).then_some(s.from))
    }

    /// 124: who submits `branch` by name: the agent the session's harness
    /// names, else the branch; the branch as its work and its head; this
    /// host; no worktree and no session, wherever the command runs, so
    /// nothing of the folder it runs in is read (record 0090; rule
    /// core.50).
    fn named(&self, branch: &str) -> Result<Submitter>;

    /// 50, 51: hand an outcome to that entry's session and no other, and
    /// say whether it was, by what way and whether its reader confirms on
    /// receipt. A carrier asked that answered no is `Undelivered`, which no
    /// round tries again; `Err` is sluice's own
    /// failure, such as a forge write the retry policy gave up on, and
    /// leaves the outcome for the next round.
    fn notify(&self, notification: &Notification) -> Result<Delivery>;
}

// ---------------------------------------------------------------------------
// B.7 The records
// ---------------------------------------------------------------------------

/// A branch's records as last written, and the version they were read at
/// (61). Files are named by path relative to the branch's record directory;
/// their bytes are the records' real format (53).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub version: u64,
    pub files: BTreeMap<String, Vec<u8>>,
}

/// Where a branch's finished records are kept, beside its live ones and
/// never in a `read` of them (130): a directory whose name no branch's can
/// take, since a git branch's components never start with a dot.
pub const FINISHED: &str = ".finished";

/// One write: whole files replaced, bytes appended to logs, and files
/// removed, all or nothing (63).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub put: BTreeMap<String, Vec<u8>>,
    pub append: BTreeMap<String, Vec<u8>>,
    /// Files the write takes away: a finished submission's live document,
    /// as the same write puts it among the finished records.
    pub remove: BTreeSet<String>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.put.is_empty() && self.append.is_empty() && self.remove.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Written {
    /// The write, and the version it left.
    Written(u64),
    /// 62: refused, the records were written since.
    Refused,
}

/// The branch's lock, held until dropped (ADR 0005; rule core.14).
pub trait Guard {}

pub trait State {
    /// The branch's lock, or `None` when another step holds it.
    fn lock<'a>(&'a self, branch: &str) -> Result<Option<Box<dyn Guard + 'a>>>;

    /// The branch's lock, once whatever holds it lets go: for a write
    /// outside a step (a submission, an amendment, a withdrawal), which
    /// waits out a round rather than leave.
    fn wait_lock<'a>(&'a self, branch: &str) -> Result<Box<dyn Guard + 'a>>;

    /// 61: the branch's live records, and the version they were read at:
    /// nothing under [`FINISHED`] (131).
    fn read(&self, branch: &str) -> Result<Snapshot>;

    /// 129: ask each remote that keeps branches' records, once, for the
    /// records of every branch on it and the hosts' signs of life: what a
    /// reader's tick does first. The same ask brings each branch's
    /// revision, the work submitted to the remote and the combinations
    /// formed on its branches, so what a reader writes is all a forecast
    /// forms from (64). Records in the host's own files need no asking. A
    /// remote that refuses leaves every branch on it read as last fetched,
    /// [`State::stale`] naming the refusal, and fails nothing.
    fn refresh(&self) -> Result<()>;

    /// 129: the branch's records as the host's own files hold them, after
    /// the last [`State::refresh`], with no fetch: what a reader's tick
    /// that decides nothing reads. A round reads with [`State::read`].
    fn read_held(&self, branch: &str) -> Result<Snapshot>;

    /// 80: why the branch's last read could not reach the records it keeps
    /// elsewhere and read what was last fetched; `None` when the read was
    /// current.
    fn stale(&self, _branch: &str) -> Option<ReadRefused> {
        None
    }

    /// 133: this host's document of a standing refusal of `branch`'s
    /// remote, as last kept; kept on this host alone, beside the branch's
    /// records and never in them, so it decides nothing.
    fn refusal(&self, _branch: &str) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    /// Keep `doc` as this host's document of a refusal of `branch`'s
    /// remote, in place of the one kept before, or, with `None`, remove it.
    fn keep_refusal(&self, _branch: &str, _doc: Option<&[u8]>) -> Result<()> {
        Ok(())
    }

    /// 77, 107: rewrite this host's sign of life, `beat` its document:
    /// its signer, its systems and the room it has. Only this host writes
    /// its own.
    fn beat(&self, _host: &str, _beat: &[u8]) -> Result<()> {
        Ok(())
    }

    /// 77: every host's last sign of life, by host: something that changes
    /// with every beat, and the beat's document.
    fn beats(&self) -> Result<BTreeMap<String, (String, Vec<u8>)>> {
        Ok(BTreeMap::new())
    }

    /// 77: the beat of `host` this host last saw, and when it first saw
    /// it, by its own clock.
    fn observed_beat(&self, _host: &str) -> Result<Option<(String, String)>> {
        Ok(None)
    }

    /// Note that this host first saw `host`'s beat `marker` at `at`.
    fn observe_beat(&self, _host: &str, _marker: &str, _at: &str) -> Result<()> {
        Ok(())
    }

    /// 73: the version of the branch's records this host last saw, and
    /// when it first saw it, by its own clock; kept on this host alone,
    /// so it decides nothing but when to try taking the branch.
    fn observed(&self, _branch: &str) -> Result<Option<(u64, String)>> {
        Ok(None)
    }

    /// Note that this host first saw `version` of the branch's records at
    /// `at`.
    fn observe(&self, _branch: &str, _version: u64, _at: &str) -> Result<()> {
        Ok(())
    }

    /// 82: make a submitted head fetchable by every host that may run its
    /// checks or keep its branch, before its submission is written.
    fn publish_head(&self, _branch: &str, _head: &Rev) -> Result<()> {
        Ok(())
    }

    /// 82: a head whose submission has left the queue.
    fn unpublish_head(&self, _branch: &str, _head: &Rev) -> Result<()> {
        Ok(())
    }

    /// 82: fetch every head submitted to the branch from another host, so
    /// combinations of them can be formed here.
    fn fetch_heads(&self, _branch: &str) -> Result<()> {
        Ok(())
    }

    /// 82: make a candidate open to claims fetchable by every host that
    /// may claim it, before it is opened.
    fn publish_candidate(&self, _branch: &str, _id: &str, _rev: &Rev) -> Result<()> {
        Ok(())
    }

    /// 82: a candidate no longer open to claims.
    fn unpublish_candidate(&self, _branch: &str, _id: &str) -> Result<()> {
        Ok(())
    }

    /// 82: fetch the branch's candidates open to claims, to run them here.
    fn fetch_candidates(&self, _branch: &str) -> Result<()> {
        Ok(())
    }

    /// 82, C.6: fetch the one candidate `id` of the branch, by its own
    /// ref, so a place can be made at its revision here, however the
    /// run's claim reached this host (record 0118).
    fn fetch_candidate(&self, _branch: &str, _id: &str) -> Result<()> {
        Ok(())
    }

    /// The submissions this host had queued on the branch in its own
    /// records, when the branch's records moved to the remote and another
    /// host's queue was there first (design D10): each a submission's
    /// document, to be submitted again under a new id.
    fn left_behind(&self, _branch: &str) -> Result<Vec<Vec<u8>>> {
        Ok(Vec::new())
    }

    /// The submissions of [`State::left_behind`] by their old ids, once
    /// submitted again.
    fn forget_left_behind(&self, _branch: &str, _ids: &[u64]) -> Result<()> {
        Ok(())
    }

    /// 61, 131: one file of the branch's finished records, by its name
    /// under [`FINISHED`], as last written.
    fn read_finished(&self, branch: &str, name: &str) -> Result<Option<Vec<u8>>>;

    /// Every file of the branch's finished records, by its name under
    /// [`FINISHED`], for a reader that asks for the branch's history.
    fn read_all_finished(&self, branch: &str) -> Result<BTreeMap<String, Vec<u8>>>;

    /// 62, 63: write all or nothing, only from the version read.
    fn write(&self, branch: &str, read_at: u64, changes: Changes) -> Result<Written>;

    /// Every branch with records.
    fn branches(&self) -> Result<Vec<String>>;

    /// The next submission number for `branch`, from the count of the
    /// records that hold it, so it repeats no number the branch already
    /// holds (26; record 0114; rule core.41).
    fn allocate_id(&self, branch: &str) -> Result<u64>;

    /// 69: the repository's results log, one JSON document per line, as
    /// `branch`'s gate reads it: the branch is the one whose gate, session
    /// run or verdict reads the log, and a profile that reads signed runs
    /// verifies them with the signers and keys that branch's own policy
    /// names, never another branch's (78; rule core.22).
    fn results(&self, branch: &str) -> Result<Vec<u8>>;

    /// 78, 87: whether `branch`'s gate at `at`, the revision a round read
    /// it at, would count a run this host signs: the host signs as a signer
    /// the branch's own policy there names, with a key that policy lists
    /// for it, as [`State::results`] verifies the branch's runs. The
    /// signer's name alone, which another branch may give another key, is
    /// not enough (rule core.22). A profile that verifies no signed run
    /// leaves the name to the gate.
    fn verifies_own(&self, _branch: &str, _at: &Rev) -> Result<bool> {
        Ok(true)
    }

    /// 69: append to the repository's results log.
    fn record_results(&self, lines: &[u8]) -> Result<()>;

    /// Could-not-run-1 116: the results set aside, one JSON document per
    /// line, beside the results log and never in it.
    fn set_asides(&self) -> Result<Vec<u8>>;

    /// Could-not-run-1 116: append one set-aside, under the records' lock.
    fn set_aside(&self, line: &[u8]) -> Result<()>;

    /// 20: keep a failed candidate's task outputs, readable with no sluice
    /// running (26); the answer says where each is. Only the last
    /// `KEPT_LOGS` candidates of a branch keep theirs.
    fn keep_logs(&self, branch: &str, candidate: &str, logs: &[TaskLog]) -> Result<Vec<TaskLog>>;

    /// ADR 0028: the tasks of each of the branch's candidates under test,
    /// as last kept, by candidate. Beside the branch's records, never in
    /// a `read` of them, and kept on the host that runs the candidate.
    fn progress(&self, branch: &str) -> Result<BTreeMap<String, Vec<TaskProgress>>>;

    /// Keep a candidate's tasks in place of those kept before, or, with
    /// `None`, remove them. Moves no version (62).
    fn put_progress(
        &self,
        branch: &str,
        candidate: &str,
        tasks: Option<&[TaskProgress]>,
    ) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(note: Option<&str>) -> ReadRefused {
        ReadRefused {
            remote: "origin".into(),
            asked: "refs/heads/main".into(),
            said: "remote: Repository not found.\nfatal: repository 'https://github.com/acme/app.git/' not found".into(),
            note: note.map(str::to_string),
        }
    }

    /// 133: the round's word to whoever ran it is today's: the branch as
    /// git names it, git's words, and the note on the next line.
    #[test]
    fn a_refused_read_says_what_it_said_before() {
        assert_eq!(
            refused(None).to_string(),
            "fetching main from origin failed: remote: Repository not found.\n\
             fatal: repository 'https://github.com/acme/app.git/' not found"
        );
        assert_eq!(
            refused(Some("origin refused a token made 5s ago")).to_string(),
            "fetching main from origin failed: remote: Repository not found.\n\
             fatal: repository 'https://github.com/acme/app.git/' not found\n\
             origin refused a token made 5s ago"
        );
        let state = ReadRefused {
            asked: "refs/sluice/state/main".into(),
            ..refused(None)
        };
        assert!(state
            .to_string()
            .starts_with("fetching refs/sluice/state/main from origin failed: "));
    }

    fn report() -> RunReport {
        RunReport {
            tasks: vec![TaskResult {
                task: "app:test".into(),
                hash: "h1".into(),
                outcome: Outcome::Pass,
                source: Source::Ran { host: "box".into() },
                took_ms: None,
                attempts: None,
            }],
            executor: Enforced {
                kind: "open".into(),
                confined: false,
                processors: None,
                memory: None,
                network: None,
                docker: false,
                grant: None,
            },
            missing: vec![],
            not_run: vec![],
            dependencies: vec![],
            failed_by: vec![],
            timed_out: false,
            detail: None,
            kept_out: vec![],
            refusals_unread: None,
        }
    }

    /// 15, 114, 149: a fail, a check failed by a task it depends on and a
    /// check left without a result each fail a run; a check not run alone
    /// does not.
    #[test]
    fn a_run_fails_on_a_fail_a_failed_dependency_or_a_missing_check() {
        assert!(!report().failed());
        let mut fail = report();
        fail.tasks[0].outcome = Outcome::Fail;
        assert!(fail.failed());
        let by = RunReport {
            failed_by: vec![FailedBy {
                task: "root:all".into(),
                by: vec!["app:test".into()],
            }],
            ..report()
        };
        assert!(by.failed());
        let missing = RunReport {
            missing: vec!["root:all".into()],
            ..report()
        };
        assert!(missing.failed());
        let not_run = RunReport {
            not_run: vec![NotRun {
                task: "root:all".into(),
                reason: "exited 77".into(),
                key: None,
            }],
            ..report()
        };
        assert!(!not_run.failed());
    }

    /// The core finds a refused read under whatever context a caller
    /// added, and finds none in an error that holds none.
    #[test]
    fn a_refused_read_is_found_under_added_context() {
        let e = anyhow::Error::new(refused(None))
            .context("reading main")
            .context("a round of main");
        assert_eq!(read_refused(&e), Some(&refused(None)));
        assert!(format!("{e:#}").ends_with("not found"), "{e:#}");
        assert_eq!(read_refused(&anyhow::anyhow!("main does not exist")), None);
    }

    /// 124: a submission by name is told by its form alone, its head its
    /// work; a worktree's own names no head, and a request's work is its
    /// `pull/<n>` (record 0090).
    #[test]
    fn a_submission_by_name_is_its_head_as_its_work() {
        let from = |worktree: &str, head: Option<&str>| Submitter {
            agent: "agent".into(),
            worktree: worktree.into(),
            path: None,
            head: head.map(str::to_string),
            host: None,
            session: None,
        };
        assert!(from("bolt/x", Some("bolt/x")).by_name());
        assert!(!from("bolt/x", None).by_name());
        assert!(!from("pull/7", Some("bolt/x")).by_name());
        assert!(!from("unit/a", Some("bolt/x")).by_name());
    }
}
