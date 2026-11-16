//! The records' real format (53; walkthrough section 7; ADR 0007).
//!
//! A branch's records are a directory of files:
//!
//! - `branch.json`: the queue's own state, one document;
//! - `submissions/<id>.json`: one document per submission;
//! - `log.jsonl`: every submission, run, result, blame and landing, one
//!   event per line, appended and never rewritten (16);
//! - `deliveries.jsonl`: each decided outcome a reader is owed, as weir's
//!   delivery records, one to a line in C.8's JSON, in the order added
//!   (51; record 0102);
//! - `home.json`: where the branch's queue lives, whether this home is
//!   open, moving or closed, and where a move took it (record 0122; C.6;
//!   design D1 of `a-queues-home-is-written-in-its-records`).
//!
//! and beside every branch, one `results.jsonl` for the repository: every
//! check's outcome under its key, for any branch's gate to read (69).
//!
//! Every JSON document names its schema. A log's lines keep the
//! walkthrough's shape, with no schema of their own: `branch.json` names
//! the log's (`log`). See `docs/plans/phase-1-questions.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sluice_seams::{
    Contested, CoveringVerdict, Decided, Enforced, NotRun, Outcome, Rev, RunDetail, RunHandle,
    Submitter, TaskLog,
};

pub const SUBMISSION_SCHEMA: &str = "sluice/submission@1";
pub const BRANCH_SCHEMA: &str = "sluice/branch@1";
pub const LOG_SCHEMA: &str = "sluice/log@1";
pub const RESULT_SCHEMA: &str = "sluice/result@1";
pub const SET_ASIDE_SCHEMA: &str = "sluice/set-aside@1";
pub const HOME_SCHEMA: &str = "sluice/home@1";

fn log_schema() -> String {
    LOG_SCHEMA.into()
}

fn result_schema() -> String {
    RESULT_SCHEMA.into()
}

pub const BRANCH_FILE: &str = "branch.json";
pub const LOG_FILE: &str = "log.jsonl";
/// Record 0122 (design D1): the home document, beside `branch.json` in
/// each home of the branch.
pub const HOME_FILE: &str = "home.json";
/// 51: the branch's deliveries, weir's records in C.8's JSON (record 0102;
/// design D3).
pub const DELIVERIES_FILE: &str = "deliveries.jsonl";

pub fn submission_file(id: u64) -> String {
    format!("submissions/{id}.json")
}

/// The live index of a branch's finished submissions, one line each.
pub const FINISHED_INDEX: &str = "finished.jsonl";

/// A finished submission's name under the finished records.
pub fn finished_name(id: u64) -> String {
    format!("{id}.json")
}

/// A finished submission's file, as a write puts it.
pub fn finished_file(id: u64) -> String {
    format!("{}/{}", sluice_seams::FINISHED, finished_name(id))
}

/// What the live records keep of a submission moved to the finished
/// ones (130): enough to order a chain whose `after` names it (7) and to
/// count a worktree's attempts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinishedLine {
    pub id: u64,
    pub state: State,
    pub worktree: String,
    /// Its submission's subject (23), for a reader to name it by (26);
    /// `None` where its submit gave none, and in lines written before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}

impl FinishedLine {
    /// What a person is shown it by (26): as [`Submission::name`].
    pub fn name(&self) -> &str {
        self.subject.as_deref().unwrap_or(&self.worktree)
    }
}

// ---------------------------------------------------------------------------
// A submission
// ---------------------------------------------------------------------------

/// The states a submission is in, in the walkthrough's words (26, 49).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// In the queue, not under test.
    Queued,
    /// An `after` submission is neither landed nor ahead of it (7).
    Waiting,
    /// In a combination under test.
    Testing,
    /// Its combination passed; it lands next.
    Passed,
    /// Its landing is written and the branch is being moved.
    Landing,
    Landed,
    /// Blamed for a failure (17).
    Failed,
    Conflict,
    Withdrawn,
}

impl State {
    /// Left the queue for good (5).
    pub fn is_final(self) -> bool {
        matches!(self, State::Landed | State::Withdrawn)
    }

    /// Out of every chain: final, or parked as blamed or conflicting
    /// until withdrawn (12, 21).
    pub fn is_out(self) -> bool {
        matches!(
            self,
            State::Landed | State::Withdrawn | State::Failed | State::Conflict
        )
    }

    pub fn word(self) -> &'static str {
        match self {
            State::Queued => "queued",
            State::Waiting => "waiting",
            State::Testing => "testing",
            State::Passed => "passed",
            State::Landing => "landing",
            State::Landed => "landed",
            State::Failed => "failed",
            State::Conflict => "conflict",
            State::Withdrawn => "withdrawn",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tested {
    pub combination: Rev,
    pub on: Rev,
    pub submissions: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedTask {
    pub task: String,
    /// Empty for a required check no run gave a key: left without a
    /// result (15), or failed by a task it depends on (149).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hash: String,
    /// Where what it printed is kept (20).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
    /// 19: its key held both a pass and a fail, which the branch's
    /// `flaky` reads as failed; `None` for a task that failed on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flaky: Option<FlakyKey>,
    /// The system it failed on, or was missing on (85, 87); `None` when
    /// the policy names no systems.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Could-not-run-1 115: the check could not run in as many runs in a
    /// row as the bound, and why it last could not; or, record 0074, no
    /// host took its run before the time limit, each that turned it down
    /// for what it lacks named with why (66, 87). `None` for a check with
    /// a result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_run: Option<String>,
    /// 149: the tasks this required check depends on that ran and failed,
    /// which its runner skipped it for; each is in the same list with its
    /// key. Empty for a check that failed, or was left without a result,
    /// on its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_by: Vec<String>,
    /// C.6, 20: the check profile's words for why the combination's tree
    /// does not bear this check's key out, for a cause other than a
    /// missing record, on a branch that reads a key only where the tree
    /// bears it out; `None` for a check that failed on its result, and in
    /// records written before it (design D5 of
    /// `a-signed-run-without-a-sandbox-proves-its-passes`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsigned: Option<String>,
}

/// 108: a replay no counted pass proves, with its key and why none does,
/// the first of design D4's five that holds (19, 78, 79, 83; design D4 of
/// `a-signed-run-without-a-sandbox-proves-its-passes`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnprovenKey {
    pub task: String,
    pub key: String,
    pub why: String,
}

/// A task read as failed for its key holding a pass and a fail (19): what
/// this run observed, and the newest result recorded at the key that says
/// otherwise (69).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlakyKey {
    /// A pass when the failure is only the key's earlier fail.
    pub here: Outcome,
    /// When this run's result was recorded (19); `None` for a result
    /// with no line, and in records written before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// The host that ran this run's check; `None` for a replay, and in
    /// records written before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub other: Recorded,
}

/// A result recorded under a key (69), as a submitter is pointed at it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recorded {
    pub outcome: Outcome,
    pub at: String,
    pub branch: String,
    pub combination: Rev,
    /// The host that ran it, as its line says; `None` for a line with
    /// none, and in records written before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Session-runs-1 clause 92: the worktree of a session run; `None`
    /// for a combination's run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// Where what that run printed is kept, when the branch's records
    /// name it (20).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictRecord {
    pub paths: Vec<String>,
    pub with: Vec<Contested>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Withdrawal {
    pub by: String,
    pub at: String,
    /// 120, 137: the host that takes the branch's pull requests in, where
    /// this host withdrew the submission at a hand-over of its intake to
    /// it, which holds no request; `None` for any other withdrawal, and in
    /// records written before it (record 0088 as amended).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handed_over_to: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submission {
    pub schema: String,
    pub id: u64,
    pub branch: String,
    pub head: Rev,
    pub from: Submitter,
    pub submitted: String,
    pub after: Vec<u64>,
    pub priority: i64,
    pub attempt: u32,
    pub state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tested: Option<Tested>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<FailedTask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict: Option<ConflictRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdrawn: Option<Withdrawal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landed_as: Option<Rev>,
    /// 122: landed by a landing this queue did not make, found when the
    /// branch it was formed on already held its work; `landed_as` is the
    /// commit that landed it there.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub landed_elsewhere: bool,
    /// Record 0122 (design D5): its number in the home it was carried
    /// from, where a move carried it under another one; written only by a
    /// carry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<u64>,
    /// Record 0122 (design D5): the home it was carried from, `here` or a
    /// remote's name, where its number changed, beside `was`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carried_from: Option<String>,
    /// Passed and waiting to land with a deeper candidate (117).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<Held>,
    /// 23: the submitter's words for what lands, which its landing commit
    /// names it by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// 140: landed beside the chain, or, read at the first round and not
    /// landed so, the entry ahead it waits behind and why; `None` for
    /// work some required check reads at the branch's revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beside: Option<Beside>,
    /// 140: for work landed beside the chain, the verdict that covered the
    /// revision it landed on, which its landing commit names; `None`
    /// otherwise, and in records written before it (70; record 0083).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub covered_by: Option<CoveringVerdict>,
    /// 125: the first head of the act it was submitted in, where the act
    /// gave several; `None` for a submission made alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub act: Option<u64>,
}

/// 140: what a submission's work was read as at the first round that
/// read it. On disk and in JSON, `true` once landed beside the chain, or
/// `{"behind": <n>, "why": "read" | "conflict"}` while it waits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "BesideForm", try_from = "BesideForm")]
pub enum Beside {
    /// Landed beside the chain, with nothing run.
    Landed,
    /// Takes the chain behind entry `behind`, for `why`.
    Behind { behind: u64, why: BesideWhy },
}

/// 140: why work no check reads at the branch's revision takes the chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BesideWhy {
    /// A formed candidate's checks would read it.
    Read,
    /// It does not merge cleanly with the work of the entry ahead.
    Conflict,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum BesideForm {
    Landed(bool),
    Behind { behind: u64, why: BesideWhy },
}

impl From<Beside> for BesideForm {
    fn from(b: Beside) -> BesideForm {
        match b {
            Beside::Landed => BesideForm::Landed(true),
            Beside::Behind { behind, why } => BesideForm::Behind { behind, why },
        }
    }
}

impl TryFrom<BesideForm> for Beside {
    type Error = String;

    fn try_from(f: BesideForm) -> Result<Beside, String> {
        match f {
            BesideForm::Landed(true) => Ok(Beside::Landed),
            BesideForm::Landed(false) => Err("beside is true or an entry it waits behind".into()),
            BesideForm::Behind { behind, why } => Ok(Beside::Behind { behind, why }),
        }
    }
}

impl Submission {
    /// What a person is shown it by (26): its subject, or where its submit
    /// gave none, the branch it was submitted from, as its landing commit
    /// names it (23).
    pub fn name(&self) -> &str {
        self.subject.as_deref().unwrap_or(&self.from.worktree)
    }

    /// 51: the outcome a round delivers of it, read from its record as the
    /// round's outbox item holds it; `None` for a submission that has not
    /// landed, been blamed or met a conflict, which the round delivers
    /// nothing of, and for a record lacking what its outcome names.
    pub fn decided(&self) -> Option<Decided> {
        match self.state {
            State::Landed => Some(Decided::Landed {
                landed_as: self.landed_as.clone()?,
                beside: self.beside == Some(Beside::Landed),
                covered_by: self.covered_by.clone(),
            }),
            State::Failed => {
                let tested = self.tested.as_ref()?;
                Some(Decided::Failed {
                    failed: self.failed.iter().map(|f| f.task.clone()).collect(),
                    combination: tested.combination.clone(),
                    submissions: tested.submissions.clone(),
                    timed_out: self.result.as_deref() == Some("timed out"),
                    logs: self
                        .failed
                        .iter()
                        .filter_map(|f| {
                            f.log.clone().map(|at| TaskLog {
                                task: f.task.clone(),
                                text: at,
                            })
                        })
                        .collect(),
                })
            }
            State::Conflict => {
                let conflict = self.conflict.as_ref()?;
                Some(Decided::Conflict {
                    paths: conflict.paths.clone(),
                    with: conflict.with.clone(),
                })
            }
            _ => None,
        }
    }

    /// 51: the notification its outcome is delivered under, `<id>:<outcome>`,
    /// as the round's outbox names it, so a repeat is known as one; `None`
    /// where [`Submission::decided`] is.
    pub fn notification(&self) -> Option<String> {
        self.decided()
            .map(|decided| format!("{}:{}", self.id, decided.word()))
    }
}

/// What a passed submission waits for under a hold (117): the deeper
/// candidate under test, by its submissions, and when the hold runs out.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    pub by: Vec<u64>,
    pub until: String,
}

// ---------------------------------------------------------------------------
// The branch's own state
// ---------------------------------------------------------------------------

/// A candidate's state: its lifecycle from formed to a verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateState {
    /// Formed and not started.
    Formed,
    /// Its place and run are decided and not yet confirmed started.
    Starting,
    Testing,
    Passed,
    Failed,
    TimedOut,
}

impl CandidateState {
    pub fn has_verdict(self) -> bool {
        matches!(
            self,
            CandidateState::Passed | CandidateState::Failed | CandidateState::TimedOut
        )
    }

    pub fn failed(self) -> bool {
        matches!(self, CandidateState::Failed | CandidateState::TimedOut)
    }

    pub fn under_test(self) -> bool {
        matches!(self, CandidateState::Starting | CandidateState::Testing)
    }

    /// Fixes the order of its submissions: under test or judged (6).
    pub fn pins(self) -> bool {
        self != CandidateState::Formed
    }
}

/// One run of a candidate: the required checks that need one system, or
/// any one system (86).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemRun {
    /// The system its checks must pass on, by the policy's name; `None`
    /// for any one system.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// The checks it runs.
    pub checks: Vec<String>,
    pub state: RunState,
    /// 75: the instance whose claim holds it, on a branch whose records
    /// are on its remote; `None` on a branch kept in one host's files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<RunHandle>,
    /// 108: started again with no replay, after a replay no trusted pass
    /// proved.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_replay: bool,
    /// 84, record 0074 (design D4): each host that turned the run down
    /// for what it lacks of what the run asks of it, a container engine
    /// that answers, with why; a host's entry goes when it claims the run.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub declined: BTreeMap<String, String>,
    /// 114, record 0105 (design D7 of the change
    /// `a-pass-nothing-proves-blames-no-one`): why its last run had no
    /// outcome, for every cause 114 gives, kept from the read that opened
    /// it again until its next report is read; `None` otherwise, and in
    /// records written before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lost: Option<LostRun>,
}

/// 114, record 0105 (design D7): a run that left checks not run and
/// judged nothing, read on `host`, the run's system where the policy names
/// one, and each check it left not run with why, in the run's order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LostRun {
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub not_run: Vec<NotRun>,
}

/// A run's state, from open to its own verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunState {
    /// Waiting for an executor of its system (87).
    Open,
    /// Its place is decided here and its run not yet confirmed started.
    Starting,
    Testing,
    Passed,
    Failed,
    TimedOut,
}

impl RunState {
    pub fn done(self) -> bool {
        matches!(
            self,
            RunState::Passed | RunState::Failed | RunState::TimedOut
        )
    }

    /// Holding a place on this host.
    pub fn held(self) -> bool {
        matches!(self, RunState::Starting | RunState::Testing)
    }
}

/// A candidate: the branch's revision `on` extended, in order, by
/// `submissions` (10). Identified by exactly those two.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "CandidateForm")]
pub struct Candidate {
    pub id: String,
    pub on: Rev,
    pub submissions: Vec<u64>,
    pub revision: Rev,
    pub state: CandidateState,
    /// Its runs, one per system its checks need (86), opened when it is
    /// chosen for testing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<SystemRun>,
    /// When it was chosen for testing: its time limit counts from here
    /// for a run no executor took (66, 87).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<FailedTask>,
    /// Started on a revision covered by no verdict: every required check
    /// had to run (70).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub every_check: bool,
    #[serde(default)]
    pub ran: usize,
    #[serde(default)]
    pub replayed: usize,
    /// Could-not-run-1 115: its runs in a row that left a check not run
    /// and failed none on its own result.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub not_run_runs: u32,
    /// When its verdict read passed: a hold runs from it (117).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passed_at: Option<String>,
    /// 140: the heads of the work landed beside the chain since it was
    /// formed, in order. It lands as `revision` with each of them, its
    /// runs and verdict standing (23, 24; rule core.17).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub carries: Vec<Rev>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// A candidate as records written before runs per system hold it: one
/// place and one run, read as its one run for any system.
#[derive(Deserialize)]
struct CandidateForm {
    id: String,
    on: Rev,
    submissions: Vec<u64>,
    revision: Rev,
    state: CandidateState,
    #[serde(default)]
    runs: Vec<SystemRun>,
    #[serde(default)]
    opened: Option<String>,
    #[serde(default)]
    place: Option<String>,
    #[serde(default)]
    run: Option<RunHandle>,
    #[serde(default)]
    failed: Vec<FailedTask>,
    #[serde(default)]
    every_check: bool,
    #[serde(default)]
    ran: usize,
    #[serde(default)]
    replayed: usize,
    #[serde(default)]
    not_run_runs: u32,
    #[serde(default)]
    passed_at: Option<String>,
    #[serde(default)]
    carries: Vec<Rev>,
}

impl From<CandidateForm> for Candidate {
    fn from(f: CandidateForm) -> Candidate {
        let mut runs = f.runs;
        if runs.is_empty() && (f.place.is_some() || f.run.is_some()) {
            runs.push(SystemRun {
                system: None,
                checks: Vec::new(),
                state: match f.state {
                    CandidateState::Starting => RunState::Starting,
                    CandidateState::Testing => RunState::Testing,
                    CandidateState::Passed => RunState::Passed,
                    CandidateState::Failed => RunState::Failed,
                    CandidateState::TimedOut => RunState::TimedOut,
                    CandidateState::Formed => RunState::Open,
                },
                place: f.place,
                run: f.run,
                no_replay: false,
                host: None,
                declined: BTreeMap::new(),
                lost: None,
            });
        }
        Candidate {
            id: f.id,
            on: f.on,
            submissions: f.submissions,
            revision: f.revision,
            state: f.state,
            runs,
            opened: f.opened,
            failed: f.failed,
            every_check: f.every_check,
            ran: f.ran,
            replayed: f.replayed,
            not_run_runs: f.not_run_runs,
            passed_at: f.passed_at,
            carries: f.carries,
        }
    }
}

impl Candidate {
    /// The places its runs hold on this host.
    pub fn places(&self) -> impl Iterator<Item = &String> {
        self.runs.iter().filter_map(|r| r.place.as_ref())
    }

    /// Its runs holding a place on this host.
    pub fn held(&self) -> usize {
        self.runs.iter().filter(|r| r.state.held()).count()
    }

    pub fn newest(&self) -> u64 {
        *self
            .submissions
            .last()
            .expect("a candidate holds a submission")
    }

    pub fn result_word(&self) -> &'static str {
        match self.state {
            CandidateState::Passed => "passed",
            CandidateState::TimedOut => "timed out",
            _ => "failed",
        }
    }
}

/// One landing commit of a landing, for the submission it lands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandingCommitRecord {
    pub id: u64,
    pub commit: Rev,
    /// The candidate whose verdict it landed on (23).
    pub tested: Rev,
}

/// A landing decided and written before the branch moves (24).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandingIntent {
    pub ids: Vec<u64>,
    pub from: Rev,
    pub to: Rev,
    pub commits: Vec<LandingCommitRecord>,
    /// 140: a landing beside the chain, of work no required check reads,
    /// with nothing run.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub beside: bool,
    /// 70, 140: the verdict that covers `to` once the branch is there: for
    /// a landing through the chain, the one its last commit names; for a
    /// landing beside, the one that covered `from`. `None` in an intent
    /// written before it, whose landing leaves the branch's `covered_by`
    /// unset (record 0083).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub covering: Option<CoveringVerdict>,
}

/// An outcome decided and not yet pushed to its session (50, 51).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxItem {
    pub id: String,
    pub submission: u64,
    /// Kept under the key `told` on disk, which an older sluice on another
    /// host reads to run its rounds (record 0070).
    #[serde(rename = "told")]
    pub outcome: DecidedRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum DecidedRecord {
    Landed {
        landed_as: Rev,
        /// 140: landed beside the chain, with no check run.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        beside: bool,
        /// 140: for a landing beside, the verdict that covers the revision
        /// it landed on; `None` otherwise, and in outcomes recorded before
        /// it (70; record 0083).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        covered_by: Option<CoveringVerdict>,
    },
    Failed {
        failed: Vec<String>,
        combination: Rev,
        submissions: Vec<u64>,
        #[serde(default)]
        timed_out: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        logs: Vec<TaskLog>,
    },
    Conflict {
        paths: Vec<String>,
        with: Vec<Contested>,
    },
}

impl DecidedRecord {
    pub fn to_decided(&self) -> Decided {
        match self.clone() {
            DecidedRecord::Landed {
                landed_as,
                beside,
                covered_by,
            } => Decided::Landed {
                landed_as,
                beside,
                covered_by,
            },
            DecidedRecord::Failed {
                failed,
                combination,
                submissions,
                timed_out,
                logs,
            } => Decided::Failed {
                failed,
                combination,
                submissions,
                timed_out,
                logs,
            },
            DecidedRecord::Conflict { paths, with } => Decided::Conflict { paths, with },
        }
    }

    pub fn word(&self) -> &'static str {
        match self {
            DecidedRecord::Landed { .. } => "landed",
            DecidedRecord::Failed { .. } => "failed",
            DecidedRecord::Conflict { .. } => "conflict",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchDoc {
    pub schema: String,
    pub branch: String,
    pub log: String,
    /// The branch's revision the candidates are formed on (10, 24).
    #[serde(default)]
    pub base: Option<Rev>,
    /// `base` is covered by a verdict: sluice landed it (70).
    #[serde(default)]
    pub verified: bool,
    /// 70, 140: the verdict that covers `base`, which each landing beside
    /// on it names; set beside `verified` by a landing under the gate the
    /// branch carries, and `None` where `verified` is not, and in records
    /// written before it, on which nothing lands beside until a landing
    /// names one (record 0083).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub covered_by: Option<CoveringVerdict>,
    /// How many candidates are under test at once (13).
    #[serde(default)]
    pub window: usize,
    #[serde(default)]
    pub next_place: u64,
    #[serde(default)]
    pub next_candidate: u64,
    #[serde(default)]
    pub candidates: Vec<Candidate>,
    #[serde(default)]
    pub landing: Option<LandingIntent>,
    /// Places decided released and not yet confirmed (25).
    #[serde(default)]
    pub release: Vec<String>,
    /// 123: each place whose release failed, still tried or held.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_releases: Vec<FailedRelease>,
    #[serde(default)]
    pub outbox: Vec<OutboxItem>,
    /// 72: the instance that keeps the branch, on a branch whose records
    /// are on its remote; `None` until one takes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<String>,
    /// 121: the revision every read of the branch must contain: its last
    /// landing, the revision the operator accepted, or the one sluice
    /// made it at. `None` in records written before it, until a round
    /// takes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landed: Option<Rev>,
    /// 121: the branch was last read at a revision lacking `landed`, and
    /// nothing is formed or landed on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<BranchHold>,
    /// 124: each host whose copy of the branch a round could not level to
    /// `landed`, by the instance's name; a host whose copy holds it has no
    /// entry, and each host writes only its own.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub copies: BTreeMap<String, CopyBehind>,
    /// 148: each write the protection refused whose cause stands, one entry
    /// per host, ref and command; each host writes its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refused_writes: Vec<RefusedWrite>,
    /// 114, record 0105 (design D6 of the change
    /// `a-pass-nothing-proves-blames-no-one`): each cause of passes a
    /// host's runs kept no proof of, standing until that host's next run
    /// proves its passes; one entry per host and reason, each host's
    /// written by the round that reads its run. No round, gate or landing
    /// reads it (rule core.14).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proof_missing: Vec<ProofMissing>,
    /// 72, record 0122 (design D9): the lead's own time of its last write
    /// of the branch, by its own clock; set by every write the lead makes,
    /// and written alone every half lease while a run of the branch is
    /// live, so no other instance's lease passes under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept: Option<String>,
}

/// 114, record 0105 (design D6): a cause of passes `host`'s runs kept no
/// proof of, in the check profile's words `why`, since the run that first
/// met it, and how many of that host's runs it touched.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofMissing {
    pub host: String,
    pub why: String,
    pub since: String,
    pub runs: u64,
}

/// 121: a branch read at a revision that lacks the landing its records
/// say it must contain, and since when.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchHold {
    pub read: Rev,
    pub lacks: Rev,
    pub since: String,
}

/// 124, S64: a host's copy of the branch left behind its landing: the
/// value it holds, `None` when the host has none, the landing it lacks,
/// why the levelling failed, and since when it has been behind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyBehind {
    pub at: Option<Rev>,
    pub lacks: Rev,
    pub why: String,
    pub since: String,
}

/// 148: a cause of writes the protection refused, standing until a day
/// passes with none of its writes refused, counted here and never from the
/// log (rule core.47).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefusedWrite {
    /// The instance whose ref was tried.
    pub host: String,
    #[serde(rename = "ref")]
    pub reference: String,
    /// The command, each full object name in it as `<revision>`
    /// ([`crate::refused_writes::command_of`]).
    pub command: String,
    /// How many of its writes were refused while it stood.
    pub times: u64,
    pub first: String,
    pub last: String,
    /// The last refused write's: the value the ref held on the host, and
    /// the one it would have set, `None` for a deletion.
    pub from: Rev,
    pub to: Option<Rev>,
}

impl BranchDoc {
    pub fn new(branch: &str) -> Self {
        BranchDoc {
            schema: BRANCH_SCHEMA.into(),
            branch: branch.into(),
            log: LOG_SCHEMA.into(),
            base: None,
            verified: false,
            covered_by: None,
            window: 0,
            next_place: 1,
            next_candidate: 1,
            candidates: Vec::new(),
            landing: None,
            release: Vec::new(),
            failed_releases: Vec::new(),
            outbox: Vec::new(),
            lead: None,
            landed: None,
            held: None,
            copies: BTreeMap::new(),
            refused_writes: Vec::new(),
            proof_missing: Vec::new(),
            kept: None,
        }
    }
}

// ---------------------------------------------------------------------------
// The home (record 0122)
// ---------------------------------------------------------------------------

/// Record 0122 (design D1): the name of the host's own files as a home.
pub const HERE: &str = "here";

/// Record 0122, C.6 (design D1): `home.json`, one in each home of a
/// branch: where its queue lives, `here` for the host's own files or a
/// remote's name, the revision whose policy chose it, and whether it is
/// open, moving or closed. It is written with the records it belongs to,
/// in one write (63).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeDoc {
    pub schema: String,
    /// The home: [`HERE`] or a remote's name.
    pub at: String,
    /// The branch's revision whose policy chose it; `None` where the
    /// branch had none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen_at: Option<Rev>,
    pub state: HomeState,
    /// While `moving`: where to, the revision whose policy named it, and
    /// since when (design D3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moving: Option<Moving>,
    /// The move that opened it: the home it came from, the host whose own
    /// files that was, and each number carried, as it was and as it is
    /// here (design D3, D5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub came_from: Option<CameFrom>,
    /// Once `closed`: the home that took it, and each submission carried
    /// there by its number here (design D3, D4, D7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<Closed>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HomeState {
    Open,
    Moving,
    Closed,
}

/// Design D3: a move written and not yet carried.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moving {
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen_at: Option<Rev>,
    pub since: String,
}

/// Design D3, D5: where an open home came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameFrom {
    pub home: String,
    /// The host whose own files it came from, or, for a branch coming home
    /// from a remote, the host that carried it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Each number carried, as it was there and as it is here.
    #[serde(default)]
    pub carried: BTreeMap<u64, u64>,
}

/// Design D3, D4: where a closed home went, and each submission carried.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Closed {
    pub to: String,
    /// By its number here, each submission carried: its number in the home
    /// that took it and, where that is a host's own files, the host.
    #[serde(default)]
    pub carried: BTreeMap<u64, CarriedTo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarriedTo {
    pub number: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

impl HomeDoc {
    /// A home opened at `at` by the policy at `chosen_at` (design D2).
    pub fn open(at: &str, chosen_at: Option<Rev>) -> HomeDoc {
        HomeDoc {
            schema: HOME_SCHEMA.into(),
            at: at.into(),
            chosen_at,
            state: HomeState::Open,
            moving: None,
            came_from: None,
            closed: None,
        }
    }

    /// Whether the queue lives here: open or moving (design D2).
    pub fn is_live(&self) -> bool {
        self.state != HomeState::Closed
    }

    /// Where a move written here goes, while it is written.
    pub fn moving_to(&self) -> Option<&str> {
        match self.state {
            HomeState::Moving => self.moving.as_ref().map(|m| m.to.as_str()),
            _ => None,
        }
    }

    /// Where a closed home went.
    pub fn closed_to(&self) -> Option<&str> {
        match self.state {
            HomeState::Closed => self.closed.as_ref().map(|c| c.to.as_str()),
            _ => None,
        }
    }
}

/// 123: a place whose release failed. It stays in the branch's `release`
/// while it is tried; once its tries pass the branch's `release_retries`
/// it leaves `release`, `stopped`, and is held until the operator releases
/// it or accepts its failure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedRelease {
    pub place: String,
    /// What failed, the end of what the release command printed included.
    pub failed: String,
    /// How many times its release was tried, the first included.
    pub tries: u32,
    /// When it last failed.
    pub at: String,
    /// It is tried no more: held.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stopped: bool,
    /// The instance that made it, on a branch whose records are on its
    /// remote: only that host can release it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

// ---------------------------------------------------------------------------
// The log
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    #[serde(default = "log_schema")]
    pub schema: String,
    pub at: String,
    #[serde(flatten)]
    pub event: Event,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Submitted {
        id: u64,
        head: Rev,
    },
    Withdrawn {
        id: u64,
        by: String,
        /// As the submission's [`Withdrawal`] names it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        handed_over_to: Option<String>,
    },
    Amended {
        id: u64,
        after: Vec<u64>,
        priority: i64,
    },
    /// The branch was found at a revision sluice did not move it to.
    Moved {
        from: Rev,
        to: Rev,
    },
    /// 122: submissions whose work the branch already held when a round
    /// came to form them, landed by a landing this queue did not make, at
    /// `to`, the commit that landed it.
    FoundLanded {
        ids: Vec<u64>,
        to: Rev,
    },
    Conflict {
        id: u64,
        paths: Vec<String>,
        with: Vec<u64>,
    },
    Testing {
        combination: Rev,
        submissions: Vec<u64>,
        place: String,
    },
    Ran {
        combination: Rev,
        task: String,
        hash: String,
        outcome: Outcome,
        host: String,
    },
    Replayed {
        combination: Rev,
        task: String,
        hash: String,
        outcome: Outcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_run_at: Option<String>,
    },
    /// Record 0122 (design D3): a move of the queue to `to` written in
    /// this home, chosen by the policy at revision `at`.
    Moving {
        to: String,
        #[serde(rename = "chosen_at", default, skip_serializing_if = "Option::is_none")]
        at: Option<Rev>,
    },
    /// Record 0122 (design D3): a written move ended, the policy at `at`
    /// naming this home again.
    Stayed {
        #[serde(rename = "chosen_at", default, skip_serializing_if = "Option::is_none")]
        at: Option<Rev>,
    },
    /// Record 0122 (design D3, D5): the queue's live submissions carried to
    /// `to`, each by the number it had and the number it has there.
    Carried {
        to: String,
        #[serde(with = "numbers")]
        numbers: BTreeMap<u64, u64>,
    },
    /// 72, 73: an instance took the branch, from the lead it names or
    /// from none.
    Took {
        by: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
    },
    /// Design D8: a candidate the records said passed, with no signed
    /// results to bear it out; it is tested again and lands nothing.
    Unsigned {
        combination: Rev,
    },
    /// 75: an instance claimed one of a candidate's runs.
    Claimed {
        combination: Rev,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system: Option<String>,
        host: String,
        /// 134: a pool's run whose dispatch went unanswered, claimed as
        /// dispatched: the forge's words saying its answer was not read.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unanswered: Option<String>,
    },
    /// 77: a claim whose host showed no sign of life for the bound: its
    /// run has no outcome and is open again.
    Lapsed {
        combination: Rev,
        host: String,
    },
    /// 84, record 0074: a host turned a run down, before claiming it, for
    /// what it lacks of what the run asks of it; once for each host and
    /// reason while it stands.
    Declined {
        combination: Rev,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system: Option<String>,
        host: String,
        why: String,
    },
    /// A run whose executor stopped before reporting, or that left checks
    /// not run and failed none on its own result: no outcome (60,
    /// could-not-run-1 114). The candidate runs again.
    Lost {
        combination: Rev,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        not_run: Vec<NotRun>,
    },
    /// 108: replays no trusted pass proved, so the checks run again with
    /// no replay. `keys` names each replay's task, key and why no counted
    /// pass proves it (design D4 of
    /// `a-signed-run-without-a-sandbox-proves-its-passes`); a line written
    /// before it reads with none (rule core.41).
    Unproven {
        combination: Rev,
        tasks: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        keys: Vec<UnprovenKey>,
    },
    /// 87, S18: checks a host ran whose results its gate does not count,
    /// signed with a key the branch's policy does not list: missing, never
    /// failed. The run is open again for a host whose results count.
    Uncounted {
        combination: Rev,
        tasks: Vec<String>,
        host: String,
    },
    /// Clause 119: what a run's runner said of it beyond
    /// its results, the required checks it keeps out of its CI mode, and
    /// why its executor could not read what it refused the run (139).
    /// Shown; nothing reads a verdict from it.
    Detail {
        combination: Rev,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system: Option<String>,
        #[serde(default, skip_serializing_if = "RunDetail::is_empty")]
        detail: RunDetail,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        kept_out: Vec<String>,
        /// 139 (design D8 of the change
        /// `a-check-its-executor-refused-has-no-outcome`): why the run's
        /// executor could not read what it refused the run, which was then
        /// read as refused nothing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refusals_unread: Option<String>,
    },
    Result {
        combination: Rev,
        result: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        failed: Vec<String>,
    },
    Blamed {
        id: u64,
        combination: Rev,
    },
    Landing {
        ids: Vec<u64>,
        from: Rev,
        to: Rev,
    },
    Landed {
        ids: Vec<u64>,
        to: Rev,
        /// 140: landed beside the chain, with no check run.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        beside: bool,
    },
    /// 121: the branch was read at a revision lacking the landing it must
    /// contain; one line for each revision read so.
    Held {
        read: Rev,
        lacks: Rev,
    },
    /// 121: the operator accepted the revision a held branch was read at,
    /// which lacked the landing named.
    Accepted {
        revision: Rev,
        lacked: Rev,
    },
    /// 124, S64: a host's copy could not be levelled to the landing; one
    /// line for each distinct value, landing and why.
    /// The copy's value is `copy` on the line, whose `at` is its time.
    #[serde(rename = "copy-behind")]
    CopyBehind {
        host: String,
        #[serde(rename = "copy")]
        at: Option<Rev>,
        lacks: Rev,
        why: String,
    },
    /// 124: a host's copy that was behind holds the landing again.
    #[serde(rename = "copy-levelled")]
    CopyLevelled {
        host: String,
        #[serde(rename = "copy")]
        at: Rev,
    },
    /// 133: a read of the branch's remote a round on `host` could not
    /// make, once per distinct ref asked and words said while it stands.
    /// Its `at` is when it happened, which a line written late keeps.
    #[serde(rename = "read-refused")]
    ReadRefused {
        host: String,
        remote: String,
        asked: String,
        said: String,
        /// 133 (design D27): a read of the branch's intake, its pull
        /// requests on the forge, rather than of its remote's refs.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        intake: bool,
    },
    /// 133: the first round on `host` that read the remote again: when the
    /// refusal began, how many seconds it stood and how many rounds it
    /// refused.
    #[serde(rename = "read-again")]
    ReadAgain {
        host: String,
        remote: String,
        since: String,
        stood: u64,
        rounds: u64,
        /// 133 (design D27): the end of a refusal of the branch's intake.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        intake: bool,
    },
    /// 114, record 0105 (design D6 of the change
    /// `a-pass-nothing-proves-blames-no-one`; rule core.28's way): a run
    /// read on `host` kept no proof of a pass, for the reason `why`; once
    /// per host and reason while it stands, never once per run.
    #[serde(rename = "proof-missing")]
    ProofMissing {
        host: String,
        why: String,
    },
    /// 114, record 0105 (design D6): the first run on `host` that proved
    /// its passes and left none without proof, ending the cause `why`:
    /// when it began, how many seconds it stood and how many of the host's
    /// runs it touched.
    #[serde(rename = "proof-kept")]
    ProofKept {
        host: String,
        why: String,
        since: String,
        stood: u64,
        runs: u64,
    },
    /// A landing that found the branch elsewhere (24; rule core.5).
    Refused {
        ids: Vec<u64>,
        expected: Rev,
        found: Option<Rev>,
    },
    Released {
        place: String,
        /// A session's place released because this, its worktree, no
        /// longer exists (session-runs-1 clause 97).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worktree: Option<String>,
        /// 123: freed at the operator's accepting that its release failed,
        /// with nothing run: what failed last, if its release ever did.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        accepted: Option<String>,
    },
    /// 123: a release that failed, and how many times it has been tried;
    /// `held` on the try that spent its retries.
    ReleaseFailed {
        place: String,
        failed: String,
        tries: u32,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        held: bool,
    },
    /// 123: a place whose working copy was gone, released once from what
    /// sluice held for it and freed whatever that did; for a session's
    /// place, its worktree.
    Gone {
        place: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worktree: Option<String>,
        /// A release command its runs stored was run.
        #[serde(default)]
        ran: bool,
        /// The release failed, and how.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        failed: Option<String>,
    },
    /// An update to the branch made by anything but sluice, refused (1);
    /// 148: once for each cause while it stands, the rest counted in the
    /// branch's document.
    Protected {
        #[serde(rename = "ref")]
        reference: String,
        from: Rev,
        /// `None` for a deletion.
        to: Option<Rev>,
        /// The command that tried it, as git ran it.
        by: String,
        /// 148: the instance whose ref was tried; `None` on a line written
        /// before it was named.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host: Option<String>,
    },
    /// 148: a cause of refused writes that stood a day with none refused,
    /// or put out by a newer one past the twenty: how many it refused, the
    /// first and the last.
    #[serde(rename = "protected-ended")]
    ProtectedEnded {
        host: String,
        #[serde(rename = "ref")]
        reference: String,
        command: String,
        times: u64,
        first: String,
        last: String,
    },
    /// 51: an outcome a carrier delivered to its session, as an earlier
    /// sluice marked it; kept as written and read as history (record 0102;
    /// rule core.41). Read under `told` too, the name a sluice before it
    /// wrote it under (record 0070).
    #[serde(alias = "told")]
    Delivered {
        id: u64,
        notification: String,
    },
    /// 51: an outcome its driver did not deliver, and why in the carrier's
    /// words; it stays owed, and no round pushes it again.
    Pending {
        id: u64,
        notification: String,
        why: String,
    },
    /// 51: an outcome handed to its reader `by` a way, `notice`,
    /// `pull-request`, `inbox` or `wait`, reaching `to` as the carrier
    /// names it, the pane, the request, the path or the agent; it confirms
    /// nothing by itself (weir 103; record 0102; design D5).
    Handed {
        id: u64,
        key: String,
        by: String,
        to: String,
    },
    /// 51: an outcome confirmed for `reader`, `by` the way that confirmed
    /// it on receipt, `confirm`, `operator` or `resubmission` (weir 102;
    /// design D5, D7, D16).
    Confirmed {
        id: u64,
        key: String,
        reader: String,
        by: String,
    },
}

/// A log line's map of numbers, keyed by each number as JSON's text: a
/// line's event is read through serde's buffer, which reads no number from
/// a key by itself.
mod numbers {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(map: &BTreeMap<u64, u64>, s: S) -> Result<S::Ok, S::Error> {
        map.iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect::<BTreeMap<String, u64>>()
            .serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<u64, u64>, D::Error> {
        BTreeMap::<String, u64>::deserialize(d)?
            .into_iter()
            .map(|(k, v)| Ok((k.parse().map_err(serde::de::Error::custom)?, v)))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// The repository's results log (69)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultKind {
    Ran,
    Replayed,
}

/// Could-not-run-1 116: the operator's record that one recorded result
/// did not judge its check, and why. The result's own line stays (rule
/// core.7); from the set-aside on, no gate reads it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetAside {
    pub schema: String,
    /// When it was set aside.
    pub at: String,
    pub task: String,
    pub hash: String,
    /// When the result was recorded: with its task, hash, branch and
    /// combination, the result it names.
    pub run_at: String,
    pub branch: String,
    pub combination: Rev,
    pub reason: String,
    /// Who set it aside, `user@host`.
    pub by: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultLine {
    #[serde(default = "result_schema")]
    pub schema: String,
    pub at: String,
    pub kind: ResultKind,
    pub task: String,
    pub hash: String,
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// 84: what the executor held the run to; a replay was observed by an
    /// earlier run, so it has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor: Option<Enforced>,
    pub branch: String,
    pub combination: Rev,
    /// Session-runs-1 clause 92: the session's worktree a session run was
    /// run in; `None` for a candidate's run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// 85: the system it ran on, by the policy's name; `None` when the
    /// policy named no system for its host, as every line written before
    /// systems is read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// 78: the signer whose signature over it verified, as the records
    /// seam read it from the remote; `None` for one host's own results,
    /// which count unsigned on a branch kept in its own files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<String>,
    /// Clause 119: how long the runner says it took,
    /// in milliseconds. Shown, never read for a verdict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub took_ms: Option<u64>,
    /// Clause 119: how many times the runner tried it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempts: Option<u32>,
}

impl ResultLine {
    /// The record of one result of a run on `combination`, for `branch`:
    /// a run with its host and executor, or a replay with neither (16,
    /// 38, 84).
    pub fn of(
        at: &str,
        task: &sluice_seams::TaskResult,
        executor: &Enforced,
        branch: &str,
        combination: &Rev,
        worktree: Option<&str>,
        system: Option<&str>,
    ) -> ResultLine {
        let (kind, host, executor) = match &task.source {
            sluice_seams::Source::Ran { host } => {
                (ResultKind::Ran, Some(host.clone()), Some(executor.clone()))
            }
            sluice_seams::Source::Replayed { .. } => (ResultKind::Replayed, None, None),
        };
        ResultLine {
            schema: RESULT_SCHEMA.into(),
            at: at.into(),
            kind,
            task: task.task.clone(),
            hash: task.hash.clone(),
            outcome: task.outcome,
            host,
            executor,
            branch: branch.into(),
            combination: combination.clone(),
            worktree: worktree.map(str::to_string),
            system: system.map(str::to_string),
            signer: None,
            took_ms: task.took_ms,
            attempts: task.attempts,
        }
    }

    /// Whether `aside` sets this result aside (could-not-run-1 116).
    pub fn set_aside_by(&self, aside: &SetAside) -> bool {
        self.task == aside.task
            && self.hash == aside.hash
            && self.at == aside.run_at
            && self.branch == aside.branch
            && self.combination == aside.combination
    }

    /// Two lines record one observation: a write repeated after a crash.
    pub fn same_as(&self, other: &ResultLine) -> bool {
        self.kind == other.kind
            && self.task == other.task
            && self.hash == other.hash
            && self.outcome == other.outcome
            && self.host == other.host
            && self.branch == other.branch
            && self.combination == other.combination
            && self.system == other.system
    }
}

pub fn parse_lines<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> anyhow::Result<Vec<T>> {
    let text = std::str::from_utf8(bytes)?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| Ok(serde_json::from_str(l)?))
        .collect()
}

pub fn to_line<T: Serialize>(value: &T) -> Vec<u8> {
    let mut v = serde_json::to_vec(value).expect("a record serializes");
    v.push(b'\n');
    v
}

pub fn to_document<T: Serialize>(value: &T) -> Vec<u8> {
    let mut v = serde_json::to_vec_pretty(value).expect("a record serializes");
    v.push(b'\n');
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submission(subject: Option<&str>) -> Submission {
        let doc = format!(
            "{{\"schema\":\"sluice/submission@1\",\"id\":12,\"branch\":\"main\",\
             \"head\":\"h12\",\"from\":{{\"agent\":\"a\",\"worktree\":\"unit/review\"}},\
             \"submitted\":\"t\",\"after\":[],\"priority\":0,\"attempt\":1,\
             \"state\":\"queued\"{}}}",
            subject.map_or(String::new(), |s| format!(",\"subject\":{s:?}"))
        );
        serde_json::from_str(&doc).unwrap()
    }

    /// 26: a submission is named by its subject, or by the branch it was
    /// submitted from where its submit gave none; a record written before
    /// subjects reads as one with none.
    /// 140: `beside` is `true` once landed beside the chain, or the entry
    /// it waits behind and why; a submission read as neither writes none,
    /// and `false` is no reading of it.
    #[test]
    fn beside_reads_as_true_or_the_entry_it_waits_behind() {
        let mut s = submission(None);
        assert!(!String::from_utf8(to_document(&s))
            .unwrap()
            .contains("beside"));
        s.beside = Some(Beside::Landed);
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["beside"], serde_json::json!(true));
        s.beside = Some(Beside::Behind {
            behind: 7,
            why: BesideWhy::Read,
        });
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(
            v["beside"],
            serde_json::json!({ "behind": 7, "why": "read" })
        );
        let back: Submission = serde_json::from_value(v).unwrap();
        assert_eq!(back.beside, s.beside);
        let conflict: Beside = serde_json::from_str(r#"{"behind":3,"why":"conflict"}"#).unwrap();
        assert_eq!(
            conflict,
            Beside::Behind {
                behind: 3,
                why: BesideWhy::Conflict
            }
        );
        assert!(serde_json::from_str::<Beside>("false").is_err());
        s.state = State::Landed;
        s.landed_as = Some(Rev::new("abc"));
        s.beside = Some(Beside::Landed);
        assert_eq!(
            s.decided(),
            Some(Decided::Landed {
                landed_as: Rev::new("abc"),
                beside: true,
                covered_by: None,
            })
        );
    }

    /// Record 0122 (design D1, D5, D9): `home.json` reads as written,
    /// open, moving and closed, and writes back the same; a submission's
    /// `carried_from` and the branch's `kept` round-trip, and the log's
    /// `moving`, `stayed` and `carried` lines are the spec's.
    #[test]
    fn a_home_document_reads_as_written() {
        let written = r#"{
  "schema": "sluice/home@1",
  "at": "origin",
  "chosen_at": "4f6fe55f",
  "state": "moving",
  "moving": {
    "to": "here",
    "chosen_at": "9a1c",
    "since": "2026-11-14T10:00:00Z"
  },
  "came_from": {
    "home": "here",
    "host": "build-1",
    "carried": {
      "52": 52,
      "53": 61
    }
  }
}
"#;
        let home: HomeDoc = serde_json::from_str(written).unwrap();
        assert_eq!(home.at, "origin");
        assert_eq!(home.chosen_at, Some(Rev::new("4f6fe55f")));
        assert_eq!(home.state, HomeState::Moving);
        assert_eq!(home.moving_to(), Some(HERE));
        assert!(home.is_live());
        let came = home.came_from.as_ref().unwrap();
        assert_eq!(came.host.as_deref(), Some("build-1"));
        assert_eq!(came.carried[&53], 61);
        assert_eq!(String::from_utf8(to_document(&home)).unwrap(), written);

        let mut closed = HomeDoc::open("origin", Some(Rev::new("4f6fe55f")));
        closed.state = HomeState::Closed;
        closed.closed = Some(Closed {
            to: HERE.into(),
            carried: BTreeMap::from([
                (
                    7,
                    CarriedTo {
                        number: 7,
                        host: Some("build-1".into()),
                    },
                ),
                (
                    8,
                    CarriedTo {
                        number: 8,
                        host: Some("build-2".into()),
                    },
                ),
            ]),
        });
        let v = serde_json::to_value(&closed).unwrap();
        assert_eq!(
            v["closed"],
            serde_json::json!({
                "to": "here",
                "carried": {
                    "7": {"number": 7, "host": "build-1"},
                    "8": {"number": 8, "host": "build-2"},
                },
            })
        );
        assert_eq!(v["state"], "closed");
        let back: HomeDoc = serde_json::from_value(v).unwrap();
        assert_eq!(back, closed);
        assert_eq!(back.closed_to(), Some(HERE));
        assert!(!back.is_live());

        let mut s = submission(None);
        s.was = Some(52);
        s.carried_from = Some(HERE.into());
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(
            (v["was"].clone(), v["carried_from"].clone()),
            (52.into(), "here".into())
        );
        assert!(!String::from_utf8(to_document(&submission(None)))
            .unwrap()
            .contains("carried_from"));

        let mut doc = BranchDoc::new("bolt/x");
        assert!(!String::from_utf8(to_document(&doc))
            .unwrap()
            .contains("kept"));
        doc.kept = Some("2026-11-14T10:02:30Z".into());
        let v = serde_json::to_value(&doc).unwrap();
        assert_eq!(v["kept"], "2026-11-14T10:02:30Z");
        assert_eq!(serde_json::from_value::<BranchDoc>(v).unwrap(), doc);

        let lines = [
            Event::Moving {
                to: "origin".into(),
                at: Some(Rev::new("9a1c")),
            },
            Event::Stayed {
                at: Some(Rev::new("9a1d")),
            },
            Event::Carried {
                to: "origin".into(),
                numbers: BTreeMap::from([(52, 52), (53, 61)]),
            },
        ];
        let text: Vec<String> = lines
            .iter()
            .map(|event| {
                String::from_utf8(to_line(&LogLine {
                    schema: LOG_SCHEMA.into(),
                    at: "2026-11-14T10:00:00Z".into(),
                    event: event.clone(),
                }))
                .unwrap()
            })
            .collect();
        assert_eq!(
            text,
            [
                "{\"schema\":\"sluice/log@1\",\"at\":\"2026-11-14T10:00:00Z\",\"event\":\"moving\",\"to\":\"origin\",\"chosen_at\":\"9a1c\"}\n",
                "{\"schema\":\"sluice/log@1\",\"at\":\"2026-11-14T10:00:00Z\",\"event\":\"stayed\",\"chosen_at\":\"9a1d\"}\n",
                "{\"schema\":\"sluice/log@1\",\"at\":\"2026-11-14T10:00:00Z\",\"event\":\"carried\",\"to\":\"origin\",\"numbers\":{\"52\":52,\"53\":61}}\n",
            ]
        );
        let back: Vec<LogLine> = parse_lines(text.concat().as_bytes()).unwrap();
        assert_eq!(back.into_iter().map(|l| l.event).collect::<Vec<_>>(), lines);
    }

    /// Record 0122 (design D1, D2): records with no `home.json` have no
    /// home, and a write of them writes none; a home set on them is written
    /// with the rest.
    #[test]
    fn a_branch_with_no_home_document_has_no_home() {
        let mut files = BTreeMap::new();
        files.insert(
            BRANCH_FILE.to_string(),
            to_document(&BranchDoc::new("bolt/x")),
        );
        let snapshot = sluice_seams::Snapshot { version: 3, files };
        let mut line = crate::line::Line::load("bolt/x", snapshot).unwrap();
        assert_eq!(line.home, None);
        assert!(line.changes().is_empty());
        line.home = Some(HomeDoc::open(HERE, Some(Rev::new("c43609b"))));
        let changes = line.changes();
        let written: HomeDoc = serde_json::from_slice(&changes.put[HOME_FILE]).unwrap();
        assert_eq!(written.at, HERE);
        assert_eq!(written.state, HomeState::Open);
        let mut files = BTreeMap::new();
        files.insert(HOME_FILE.to_string(), changes.put[HOME_FILE].clone());
        let read = crate::line::Line::load("bolt/x", sluice_seams::Snapshot { version: 4, files })
            .unwrap();
        assert_eq!(read.home, Some(written));
    }

    #[test]
    fn a_submission_is_named_by_its_subject_or_its_branch() {
        let given = submission(Some("Batch review cards"));
        assert_eq!(given.subject.as_deref(), Some("Batch review cards"));
        assert_eq!(given.name(), "Batch review cards");
        let none = submission(None);
        assert_eq!(none.subject, None);
        assert_eq!(none.name(), "unit/review");
        assert!(!String::from_utf8(to_document(&none))
            .unwrap()
            .contains("subject"));
    }

    /// 26, 130: an index line written before subjects were kept reads with
    /// none, named by its worktree, and writes back as it was.
    #[test]
    fn a_finished_line_without_a_subject_reads_with_none() {
        let old = "{\"id\":1,\"state\":\"landed\",\"worktree\":\"unit/a\"}\n";
        let back: Vec<FinishedLine> = parse_lines(old.as_bytes()).unwrap();
        assert_eq!(back[0].subject, None);
        assert_eq!(back[0].name(), "unit/a");
        assert_eq!(String::from_utf8(to_line(&back[0])).unwrap(), old);
        let given = FinishedLine {
            subject: Some("Batch review cards".into()),
            ..back[0].clone()
        };
        assert_eq!(given.name(), "Batch review cards");
    }

    /// A document written before the hold existed reads as before, with
    /// no revision it must contain and no hold, and writes back unchanged.
    #[test]
    fn a_branch_document_without_the_hold_reads_as_before() {
        let old = r#"{
  "schema": "sluice/branch@1",
  "branch": "bolt/x",
  "log": "sluice/log@1",
  "base": "c43609b",
  "verified": true,
  "window": 4,
  "next_place": 3,
  "next_candidate": 5,
  "candidates": [],
  "landing": null,
  "release": [],
  "outbox": []
}
"#;
        let doc: BranchDoc = serde_json::from_str(old).unwrap();
        assert_eq!(doc.landed, None);
        assert_eq!(doc.held, None);
        assert!(doc.copies.is_empty());
        assert_eq!(String::from_utf8(to_document(&doc)).unwrap(), old);
    }

    /// 35: a document an earlier sluice wrote with the places whose
    /// candidate passed, which it saved caches from, reads; the list is
    /// dropped when it is written back, and its places stay to release.
    #[test]
    fn a_branch_document_with_the_passed_places_reads() {
        let old = r#"{
  "schema": "sluice/branch@1",
  "branch": "bolt/x",
  "log": "sluice/log@1",
  "base": "c43609b",
  "verified": true,
  "window": 4,
  "next_place": 3,
  "next_candidate": 5,
  "candidates": [],
  "landing": null,
  "release": [
    "sluice/try/bolt/x/1"
  ],
  "passed": [
    "sluice/try/bolt/x/1"
  ],
  "outbox": []
}
"#;
        let doc: BranchDoc = serde_json::from_str(old).unwrap();
        assert_eq!(doc.release, ["sluice/try/bolt/x/1"]);
        let back = String::from_utf8(to_document(&doc)).unwrap();
        assert!(!back.contains("passed"), "{back}");
        assert!(
            back.contains("\"release\": [\n    \"sluice/try/bolt/x/1\"\n  ]"),
            "{back}"
        );
    }

    /// 124: each host's copy left behind round-trips under `copies`, by
    /// the host's name.
    #[test]
    fn a_branch_document_with_a_copy_behind_round_trips() {
        let mut doc = BranchDoc::new("bolt/x");
        doc.landed = Some(Rev::new("c43609b"));
        doc.copies.insert(
            "deneb".into(),
            CopyBehind {
                at: Some(Rev::new("10e4791")),
                lacks: Rev::new("c43609b"),
                why: "git holds the lock on refs/heads/bolt/x".into(),
                since: "2026-10-23T12:00:00Z".into(),
            },
        );
        let bytes = to_document(&doc);
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            v["copies"],
            serde_json::json!({
                "deneb": {
                    "at": "10e4791", "lacks": "c43609b",
                    "why": "git holds the lock on refs/heads/bolt/x",
                    "since": "2026-10-23T12:00:00Z",
                },
            })
        );
        assert_eq!(serde_json::from_slice::<BranchDoc>(&bytes).unwrap(), doc);
    }

    #[test]
    fn a_copy_behind_and_levelled_are_log_lines() {
        let lines = [
            Event::CopyBehind {
                host: "deneb".into(),
                at: Some(Rev::new("10e4791")),
                lacks: Rev::new("c43609b"),
                why: "git holds the lock on refs/heads/bolt/x".into(),
            },
            Event::CopyLevelled {
                host: "deneb".into(),
                at: Rev::new("c43609b"),
            },
        ];
        let text: Vec<String> = lines
            .iter()
            .map(|event| {
                let line = LogLine {
                    schema: LOG_SCHEMA.into(),
                    at: "2026-10-23T12:00:00Z".into(),
                    event: event.clone(),
                };
                String::from_utf8(to_line(&line)).unwrap()
            })
            .collect();
        assert_eq!(
            text,
            [
                "{\"schema\":\"sluice/log@1\",\"at\":\"2026-10-23T12:00:00Z\",\"event\":\"copy-behind\",\"host\":\"deneb\",\"copy\":\"10e4791\",\"lacks\":\"c43609b\",\"why\":\"git holds the lock on refs/heads/bolt/x\"}\n",
                "{\"schema\":\"sluice/log@1\",\"at\":\"2026-10-23T12:00:00Z\",\"event\":\"copy-levelled\",\"host\":\"deneb\",\"copy\":\"c43609b\"}\n",
            ]
        );
        let back: Vec<LogLine> = parse_lines(text.concat().as_bytes()).unwrap();
        assert_eq!(back.into_iter().map(|l| l.event).collect::<Vec<_>>(), lines);
    }

    /// 133: a refused read and its end are the spec's lines.
    #[test]
    fn a_refused_read_and_its_end_are_log_lines() {
        let lines = [
            LogLine {
                schema: LOG_SCHEMA.into(),
                at: "2026-10-26T09:16:40Z".into(),
                event: Event::ReadRefused {
                    host: "build-1".into(),
                    remote: "origin".into(),
                    asked: "refs/heads/main".into(),
                    said: "remote: Repository not found.\n\
                           fatal: repository 'https://github.com/acme/app.git/' not found"
                        .into(),
                    intake: false,
                },
            },
            LogLine {
                schema: LOG_SCHEMA.into(),
                at: "2026-10-26T09:46:40Z".into(),
                event: Event::ReadAgain {
                    host: "build-1".into(),
                    remote: "origin".into(),
                    since: "2026-10-26T09:16:40Z".into(),
                    stood: 1800,
                    rounds: 12,
                    intake: false,
                },
            },
        ];
        let text: Vec<String> = lines
            .iter()
            .map(|l| String::from_utf8(to_line(l)).unwrap())
            .collect();
        assert_eq!(
            text,
            [
                "{\"schema\":\"sluice/log@1\",\"at\":\"2026-10-26T09:16:40Z\",\"event\":\"read-refused\",\"host\":\"build-1\",\"remote\":\"origin\",\"asked\":\"refs/heads/main\",\"said\":\"remote: Repository not found.\\nfatal: repository 'https://github.com/acme/app.git/' not found\"}\n",
                "{\"schema\":\"sluice/log@1\",\"at\":\"2026-10-26T09:46:40Z\",\"event\":\"read-again\",\"host\":\"build-1\",\"remote\":\"origin\",\"since\":\"2026-10-26T09:16:40Z\",\"stood\":1800,\"rounds\":12}\n",
            ]
        );
        let back: Vec<LogLine> = parse_lines(text.concat().as_bytes()).unwrap();
        assert_eq!(back, lines);
    }

    /// 114, record 0105 (design D6, D7): a missing proof and its end are
    /// the spec's lines, and a branch's document and a run written before
    /// `proof_missing` and `lost` read with none and write back without
    /// them.
    #[test]
    fn a_missing_proof_and_its_end_are_log_lines() {
        let why = "it passed, but nothing the run kept proves it ran: \
                   moon's manifest of it is not in /repo/.moon/cache";
        let lines = [
            LogLine {
                schema: LOG_SCHEMA.into(),
                at: "2026-11-09T09:16:40Z".into(),
                event: Event::ProofMissing {
                    host: "build-1".into(),
                    why: why.into(),
                },
            },
            LogLine {
                schema: LOG_SCHEMA.into(),
                at: "2026-11-09T09:46:40Z".into(),
                event: Event::ProofKept {
                    host: "build-1".into(),
                    why: why.into(),
                    since: "2026-11-09T09:16:40Z".into(),
                    stood: 1800,
                    runs: 3,
                },
            },
        ];
        let text: Vec<String> = lines
            .iter()
            .map(|l| String::from_utf8(to_line(l)).unwrap())
            .collect();
        assert_eq!(
            text,
            [
                format!("{{\"schema\":\"sluice/log@1\",\"at\":\"2026-11-09T09:16:40Z\",\"event\":\"proof-missing\",\"host\":\"build-1\",\"why\":\"{why}\"}}\n"),
                format!("{{\"schema\":\"sluice/log@1\",\"at\":\"2026-11-09T09:46:40Z\",\"event\":\"proof-kept\",\"host\":\"build-1\",\"why\":\"{why}\",\"since\":\"2026-11-09T09:16:40Z\",\"stood\":1800,\"runs\":3}}\n"),
            ]
        );
        let back: Vec<LogLine> = parse_lines(text.concat().as_bytes()).unwrap();
        assert_eq!(back, lines);

        // A branch's document written before `proof_missing`.
        let old = r#"{
  "schema": "sluice/branch@1",
  "branch": "main",
  "log": "sluice/log@1",
  "base": "c43609b",
  "verified": true,
  "window": 4,
  "next_place": 3,
  "next_candidate": 5,
  "candidates": [],
  "landing": null,
  "release": [],
  "outbox": []
}
"#;
        let doc: BranchDoc = serde_json::from_str(old).unwrap();
        assert!(doc.proof_missing.is_empty());
        assert_eq!(String::from_utf8(to_document(&doc)).unwrap(), old);
        let mut standing = doc.clone();
        standing.proof_missing.push(ProofMissing {
            host: "build-1".into(),
            why: why.into(),
            since: "2026-11-09T09:16:40Z".into(),
            runs: 3,
        });
        let v = serde_json::to_value(&standing).unwrap();
        assert_eq!(
            v["proof_missing"],
            serde_json::json!([{
                "host": "build-1",
                "why": why,
                "since": "2026-11-09T09:16:40Z",
                "runs": 3,
            }])
        );
        assert_eq!(serde_json::from_value::<BranchDoc>(v).unwrap(), standing);

        // A run written before `lost`.
        let run = r#"{"checks":["a:test"],"state":"open"}"#;
        let read: SystemRun = serde_json::from_str(run).unwrap();
        assert_eq!(read.lost, None);
        assert_eq!(serde_json::to_string(&read).unwrap(), run);
        let mut lost = read.clone();
        lost.lost = Some(LostRun {
            host: "build-1".into(),
            system: None,
            not_run: vec![NotRun {
                task: "a:test".into(),
                reason: why.into(),
                key: None,
            }],
        });
        let v = serde_json::to_value(&lost).unwrap();
        assert_eq!(
            v["lost"],
            serde_json::json!({
                "host": "build-1",
                "not_run": [{"task": "a:test", "reason": why}],
            })
        );
        assert_eq!(serde_json::from_value::<SystemRun>(v).unwrap(), lost);
    }

    /// 148: a branch's document written before refused writes were counted
    /// reads with none standing and writes back unchanged, and a
    /// `protected` line written before it named its host reads as before.
    #[test]
    fn records_written_before_refused_writes_still_read() {
        let old = r#"{
  "schema": "sluice/branch@1",
  "branch": "main",
  "log": "sluice/log@1",
  "base": "c43609b",
  "verified": true,
  "window": 4,
  "next_place": 3,
  "next_candidate": 5,
  "candidates": [],
  "landing": null,
  "release": [],
  "outbox": []
}
"#;
        let doc: BranchDoc = serde_json::from_str(old).unwrap();
        assert!(doc.refused_writes.is_empty());
        assert_eq!(String::from_utf8(to_document(&doc)).unwrap(), old);
        let line = "{\"schema\":\"sluice/log@1\",\"at\":\"2026-10-30T05:50:23Z\",\
                    \"event\":\"protected\",\"ref\":\"refs/heads/main\",\
                    \"from\":\"10e4791\",\"to\":\"8079db3\",\
                    \"by\":\"git update-ref refs/heads/main 8079db3 10e4791\"}\n";
        let read: Vec<LogLine> = parse_lines(line.as_bytes()).unwrap();
        assert_eq!(
            read[0].event,
            Event::Protected {
                reference: "refs/heads/main".into(),
                from: Rev::new("10e4791"),
                to: Some(Rev::new("8079db3")),
                by: "git update-ref refs/heads/main 8079db3 10e4791".into(),
                host: None,
            }
        );
        assert_eq!(String::from_utf8(to_line(&read[0])).unwrap(), line);
    }

    /// 148: a cause in the branch's document, a deletion's `to` written as
    /// `null`, and the `protected` and `protected-ended` lines are the
    /// spec's.
    #[test]
    fn a_refused_write_and_its_end_are_the_documents_and_the_logs() {
        let mut doc = BranchDoc::new("main");
        doc.refused_writes.push(RefusedWrite {
            host: "build-1".into(),
            reference: "refs/heads/main".into(),
            command: "git branch -D main".into(),
            times: 3,
            first: "2026-10-30T05:50:23Z".into(),
            last: "2026-11-02T16:01:20Z".into(),
            from: Rev::new("10e4791"),
            to: None,
        });
        let v = serde_json::to_value(&doc).unwrap();
        assert_eq!(
            v["refused_writes"],
            serde_json::json!([{
                "host": "build-1",
                "ref": "refs/heads/main",
                "command": "git branch -D main",
                "times": 3,
                "first": "2026-10-30T05:50:23Z",
                "last": "2026-11-02T16:01:20Z",
                "from": "10e4791",
                "to": null,
            }])
        );
        let back: BranchDoc = serde_json::from_value(v).unwrap();
        assert_eq!(back, doc);
        let lines = [
            LogLine {
                schema: LOG_SCHEMA.into(),
                at: "2026-10-30T05:50:23Z".into(),
                event: Event::Protected {
                    reference: "refs/heads/main".into(),
                    from: Rev::new("10e4791"),
                    to: None,
                    by: "git branch -D main".into(),
                    host: Some("build-1".into()),
                },
            },
            LogLine {
                schema: LOG_SCHEMA.into(),
                at: "2026-11-03T16:01:20Z".into(),
                event: Event::ProtectedEnded {
                    host: "build-1".into(),
                    reference: "refs/heads/main".into(),
                    command: "git branch -D main".into(),
                    times: 3,
                    first: "2026-10-30T05:50:23Z".into(),
                    last: "2026-11-02T16:01:20Z".into(),
                },
            },
        ];
        let text: Vec<String> = lines
            .iter()
            .map(|l| String::from_utf8(to_line(l)).unwrap())
            .collect();
        assert_eq!(
            text,
            [
                "{\"schema\":\"sluice/log@1\",\"at\":\"2026-10-30T05:50:23Z\",\"event\":\"protected\",\"ref\":\"refs/heads/main\",\"from\":\"10e4791\",\"to\":null,\"by\":\"git branch -D main\",\"host\":\"build-1\"}\n",
                "{\"schema\":\"sluice/log@1\",\"at\":\"2026-11-03T16:01:20Z\",\"event\":\"protected-ended\",\"host\":\"build-1\",\"ref\":\"refs/heads/main\",\"command\":\"git branch -D main\",\"times\":3,\"first\":\"2026-10-30T05:50:23Z\",\"last\":\"2026-11-02T16:01:20Z\"}\n",
            ]
        );
        let back: Vec<LogLine> = parse_lines(text.concat().as_bytes()).unwrap();
        assert_eq!(back, lines);
    }

    #[test]
    fn a_branch_document_with_the_hold_round_trips() {
        let mut doc = BranchDoc::new("bolt/x");
        doc.landed = Some(Rev::new("c43609b"));
        doc.held = Some(BranchHold {
            read: Rev::new("10e4791"),
            lacks: Rev::new("c43609b"),
            since: "2026-10-23T12:00:00Z".into(),
        });
        let bytes = to_document(&doc);
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["landed"], "c43609b");
        assert_eq!(
            v["held"],
            serde_json::json!({
                "read": "10e4791", "lacks": "c43609b", "since": "2026-10-23T12:00:00Z",
            })
        );
        assert_eq!(serde_json::from_slice::<BranchDoc>(&bytes).unwrap(), doc);
    }

    #[test]
    fn the_hold_and_the_acceptance_are_log_lines() {
        let lines = [
            Event::Held {
                read: Rev::new("10e4791"),
                lacks: Rev::new("c43609b"),
            },
            Event::Accepted {
                revision: Rev::new("10e4791"),
                lacked: Rev::new("c43609b"),
            },
        ];
        let text: Vec<String> = lines
            .iter()
            .map(|event| {
                let line = LogLine {
                    schema: LOG_SCHEMA.into(),
                    at: "2026-10-23T12:00:00Z".into(),
                    event: event.clone(),
                };
                String::from_utf8(to_line(&line)).unwrap()
            })
            .collect();
        assert_eq!(
            text,
            [
                "{\"schema\":\"sluice/log@1\",\"at\":\"2026-10-23T12:00:00Z\",\"event\":\"held\",\"read\":\"10e4791\",\"lacks\":\"c43609b\"}\n",
                "{\"schema\":\"sluice/log@1\",\"at\":\"2026-10-23T12:00:00Z\",\"event\":\"accepted\",\"revision\":\"10e4791\",\"lacked\":\"c43609b\"}\n",
            ]
        );
        let back: Vec<LogLine> = parse_lines(text.concat().as_bytes()).unwrap();
        assert_eq!(back.into_iter().map(|l| l.event).collect::<Vec<_>>(), lines);
    }

    /// 123: a branch file written before failed releases reads with none,
    /// and one holding them, and the new events, reads back as written.
    #[test]
    fn failed_releases_read_and_round_trip() {
        let old = r#"{"schema":"sluice/branch@1","branch":"main","log":"sluice/log@1",
            "release":["sluice/try/main/48"],"outbox":[]}"#;
        let doc: BranchDoc = serde_json::from_str(old).unwrap();
        assert!(doc.failed_releases.is_empty());
        assert_eq!(doc.release, ["sluice/try/main/48"]);
        let written = String::from_utf8(to_document(&doc)).unwrap();
        assert!(!written.contains("failed_releases"), "{written}");

        let mut doc = BranchDoc::new("main");
        doc.failed_releases.push(FailedRelease {
            place: "sluice/try/main/48".into(),
            failed: "usage: scripts/preview up".into(),
            tries: 4,
            at: "2026-10-23T09:00:00Z".into(),
            stopped: true,
            host: Some("deneb".into()),
        });
        let back: BranchDoc = serde_json::from_slice(&to_document(&doc)).unwrap();
        assert_eq!(back.failed_releases, doc.failed_releases);

        for event in [
            Event::ReleaseFailed {
                place: "sluice/try/main/48".into(),
                failed: "usage: scripts/preview up".into(),
                tries: 4,
                held: true,
            },
            Event::Gone {
                place: "session//w/a".into(),
                worktree: Some("/w/a".into()),
                ran: true,
                failed: Some("usage: scripts/preview up".into()),
            },
            Event::Released {
                place: "sluice/try/main/48".into(),
                worktree: None,
                accepted: Some("usage: scripts/preview up".into()),
            },
        ] {
            let line = LogLine {
                schema: LOG_SCHEMA.into(),
                at: "2026-10-23T09:00:00Z".into(),
                event,
            };
            let text = serde_json::to_string(&line).unwrap();
            assert_eq!(
                serde_json::from_str::<LogLine>(&text).unwrap(),
                line,
                "{text}"
            );
        }
        let line: LogLine = serde_json::from_str(
            r#"{"at":"2026-10-23T09:00:00Z","event":"released","place":"sluice/try/main/48"}"#,
        )
        .unwrap();
        assert!(matches!(line.event, Event::Released { accepted: None, .. }));
        let line: LogLine = serde_json::from_str(
            r#"{"at":"2026-10-23T09:00:00Z","event":"gone","place":"session//w/a","worktree":"/w/a"}"#,
        )
        .unwrap();
        assert!(matches!(
            line.event,
            Event::Gone {
                ran: false,
                failed: None,
                ..
            }
        ));
    }

    /// 51: a submitter recorded before sessions were reads with no host
    /// and no session and writes back byte for byte; one with both reads
    /// back as it was written.
    #[test]
    fn a_submitter_recorded_before_sessions_reads_as_it_was() {
        let old = r#"{"agent":"a","worktree":"unit/review","path":"/w/review"}"#;
        let from: Submitter = serde_json::from_str(old).unwrap();
        assert_eq!((&from.host, &from.session), (&None, &None));
        assert_eq!(serde_json::to_string(&from).unwrap(), old);

        let both = Submitter {
            host: Some("deneb".into()),
            session: Some("herdr:w1:p2@/run/herdr.sock".into()),
            ..from
        };
        let text = serde_json::to_string(&both).unwrap();
        assert_eq!(serde_json::from_str::<Submitter>(&text).unwrap(), both);
    }

    /// 51, record 0102 (D12): a submission holds no `delivered` mark; one
    /// an earlier sluice wrote, under that name or `told`, is passed over
    /// and written back without it.
    #[test]
    fn a_submission_holds_no_delivered_mark() {
        let old = submission(None);
        let doc = String::from_utf8(to_document(&old)).unwrap();
        let landed = doc.replacen("\"state\": \"queued\"", "\"state\": \"landed\"", 1);
        for marked in [
            "\"state\": \"landed\",\n  \"delivered\": \"2026-10-27T10:00:00Z\",\n  \"delivered_to\": \"/w/review\"",
            "\"state\": \"landed\",\n  \"told\": \"2026-10-27T10:00:00Z\",\n  \"told_to\": \"/w/review\"",
        ] {
            let earlier = doc.replacen("\"state\": \"queued\"", marked, 1);
            let read: Submission = serde_json::from_str(&earlier).unwrap();
            let written = String::from_utf8(to_document(&read)).unwrap();
            assert_eq!(written, landed);
            assert!(!written.contains("delivered"), "{written}");
            assert!(!written.contains("told"), "{written}");
        }
    }

    /// 51, record 0070: a `told` line an earlier sluice wrote reads as
    /// `Delivered`, which is written `delivered`; the `pending` line
    /// round-trips.
    #[test]
    fn a_told_line_reads_as_delivered() {
        let earlier = r#"{"schema":"sluice/log@1","at":"2026-10-27T10:00:00Z","event":"told","id":5,"notification":"5:landed"}"#
            .to_string();
        let line: LogLine = serde_json::from_str(&earlier).unwrap();
        assert_eq!(
            line.event,
            Event::Delivered {
                id: 5,
                notification: "5:landed".into(),
            }
        );
        let text = serde_json::to_string(&line).unwrap();
        assert!(text.contains("\"event\":\"delivered\""), "{text}");

        let line = LogLine {
            schema: LOG_SCHEMA.into(),
            at: "2026-10-28T09:00:00Z".into(),
            event: Event::Pending {
                id: 15,
                notification: "15:landed".into(),
                why: "agent is blocked".into(),
            },
        };
        let text = serde_json::to_string(&line).unwrap();
        assert!(text.contains("\"event\":\"pending\""), "{text}");
        assert_eq!(serde_json::from_str::<LogLine>(&text).unwrap(), line);
    }

    /// 50, record 0070: an outbox item keeps its key `told` on disk, which
    /// an older sluice on another host reads, and round-trips through it.
    #[test]
    fn an_outbox_item_keeps_its_told_key() {
        let item = OutboxItem {
            id: "5:landed".into(),
            submission: 5,
            outcome: DecidedRecord::Landed {
                landed_as: Rev::new("abc1234"),
                beside: false,
                covered_by: None,
            },
        };
        let text = serde_json::to_string(&item).unwrap();
        assert_eq!(
            text,
            r#"{"id":"5:landed","submission":5,"told":{"outcome":"landed","landed_as":"abc1234"}}"#
        );
        assert_eq!(serde_json::from_str::<OutboxItem>(&text).unwrap(), item);
    }

    /// 70, 140, record 0083: a branch's document written before the
    /// covering verdict was kept, `verified` and naming none, with a
    /// landing intent naming none and a landed beside outcome in its
    /// outbox, reads with none and writes back unchanged; so does a
    /// submission landed beside before it. One that names it round-trips.
    #[test]
    fn a_branch_and_an_outcome_from_before_covered_by_still_read() {
        let old = r#"{
  "schema": "sluice/branch@1",
  "branch": "main",
  "log": "sluice/log@1",
  "base": "c43609b",
  "verified": true,
  "window": 4,
  "next_place": 3,
  "next_candidate": 5,
  "candidates": [],
  "landing": {
    "ids": [
      5
    ],
    "from": "c43609b",
    "to": "abc1234",
    "commits": [
      {
        "id": 5,
        "commit": "abc1234",
        "tested": "abc1234"
      }
    ],
    "beside": true
  },
  "release": [],
  "outbox": [
    {
      "id": "5:landed",
      "submission": 5,
      "told": {
        "outcome": "landed",
        "landed_as": "abc1234",
        "beside": true
      }
    }
  ]
}
"#;
        let doc: BranchDoc = serde_json::from_str(old).unwrap();
        assert!(doc.verified);
        assert_eq!(doc.covered_by, None);
        assert_eq!(doc.landing.as_ref().unwrap().covering, None);
        assert_eq!(
            doc.outbox[0].outcome.to_decided(),
            Decided::Landed {
                landed_as: Rev::new("abc1234"),
                beside: true,
                covered_by: None,
            }
        );
        assert_eq!(String::from_utf8(to_document(&doc)).unwrap(), old);

        let mut doc = String::from_utf8(to_document(&submission(None))).unwrap();
        doc = doc.replacen(
            "\"state\": \"queued\"",
            "\"state\": \"landed\",\n  \"landed_as\": \"abc1234\",\n  \"beside\": true",
            1,
        );
        let read: Submission = serde_json::from_str(&doc).unwrap();
        assert_eq!(read.covered_by, None);
        assert_eq!(
            read.decided(),
            Some(Decided::Landed {
                landed_as: Rev::new("abc1234"),
                beside: true,
                covered_by: None,
            })
        );
        assert_eq!(String::from_utf8(to_document(&read)).unwrap(), doc);

        let covering = CoveringVerdict {
            at: Rev::new("e25fea4"),
            result: "passed".into(),
            ran: 141,
            replayed: 0,
        };
        let mut named: BranchDoc = serde_json::from_str(old).unwrap();
        named.covered_by = Some(covering.clone());
        named.landing.as_mut().unwrap().covering = Some(covering.clone());
        named.outbox[0].outcome = DecidedRecord::Landed {
            landed_as: Rev::new("abc1234"),
            beside: true,
            covered_by: Some(covering.clone()),
        };
        let text = String::from_utf8(to_document(&named)).unwrap();
        assert!(
            text.contains(
                "\"covered_by\": {\n    \"at\": \"e25fea4\",\n    \"result\": \"passed\",\n    \
                 \"ran\": 141,\n    \"replayed\": 0\n  }"
            ),
            "{text}"
        );
        assert_eq!(serde_json::from_str::<BranchDoc>(&text).unwrap(), named);
        let landed = Submission {
            covered_by: Some(covering.clone()),
            ..read
        };
        let text = String::from_utf8(to_document(&landed)).unwrap();
        assert_eq!(serde_json::from_str::<Submission>(&text).unwrap(), landed);
        assert_eq!(
            landed.decided(),
            Some(Decided::Landed {
                landed_as: Rev::new("abc1234"),
                beside: true,
                covered_by: Some(covering),
            })
        );
    }

    /// 51: a landed, blamed or conflicting submission's record names the
    /// outcome its round's outbox item holds, under the same notification.
    #[test]
    fn a_decided_submission_names_the_outcome_its_round_delivers() {
        let landed = Submission {
            state: State::Landed,
            landed_as: Some(Rev::new("abc1234")),
            ..submission(None)
        };
        assert_eq!(
            landed.decided(),
            Some(Decided::Landed {
                landed_as: Rev::new("abc1234"),
                beside: false,
                covered_by: None,
            })
        );
        assert_eq!(landed.notification().as_deref(), Some("12:landed"));

        let failed = Submission {
            state: State::Failed,
            tested: Some(Tested {
                combination: Rev::new("c1"),
                on: Rev::new("b1"),
                submissions: vec![11, 12],
            }),
            result: Some("failed".into()),
            failed: vec![
                FailedTask {
                    task: "cli:test".into(),
                    hash: "h".into(),
                    log: Some("logs/c1/cli-test.log".into()),
                    flaky: None,
                    system: None,
                    not_run: None,
                    failed_by: Vec::new(),
                    unsigned: None,
                },
                FailedTask {
                    task: "core:test".into(),
                    hash: String::new(),
                    log: None,
                    flaky: None,
                    system: None,
                    not_run: None,
                    failed_by: Vec::new(),
                    unsigned: None,
                },
            ],
            ..submission(None)
        };
        assert_eq!(
            failed.decided(),
            Some(Decided::Failed {
                failed: vec!["cli:test".into(), "core:test".into()],
                combination: Rev::new("c1"),
                submissions: vec![11, 12],
                timed_out: false,
                logs: vec![TaskLog {
                    task: "cli:test".into(),
                    text: "logs/c1/cli-test.log".into(),
                }],
            })
        );
        assert_eq!(failed.notification().as_deref(), Some("12:failed"));

        let timed_out = Submission {
            result: Some("timed out".into()),
            ..failed
        };
        assert!(matches!(
            timed_out.decided(),
            Some(Decided::Failed {
                timed_out: true,
                ..
            })
        ));

        let with = vec![Contested {
            id: 11,
            from: submission(None).from,
        }];
        let conflict = Submission {
            state: State::Conflict,
            conflict: Some(ConflictRecord {
                paths: vec!["src/lib.rs".into()],
                with: with.clone(),
            }),
            ..submission(None)
        };
        assert_eq!(
            conflict.decided(),
            Some(Decided::Conflict {
                paths: vec!["src/lib.rs".into()],
                with,
            })
        );
        assert_eq!(conflict.notification().as_deref(), Some("12:conflict"));
    }

    /// 51: a submission queued or withdrawn has no outcome a round tells.
    #[test]
    fn a_queued_or_withdrawn_submission_names_no_outcome() {
        let queued = submission(None);
        assert_eq!((queued.decided(), queued.notification()), (None, None));
        let withdrawn = Submission {
            state: State::Withdrawn,
            withdrawn: Some(Withdrawal {
                by: "a".into(),
                at: "t".into(),
                handed_over_to: None,
            }),
            ..submission(None)
        };
        assert_eq!(
            (withdrawn.decided(), withdrawn.notification()),
            (None, None)
        );
    }

    /// 120, 137: a hand-over's withdrawal names the host it handed over
    /// to, in the record and in the log line; one written before it, and
    /// any other withdrawal, names none and is written back as it was read
    /// (record 0088 as amended).
    #[test]
    fn a_hand_overs_withdrawal_names_its_host_and_an_older_one_reads_as_it_was() {
        let old = r#"{"by":"b takes main's pull requests in","at":"2026-10-30T16:18:13Z"}"#;
        let w: Withdrawal = serde_json::from_str(old).unwrap();
        assert_eq!(w.handed_over_to, None);
        assert_eq!(serde_json::to_string(&w).unwrap(), old);

        let new = r#"{"by":"b takes main's pull requests in","at":"t","handed_over_to":"b"}"#;
        let w: Withdrawal = serde_json::from_str(new).unwrap();
        assert_eq!(w.handed_over_to.as_deref(), Some("b"));
        assert_eq!(serde_json::to_string(&w).unwrap(), new);

        let old =
            r#"{"schema":"sluice/log@1","at":"t","event":"withdrawn","id":4,"by":"operator"}"#;
        let l: LogLine = serde_json::from_str(old).unwrap();
        assert!(matches!(
            &l.event,
            Event::Withdrawn {
                handed_over_to: None,
                ..
            }
        ));
        assert_eq!(serde_json::to_string(&l).unwrap(), old);
        let new = LogLine {
            schema: "sluice/log@1".into(),
            at: "t".into(),
            event: Event::Withdrawn {
                id: 4,
                by: "b takes main's pull requests in".into(),
                handed_over_to: Some("b".into()),
            },
        };
        let written = serde_json::to_string(&new).unwrap();
        assert!(written.ends_with(r#""handed_over_to":"b"}"#), "{written}");
        assert_eq!(serde_json::from_str::<LogLine>(&written).unwrap(), new);
    }

    /// A flaky failure recorded before its times and hosts reads with
    /// them left out, and is written back without them.
    #[test]
    fn a_flaky_failure_written_before_its_hosts_reads_as_it_was() {
        let old = r#"{"task":"a:test","hash":"h","flaky":{"here":"pass","other":{"outcome":"fail","at":"t","branch":"main","combination":"c"}}}"#;
        let f: FailedTask = serde_json::from_str(old).unwrap();
        let k = f.flaky.as_ref().expect("flaky");
        assert_eq!((&k.at, &k.host, &k.other.host), (&None, &None, &None));
        assert_eq!(serde_json::to_string(&f).unwrap(), old);
    }

    /// 108, C.6 (design D4 and D5 of
    /// `a-signed-run-without-a-sandbox-proves-its-passes`): an `unproven`
    /// line with its keys, and a failed check with why its key was not
    /// borne out, each read back as written; a line and a submission
    /// written before these fields read with none and are written without
    /// them.
    #[test]
    fn an_unproven_line_and_an_unsigned_check_read_back() {
        let line = LogLine {
            schema: LOG_SCHEMA.into(),
            at: "2026-11-11T09:00:00Z".into(),
            event: Event::Unproven {
                combination: Rev::new("c1"),
                tasks: vec!["a:test".into()],
                keys: vec![UnprovenKey {
                    task: "a:test".into(),
                    key: "k1".into(),
                    why: "no result of its key is signed by a signer the branch counts".into(),
                }],
            },
        };
        let json = serde_json::to_string(&line).unwrap();
        assert!(
            json.contains(r#""keys":[{"task":"a:test","key":"k1","why":"#),
            "{json}"
        );
        assert_eq!(serde_json::from_str::<LogLine>(&json).unwrap(), line);

        let old = r#"{"schema":"sluice/log@1","at":"2026-11-11T09:00:00Z","event":"unproven","combination":"c1","tasks":["a:test"]}"#;
        let read: LogLine = serde_json::from_str(old).unwrap();
        assert!(
            matches!(&read.event, Event::Unproven { keys, .. } if keys.is_empty()),
            "{read:?}"
        );
        assert_eq!(serde_json::to_string(&read).unwrap(), old);

        let unsigned = FailedTask {
            task: "a:test".into(),
            hash: "k1".into(),
            log: None,
            flaky: None,
            system: None,
            not_run: None,
            failed_by: Vec::new(),
            unsigned: Some("a:test: a/x is not in the tree".into()),
        };
        let json = serde_json::to_string(&unsigned).unwrap();
        assert_eq!(
            json,
            r#"{"task":"a:test","hash":"k1","unsigned":"a:test: a/x is not in the tree"}"#
        );
        assert_eq!(serde_json::from_str::<FailedTask>(&json).unwrap(), unsigned);

        let failed = Submission {
            state: State::Failed,
            result: Some("failed".into()),
            failed: vec![FailedTask {
                unsigned: None,
                ..unsigned.clone()
            }],
            ..submission(None)
        };
        let before = serde_json::to_string(&failed).unwrap();
        assert!(!before.contains("unsigned"), "{before}");
        let read: Submission = serde_json::from_str(&before).unwrap();
        assert_eq!(read.failed[0].unsigned, None);
        assert_eq!(serde_json::to_string(&read).unwrap(), before);
    }

    /// 138 (check-grant spec, "An old line read back"): a result line
    /// whose executor carries a grant reads back with it; one whose
    /// executor names none, written before the grant, reads with `None`
    /// and is written again without the key.
    #[test]
    fn a_result_line_with_no_grant_reads_back() {
        let new = r#"{"schema":"sluice/result@1","at":"t","kind":"ran","task":"a:test","hash":"h","outcome":"pass","host":"h1","executor":{"kind":"sandbox","confined":false,"grant":"sb1:0123456789abcdef0123456789abcdef"},"branch":"main","combination":"c"}"#;
        let read: ResultLine = serde_json::from_str(new).unwrap();
        assert_eq!(
            read.executor.as_ref().and_then(|e| e.grant.as_deref()),
            Some("sb1:0123456789abcdef0123456789abcdef")
        );
        assert_eq!(serde_json::to_string(&read).unwrap(), new);

        let old = r#"{"schema":"sluice/result@1","at":"t","kind":"ran","task":"a:test","hash":"h","outcome":"fail","host":"h1","executor":{"kind":"open","confined":false},"branch":"main","combination":"c"}"#;
        let read: ResultLine = serde_json::from_str(old).unwrap();
        let executor = read.executor.as_ref().expect("a ran line's executor");
        assert_eq!(executor.grant, None);
        let written = serde_json::to_string(&read).unwrap();
        assert!(!written.contains("grant"), "{written}");
        assert_eq!(written, old);
    }

    /// 149: a failed check recorded before `failed_by` reads with none,
    /// and is written back as it was; one failed by a task it depends on
    /// names it, with no key of its own.
    #[test]
    fn a_failed_check_written_before_failed_by_reads_as_it_was() {
        for old in [
            r#"{"task":"root:integration"}"#,
            r#"{"task":"a:test","hash":"h","log":"logs/c1/a-test.log"}"#,
            r#"{"task":"site:check","not_run":"exited 77"}"#,
        ] {
            let f: FailedTask = serde_json::from_str(old).unwrap();
            assert!(f.failed_by.is_empty());
            assert_eq!(serde_json::to_string(&f).unwrap(), old);
        }
        let by = FailedTask {
            task: "root:integration".into(),
            hash: String::new(),
            log: None,
            flaky: None,
            system: None,
            not_run: None,
            failed_by: vec!["api:integration".into()],
            unsigned: None,
        };
        let json = serde_json::to_string(&by).unwrap();
        assert_eq!(
            json,
            r#"{"task":"root:integration","failed_by":["api:integration"]}"#
        );
        assert_eq!(serde_json::from_str::<FailedTask>(&json).unwrap(), by);
    }
}
