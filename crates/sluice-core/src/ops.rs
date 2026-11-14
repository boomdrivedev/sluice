//! What a driver asks of sluice: submit, forecast, amend, withdraw and
//! read a state (B.5, 47 to 49). None of these forms a candidate it
//! keeps, runs a check or moves a branch (1; rule core.4).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use sluice_seams::{Contested, Formed, HostCopy, Rev, Session, Submitter};

use crate::line::Line;
use crate::order::{self, Entry};
use crate::overview::{after_landed, BranchView};
use crate::records::{CandidateState, Event, State, Submission, Withdrawal, SUBMISSION_SCHEMA};
use crate::{Found, Sluice};

/// What a submit names as a head it submits: a session's place, or a
/// branch sluice lands on, named (124).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmitHead {
    /// A session's place, as its driver names it: the head it holds.
    Worktree(String),
    /// 124: a branch sluice lands on, named from anywhere in the
    /// repository: the branch as its policy holds it, its work and its
    /// head, submitted from no worktree and no session (record 0090; rule
    /// core.50).
    Branch(String),
}

impl SubmitHead {
    /// The place or the branch, as the submit named it: what a refusal of
    /// it is answered with.
    pub fn name(&self) -> &str {
        match self {
            SubmitHead::Worktree(name) | SubmitHead::Branch(name) => name,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmitRequest {
    /// The session's place, or the branch named (124).
    pub head: SubmitHead,
    pub branch: String,
    pub after: Vec<u64>,
    pub priority: i64,
    /// 23: the submitter's words for what lands.
    pub subject: Option<String>,
}

/// 125: several heads submitted to one branch as one act.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupRequest {
    pub branch: String,
    /// The places, or the branches named (124), in the order their heads
    /// land.
    pub heads: Vec<SubmitHead>,
    /// Every submission of the act lands after these, alike.
    pub after: Vec<u64>,
    pub priority: i64,
    /// 23, 125: each head's subject, in the heads' order, or none.
    pub subjects: Vec<String>,
}

impl GroupRequest {
    /// Each head's place or branch, as the act named it, in its order.
    pub fn names(&self) -> Vec<String> {
        self.heads.iter().map(|h| h.name().to_string()).collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accepted {
    pub id: u64,
    /// Submissions queued ahead of it on the branch, in the order a round
    /// takes them (6, 7).
    pub ahead: Vec<u64>,
    /// The earlier submission from the same place it withdrew (67).
    pub withdrew: Vec<u64>,
    /// What the submitter should know of a submission taken: that the
    /// runner could not be asked which required checks it keeps out of
    /// CI, and why (ADR 0050).
    pub warnings: Vec<String>,
    /// 124: the worktree's head, this host's copy of a branch sluice lands
    /// on, where it differs from the remote's revision submitted.
    pub copy: Option<CopyDiffers>,
}

/// 124: the head a session submits, and how the worktree's head differs
/// from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmittedHead {
    pub head: Rev,
    /// The head is the remote's revision of a branch sluice lands on, or a
    /// branch named (124): the submit pushes nothing to it (1).
    pub from_remote: bool,
    pub copy: Option<CopyDiffers>,
}

/// 124: a worktree on a branch sluice lands on, kept on a remote, whose
/// head, this host's copy of the branch, differs from the remote's
/// revision of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CopyDiffers {
    pub branch: String,
    /// The worktree's head: this host's copy.
    pub copy: Rev,
    /// The remote the branch's policy names.
    pub remote: String,
    /// The remote's revision of the branch, which is submitted.
    pub at: Rev,
    pub how: CopyHow,
}

/// How a host's copy stands to the remote's revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CopyHow {
    /// The remote's revision holds the copy.
    Behind,
    /// The copy holds the remote's revision.
    Ahead,
    /// Neither holds the other.
    Apart,
}

impl CopyHow {
    /// How the copy stands to the remote's revision, as a submit says it.
    pub fn words(self) -> &'static str {
        match self {
            CopyHow::Behind => "behind",
            CopyHow::Ahead => "ahead of",
            CopyHow::Apart => "apart from",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmitRefusal {
    /// 2: the place holds more than its commits.
    Dirty(Vec<String>),
    /// 3: the place is on no branch.
    Detached,
    NoBranch,
    /// No `[branches]` table at the branch's revision names it (56).
    NotManaged,
    /// `after` names submissions the branch does not hold.
    UnknownAfter(Vec<u64>),
    /// The branch named as the head to forecast or to submit has no
    /// revision.
    NoHead(String),
    /// 124: the branch named as the head to submit is the branch it would
    /// be submitted to.
    ToItself,
    /// 124: the branch named as the head to submit is no branch sluice
    /// lands on: no `[branches]` table at its revision names it (56). A
    /// branch of a worktree's own is submitted from that worktree (2).
    NotLandedOn,
    /// 125: a group names this place already, as the place given.
    NamedTwice(String),
    /// ADR 0050: the branch's gate requires checks the runner, in the
    /// submitting worktree, keeps out of CI, so the branch cannot pass.
    KeptOut(Vec<KeptOut>),
}

/// ADR 0050: a check the branch's gate requires that the runner keeps out
/// of CI in the submitting worktree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptOut {
    /// The task, as the runner names it.
    pub task: String,
    /// The required entry that names it: the task, or a tag it carries.
    pub entry: String,
    /// The line of `.config/sluice.toml`, at the branch's revision, the
    /// entry is written on.
    pub line: usize,
}

/// Who asks to change a submission (48, 47b; actors table).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Requester {
    /// The session that submitted, by its agent's name.
    Session(String),
    Planner,
    Operator,
}

impl Requester {
    fn word(&self) -> String {
        match self {
            Requester::Session(a) => a.clone(),
            Requester::Planner => "planner".into(),
            Requester::Operator => "operator".into(),
        }
    }

    fn may_change(&self, sub: &Submission) -> bool {
        match self {
            Requester::Session(a) => *a == sub.from.agent,
            Requester::Planner | Requester::Operator => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AmendRefusal {
    /// On none of the branches read: the branches that could not be read,
    /// none when every branch was (26).
    NoSuchSubmission(Vec<Unread>),
    /// On more than one branch: it names none of them alone, and nothing
    /// is changed (26; record 0114).
    Several(Several),
    /// A session may not change another's order (actors table).
    NotYours,
    /// Under test or judged: its order is fixed (6).
    UnderTest,
    /// Landed, withdrawn, blamed or conflicting.
    NotQueued(State),
    UnknownAfter(Vec<u64>),
}

/// What a lookup by number found (26; record 0114).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lookup {
    Found(Submission),
    /// On none of the branches read: the branches that could not be read,
    /// none when every branch was.
    Unfound(Vec<Unread>),
    /// On more than one branch, so it names none of them alone.
    Several(Several),
}

/// How a withdrawal went: done now, or done before (5, 48).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Withdrew {
    Now,
    /// Withdrawn already: it left the queue by withdrawal, so asking again
    /// changes nothing and writes nothing.
    Already,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WithdrawRefusal {
    /// As [`AmendRefusal::NoSuchSubmission`].
    NoSuchSubmission(Vec<Unread>),
    /// As [`AmendRefusal::Several`].
    Several(Several),
    NotYours,
    /// Landed or landing (5).
    Final(State),
}

/// 64, 47a: what a head would meet if submitted now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Forecast {
    Clear,
    Conflict {
        paths: Vec<String>,
        with: Vec<Contested>,
    },
}

/// A forecast with what it was formed from, so a reader can tell whether
/// any of it has moved since and forecast again only then (64).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Forecasted {
    pub head: Rev,
    /// The branch's revision the head was formed on.
    pub onto: Rev,
    /// The submissions formed ahead of the head, in order.
    pub ahead: Vec<u64>,
    pub forecast: Forecast,
}

/// 64: a branch's queue as a forecast forms on it, read from its records
/// or from a reader's read of them: one forming path for both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Queue {
    /// The branch's revision the forecast forms on.
    tip: Rev,
    /// The submissions not out of the queue, by number.
    subs: BTreeMap<u64, Queued>,
    /// The combinations formed on the branch (10).
    combinations: Vec<Combination>,
    /// The landed submissions a queued one's `after` names (7).
    landed: BTreeSet<u64>,
}

/// A submission in the queue, as a forecast orders and forms it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Queued {
    head: Rev,
    from: Submitter,
    state: State,
    priority: i64,
    after: Vec<u64>,
}

/// A combination formed on the branch, as a forecast takes it (10).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Combination {
    on: Rev,
    submissions: Vec<u64>,
    revision: Rev,
    state: CandidateState,
}

impl Queued {
    fn of(s: &Submission) -> Queued {
        Queued {
            head: s.head.clone(),
            from: s.from.clone(),
            state: s.state,
            priority: s.priority,
            after: s.after.clone(),
        }
    }
}

impl Queue {
    /// The queue the records hold, formed on `tip`, the branch's revision
    /// as its caller just read it.
    pub(crate) fn from_line(line: &Line, tip: Rev) -> Queue {
        Queue {
            tip,
            subs: line
                .subs
                .values()
                .filter(|s| !s.state.is_out())
                .map(|s| (s.id, Queued::of(s)))
                .collect(),
            combinations: line
                .doc
                .candidates
                .iter()
                .map(|c| Combination {
                    on: c.on.clone(),
                    submissions: c.submissions.clone(),
                    revision: c.revision.clone(),
                    state: c.state,
                })
                .collect(),
            landed: after_landed(line).into_iter().collect(),
        }
    }

    /// 64: the queue a reader's read of the branch found, formed on the
    /// revision that read found; `None` where it names none, or names a
    /// combination with no revision it was formed on, as a document from
    /// before them does.
    pub(crate) fn from_view(view: &BranchView) -> Option<Queue> {
        let combinations = view
            .candidates
            .iter()
            .map(|c| {
                Some(Combination {
                    on: c.on.clone()?,
                    submissions: c.submissions.clone(),
                    revision: c.revision.clone(),
                    state: c.state,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Queue {
            tip: view.revision.clone()?,
            subs: view
                .submissions
                .iter()
                .filter(|s| !s.state.is_out())
                .map(|s| (s.id, Queued::of(s)))
                .collect(),
            combinations,
            landed: view.after_landed.iter().copied().collect(),
        })
    }

    fn sub(&self, id: u64) -> Result<&Queued> {
        self.subs
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("no submission {id} in the queue"))
    }
}

/// A branch's queue as read from its records (26, 49).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub branch: String,
    pub at: Option<Rev>,
    /// 105: the branch itself is gone: the host holds no revision of it,
    /// read with no fetch where the records have seen one. Its records
    /// stay, and are read as any branch's.
    pub gone: bool,
    /// 26, 105: the branch has records: its read found some, or, where it
    /// found none of a branch the host holds no revision of, the records
    /// list it, as the overview does.
    pub recorded: bool,
    pub testing: usize,
    pub window: usize,
    pub parallel: Option<usize>,
    /// The submissions in the branch's live records.
    pub submissions: Vec<Submission>,
    /// 6 to 8: the branch's chain on `at`, in the order a round takes it,
    /// what is landing first ([`Line::order`]).
    pub chain: Vec<u64>,
    /// How many have moved to its finished records, which
    /// [`Sluice::history`] reads.
    pub finished: usize,
    /// Each submission moved to its finished records by its name, as the
    /// finished index keeps it, for a row that names one (26, 130).
    pub named: BTreeMap<u64, String>,
    /// What the policy at `at` says that sluice reads differently than
    /// it is written.
    pub warnings: Vec<String>,
    /// 80: the remote the branch's state could not be read from just now,
    /// so what is shown is what was last fetched from it.
    pub stale: Option<String>,
    /// 123: the branch's checkouts whose release failed, still tried or
    /// held.
    pub failed_releases: Vec<crate::records::FailedRelease>,
    /// 121: the branch was last read at a revision lacking its last
    /// landing, and nothing is formed or landed on it.
    pub held: Option<crate::records::BranchHold>,
    /// 124, S64: this host's copy of the branch, while a round could not
    /// level it to the landing.
    pub copy: Option<crate::records::CopyBehind>,
    /// 133: this host's refusal of the branch's remote, while it stands
    /// (record 0061).
    pub refused: Option<Refused>,
    /// 133 (design D27): this host's refusal of the branch's intake, its
    /// pull requests on the forge, while it stands: what its latest pass
    /// could not ask, since when and for how many passes.
    pub intake_refused: Option<Refused>,
    /// 148: every host's causes of writes the protection refused that
    /// stand now, newest last refusal first (design D5).
    pub refused_writes: Vec<crate::records::RefusedWrite>,
    /// 124, 148: whether the branch is kept on a remote, so the refused
    /// writes were to sluice's copy of it, for the words after them
    /// (design D5). Read only while any stand, and `false` while none do.
    pub on_remote: bool,
}

/// 133: what this host's rounds of a branch could not read of its remote,
/// as the host keeps it: the ref and git's words its latest refused round
/// met, since when and for how many rounds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Refused {
    pub remote: String,
    pub asked: String,
    pub said: String,
    pub since: String,
    pub rounds: u64,
}

impl Status {
    /// How many submissions are still in a chain, as the overview counts
    /// a branch's `depth`.
    pub fn depth(&self) -> usize {
        crate::overview::in_queue(&self.submissions).count()
    }

    /// No such branch, and no records of one: nothing sluice lands on
    /// and nothing a status reads (105). A branch gone whose records stay
    /// is known by them.
    pub fn is_unknown(&self) -> bool {
        self.at.is_none() && !self.recorded
    }

    /// 6, 7: the submissions ahead of submission `id` in the queue, in the
    /// order a round takes them ([`order::ahead_of`]).
    pub fn ahead_of(&self, id: u64) -> Vec<u64> {
        order::ahead_of(id, &self.chain, |i| {
            self.submissions.iter().find(|s| s.id == i)
        })
    }
}

impl Sluice<'_> {
    /// 124, 1: the head `session` submits. From a worktree that has checked
    /// out a branch sluice lands on, kept on a remote, it is the remote's
    /// revision of that branch, never the worktree's head, which is this
    /// host's copy; where the two differ, it says how. From any other
    /// worktree, the session's head. It reads, and writes nothing (rule
    /// core.4).
    pub fn submitted_head(&self, session: &Session) -> Result<SubmittedHead> {
        let own = SubmittedHead {
            head: session.head.clone(),
            from_remote: false,
            copy: None,
        };
        let branch = &session.from.worktree;
        // A name that is no branch, as a pull request's `pull/<n>`, reads
        // none.
        let Some(at) = self.seams.branch.read(branch)? else {
            return Ok(own);
        };
        let Some(rules) = self.rules_at(branch, &at)? else {
            return Ok(own);
        };
        if self.seams.branch.copy(branch)? == HostCopy::Itself {
            return Ok(own);
        }
        let copy = (session.head != at)
            .then(|| -> Result<CopyDiffers> {
                let how = if self.seams.branch.contains(&at, &session.head)? {
                    CopyHow::Behind
                } else if self.seams.branch.contains(&session.head, &at)? {
                    CopyHow::Ahead
                } else {
                    CopyHow::Apart
                };
                Ok(CopyDiffers {
                    branch: branch.clone(),
                    copy: session.head.clone(),
                    remote: rules.remote.clone().unwrap_or_default(),
                    at: at.clone(),
                    how,
                })
            })
            .transpose()?;
        Ok(SubmittedHead {
            head: at,
            from_remote: true,
            copy,
        })
    }

    /// 47, 1, 2, 3, 67: admit a head to a branch's queue, and nothing
    /// more: a group of one.
    pub fn submit(&self, req: &SubmitRequest) -> Result<Result<Accepted, SubmitRefusal>> {
        let group = GroupRequest {
            branch: req.branch.clone(),
            heads: vec![req.head.clone()],
            after: req.after.clone(),
            priority: req.priority,
            subjects: req.subject.iter().cloned().collect(),
        };
        Ok(match self.submit_group(&group)? {
            Ok(mut accepted) => Ok(accepted.remove(0)),
            Err(mut refused) => Err(refused.remove(0).1),
        })
    }

    /// 125, 47, 1, 2, 3, 67, 124: admit several heads to a branch's queue
    /// as one act, in the order given, or none of them. Every head is read
    /// before anything is written, and every refusal is answered with the
    /// place or the branch it refuses; then the submissions are written in
    /// one write of the branch's records, so no reader sees some of them
    /// and not the rest (62; rule core.14). Each lands after the one
    /// before by its order of submission (6), never as another's `after`.
    pub fn submit_group(
        &self,
        req: &GroupRequest,
    ) -> Result<Result<Vec<Accepted>, Vec<(String, SubmitRefusal)>>> {
        if req.heads.is_empty() {
            bail!("a group submit to {} names no worktree", req.branch);
        }
        if !req.subjects.is_empty() && req.subjects.len() != req.heads.len() {
            bail!(
                "a group submit to {} gives {} subjects for {} worktrees",
                req.branch,
                req.subjects.len(),
                req.heads.len()
            );
        }
        let mut refused = Vec::new();
        // Each head read, by the name it was given, with the head a branch
        // named submits, read with it.
        let mut sessions: Vec<(&str, Session, Option<SubmittedHead>)> = Vec::new();
        for head in &req.heads {
            let name = head.name();
            let (session, submitted) = match head {
                SubmitHead::Worktree(worktree) => (self.seams.driver.session(worktree)?, None),
                SubmitHead::Branch(branch) => match self.named_session(branch, &req.branch)? {
                    Ok((session, submitted)) => (session, Some(submitted)),
                    Err(r) => {
                        refused.push((name.to_string(), r));
                        continue;
                    }
                },
            };
            let before = sessions
                .iter()
                .find(|(n, s, _)| *n == name || s.from.worktree == session.from.worktree);
            if let Some((earlier, _, _)) = before {
                refused.push((
                    name.to_string(),
                    SubmitRefusal::NamedTwice(earlier.to_string()),
                ));
            } else if session.detached {
                refused.push((name.to_string(), SubmitRefusal::Detached));
            } else if !session.dirty.is_empty() {
                refused.push((
                    name.to_string(),
                    SubmitRefusal::Dirty(session.dirty.clone()),
                ));
            }
            sessions.push((name, session, submitted));
        }
        // What the branch refuses, it refuses for every head.
        let mut whole = |r: SubmitRefusal| {
            for h in &req.heads {
                refused.push((h.name().to_string(), r.clone()));
            }
        };
        let tip = self.seams.branch.read(&req.branch)?;
        match &tip {
            None => whole(SubmitRefusal::NoBranch),
            Some(tip) if self.rules_at(&req.branch, tip)?.is_none() => {
                whole(SubmitRefusal::NotManaged)
            }
            Some(_) => {
                let line = self.read_line(&req.branch)?;
                let unknown: Vec<u64> = req
                    .after
                    .iter()
                    .copied()
                    .filter(|a| !line.has(*a))
                    .collect();
                if !unknown.is_empty() {
                    whole(SubmitRefusal::UnknownAfter(unknown));
                }
            }
        }
        let (Some(tip), true) = (tip, refused.is_empty()) else {
            return Ok(Err(refused));
        };
        // ADR 0050: each place's runner is asked which of the gate's
        // required checks it keeps out of CI; a place that keeps one out is
        // refused, and the act with it. A branch named has no worktree to
        // ask, so is taken with nothing asked.
        let mut warnings = Vec::new();
        for (name, session, _) in &sessions {
            match self.check_kept_out(session, &req.branch, &tip)? {
                Ok(w) => warnings.push(w),
                Err(r) => refused.push((name.to_string(), r)),
            }
        }
        if !refused.is_empty() {
            return Ok(Err(refused));
        }
        // 124: from a worktree on a branch sluice lands on, the remote's
        // revision of it; a branch named, as it was read.
        let submitted = sessions
            .iter()
            .map(|(_, s, named)| match named {
                Some(h) => Ok(h.clone()),
                None => self.submitted_head(s),
            })
            .collect::<Result<Vec<_>>>()?;
        let sessions: Vec<Session> = sessions.into_iter().map(|(_, s, _)| s).collect();
        // 82: every head is fetchable by every host before any is
        // submitted.
        for s in &submitted {
            self.seams.state.publish_head(&req.branch, &s.head)?;
        }
        let mut ids = Vec::new();
        for _ in &sessions {
            ids.push(self.seams.state.allocate_id(&req.branch)?);
        }
        let now = self.now();
        let _lock = self.seams.state.wait_lock(&req.branch)?;
        let accepted = self.update(&req.branch, |l| {
            let mut accepted = Vec::new();
            // 125, 140: the act's heads, named by its first, where it has
            // several.
            let act = (ids.len() > 1).then(|| ids[0]);
            for (k, ((&id, session), head)) in ids.iter().zip(&sessions).zip(&submitted).enumerate()
            {
                let subject = req.subjects.get(k).cloned();
                accepted.push(admit(l, req, id, act, session, head, subject, &now));
            }
            Ok(accepted)
        })?;
        let accepted = accepted
            .into_iter()
            .zip(warnings)
            .map(|(a, warnings)| Accepted { warnings, ..a })
            .collect();
        // 111: work on a branch whose records are on its remote may be
        // taken by no instance now running: the first pool is asked to
        // advance the branch, once for the act. A wake that is lost delays
        // it and loses nothing (59).
        if let Some(policy) = self.policy_at(&tip)? {
            let rules = policy.rules_for(&req.branch)?.cloned();
            if let (Some(rules), Some(pool)) = (rules, policy.pools.first()) {
                if rules.signed {
                    if let Err(e) =
                        self.seams
                            .tests
                            .wake(&pool.kind, &req.branch, rules.remote.as_deref())
                    {
                        tracing::warn!(
                            sluice.branch = %req.branch,
                            "waking the pool {} failed: {e:#}",
                            pool.name
                        );
                    }
                }
            }
        }
        Ok(Ok(accepted))
    }

    /// 124: the session of `branch`, named as the head to submit to
    /// `target`, and the head it submits: who the driver says submits it,
    /// the branch as its work and its head, with no worktree and no session
    /// (record 0090), and the branch as one read of the branch seam gives
    /// it, the remote's revision where its policy names a remote. It asks
    /// the driver for no place's session, so reads nothing of the folder
    /// it runs in (rule core.48), and says nothing of the host's copy of
    /// the branch, which no worktree submits. Refused where `branch` is
    /// `target`, has no revision, or is no branch sluice lands on (2).
    /// Writes nothing (rule core.4).
    pub fn named_session(
        &self,
        branch: &str,
        target: &str,
    ) -> Result<std::result::Result<(Session, SubmittedHead), SubmitRefusal>> {
        if branch == target {
            return Ok(Err(SubmitRefusal::ToItself));
        }
        let Some(at) = self.seams.branch.read(branch)? else {
            return Ok(Err(SubmitRefusal::NoHead(branch.into())));
        };
        if self.rules_at(branch, &at)?.is_none() {
            return Ok(Err(SubmitRefusal::NotLandedOn));
        }
        let session = Session {
            head: at.clone(),
            from: self.seams.driver.named(branch)?,
            dirty: Vec::new(),
            detached: false,
        };
        let submitted = SubmittedHead {
            head: at,
            from_remote: true,
            copy: None,
        };
        Ok(Ok((session, submitted)))
    }

    /// ADR 0050: refuse `session`'s submission to `branch`, whose revision
    /// is `tip`, when its gate requires checks the runner keeps out of CI
    /// in the session's worktree; otherwise the warnings to take it with.
    /// A session with no worktree on this host has nothing to ask. Writes
    /// nothing.
    pub fn check_kept_out(
        &self,
        session: &Session,
        branch: &str,
        tip: &Rev,
    ) -> Result<std::result::Result<Vec<String>, SubmitRefusal>> {
        let Some(path) = &session.from.path else {
            return Ok(Ok(Vec::new()));
        };
        Ok(match self.kept_out_in(branch, tip, Path::new(path))? {
            Ok(kept) if !kept.is_empty() => Err(SubmitRefusal::KeptOut(kept)),
            Ok(_) => Ok(Vec::new()),
            Err(why) => Ok(vec![why]),
        })
    }

    /// ADR 0050: the checks `branch`'s gate at `tip` requires that the
    /// runner keeps out of CI in the worktree at `worktree`, which holds
    /// the head to submit (2). The gate and its lines are the branch's
    /// (56); only the runner's own setting is the worktree's. `Err` says
    /// why the runner could not be asked, for a warning: the submission
    /// is taken unchecked. Writes nothing.
    pub fn kept_out_in(
        &self,
        branch: &str,
        tip: &Rev,
        worktree: &Path,
    ) -> Result<std::result::Result<Vec<KeptOut>, String>> {
        let Some(policy) = self.policy_at(tip)? else {
            return Ok(Ok(Vec::new()));
        };
        let Some(rules) = policy.rules_for(branch)? else {
            return Ok(Ok(Vec::new()));
        };
        if rules.required.is_empty() {
            return Ok(Ok(Vec::new()));
        }
        let kept = match self
            .seams
            .tests
            .kept_out(worktree, &policy.tests, &rules.required)
        {
            Ok(k) => k,
            Err(e) => {
                return Ok(Err(format!(
                    "could not check which of {branch}'s required checks the task runner in \
                     this worktree keeps out of CI, so the submission was taken unchecked: {e:#}"
                )))
            }
        };
        Ok(Ok(kept
            .into_iter()
            .map(|k| {
                let line = rules
                    .required
                    .iter()
                    .position(|r| *r == k.entry)
                    .and_then(|i| rules.required_lines.get(i).copied())
                    .unwrap_or(0);
                KeptOut {
                    task: k.task,
                    entry: k.entry,
                    line,
                }
            })
            .collect()))
    }

    /// 64, 65, 47a: the conflicts a place's head would meet if submitted
    /// now, placed after everything already submitted. Writes nothing.
    pub fn forecast(
        &self,
        worktree: &str,
        branch: &str,
        after: &[u64],
    ) -> Result<Result<Forecasted, SubmitRefusal>> {
        let session = self.seams.driver.session(worktree)?;
        if session.detached {
            return Ok(Err(SubmitRefusal::Detached));
        }
        self.forecast_head(&session.head, &session.from.worktree, branch, after)
    }

    /// 64, 65, 47a: the same for the revision of the branch `from`, the
    /// head a place on it would hold, read with no place of it: a bolt's
    /// against the branch it lands on. Its own submissions are those made
    /// from that branch (67).
    pub fn forecast_branch(
        &self,
        from: &str,
        branch: &str,
        after: &[u64],
    ) -> Result<Result<Forecasted, SubmitRefusal>> {
        let Some(head) = self.branch_read(from)? else {
            if self.held {
                return Err(crate::Unheld::Branch(from.into()).into());
            }
            return Ok(Err(SubmitRefusal::NoHead(from.into())));
        };
        self.forecast_head(&head, from, branch, after)
    }

    /// 64, 65, 47a: the forecast of `head` on the branch as a round reads
    /// it, or, where this sluice reads held, as the host holds it since the
    /// host process's last reading, with nothing fetched: what the host
    /// lacks of it is then [`crate::Unheld`] (129; design D16). `own` is
    /// the work the head is, as its driver names it: submissions of it are
    /// left out, as a submission of it would withdraw them (67).
    pub fn forecast_head(
        &self,
        head: &Rev,
        own: &str,
        branch: &str,
        after: &[u64],
    ) -> Result<Result<Forecasted, SubmitRefusal>> {
        let Some(tip) = self.branch_read(branch)? else {
            if self.held {
                return Err(crate::Unheld::Branch(branch.into()).into());
            }
            return Ok(Err(SubmitRefusal::NoBranch));
        };
        if self.rules_at(branch, &tip)?.is_none() {
            return Ok(Err(SubmitRefusal::NotManaged));
        }
        Ok(Ok(self.forecast_onto(head, own, branch, tip, after)?))
    }

    /// 64, 124, 129: the forecast of `head` as a submission of `own` to
    /// the branch `view` is of, formed on the queue a reader's read of it
    /// found, with nothing fetched. `None` where that read names no
    /// revision of the branch, or could not read it, or the host lacks
    /// the revision, the head, a queued submission's work or a
    /// combination formed on that revision: the forecast is then read as
    /// a round reads the branch.
    pub fn forecast_read(
        &self,
        view: &BranchView,
        head: &Rev,
        own: &str,
        after: &[u64],
    ) -> Result<Option<Result<Forecasted, SubmitRefusal>>> {
        if view.unread.is_some() {
            return Ok(None);
        }
        let Some(queue) = Queue::from_view(view) else {
            return Ok(None);
        };
        let branch = self.seams.branch;
        if !branch.holds(&queue.tip)? || !branch.holds(head)? || self.lacks(&queue)? {
            return Ok(None);
        }
        if self.rules_at(&view.branch, &queue.tip)?.is_none() {
            return Ok(Some(Err(SubmitRefusal::NotManaged)));
        }
        Ok(Some(Ok(self.forecast_on(&queue, head, own, after)?)))
    }

    /// Whether this host lacks work a forecast on `queue` may form from:
    /// a queued submission's head, or a combination formed on its tip.
    fn lacks(&self, queue: &Queue) -> Result<bool> {
        let branch = self.seams.branch;
        for s in queue.subs.values() {
            if !branch.holds(&s.head)? {
                return Ok(true);
            }
        }
        for c in queue.combinations.iter().filter(|c| c.on == queue.tip) {
            if !branch.holds(&c.revision)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The forecast on `tip`, the branch's revision as its caller just read
    /// it: `sluice exec`'s, which read it for its run. Work submitted from
    /// another host, or a combination one formed, that is not on this host
    /// is fetched first, as a round fetches it (124).
    pub(crate) fn forecast_onto(
        &self,
        head: &Rev,
        own: &str,
        branch: &str,
        tip: Rev,
        after: &[u64],
    ) -> Result<Forecasted> {
        let line = self.read_line(branch)?;
        let queue = Queue::from_line(&line, tip);
        if self.lacks(&queue)? {
            // 129 (design D16): a forecast read held fetches nothing.
            if self.held {
                return Err(crate::Unheld::Work(branch.into()).into());
            }
            self.seams.state.fetch_heads(branch)?;
            self.seams.state.fetch_candidates(branch)?;
        }
        self.forecast_on(&queue, head, own, after)
    }

    /// 64, 65, 47a: the forecast of `head`, submitted by `own`, on
    /// `queue`, read from the records or a reader's read of them.
    fn forecast_on(
        &self,
        queue: &Queue,
        head: &Rev,
        own: &str,
        after: &[u64],
    ) -> Result<Forecasted> {
        let tip = queue.tip.clone();
        // The queue as the next step would order it, with the head at the
        // back.
        const PROBE: u64 = u64::MAX;
        let mut entries = Vec::new();
        for (id, s) in &queue.subs {
            if s.state == State::Landing || s.from.worktree == own {
                continue;
            }
            entries.push(Entry {
                id: *id,
                priority: s.priority,
                after: s.after.clone(),
            });
        }
        entries.push(Entry {
            id: PROBE,
            priority: i64::MIN,
            after: Vec::new(),
        });
        let pinned: Vec<Vec<u64>> = queue
            .combinations
            .iter()
            .filter(|c| c.on == tip && c.state.pins())
            .map(|c| c.submissions.clone())
            .collect();
        let ord = order::order(&entries, &queue.landed, &pinned);
        // Its `after` entries are ahead of it, waiting or not.
        let mut ahead_ids: Vec<u64> = ord.chain.into_iter().filter(|i| *i != PROBE).collect();
        for a in after {
            if !ahead_ids.contains(a) && queue.subs.contains_key(a) {
                ahead_ids.push(*a);
            }
        }

        let mut prev = tip.clone();
        let mut prefix: Vec<u64> = Vec::new();
        let mut ahead: Vec<(u64, Rev)> = Vec::new();
        for id in ahead_ids {
            prefix.push(id);
            // A candidate a round formed on this revision with exactly these
            // submissions is the same merge (10): taken, not formed again.
            if let Some(c) = queue
                .combinations
                .iter()
                .find(|c| c.on == tip && c.submissions == prefix)
            {
                prev = c.revision.clone();
                ahead.push((id, prev.clone()));
                continue;
            }
            // An entry ahead that conflicts itself would leave the chain.
            match self.seams.branch.form(&prev, &queue.sub(id)?.head)? {
                Formed::Revision(r) => {
                    prev = r.clone();
                    ahead.push((id, r));
                }
                Formed::Conflict { .. } => {
                    prefix.pop();
                }
            }
        }
        let forecast = match self.seams.branch.form(&prev, head)? {
            Formed::Revision(_) => Forecast::Clear,
            Formed::Conflict { paths } => {
                let work = |id| queue.sub(id).map(|s| (s.head.clone(), s.from.clone()));
                Forecast::Conflict {
                    with: self.contested(&work, &tip, &ahead, head)?,
                    paths,
                }
            }
        };
        Ok(Forecasted {
            head: head.clone(),
            onto: tip,
            ahead: ahead.iter().map(|(id, _)| *id).collect(),
            forecast,
        })
    }

    /// 47b, 3, 6: change a queued submission's `after` or priority, until
    /// it is under test. A number more than one branch holds is refused,
    /// naming each, and nothing is changed (26; record 0114).
    pub fn amend(
        &self,
        id: u64,
        by: &Requester,
        after: Option<Vec<u64>>,
        priority: Option<i64>,
    ) -> Result<Result<(), AmendRefusal>> {
        self.amend_in(None, id, by, after, priority)
    }

    /// [`Sluice::amend`] of `branch`'s submission `id`, reading `branch`
    /// alone: a branch that holds no such number answers
    /// `NoSuchSubmission(vec![])`, and one that cannot be read names
    /// itself alone (26; record 0114).
    pub fn amend_on(
        &self,
        branch: &str,
        id: u64,
        by: &Requester,
        after: Option<Vec<u64>>,
        priority: Option<i64>,
    ) -> Result<Result<(), AmendRefusal>> {
        self.amend_in(Some(branch), id, by, after, priority)
    }

    /// [`Sluice::amend`], on `on` alone where it is named.
    fn amend_in(
        &self,
        on: Option<&str>,
        id: u64,
        by: &Requester,
        after: Option<Vec<u64>>,
        priority: Option<i64>,
    ) -> Result<Result<(), AmendRefusal>> {
        let branch = match self.find(id, on)? {
            Found::On(branch) => branch,
            Found::Unfound(unread) => return Ok(Err(AmendRefusal::NoSuchSubmission(unread))),
            Found::Several(several) => return Ok(Err(AmendRefusal::Several(several))),
        };
        // Finished, it is past changing: refused as it was before it moved.
        if let Some(s) = self.only_finished(&branch, id)? {
            if !by.may_change(&s) {
                return Ok(Err(AmendRefusal::NotYours));
            }
            return Ok(Err(AmendRefusal::NotQueued(s.state)));
        }
        let now = self.now();
        let _lock = self.seams.state.wait_lock(&branch)?;
        self.update(&branch, |l| {
            let s = l.sub(id)?.clone();
            if !by.may_change(&s) {
                return Ok(Err(AmendRefusal::NotYours));
            }
            if s.state.is_out() || s.state == State::Landing {
                return Ok(Err(AmendRefusal::NotQueued(s.state)));
            }
            if under_test(l, id) {
                return Ok(Err(AmendRefusal::UnderTest));
            }
            if let Some(a) = &after {
                let unknown: Vec<u64> = a
                    .iter()
                    .copied()
                    .filter(|x| !l.has(*x) || *x == id)
                    .collect();
                if !unknown.is_empty() {
                    return Ok(Err(AmendRefusal::UnknownAfter(unknown)));
                }
            }
            let s = l.sub_mut(id);
            if let Some(a) = &after {
                s.after = a.clone();
            }
            if let Some(p) = priority {
                s.priority = p;
            }
            let (after, priority) = (s.after.clone(), s.priority);
            l.log(
                &now,
                Event::Amended {
                    id,
                    after,
                    priority,
                },
            );
            Ok(Ok(()))
        })
    }

    /// 48, 5: withdraw a submission, by its submitter, its planner or the
    /// operator. A number more than one branch holds is refused, naming
    /// each, and nothing is withdrawn (26; record 0114).
    pub fn withdraw(&self, id: u64, by: &Requester) -> Result<Result<Withdrew, WithdrawRefusal>> {
        self.withdraw_in(None, id, by)
    }

    /// [`Sluice::withdraw`] of `branch`'s submission `id`, reading
    /// `branch` alone, as [`Sluice::amend_on`] reads it (26; record 0114).
    pub fn withdraw_on(
        &self,
        branch: &str,
        id: u64,
        by: &Requester,
    ) -> Result<Result<Withdrew, WithdrawRefusal>> {
        self.withdraw_in(Some(branch), id, by)
    }

    /// [`Sluice::withdraw`], on `on` alone where it is named.
    fn withdraw_in(
        &self,
        on: Option<&str>,
        id: u64,
        by: &Requester,
    ) -> Result<Result<Withdrew, WithdrawRefusal>> {
        let branch = match self.find(id, on)? {
            Found::On(branch) => branch,
            Found::Unfound(unread) => return Ok(Err(WithdrawRefusal::NoSuchSubmission(unread))),
            Found::Several(several) => return Ok(Err(WithdrawRefusal::Several(several))),
        };
        // Withdrawn or landed is final (5): answered from the records, with
        // no lock taken and nothing written. One withdrawn already was asked
        // for before, so asking again changes nothing.
        let line = self.read_line(&branch)?;
        let done = match line.subs.get(&id) {
            Some(s) => (s.state == State::Withdrawn).then(|| s.clone()),
            None if line.finished.contains_key(&id) => self.finished_sub(&branch, id)?,
            None => None,
        };
        if let Some(s) = done {
            if !by.may_change(&s) {
                return Ok(Err(WithdrawRefusal::NotYours));
            }
            if s.state == State::Withdrawn {
                return Ok(Ok(Withdrew::Already));
            }
            return Ok(Err(WithdrawRefusal::Final(s.state)));
        }
        let now = self.now();
        let _lock = self.seams.state.wait_lock(&branch)?;
        self.update(&branch, |l| {
            let s = l.sub(id)?.clone();
            if !by.may_change(&s) {
                return Ok(Err(WithdrawRefusal::NotYours));
            }
            // Withdrawn since the read above.
            if s.state == State::Withdrawn {
                return Ok(Ok(Withdrew::Already));
            }
            if s.state.is_final() || s.state == State::Landing {
                return Ok(Err(WithdrawRefusal::Final(s.state)));
            }
            let s = l.sub_mut(id);
            s.state = State::Withdrawn;
            s.withdrawn = Some(Withdrawal {
                by: by.word(),
                at: now.clone(),
                handed_over_to: None,
            });
            l.log(
                &now,
                Event::Withdrawn {
                    id,
                    by: by.word(),
                    handed_over_to: None,
                },
            );
            Ok(Ok(Withdrew::Now))
        })
    }

    /// 49, 26: a submission as last written, read with no step running,
    /// from the branches that read (rule core.11): none where no branch
    /// read holds the number, or where more than one does, since such a
    /// number names none of them alone (record 0114).
    pub fn state(&self, id: u64) -> Result<Option<Submission>> {
        self.state_in(None, id)
    }

    /// [`Sluice::state`] of `branch`'s submission `id`, reading `branch`
    /// alone, whatever other branch holds the number (26; record 0114).
    pub fn state_on(&self, branch: &str, id: u64) -> Result<Option<Submission>> {
        self.state_in(Some(branch), id)
    }

    /// [`Sluice::state`], on `on` alone where it is named.
    fn state_in(&self, on: Option<&str>, id: u64) -> Result<Option<Submission>> {
        let Found::On(branch) = self.find(id, on)? else {
            return Ok(None);
        };
        self.held_on(&branch, id)
    }

    /// Submission `id` as `branch`'s records hold it, live or finished.
    fn held_on(&self, branch: &str, id: u64) -> Result<Option<Submission>> {
        let mut line = self.read_line(branch)?;
        if let Some(s) = line.subs.remove(&id) {
            return Ok(Some(s));
        }
        self.finished_sub(branch, id)
    }

    /// 49, 26: submission `id` as `sluice wait <n>` and `sluice status
    /// <n>` look it up: as last written, or the submission it became when
    /// its branch's records moved to its remote (design D10). Every branch
    /// is read. A branch that cannot be read is passed by where one branch
    /// read holds the number; named, with the words it was refused with,
    /// where none does (rule core.11); and named beside the branches that
    /// hold it where more than one does, since such a number names none of
    /// them alone (record 0114).
    pub fn lookup(&self, id: u64) -> Result<Lookup> {
        self.lookup_in(None, id)
    }

    /// [`Sluice::lookup`] of `branch`'s submission `id`, reading `branch`
    /// alone: one that holds no such number, nor a submission it became,
    /// answers `Unfound(vec![])`, and one that cannot be read is named
    /// alone, so no other branch is ever named (26; record 0114).
    pub fn lookup_on(&self, branch: &str, id: u64) -> Result<Lookup> {
        self.lookup_in(Some(branch), id)
    }

    /// [`Sluice::lookup`], on `on` alone where it is named.
    fn lookup_in(&self, on: Option<&str>, id: u64) -> Result<Lookup> {
        let mut unread = match self.find(id, on)? {
            Found::On(branch) => match self.held_on(&branch, id)? {
                Some(s) => return Ok(Lookup::Found(s)),
                None => Vec::new(),
            },
            Found::Unfound(unread) => unread,
            Found::Several(several) => return Ok(Lookup::Several(several)),
        };
        let mut moved = self.moved(id, on, &mut unread)?;
        Ok(match moved.len() {
            0 => Lookup::Unfound(unread),
            1 => Lookup::Found(moved.remove(0)),
            _ => Lookup::Several(Several {
                branches: moved.into_iter().map(|s| s.branch).collect(),
                unread,
            }),
        })
    }

    /// 122: the commit submission `id`'s work is on its branch as, if it
    /// is: its record's `landed_as` once it landed; or, while the records
    /// hold the landing that lands it, its commit there once the branch is
    /// at the landing's revision or past it, as a round confirms a landing
    /// (24). None where the number is on no branch read, or on more than
    /// one, since it then names none of them alone (26; record 0114). Read
    /// with no step running; it writes nothing (rule core.4).
    pub fn landed_as(&self, id: u64) -> Result<Option<Rev>> {
        self.landed_as_in(None, id)
    }

    /// [`Sluice::landed_as`] of `branch`'s submission `id`, reading
    /// `branch` alone (26; record 0114).
    pub fn landed_as_on(&self, branch: &str, id: u64) -> Result<Option<Rev>> {
        self.landed_as_in(Some(branch), id)
    }

    /// [`Sluice::landed_as`], on `on` alone where it is named.
    fn landed_as_in(&self, on: Option<&str>, id: u64) -> Result<Option<Rev>> {
        let Found::On(branch) = self.find(id, on)? else {
            return Ok(None);
        };
        let line = self.read_line(&branch)?;
        let Some(sub) = line.subs.get(&id) else {
            let finished = self.finished_sub(&branch, id)?;
            return Ok(finished
                .filter(|s| s.state == State::Landed)
                .and_then(|s| s.landed_as));
        };
        if sub.state == State::Landed {
            return Ok(sub.landed_as.clone());
        }
        let Some(intent) = &line.doc.landing else {
            return Ok(None);
        };
        let Some(commit) = intent.commits.iter().find(|c| c.id == id) else {
            return Ok(None);
        };
        let moved = match self.seams.branch.read(&branch)? {
            Some(at) => at == intent.to || self.seams.branch.contains(&at, &intent.to)?,
            None => false,
        };
        Ok(moved.then(|| commit.commit.clone()))
    }

    /// Design D10: the submission that `old`, a submission of this host's
    /// own records, became when the branch's records moved to its remote,
    /// from the branches that read (26): none where it became one on more
    /// than one branch, since it then names none of them alone (record
    /// 0114).
    pub fn moved_to(&self, old: u64) -> Result<Option<Submission>> {
        let mut moved = self.moved(old, None, &mut Vec::new())?;
        Ok((moved.len() == 1).then(|| moved.remove(0)))
    }

    /// The submission `old` became on each branch, read from every branch,
    /// or from `on` alone where it is named, in the records' order, adding
    /// to `unread` each branch it passed by that is not there yet (26;
    /// record 0114).
    fn moved(
        &self,
        old: u64,
        on: Option<&str>,
        unread: &mut Vec<Unread>,
    ) -> Result<Vec<Submission>> {
        let branches = match on {
            Some(b) => vec![b.to_string()],
            None => self.seams.state.branches()?,
        };
        let mut moved = Vec::new();
        for b in branches {
            match self.moved_on(&b, old) {
                Ok(Some(s)) => moved.push(s),
                Ok(None) => {}
                Err(e) => {
                    if !unread.iter().any(|u| u.branch == b) {
                        unread.push(Unread::of(&b, &e));
                    }
                }
            }
        }
        Ok(moved)
    }

    /// The submission of `branch` that `old` became, if any.
    fn moved_on(&self, branch: &str, old: u64) -> Result<Option<Submission>> {
        let line = self.read_line(branch)?;
        if let Some(s) = line.subs.values().find(|s| s.was == Some(old)) {
            return Ok(Some(s.clone()));
        }
        for id in line.finished.keys() {
            if let Some(s) = self.finished_sub(branch, *id)? {
                if s.was == Some(old) {
                    return Ok(Some(s));
                }
            }
        }
        Ok(None)
    }

    /// Submission `id` of `branch` when it is among the finished records
    /// alone, which nothing changes again.
    fn only_finished(&self, branch: &str, id: u64) -> Result<Option<Submission>> {
        let line = self.read_line(branch)?;
        if line.subs.contains_key(&id) || !line.finished.contains_key(&id) {
            return Ok(None);
        }
        self.finished_sub(branch, id)
    }

    /// Clause 131: a submission moved to the finished records, read by its
    /// branch and number.
    pub(crate) fn finished_sub(&self, branch: &str, id: u64) -> Result<Option<Submission>> {
        match self
            .seams
            .state
            .read_finished(branch, &crate::records::finished_name(id))?
        {
            None => Ok(None),
            Some(bytes) => Ok(Some(serde_json::from_slice(&bytes).with_context(|| {
                format!("reading {branch}'s finished submission {id}")
            })?)),
        }
    }

    /// The latest submission from each of `worktrees` the branch has had,
    /// live or finished, from one read of its live records and one file
    /// each for those whose latest is finished.
    pub fn latest_of(
        &self,
        branch: &str,
        worktrees: &[String],
    ) -> Result<BTreeMap<String, Submission>> {
        let line = self.read_line(branch)?;
        let mut out = BTreeMap::new();
        for w in worktrees {
            let live = line.subs.values().rfind(|s| &s.from.worktree == w);
            let finished = line
                .finished
                .values()
                .filter(|f| &f.worktree == w)
                .map(|f| f.id)
                .max();
            let latest = match (live, finished) {
                (Some(s), Some(f)) if f > s.id => self.finished_sub(branch, f)?,
                (Some(s), _) => Some(s.clone()),
                (None, Some(f)) => self.finished_sub(branch, f)?,
                (None, None) => None,
            };
            if let Some(s) = latest {
                out.insert(w.clone(), s);
            }
        }
        Ok(out)
    }

    /// Every submission the branch has had, live and finished, in order:
    /// its history, for a reader that asks for it.
    pub fn history(&self, branch: &str) -> Result<Vec<Submission>> {
        let mut all = self.read_line(branch)?.subs;
        for (name, bytes) in self.seams.state.read_all_finished(branch)? {
            let s: Submission = serde_json::from_slice(&bytes)
                .with_context(|| format!("reading {branch}'s finished {name}"))?;
            all.entry(s.id).or_insert(s);
        }
        Ok(all.into_values().collect())
    }

    /// 1: whether sluice guards `branch` at its revision `at`: the
    /// policy read from that revision names it (56). An update to it by
    /// anything but sluice is refused.
    pub fn protects(&self, branch: &str, at: &Rev) -> Result<bool> {
        Ok(self.rules_at(branch, at)?.is_some())
    }

    /// The submissions that still need `branch`: in a chain, or landing.
    /// Its deletion is refused while there are any. 124: read from the
    /// records as the host holds them, with no fetch, since the protection
    /// that asks fetches nothing to judge a write (record 0110).
    pub fn needing(&self, branch: &str) -> Result<Vec<u64>> {
        Ok(Line::load(branch, self.seams.state.read_held(branch)?)?
            .subs
            .values()
            .filter(|s| !s.state.is_out())
            .map(|s| s.id)
            .collect())
    }

    /// 1, 124: report an update to `branch` that protection refused on
    /// this host. 148: logged once for each cause while it stands, its
    /// command with each full revision as `<revision>`, and counted in the
    /// branch's document after (rule core.47). The caller holds the
    /// branch's lock.
    pub fn record_protected(
        &self,
        branch: &str,
        from: &Rev,
        to: Option<&Rev>,
        by: &str,
    ) -> Result<()> {
        let now = self.now();
        let reference = format!("refs/heads/{branch}");
        self.update(branch, |l| {
            l.refuse_write(&now, &self.peer.name, &reference, by, from, to);
            Ok(())
        })
    }

    /// 129: whether `branch`'s records hold a submission not out, a
    /// combination under test, or a checkout to release, the question
    /// `sluice run` asks of each branch on every pass, read from the records
    /// alone: no revision of the branch and no policy, so a branch with
    /// nothing in play costs its pass the records' read. A run goes on
    /// after its queue has emptied, and is read, or dropped as obsolete, at
    /// the driver's next pass (66; design D4).
    pub fn in_play(&self, branch: &str) -> Result<bool> {
        let line = Line::load(branch, self.seams.state.read_held(branch)?)?;
        Ok(line.subs.values().any(|s| !s.state.is_out())
            || line.doc.candidates.iter().any(|c| c.state.under_test())
            || !line.doc.release.is_empty())
    }

    /// 26: a branch's queue as last written, at the revision its records
    /// last saw, as the overview reads it (ADR 0046): a remote branch is
    /// not fetched for a read. Only a branch whose records have seen no
    /// revision yet is read from the branch itself.
    ///
    /// 56, 124, 129: its policy at the branch's revision as this host holds
    /// it, so a policy mended outside sluice is read whole with no round
    /// run, and one refused there names that revision; at the revision its
    /// records last saw only where the host holds none (ADR 0046 as
    /// amended 2026-11-06; rule core.15). Nothing is fetched for it and
    /// nothing written.
    ///
    /// 105: a branch removed is read from its records as any other, and
    /// said to be gone: by that read where it was made, and otherwise by
    /// the revision the host holds of it, with no fetch (record 0091).
    pub fn status(&self, branch: &str) -> Result<Status> {
        let snapshot = self.state_read(branch)?;
        let found = snapshot.version > 0 || !snapshot.files.is_empty();
        let kept_out = kept_out(&Line::read_log(&snapshot)?);
        let line = Line::load(branch, snapshot)?;
        // The revision this host holds of the branch, read with no fetch,
        // once the branch is read where its records have seen no revision:
        // what says it gone and what its policy is read at.
        let (at, held, gone) = match &line.doc.base {
            Some(base) => {
                let held = self.seams.branch.held(branch)?;
                let gone = held.is_none();
                (Some(base.clone()), held, gone)
            }
            None => {
                let read = self.branch_read(branch)?;
                let gone = read.is_none();
                (read, self.seams.branch.held(branch)?, gone)
            }
        };
        let recorded = found || (gone && self.seams.state.branches()?.iter().any(|b| b == branch));
        let policy = match held.or_else(|| at.clone()) {
            Some(rev) => self.policy_at(&rev)?,
            None => None,
        };
        let parallel = match &policy {
            Some(p) => p.rules_for(branch)?.map(|r| r.parallel),
            None => None,
        };
        let mut warnings = policy.map(|p| p.warnings).unwrap_or_default();
        warnings.extend(kept_out.iter().map(|t| kept_out_warning(branch, t)));
        let refused = self
            .refusal(branch)?
            .filter(|r| r.ended.is_none())
            .and_then(|r| {
                let latest = r.latest()?.clone();
                Some(Refused {
                    remote: r.remote,
                    asked: latest.asked,
                    said: latest.said,
                    since: r.since,
                    rounds: r.rounds,
                })
            });
        let intake_refused = self
            .intake_refusal(branch)?
            .filter(|r| r.ended.is_none())
            .and_then(|r| {
                let latest = r.latest()?.clone();
                Some(Refused {
                    remote: r.remote,
                    asked: latest.asked,
                    said: latest.said,
                    since: r.since,
                    rounds: r.rounds,
                })
            });
        let refused_writes =
            crate::refused_writes::standing(&line.doc.refused_writes, || self.now());
        let on_remote =
            !refused_writes.is_empty() && self.seams.branch.copy(branch)? != HostCopy::Itself;
        let chain = line.order(at.as_ref(), true).chain;
        Ok(Status {
            branch: branch.into(),
            at,
            gone,
            recorded,
            testing: line
                .doc
                .candidates
                .iter()
                .filter(|c| c.state.under_test())
                .count(),
            window: line.doc.window,
            parallel,
            held: line.doc.held.clone(),
            copy: line.doc.copies.get(&self.peer.name).cloned(),
            finished: line.finished.len(),
            named: line
                .finished
                .values()
                .map(|f| (f.id, f.name().to_string()))
                .collect(),
            failed_releases: line.doc.failed_releases.clone(),
            refused_writes,
            on_remote,
            chain,
            submissions: line.subs.into_values().collect(),
            warnings,
            stale: self.seams.state.stale(branch).map(|r| r.remote),
            refused,
            intake_refused,
        })
    }
}

/// A branch whose records could not be read, in the reader's words,
/// whole (26, 133): a lookup passes it by, and a collection reads the
/// other branches still.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unread {
    pub branch: String,
    pub error: String,
}

/// A number more than one branch holds: it names none of them alone, so a
/// lookup by it is refused naming each (26; record 0114).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Several {
    /// Each branch that holds the number, in the records' order.
    pub branches: Vec<String>,
    /// Each branch that could not be read, with its words.
    pub unread: Vec<Unread>,
}

impl Unread {
    /// `branch`, refused with `e`, worded as the overview words a branch
    /// it could not read.
    pub fn of(branch: &str, e: &anyhow::Error) -> Unread {
        Unread {
            branch: branch.into(),
            error: format!("{e:#}"),
        }
    }
}

/// ADR 0044: the required checks the latest run of each system said its
/// task runner keeps out of CI, so never runs.
pub fn kept_out(log: &[crate::records::LogLine]) -> Vec<String> {
    let mut seen: BTreeMap<Option<String>, Vec<String>> = BTreeMap::new();
    for l in log {
        if let Event::Detail {
            system, kept_out, ..
        } = &l.event
        {
            seen.insert(system.clone(), kept_out.clone());
        }
    }
    let mut all: Vec<String> = seen.into_values().flatten().collect();
    all.sort();
    all.dedup();
    all
}

/// What `sluice status` and a waiting `submit` say of a check `kept_out`
/// names.
pub fn kept_out_warning(branch: &str, task: &str) -> String {
    format!(
        "{branch} requires {task}, which its task runner keeps out of CI, so it is never run: \
         {branch} cannot pass while it is required"
    )
}

/// 47, 67: withdraw the earlier submissions from `session`'s place and
/// queue the head it submits, `submitted` (124), as submission `id`, in
/// `l`, keeping its `subject` (23).
#[allow(clippy::too_many_arguments)]
fn admit(
    l: &mut Line,
    req: &GroupRequest,
    id: u64,
    act: Option<u64>,
    session: &Session,
    submitted: &SubmittedHead,
    subject: Option<String>,
    now: &str,
) -> Accepted {
    let mine: Vec<u64> = l
        .subs
        .values()
        .filter(|s| s.from.worktree == session.from.worktree)
        .map(|s| s.id)
        .collect();
    let attempt = l.from_worktree(&session.from.worktree) as u32 + 1;
    let mut withdrew = Vec::new();
    let mut acted_on = Vec::new();
    for &old in &mine {
        let s = l.sub_mut(old);
        if !s.state.is_final() && s.state != State::Landing {
            // 67, D16: a blame or a conflict submitted again is its reader
            // acting on it, which confirms it (weir 102).
            if matches!(s.state, State::Failed | State::Conflict) {
                acted_on.extend(
                    s.notification()
                        .map(|key| (old, key, s.from.worktree.clone())),
                );
            }
            s.state = State::Withdrawn;
            s.withdrawn = Some(Withdrawal {
                by: "resubmission".into(),
                at: now.into(),
                handed_over_to: None,
            });
            withdrew.push(old);
        }
    }
    for &old in &withdrew {
        l.log(
            now,
            Event::Withdrawn {
                id: old,
                by: "resubmission".into(),
                handed_over_to: None,
            },
        );
    }
    for (old, key, reader) in acted_on {
        l.confirm_resubmitted(old, &key, &reader, now);
    }
    l.subs.insert(
        id,
        Submission {
            schema: SUBMISSION_SCHEMA.into(),
            id,
            branch: req.branch.clone(),
            head: submitted.head.clone(),
            from: session.from.clone(),
            submitted: now.into(),
            after: req.after.clone(),
            priority: req.priority,
            attempt,
            state: State::Queued,
            tested: None,
            result: None,
            failed: Vec::new(),
            conflict: None,
            withdrawn: None,
            landed_as: None,
            landed_elsewhere: false,
            was: None,
            held: None,
            subject,
            beside: None,
            covered_by: None,
            act,
        },
    );
    l.log(
        now,
        Event::Submitted {
            id,
            head: submitted.head.clone(),
        },
    );
    // 6, 7: ahead of it in the order a round takes the queue, as status
    // reads it.
    let chain = l.order(l.doc.base.as_ref(), true).chain;
    let ahead = order::ahead_of(id, &chain, |i| l.subs.get(&i));
    Accepted {
        id,
        ahead,
        withdrew,
        warnings: Vec::new(),
        copy: submitted.copy.clone(),
    }
}

/// In a candidate under test or judged on the branch's revision (6).
fn under_test(line: &Line, id: u64) -> bool {
    line.doc.candidates.iter().any(|c| {
        Some(&c.on) == line.doc.base.as_ref() && c.state.pins() && c.submissions.contains(&id)
    })
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;

    use sluice_seams::Decided;

    use chrono::{DateTime, TimeZone, Utc};
    use sluice_seams::{
        Adopted, Branch, Changes, CheckedOut, Checkout, Delivery, Driver, GoneRelease, Guard,
        HostRun, LandingCommit, LastResult, Moved, Notification, Place, Platform, PreviewPlace,
        Progress, Proof, ReleasePolicy, Released, RunHandle, RunReport, RunRequest, Running,
        SessionPlace, Settings, Snapshot, State as Records, Submitter, TaskLog, TaskProgress,
        Tests, Written, FINISHED,
    };

    use super::*;
    use crate::policy::POLICY_PATH;
    use crate::{Clock, Seams};

    const BRANCH: &str = "main";

    /// A host with one branch under a policy, its places, and its
    /// records, holding what a submit reads and writes and nothing more.
    #[derive(Default)]
    struct Host {
        places: RefCell<BTreeMap<String, Session>>,
        records: RefCell<Snapshot>,
        writes: Cell<usize>,
        next_id: Cell<u64>,
        /// The policy every revision holds.
        policy: RefCell<String>,
        /// Branches with records that cannot be read.
        unreadable: RefCell<Vec<String>>,
        /// The results log as each branch's gate reads it, each a branch
        /// with records.
        results: RefCell<BTreeMap<String, Vec<u8>>>,
        /// The results set aside.
        asides: RefCell<Vec<u8>>,
    }

    impl Host {
        fn new() -> Host {
            let h = Host::default();
            h.next_id.set(1);
            *h.policy.borrow_mut() = "[branches.main]\n".into();
            h
        }

        /// A clean place `worktree` holding a head of its own.
        fn place(&self, worktree: &str) {
            self.places.borrow_mut().insert(
                worktree.into(),
                Session {
                    head: Rev::new(format!("head-of-{worktree}")),
                    from: Submitter {
                        agent: format!("agent-{worktree}"),
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

        fn dirty(&self, worktree: &str, path: &str) {
            let mut places = self.places.borrow_mut();
            places.get_mut(worktree).unwrap().dirty = vec![path.into()];
        }

        fn sluice(&self) -> Sluice<'_> {
            let seams = Seams {
                checkout: self,
                tests: self,
                branch: self,
                driver: self,
                state: self,
            };
            Sluice::new(seams, &Noon)
        }

        fn group(&self, worktrees: &[&str], after: &[u64]) -> GroupRequest {
            GroupRequest {
                branch: BRANCH.into(),
                heads: worktrees
                    .iter()
                    .map(|w| SubmitHead::Worktree(w.to_string()))
                    .collect(),
                after: after.to_vec(),
                priority: 0,
                subjects: Vec::new(),
            }
        }
    }

    struct Noon;

    impl Clock for Noon {
        fn now(&self) -> DateTime<Utc> {
            Utc.with_ymd_and_hms(2026, 10, 23, 12, 0, 0).unwrap()
        }
    }

    struct Held;

    impl Guard for Held {}

    impl Driver for Host {
        fn session(&self, worktree: &str) -> Result<Session> {
            self.places
                .borrow()
                .get(worktree)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("no place {worktree}"))
        }

        fn named(&self, branch: &str) -> Result<Submitter> {
            Ok(Submitter {
                agent: branch.into(),
                worktree: branch.into(),
                path: None,
                head: Some(branch.into()),
                host: None,
                session: None,
            })
        }

        fn notify(&self, _: &Notification) -> Result<Delivery> {
            unreachable!("a submit tells no one")
        }
    }

    impl Branch for Host {
        fn read(&self, branch: &str) -> Result<Option<Rev>> {
            Ok((branch == BRANCH).then(|| Rev::new("tip")))
        }

        fn held(&self, branch: &str) -> Result<Option<Rev>> {
            Branch::read(self, branch)
        }

        fn copy(&self, _: &str) -> Result<HostCopy> {
            Ok(HostCopy::Itself)
        }

        fn level(&self, _: &str, _: Option<&Rev>, _: &Rev) -> Result<Moved> {
            unreachable!("a submit levels no copy")
        }

        fn read_file(&self, _: &Rev, path: &str) -> Result<Option<Vec<u8>>> {
            Ok((path == POLICY_PATH).then(|| self.policy.borrow().clone().into_bytes()))
        }

        fn form(&self, _: &Rev, _: &Rev) -> Result<Formed> {
            unreachable!("a submit forms nothing")
        }

        fn commit(&self, _: &LandingCommit) -> Result<Rev> {
            unreachable!("a submit commits nothing")
        }

        fn contains(&self, _: &Rev, _: &Rev) -> Result<bool> {
            unreachable!("a submit reads no history")
        }

        fn move_to(&self, _: &str, _: &Rev, _: &Rev) -> Result<Moved> {
            unreachable!("a submit moves no branch")
        }

        fn make(&self, _: &str, _: &Rev) -> Result<Moved> {
            unreachable!("a submit makes no branch")
        }

        fn remove(&self, _: &str, _: &Rev) -> Result<Moved> {
            unreachable!("a submit removes no branch")
        }
    }

    impl Records for Host {
        fn lock<'a>(&'a self, _: &str) -> Result<Option<Box<dyn Guard + 'a>>> {
            Ok(Some(Box::new(Held)))
        }

        fn wait_lock<'a>(&'a self, _: &str) -> Result<Box<dyn Guard + 'a>> {
            Ok(Box::new(Held))
        }

        fn refresh(&self) -> Result<()> {
            Ok(())
        }

        fn read_held(&self, branch: &str) -> Result<Snapshot> {
            Records::read(self, branch)
        }

        fn read(&self, branch: &str) -> Result<Snapshot> {
            if self.unreadable.borrow().iter().any(|b| b == branch) {
                anyhow::bail!("remote: Repository not found.");
            }
            let records = self.records.borrow();
            Ok(Snapshot {
                version: records.version,
                files: records
                    .files
                    .iter()
                    .filter(|(path, _)| !path.starts_with(FINISHED))
                    .map(|(path, bytes)| (path.clone(), bytes.clone()))
                    .collect(),
            })
        }

        fn read_finished(&self, branch: &str, name: &str) -> Result<Option<Vec<u8>>> {
            Ok(self.read_all_finished(branch)?.remove(name))
        }

        fn read_all_finished(&self, _: &str) -> Result<BTreeMap<String, Vec<u8>>> {
            let prefix = format!("{FINISHED}/");
            Ok(self
                .records
                .borrow()
                .files
                .iter()
                .filter_map(|(path, bytes)| {
                    Some((path.strip_prefix(&prefix)?.to_string(), bytes.clone()))
                })
                .collect())
        }

        fn write(&self, _: &str, read_at: u64, changes: Changes) -> Result<Written> {
            let mut records = self.records.borrow_mut();
            if records.version != read_at {
                return Ok(Written::Refused);
            }
            records.files.extend(changes.put);
            for (path, bytes) in changes.append {
                records.files.entry(path).or_default().extend(bytes);
            }
            records
                .files
                .retain(|path, _| !changes.remove.contains(path));
            records.version += 1;
            self.writes.set(self.writes.get() + 1);
            Ok(Written::Written(records.version))
        }

        fn branches(&self) -> Result<Vec<String>> {
            let mut all = self.unreadable.borrow().clone();
            all.push(BRANCH.into());
            for b in self.results.borrow().keys() {
                if !all.contains(b) {
                    all.push(b.clone());
                }
            }
            Ok(all)
        }

        fn allocate_id(&self, _branch: &str) -> Result<u64> {
            let id = self.next_id.get();
            self.next_id.set(id + 1);
            Ok(id)
        }

        fn results_for(&self, branch: &str, ask: &sluice_seams::ResultAsk) -> Result<Vec<u8>> {
            Ok(ask.answer(
                &self
                    .results
                    .borrow()
                    .get(branch)
                    .cloned()
                    .unwrap_or_default(),
            ))
        }

        fn record_results(&self, _: &[u8]) -> Result<()> {
            unreachable!("a submit records no result")
        }

        fn set_asides(&self) -> Result<Vec<u8>> {
            Ok(self.asides.borrow().clone())
        }

        fn set_aside(&self, line: &[u8]) -> Result<()> {
            self.asides.borrow_mut().extend(line);
            Ok(())
        }

        fn keep_logs(&self, _: &str, _: &str, _: &[TaskLog]) -> Result<Vec<TaskLog>> {
            unreachable!("a submit keeps no log")
        }

        fn progress(&self, _: &str) -> Result<BTreeMap<String, Vec<TaskProgress>>> {
            Ok(BTreeMap::new())
        }

        fn put_progress(&self, _: &str, _: &str, _: Option<&[TaskProgress]>) -> Result<()> {
            unreachable!("a submit keeps no progress")
        }
    }

    impl Checkout for Host {
        fn materialize(&self, _: &str, _: &Rev, _: &Settings) -> Result<Place> {
            unreachable!("a submit makes no place")
        }

        fn release(&self, _: &str) -> Result<()> {
            unreachable!("a submit releases no place")
        }

        fn gone(&self, _: &str) -> Result<bool> {
            unreachable!("a submit reads no place")
        }

        fn free(&self, _: &str) -> Result<()> {
            unreachable!("a submit releases no place")
        }

        fn release_gone(&self, _: &str) -> Result<GoneRelease> {
            unreachable!("a submit releases no place")
        }

        fn remove_unnamed_scratch(&self) -> Result<Vec<std::path::PathBuf>> {
            Ok(Vec::new())
        }

        fn made(&self) -> Result<Vec<String>> {
            unreachable!("a submit reads no place")
        }

        fn adopt(&self, _: &str, _: &str, _: &Settings, _: &str) -> Result<Place> {
            unreachable!("a submit adopts no place")
        }

        fn adopted(&self, _: &str) -> Result<Option<Adopted>> {
            unreachable!("a submit reads no place")
        }

        fn release_session(&self, _: &str, _: Option<&ReleasePolicy>) -> Result<Released> {
            unreachable!("a submit releases no place")
        }

        fn session_places(&self) -> Result<Vec<SessionPlace>> {
            unreachable!("a submit reads no place")
        }

        fn ran(&self, _: &str, _: &LastResult) -> Result<()> {
            unreachable!("a submit runs nothing")
        }

        fn checked_out(&self) -> Result<Vec<CheckedOut>> {
            unreachable!("a submit reads no worktree")
        }

        fn preview(
            &self,
            _: &str,
            _: &Rev,
            _: &str,
            _: &Settings,
            _: &BTreeMap<String, String>,
            _: &str,
        ) -> Result<(Place, PreviewPlace)> {
            unreachable!("a submit previews nothing")
        }

        fn preview_run(&self, _: &str, _: &RunHandle) -> Result<()> {
            unreachable!("a submit previews nothing")
        }

        fn previews(&self) -> Result<Vec<PreviewPlace>> {
            unreachable!("a submit previews nothing")
        }

        fn release_preview(&self, _: &str, _: Option<&str>, _: u32) -> Result<bool> {
            unreachable!("a submit previews nothing")
        }
    }

    impl Tests for Host {
        fn start(&self, _: &RunRequest, _: sluice_seams::Part) -> Result<RunHandle> {
            unreachable!("a submit runs nothing")
        }

        fn hold(&self, _: sluice_seams::RunKind) -> Result<Option<sluice_seams::Part>> {
            unreachable!("a submit runs nothing")
        }

        fn progress(&self, _: &RunHandle) -> Result<Progress> {
            unreachable!("a submit runs nothing")
        }

        fn unmet(&self, _: &sluice_seams::Limits) -> Result<Option<String>> {
            unreachable!("a submit runs nothing")
        }

        fn switched(&self, _: &RunHandle) -> Result<Option<String>> {
            Ok(None)
        }

        fn refused(&self, _: &RunHandle) -> Result<sluice_seams::Refusals> {
            Ok(sluice_seams::Refusals::None)
        }

        fn bound(&self) -> Result<usize> {
            unreachable!("a submit runs nothing")
        }

        fn platform(&self) -> Result<Platform> {
            unreachable!("a submit runs nothing")
        }

        fn running(&self) -> Result<Running> {
            unreachable!("a submit runs nothing")
        }

        fn proven(&self, _: &RunHandle, _: &RunReport) -> Result<Proof> {
            unreachable!("a submit runs nothing")
        }

        fn session_runs(&self) -> Result<Vec<HostRun>> {
            unreachable!("a submit runs nothing")
        }

        fn logs(&self, _: &RunHandle, _: &[String]) -> Result<Vec<TaskLog>> {
            unreachable!("a submit runs nothing")
        }

        fn tasks(&self, _: &RunHandle) -> Result<Vec<TaskProgress>> {
            unreachable!("a submit runs nothing")
        }
    }

    fn line(h: &Host) -> Line {
        Line::load(BRANCH, Records::read(h, BRANCH).unwrap()).unwrap()
    }

    /// Submission `id` of `main`, from `worktree`, in `state`, as decided:
    /// a landing with its commit, a blame with its combination.
    fn decided(id: u64, worktree: &str, state: State) -> Submission {
        let mut s: Submission = serde_json::from_value(serde_json::json!({
            "schema": SUBMISSION_SCHEMA, "id": id, "branch": BRANCH, "head": format!("h{id}"),
            "from": { "agent": "a", "worktree": worktree },
            "submitted": "t", "after": [], "priority": 0, "attempt": 1,
            "state": state.word(),
        }))
        .unwrap();
        match state {
            State::Landed => s.landed_as = Some(Rev::new(format!("landed-{id}"))),
            State::Failed => {
                s.tested = Some(crate::records::Tested {
                    combination: Rev::new(format!("c{id}")),
                    on: Rev::new("tip"),
                    submissions: vec![id],
                });
                s.result = Some("failed".into());
            }
            _ => {}
        }
        s
    }

    fn keep(h: &Host, s: &Submission) {
        h.records.borrow_mut().files.insert(
            crate::records::submission_file(s.id),
            crate::records::to_document(s),
        );
    }

    fn me(worktree: &str) -> Submitter {
        Submitter {
            agent: "a".into(),
            worktree: worktree.into(),
            path: None,
            head: None,
            host: None,
            session: None,
        }
    }

    /// The delivery of submission `id`'s outcome under `key`, owed to
    /// `reader`, among the records' deliveries, as a round records it.
    fn owe(h: &Host, key: &str, reader: &str) {
        let mut records = h.records.borrow_mut();
        let file = records
            .files
            .entry(crate::records::DELIVERIES_FILE.into())
            .or_default();
        let held = weir_delivery::json::read_lines(std::str::from_utf8(file).unwrap()).unwrap();
        let weir_delivery::Recorded::Add(add) = weir_delivery::record(
            &held,
            key,
            "[sluice] an outcome.",
            &[reader],
            "2026-10-23T11:00:00Z",
            168 * 3600,
        )
        .unwrap() else {
            panic!("{key} recorded twice");
        };
        file.extend(weir_delivery::json::write_lines(&add).into_bytes());
    }

    /// `s` kept among the live records, and, decided, its outcome owed to
    /// its work.
    fn keep_owed(h: &Host, s: &Submission) {
        keep(h, s);
        if let Some(key) = s.notification() {
            owe(h, &key, &s.from.worktree);
        }
    }

    fn found(h: &Host, worktree: &str) -> Vec<u64> {
        let s = h.sluice();
        let owed = s.owed(&me(worktree), crate::owed::Collect::All).unwrap();
        owed.outcomes.iter().map(|o| o.submission.id).collect()
    }

    fn found_one(h: &Host, id: u64, worktree: &str) -> Vec<u64> {
        let s = h.sluice();
        let one = crate::owed::Collect::One {
            branch: BRANCH.into(),
            id,
        };
        let owed = s.owed(&me(worktree), one).unwrap();
        owed.outcomes.iter().map(|o| o.submission.id).collect()
    }

    /// 51: a folder collects its own work's outcomes, by its branch or the
    /// branch its pull request was opened from, and not another's.
    #[test]
    fn another_worktrees_outcome_is_not_found() {
        let h = Host::new();
        keep_owed(&h, &decided(1, "unit-b", State::Failed));
        keep_owed(&h, &decided(2, "unit-a", State::Landed));
        let mut pull = decided(3, "pull/7", State::Failed);
        pull.from.head = Some("unit-a".into());
        keep_owed(&h, &pull);
        assert_eq!(found(&h, "unit-a"), [2, 3]);
        assert_eq!(found(&h, "unit-b"), [1]);
        assert!(found(&h, "unit-c").is_empty());
    }

    /// 51: a submission withdrawn, still queued, or whose reader confirmed
    /// its outcome is owed nothing.
    #[test]
    fn a_withdrawn_queued_or_confirmed_submission_is_not_found() {
        let h = Host::new();
        let mut withdrawn = decided(1, "unit-a", State::Failed);
        keep_owed(&h, &withdrawn);
        withdrawn.state = State::Withdrawn;
        keep(&h, &withdrawn);
        keep(&h, &decided(2, "unit-a", State::Queued));
        keep(&h, &decided(3, "unit-a", State::Testing));
        keep_owed(&h, &decided(4, "unit-a", State::Failed));
        {
            let mut records = h.records.borrow_mut();
            let file = records
                .files
                .get_mut(crate::records::DELIVERIES_FILE)
                .unwrap();
            let held = weir_delivery::json::read_lines(std::str::from_utf8(file).unwrap()).unwrap();
            let weir_delivery::Confirmed::Add(add) =
                weir_delivery::confirm(&held, "4:failed", "unit-a", "2026-10-23T11:30:00Z")
                    .unwrap()
            else {
                panic!("confirmed before");
            };
            file.extend(weir_delivery::json::write_lines(&add).into_bytes());
        }
        assert!(found(&h, "unit-a").is_empty());
    }

    /// 51, 130, 131: a landing moved to the finished records and owed is
    /// found, and handed over in the branch's deliveries alone, its record
    /// and the index left as they were; it stays owed.
    #[test]
    fn a_finished_owed_landing_is_found_and_handed_over() {
        let h = Host::new();
        let landed = decided(5, "unit-a", State::Landed);
        let index = crate::records::to_line(&crate::records::FinishedLine {
            id: 5,
            state: State::Landed,
            worktree: "unit-a".into(),
            subject: None,
        });
        {
            let mut records = h.records.borrow_mut();
            records.files.insert(
                crate::records::finished_file(5),
                crate::records::to_document(&landed),
            );
            records
                .files
                .insert(crate::records::FINISHED_INDEX.into(), index.clone());
        }
        owe(&h, "5:landed", "unit-a");
        let s = h.sluice();
        let owed = s.owed(&me("unit-a"), crate::owed::Collect::All).unwrap();
        assert_eq!(owed.outcomes.len(), 1);
        assert_eq!(owed.outcomes[0].key, "5:landed");
        assert_eq!(owed.outcomes[0].reader, "unit-a");
        assert_eq!(
            owed.outcomes[0].decided,
            Decided::Landed {
                landed_as: Rev::new("landed-5"),
                beside: false,
                covered_by: None,
            }
        );
        owed.hand("inbox", "/w/unit-a", false).unwrap();
        assert_eq!(s.state(5).unwrap().unwrap(), landed, "its record as it was");
        assert_eq!(
            h.records.borrow().files[crate::records::FINISHED_INDEX],
            index
        );
        let log = String::from_utf8(h.records.borrow().files["log.jsonl"].clone()).unwrap();
        assert!(
            log.contains(
                "\"event\":\"handed\",\"id\":5,\"key\":\"5:landed\",\"by\":\"inbox\",\
                 \"to\":\"/w/unit-a\""
            ),
            "{log}"
        );
        assert_eq!(found(&h, "unit-a"), [5], "owed until it is confirmed");
    }

    /// 51, record 0102 (D12): an outcome decided before deliveries were
    /// recorded is owed by nothing, whatever an earlier sluice marked on
    /// its record.
    #[test]
    fn an_outcome_decided_before_deliveries_is_owed_by_nothing() {
        let h = Host::new();
        keep(&h, &decided(2, "unit-a", State::Landed));
        assert!(found(&h, "unit-a").is_empty());
        assert!(found_one(&h, 2, "unit-a").is_empty());
    }

    /// 51, 67: a submission blamed and then withdrawn, as a resubmission
    /// withdraws it, keeps its blame and the outbox item the blame left, and
    /// is owed nothing, live or finished.
    #[test]
    fn a_withdrawn_submissions_earlier_blame_is_never_owed() {
        let h = Host::new();
        let mut blamed = decided(4, "unit-a", State::Failed);
        owe(&h, "4:failed", "unit-a");
        blamed.state = State::Withdrawn;
        blamed.withdrawn = Some(crate::records::Withdrawal {
            by: "resubmission".into(),
            at: "2026-10-29T02:40:56Z".into(),
            handed_over_to: None,
        });
        keep(&h, &blamed);
        let mut doc = crate::records::BranchDoc::new(BRANCH);
        doc.outbox.push(crate::records::OutboxItem {
            id: "4:failed".into(),
            submission: 4,
            outcome: crate::records::DecidedRecord::Failed {
                failed: vec!["cli:test".into()],
                combination: Rev::new("c4"),
                submissions: vec![4],
                timed_out: false,
                logs: Vec::new(),
            },
        });
        let mut finished = decided(3, "unit-a", State::Failed);
        owe(&h, "3:failed", "unit-a");
        finished.state = State::Withdrawn;
        let index = crate::records::to_line(&crate::records::FinishedLine {
            id: 3,
            state: State::Withdrawn,
            worktree: "unit-a".into(),
            subject: None,
        });
        {
            let mut records = h.records.borrow_mut();
            records.files.insert(
                crate::records::BRANCH_FILE.into(),
                crate::records::to_document(&doc),
            );
            records.files.insert(
                crate::records::finished_file(3),
                crate::records::to_document(&finished),
            );
            records
                .files
                .insert(crate::records::FINISHED_INDEX.into(), index);
        }
        assert!(found(&h, "unit-a").is_empty());
        assert!(found_one(&h, 4, "unit-a").is_empty());
        assert!(found_one(&h, 3, "unit-a").is_empty());
    }

    /// 51: an outcome whose outbox item stands is read as the item has it,
    /// and handing it over leaves the item for the round, which pushes
    /// nothing handed over (design D10).
    #[test]
    fn an_outcome_queued_is_read_as_its_item_and_handed_over_by_its_key() {
        let h = Host::new();
        let failed = decided(4, "unit-a", State::Failed);
        keep_owed(&h, &failed);
        let mut doc = crate::records::BranchDoc::new(BRANCH);
        doc.outbox.push(crate::records::OutboxItem {
            id: "4:failed".into(),
            submission: 4,
            outcome: crate::records::DecidedRecord::Failed {
                failed: vec!["cli:test".into()],
                combination: Rev::new("c4"),
                submissions: vec![4],
                timed_out: false,
                logs: Vec::new(),
            },
        });
        h.records.borrow_mut().files.insert(
            crate::records::BRANCH_FILE.into(),
            crate::records::to_document(&doc),
        );
        let s = h.sluice();
        let owed = s.owed(&me("unit-a"), crate::owed::Collect::All).unwrap();
        assert_eq!(owed.outcomes[0].key, "4:failed");
        assert!(matches!(
            &owed.outcomes[0].decided,
            Decided::Failed { failed, .. } if failed == &["cli:test"]
        ));
        owed.hand("inbox", "herdr:w1:p2@/run/herdr.sock", false)
            .unwrap();
        assert_eq!(line(&h).doc.outbox.len(), 1, "the round's to let go");
        let log = String::from_utf8(h.records.borrow().files["log.jsonl"].clone()).unwrap();
        assert!(log.contains("\"key\":\"4:failed\""), "{log}");
        assert!(
            log.contains("\"to\":\"herdr:w1:p2@/run/herdr.sock\""),
            "{log}"
        );
    }

    /// 51: a collect dropped before it hands over writes nothing, so it is
    /// owed with no hand-over by it.
    #[test]
    fn a_collect_dropped_without_handing_over_writes_nothing() {
        let h = Host::new();
        keep_owed(&h, &decided(2, "unit-a", State::Landed));
        let before = h.writes.get();
        {
            let s = h.sluice();
            let owed = s.owed(&me("unit-a"), crate::owed::Collect::All).unwrap();
            assert_eq!(owed.outcomes.len(), 1);
            assert!(owed.outcomes[0].owed.handovers.is_empty());
        }
        assert_eq!(h.writes.get(), before);
        assert_eq!(found(&h, "unit-a"), [2]);
    }

    /// 51: a wait reporting its own worktree's owed outcome finds that
    /// submission alone, by its branch or the branch its pull request was
    /// opened from, and a landing moved to the finished records too.
    #[test]
    fn the_narrowed_collect_finds_the_askers_owed_submission() {
        let h = Host::new();
        keep_owed(&h, &decided(1, "unit-a", State::Failed));
        keep_owed(&h, &decided(2, "unit-a", State::Landed));
        let mut pull = decided(3, "pull/7", State::Failed);
        pull.from.head = Some("unit-a".into());
        keep_owed(&h, &pull);
        assert_eq!(found_one(&h, 2, "unit-a"), [2]);
        assert_eq!(found_one(&h, 1, "unit-a"), [1]);
        assert_eq!(found_one(&h, 3, "unit-a"), [3]);
        let landed = decided(5, "unit-a", State::Landed);
        let index = crate::records::to_line(&crate::records::FinishedLine {
            id: 5,
            state: State::Landed,
            worktree: "unit-a".into(),
            subject: None,
        });
        {
            let mut records = h.records.borrow_mut();
            records.files.insert(
                crate::records::finished_file(5),
                crate::records::to_document(&landed),
            );
            records
                .files
                .insert(crate::records::FINISHED_INDEX.into(), index);
        }
        owe(&h, "5:landed", "unit-a");
        assert_eq!(found_one(&h, 5, "unit-a"), [5]);
    }

    /// 20, 51: a wait on another worktree's submission, on one not decided
    /// or on no submission finds nothing.
    #[test]
    fn the_narrowed_collect_finds_nothing_not_the_askers_to_hand_over() {
        let h = Host::new();
        keep_owed(&h, &decided(1, "unit-b", State::Failed));
        keep(&h, &decided(3, "unit-a", State::Queued));
        assert!(found_one(&h, 1, "unit-a").is_empty());
        assert!(found_one(&h, 3, "unit-a").is_empty());
        assert!(found_one(&h, 9, "unit-a").is_empty());
        assert_eq!(found_one(&h, 1, "unit-b"), [1]);
    }

    /// 51: handing the narrowed collect over hands over the one submission
    /// named, and leaves the asker's others with no hand-over.
    #[test]
    fn the_narrowed_collect_hands_over_only_the_one_named() {
        let h = Host::new();
        keep_owed(&h, &decided(1, "unit-a", State::Failed));
        keep_owed(&h, &decided(2, "unit-a", State::Landed));
        let s = h.sluice();
        let one = crate::owed::Collect::One {
            branch: BRANCH.into(),
            id: 2,
        };
        s.owed(&me("unit-a"), one)
            .unwrap()
            .hand("wait", "/w/unit-a", false)
            .unwrap();
        let owed = s.owed(&me("unit-a"), crate::owed::Collect::New).unwrap();
        assert_eq!(
            owed.outcomes
                .iter()
                .map(|o| o.submission.id)
                .collect::<Vec<_>>(),
            [1],
            "only the other has no hand-over"
        );
        assert_eq!(found(&h, "unit-a"), [1, 2]);
    }

    /// Rule core.11: a branch whose records cannot be read is named with
    /// the reader's words, and every other branch is read still.
    #[test]
    fn an_unreadable_branch_is_named_and_the_others_read() {
        let h = Host::new();
        h.unreadable.borrow_mut().push("bolt/x".into());
        keep_owed(&h, &decided(2, "unit-a", State::Landed));
        let s = h.sluice();
        let owed = s.owed(&me("unit-a"), crate::owed::Collect::All).unwrap();
        assert_eq!(owed.outcomes.len(), 1);
        assert_eq!(
            owed.unread,
            [Unread {
                branch: "bolt/x".into(),
                error: "remote: Repository not found.".into(),
            }]
        );
    }

    /// 125: a dirty second place refuses the act whole: nothing is
    /// written, no number is taken, and the refusal names that place.
    #[test]
    fn a_dirty_place_writes_nothing_and_is_named() {
        let h = Host::new();
        for w in ["b1", "b2", "b3"] {
            h.place(w);
        }
        h.dirty("b2", "notes.txt");
        let got = h.sluice().submit_group(&h.group(&["b1", "b2", "b3"], &[]));
        assert_eq!(
            got.unwrap(),
            Err(vec![(
                "b2".to_string(),
                SubmitRefusal::Dirty(vec!["notes.txt".into()])
            )])
        );
        assert_eq!(h.writes.get(), 0);
        assert_eq!(h.next_id.get(), 1, "no number taken");
        assert!(line(&h).subs.is_empty());
    }

    /// 125: a place named twice, by its name or another resolving to it,
    /// is refused, naming the place it repeats; so is every place when the
    /// branch refuses the act.
    #[test]
    fn every_refusal_is_given_with_its_place() {
        let h = Host::new();
        h.place("b1");
        h.place("b2");
        let got = h
            .sluice()
            .submit_group(&h.group(&["b1", "b2", "b1"], &[]))
            .unwrap();
        assert_eq!(
            got,
            Err(vec![("b1".into(), SubmitRefusal::NamedTwice("b1".into()))])
        );
        let got = h
            .sluice()
            .submit_group(&h.group(&["b1", "b2"], &[40]))
            .unwrap();
        assert_eq!(
            got,
            Err(vec![
                ("b1".into(), SubmitRefusal::UnknownAfter(vec![40])),
                ("b2".into(), SubmitRefusal::UnknownAfter(vec![40])),
            ])
        );
        assert_eq!(h.writes.get(), 0);
    }

    /// 125, 6: three places make one version of the records holding three
    /// submissions in the order given, numbered in it, none naming another
    /// of the act as one it lands after; `after` and `priority` are on
    /// every one alike.
    #[test]
    fn three_places_are_one_write_in_order() {
        let h = Host::new();
        h.place("earlier");
        let earlier = h
            .sluice()
            .submit_group(&h.group(&["earlier"], &[]))
            .unwrap()
            .unwrap()[0]
            .id;
        let before = h.records.borrow().version;
        for w in ["b1", "b2", "b3"] {
            h.place(w);
        }
        let mut req = h.group(&["b1", "b2", "b3"], &[earlier]);
        req.priority = 3;
        let accepted = h.sluice().submit_group(&req).unwrap().unwrap();
        assert_eq!(h.records.borrow().version, before + 1, "one write");
        let ids: Vec<u64> = accepted.iter().map(|a| a.id).collect();
        assert_eq!(ids, vec![2, 3, 4]);
        assert_eq!(accepted[2].ahead, vec![1, 2, 3]);
        let l = line(&h);
        for (id, w) in ids.iter().zip(["b1", "b2", "b3"]) {
            let s = &l.subs[id];
            assert_eq!(s.from.worktree, w);
            assert_eq!(s.state, State::Queued);
            assert_eq!(s.after, vec![earlier], "none names another of the act");
            assert_eq!(s.priority, 3);
        }
    }

    /// 6, 7: what a submit names ahead of it, and the chain status reads,
    /// follow the order a round takes the queue in, not the order of
    /// submission: one at priority 5 is ahead of one submitted before it
    /// at 0, and one at priority 9 after that one is behind both.
    #[test]
    fn ahead_is_the_queues_order() {
        let h = Host::new();
        for w in ["a", "b", "c"] {
            h.place(w);
        }
        let submit = |w: &str, after: &[u64], priority: i64| {
            let mut req = h.group(&[w], after);
            req.priority = priority;
            h.sluice().submit_group(&req).unwrap().unwrap().remove(0)
        };
        let a = submit("a", &[], 0);
        let b = submit("b", &[], 5);
        assert!(b.ahead.is_empty(), "{:?}", b.ahead);
        let c = submit("c", &[a.id], 9);
        assert_eq!(c.ahead, [b.id, a.id]);
        let status = h.sluice().status(BRANCH).unwrap();
        assert_eq!(status.chain, [b.id, a.id, c.id]);
        assert_eq!(status.ahead_of(a.id), [b.id]);
        assert_eq!(status.ahead_of(c.id), [b.id, a.id]);
    }

    /// 23, 125: a group given a subject per place keeps each on its own
    /// submission, in order; one given none keeps none; any other count
    /// is a caller's error and writes nothing.
    #[test]
    fn each_place_keeps_its_own_subject() {
        let h = Host::new();
        h.place("b1");
        h.place("b2");
        let mut req = h.group(&["b1", "b2"], &[]);
        req.subjects = vec!["Batch review cards".into(), "Card filters".into()];
        let ids: Vec<u64> = h
            .sluice()
            .submit_group(&req)
            .unwrap()
            .unwrap()
            .iter()
            .map(|a| a.id)
            .collect();
        let l = line(&h);
        let subjects: Vec<_> = ids.iter().map(|id| l.subs[id].subject.clone()).collect();
        assert_eq!(
            subjects,
            vec![
                Some("Batch review cards".to_string()),
                Some("Card filters".to_string())
            ]
        );
        let accepted = h
            .sluice()
            .submit_group(&h.group(&["b1", "b2"], &[]))
            .unwrap()
            .unwrap();
        let l = line(&h);
        assert!(accepted.iter().all(|a| l.subs[&a.id].subject.is_none()));
        let before = h.writes.get();
        req.subjects.pop();
        let e = h.sluice().submit_group(&req).unwrap_err();
        assert!(
            format!("{e:#}").contains("1 subjects for 2 worktrees"),
            "{e:#}"
        );
        assert_eq!(h.writes.get(), before);
    }

    /// 23: a submission written before subjects were kept reads as having
    /// none, and one with none writes no `subject`.
    #[test]
    fn a_submission_with_no_subject_reads_and_writes_as_before() {
        let json = serde_json::json!({
            "schema": SUBMISSION_SCHEMA, "id": 4, "branch": "main", "head": "h",
            "from": { "agent": "a", "worktree": "unit/a" },
            "submitted": "2026-10-23T10:00:00Z", "after": [], "priority": 0,
            "attempt": 1, "state": "queued",
        });
        let s: Submission = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(s.subject, None);
        assert_eq!(serde_json::to_value(&s).unwrap(), json);
    }

    /// 67: each place's earlier submission is withdrawn in the same write.
    #[test]
    fn each_places_earlier_submission_is_withdrawn() {
        let h = Host::new();
        h.place("b1");
        h.place("b2");
        h.sluice()
            .submit_group(&h.group(&["b1"], &[]))
            .unwrap()
            .unwrap();
        let accepted = h
            .sluice()
            .submit_group(&h.group(&["b1", "b2"], &[]))
            .unwrap()
            .unwrap();
        assert_eq!(accepted[0].withdrew, vec![1]);
        assert!(accepted[1].withdrew.is_empty());
        let s = h.sluice();
        assert_eq!(s.state(1).unwrap().unwrap().state, State::Withdrawn);
        assert_eq!(s.state(2).unwrap().unwrap().attempt, 2);
    }

    /// 124: a policy field no sluice yet reads leaves the branch guarded;
    /// every reading but guarding's refuses it.
    #[test]
    fn guarding_passes_over_a_field_it_does_not_know() {
        let h = Host::new();
        *h.policy.borrow_mut() = "[branches.main]\nlater = true\n[later]\nx = 1\n".into();
        let tip = Rev::new("tip");
        assert!(h.sluice().guarding().protects(BRANCH, &tip).unwrap());
        assert!(!h.sluice().guarding().protects("unit/x", &tip).unwrap());
        let e = format!("{:#}", h.sluice().protects(BRANCH, &tip).unwrap_err());
        assert!(e.contains("later"), "{e}");
    }

    /// 129, design D4: whether a branch is in play is read from its
    /// records alone, no revision and no policy read: none while nothing is
    /// queued, under test or to release; one queued submission is in play,
    /// and so is a combination under test whose submission was withdrawn,
    /// and then a checkout whose release failed, until it is released.
    #[test]
    fn in_play_reads_the_records_alone() {
        let f = sluice_fakes::Fakes::new();
        f.world.init(
            "main",
            &[(POLICY_PATH, "[branches.main]\nrequired = [\"#gate\"]\n")],
        );
        let seams = Seams {
            checkout: &f.checkout,
            tests: &f.tests,
            branch: &f.branch,
            driver: &f.driver,
            state: &f.state,
        };
        let s = Sluice::new(seams, &Noon);
        let (revisions, files) = (f.world.revision_reads(), f.world.file_reads());
        assert!(!s.in_play("main").unwrap());
        assert!(!s.in_play("none").unwrap());
        assert_eq!(f.world.revision_reads(), revisions);
        assert_eq!(f.world.file_reads(), files);

        let head = f.world.work(&f.world.tip("main"), &[("a", Some("1"))]);
        f.world.session("unit/a", "agent", &head);
        s.submit(&SubmitRequest {
            head: SubmitHead::Worktree("unit/a".into()),
            branch: "main".into(),
            after: vec![],
            priority: 0,
            subject: None,
        })
        .unwrap()
        .unwrap();
        let in_play = || {
            let (revisions, files) = (f.world.revision_reads(), f.world.file_reads());
            let in_play = s.in_play("main").unwrap();
            assert_eq!(f.world.revision_reads(), revisions);
            assert_eq!(f.world.file_reads(), files);
            in_play
        };
        assert!(in_play());

        f.world.task("a:test", "");
        f.world.hold_runs(true);
        s.step("main").unwrap();
        s.withdraw(1, &Requester::Operator).unwrap().unwrap();
        let line = s.read_line("main").unwrap();
        assert!(line.subs.values().all(|s| s.state.is_out()));
        assert!(line.doc.candidates.iter().any(|c| c.state.under_test()));
        assert!(in_play(), "a combination under test");

        f.world.fail_next_release();
        s.step("main").unwrap();
        let line = s.read_line("main").unwrap();
        assert!(line.doc.candidates.is_empty());
        assert_eq!(line.doc.release.len(), 1);
        assert!(in_play(), "a checkout to release");

        s.step("main").unwrap();
        assert!(s.read_line("main").unwrap().doc.release.is_empty());
        assert!(!in_play());
    }

    /// A result of `task` at `at` on `branch`, failed.
    fn failed(task: &str, at: &str, branch: &str) -> Vec<u8> {
        format!(
            "{{\"at\":\"{at}\",\"kind\":\"ran\",\"task\":\"{task}\",\"hash\":\"h-{task}\",\
             \"outcome\":\"fail\",\"branch\":\"{branch}\",\"combination\":\"c\"}}\n"
        )
        .into_bytes()
    }

    /// Could-not-run-1 116, design D1: a result is set aside from among
    /// every branch's results, a result only a second branch's gate reads
    /// found there, and one two branches read found once.
    #[test]
    fn a_result_only_a_second_branch_reads_is_set_aside() {
        let h = Host::new();
        h.results
            .borrow_mut()
            .insert(BRANCH.into(), failed("a:test", "t1", BRANCH));
        let mut release = failed("b:test", "t2", "release");
        release.extend(failed("a:test", "t1", BRANCH));
        h.results.borrow_mut().insert("release".into(), release);
        let s = h.sluice();
        let aside = s
            .set_aside("b:test", "t2", None, "the host was down", "op@host")
            .unwrap();
        assert_eq!(
            (aside.branch.as_str(), aside.hash.as_str()),
            ("release", "h-b:test")
        );
        assert!(s
            .set_aside("b:test", "t2", None, "twice", "op@host")
            .is_err());
        let both = s.set_aside("a:test", "t1", None, "read twice, kept once", "op@host");
        assert_eq!(both.unwrap().branch, BRANCH);
    }
}

#[cfg(test)]
mod forecasts {
    use chrono::{DateTime, TimeZone, Utc};
    use sluice_fakes::Fakes;

    use super::*;
    use crate::policy::POLICY_PATH;
    use crate::{Clock, Seams};

    const POLICY: &str = "[branches.main]\nrequired = [\"#gate\"]\nparallel = 4\n";

    struct Noon;

    impl Clock for Noon {
        fn now(&self) -> DateTime<Utc> {
            Utc.with_ymd_and_hms(2026, 10, 29, 12, 0, 0).unwrap()
        }
    }

    fn sluice(f: &Fakes) -> Sluice<'_> {
        let seams = Seams {
            checkout: &f.checkout,
            tests: &f.tests,
            branch: &f.branch,
            driver: &f.driver,
            state: &f.state,
        };
        Sluice::new(seams, &Noon)
    }

    fn submit(s: &Sluice<'_>, worktree: &str) -> u64 {
        let req = SubmitRequest {
            head: SubmitHead::Worktree(worktree.into()),
            branch: "main".into(),
            after: Vec::new(),
            priority: 0,
            subject: None,
        };
        s.submit(&req).unwrap().unwrap().id
    }

    /// `main` with `#1` under test, its combination formed on `main`'s
    /// revision, and `#2` queued behind it; and a head of `unit/c` that
    /// conflicts with `#1`.
    fn queued() -> (Fakes, Rev, Rev) {
        let f = Fakes::new();
        let tip = f.world.init("main", &[(POLICY_PATH, POLICY), ("lib", "1")]);
        f.world.task("a:test", "");
        f.world.hold_runs(true);
        let one = f.world.work(&tip, &[("lib", Some("2"))]);
        f.world.session("unit/a", "agent-a", &one);
        let two = f.world.work(&tip, &[("other", Some("x"))]);
        f.world.session("unit/b", "agent-b", &two);
        let s = sluice(&f);
        assert_eq!(submit(&s, "unit/a"), 1);
        s.step("main").unwrap();
        assert_eq!(submit(&s, "unit/b"), 2);
        let head = f.world.work(&tip, &[("lib", Some("3"))]);
        (f, head, one)
    }

    fn view(s: &Sluice<'_>) -> BranchView {
        s.overview(None)
            .unwrap()
            .into_iter()
            .find(|v| v.branch == "main")
            .expect("main has records")
    }

    /// 64, 65: a forecast formed on a reader's read of the queue is the
    /// forecast the records of the same queue give: the same revision,
    /// the same submissions ahead, and the same conflict, named with whom.
    #[test]
    fn a_forecast_from_a_view_is_the_forecast_from_the_records() {
        let (f, head, _) = queued();
        let s = sluice(&f);
        let tip = f.world.tip("main");
        let view = view(&s);
        assert_eq!(view.revision.as_ref(), Some(&tip));
        assert!(view.candidates.iter().any(|c| c.on.as_ref() == Some(&tip)));
        let records = s.forecast_onto(&head, "unit/c", "main", tip, &[]).unwrap();
        let read = s
            .forecast_read(&view, &head, "unit/c", &[])
            .unwrap()
            .expect("formed on the read")
            .unwrap();
        assert_eq!(read, records);
        assert_eq!(read.ahead, [1, 2]);
        let Forecast::Conflict { paths, with } = &read.forecast else {
            panic!("no conflict: {read:?}");
        };
        assert_eq!(paths, &["lib"]);
        let with: Vec<(u64, &str)> = with
            .iter()
            .map(|c| (c.id, c.from.worktree.as_str()))
            .collect();
        assert_eq!(with, [(1, "unit/a")]);
    }

    /// 64: a read that names no revision of the branch, one that could not
    /// read it, or one from before the revisions a combination was formed
    /// on were named, is no read to form on.
    #[test]
    fn a_view_with_no_revision_gives_none() {
        let (f, head, _) = queued();
        let s = sluice(&f);
        let mut none = view(&s);
        none.revision = None;
        assert_eq!(s.forecast_read(&none, &head, "unit/c", &[]).unwrap(), None);
        let mut before = view(&s);
        before.candidates[0].on = None;
        assert_eq!(
            s.forecast_read(&before, &head, "unit/c", &[]).unwrap(),
            None
        );
        let unread = BranchView::unread("main", &anyhow::anyhow!("refused"));
        assert_eq!(
            s.forecast_read(&unread, &head, "unit/c", &[]).unwrap(),
            None
        );
    }

    /// 64, 124: where the host lacks a queued submission's head, or the
    /// head forecast, the read is not formed on; the records' forecast
    /// forms all the same.
    #[test]
    fn a_missing_head_gives_none() {
        let (f, head, one) = queued();
        let s = sluice(&f);
        let tip = f.world.tip("main");
        let read = view(&s);
        f.world.lack(&one, true);
        assert_eq!(s.forecast_read(&read, &head, "unit/c", &[]).unwrap(), None);
        f.world.lack(&one, false);
        f.world.lack(&head, true);
        assert_eq!(s.forecast_read(&read, &head, "unit/c", &[]).unwrap(), None);
        f.world.lack(&head, false);
        f.world.lack(&one, true);
        let formed = s.forecast_onto(&head, "unit/c", "main", tip, &[]).unwrap();
        assert_eq!(formed.ahead, [1, 2]);
    }
}
