//! The step function: one round of a branch's work (ADR 0005; walkthrough
//! section 6).
//!
//! A step takes the branch's lock, then:
//!
//! 1. **recover**: finishes what the records say was decided and not
//!    confirmed done (59; rule core.14): a place decided and maybe not
//!    started, places to release, a landing intent, outcomes to tell;
//! 2. **decide**: reads the branch, the gate at its revision and every
//!    run that finished; orders, forms, attributes, chooses a landing and
//!    the candidates to start; and writes all of it to the records in one
//!    write, before any of it is done (62);
//! 3. **perform**: does what was decided, confirming each effect in the
//!    records once done.
//!
//! A crash anywhere leaves records from which the next step finishes the
//! work correctly (24, 51, S10).
//!
//! Each round is a `tracing` span, with its parts beneath it, and what a
//! reader counts is a `tracing` event: the binary exports them when asked
//! to (ADR 0014). Nothing here reads them back: the records alone decide.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, bail, Result};
use sluice_seams::{
    read_refused, AtBound, Contested, CoveringVerdict, Delivery, Formed, HostCopy, LandingCommit,
    Limits, Moved, NotRun, Notification, Outcome, Part, Progress, ReadRefused, Reading, Refusals,
    Rev, RunHandle, RunKind, RunReport, RunRequest, Source, Submitter, TaskLog, Written,
};
use tracing::{info, info_span, warn};

use crate::line::Line;
use crate::overview::Room;
use crate::policy::{Rules, POLICY_PATH};
use crate::records::{
    Beside, BesideWhy, BranchHold, Candidate, CandidateState, ConflictRecord, CopyBehind,
    DecidedRecord, Event, FailedTask, Held, LandingCommitRecord, LandingIntent, LostRun,
    OutboxItem, Recorded, ResultKind, ResultLine, RunState, State, SystemRun, Tested,
};
use crate::refused::Refusal;
use crate::release::{ReleaseOutcome, Releasing};
use crate::text::{beside_message, landing_message};
use crate::{proof, verdict, Sluice, OUTPUT};

/// Could-not-run-1 115: the deployment's bound on a candidate's runs in a
/// row that leave a check not run and fail none on its own result.
pub const NOT_RUN_BOUND: u32 = 3;

/// 123: how many more times a checkout's release that failed is tried
/// when the branch's policy names no `release_retries`.
pub const RELEASE_RETRIES: u32 = 3;

/// A round: how its step went, the session places it released because
/// their worktrees no longer exist (session-runs-1 clause 97), the
/// previews it released because nobody can use them (previews-1 clause
/// 101), and the combinations' checkouts whose release failed, was held
/// or found their working copy gone (123).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Round {
    pub step: Step,
    pub released: Vec<crate::session::Abandoned>,
    pub previews: Vec<crate::preview::Dropped>,
    pub releases: Vec<ReleaseOutcome>,
    /// 121: the hold, as the records keep it, of a round that held the
    /// branch.
    pub held: Option<BranchHold>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Another step holds the branch's lock (rule core.14).
    Busy,
    /// Another instance keeps the branch, and its lease has not passed
    /// (72, 73).
    Led,
    /// The records were written between this step's read and its write;
    /// nothing was done (62).
    Contended,
    /// 121: the branch was read at a revision lacking the landing it must
    /// contain; nothing was formed, started or landed.
    Held,
    Stepped,
}

/// What a levelling found of this host's copy of a branch (124).
enum Levelled {
    /// The branch has no copy apart from it on this host.
    Itself,
    /// The copy holds the landing.
    Holds,
    /// The copy is left at `at`, for `why`.
    Behind { at: Option<Rev>, why: String },
}

/// Design D3 (84; record 0074): what this host lacks of what a branch's
/// runs ask of it, looked at once a round, the first time one of them
/// would be claimed, and kept for that round alone.
struct Unmet {
    limits: Limits,
    looked: std::cell::OnceCell<Option<String>>,
}

impl Unmet {
    fn of(limits: Limits) -> Unmet {
        Unmet {
            limits,
            looked: std::cell::OnceCell::new(),
        }
    }

    /// What the round's look found the host lacks; `None` where it has
    /// not looked or found nothing lacking.
    fn found(&self) -> Option<&str> {
        self.looked.get().and_then(|w| w.as_deref())
    }
}

/// Design D4: what starting a claimed run came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Started {
    /// Its tests are running under this handle.
    Running(RunHandle),
    /// The host had no room for it: it is open again, counting toward
    /// nothing (71).
    AtBound,
    /// Its tests did not run, for this reason: the run is recorded as one
    /// whose tests could not run (114, 115, 139).
    NotRun(String),
    /// Its place could not be made, for this reason: the run never began,
    /// so it is open again with no outcome, its record saying why, and it
    /// counts toward no runs in a row (60, 114; record 0118).
    Unmade(String),
}

/// What starting a branch's claimed runs reads of it, once.
struct StartFrom {
    base: Rev,
    /// Whether `base` is covered by a verdict under its gate (70).
    verified: bool,
    policy: crate::policy::Policy,
    rules: Rules,
    /// The policy's name for the system this host is (85).
    home: Option<String>,
}

/// 140: how a queued entry's work was read beside the chain.
enum Read {
    /// Some required check reads it at the branch's revision, or it
    /// cannot be read: it takes the chain as any submission.
    Chain,
    /// It takes the chain, waiting behind that entry, for that.
    Behind(u64, BesideWhy),
    /// It lands beside the chain: `formed` is the branch's revision with
    /// it, `merged` each formed candidate as it would land with it.
    Beside { formed: Rev, merged: Vec<Rev> },
}

/// 71 (rule core.49; record 0087): the parts of the host's bound a
/// round holds for the runs it marked starting, by each run's place, from
/// the count that chose them to their starts. A part left when the round
/// ends, or dropped with it, gives its room back.
type Parts = BTreeMap<String, Part>;

/// 88: whether `run` is held by a claim a lead gave a pool's job, which the
/// lead marked starting with no room of its own host's, the job's to hold
/// at its start (S19).
fn given_out(run: &SystemRun) -> bool {
    run.host.as_deref().is_some_and(|h| h.starts_with("pool/"))
}

/// What a round's decide step did.
enum Decided {
    /// Its write was made; what the round looked at of the host, and the
    /// parts it holds for the runs it marked starting, go on to its starts
    /// (design D4).
    Written(Unmet, Parts),
    /// 121: it wrote the hold and nothing else.
    Held,
    /// Its write was refused (62).
    Contended,
}

impl Sluice<'_> {
    /// One round of `branch`'s work.
    pub fn step(&self, branch: &str) -> Result<Step> {
        Ok(self.round(branch)?.step)
    }

    /// One round of `branch`'s work, first releasing every session place
    /// on the host whose worktree no longer exists (session-runs-1
    /// clause 97) and every preview nobody can use (previews-1 clause
    /// 101), before the branch's lock is taken: each release takes the
    /// lock of its own place's branch.
    pub fn round(&self, branch: &str) -> Result<Round> {
        // 129 (design D16): a round reads afresh, never as the host holds
        // the branch since another process's ask.
        if self.held {
            bail!("a round of {branch} never reads its records as held");
        }
        let round = info_span!("sluice.round", sluice.branch = branch, sluice.step = "");
        let _round = round.enter();
        let released = info_span!("sluice.abandoned").in_scope(|| self.release_abandoned())?;
        let previews =
            info_span!("sluice.previews").in_scope(|| self.release_unusable_previews())?;
        let mut releases = Vec::new();
        let step = self.step_locked(branch, &round, &mut releases)?;
        let held = match step {
            Step::Held => self.read_line(branch)?.doc.held,
            _ => None,
        };
        Ok(Round {
            step,
            released,
            previews,
            releases,
            held,
        })
    }

    /// The round under the branch's lock; each release it tried that was
    /// not plainly released goes in `releases`, and no place is tried
    /// twice in one round (123).
    ///
    /// 133: what the round met of the branch's remote is counted here, once
    /// a round (design D3). A round that never took the lock never reached
    /// the remote, and counts as nothing. A round whose error holds a read
    /// of the remote it could not make is refused, and returns that error
    /// unchanged; one that ended well read the branch and its records, and
    /// ends a standing refusal; any other error counts as nothing.
    fn step_locked(
        &self,
        branch: &str,
        round: &tracing::Span,
        releases: &mut Vec<ReleaseOutcome>,
    ) -> Result<Step> {
        let Some(_lock) = self.seams.state.lock(branch)? else {
            round.record("sluice.step", "busy");
            return Ok(Step::Busy);
        };
        let stepped = self.step_held(branch, round, releases);
        match &stepped {
            Ok(_) => self.note_read_again(branch),
            Err(e) => {
                if let Some(refused) = read_refused(e) {
                    round.record("sluice.step", "refused");
                    self.note_refused(branch, refused);
                }
            }
        }
        stepped
    }

    /// The round's work, under the branch's lock.
    fn step_held(
        &self,
        branch: &str,
        round: &tracing::Span,
        releases: &mut Vec<ReleaseOutcome>,
    ) -> Result<Step> {
        if !self.keeps(branch)? {
            // 75: another instance keeps the branch; this one takes, runs
            // and reports runs of its own.
            info_span!("sluice.claim").in_scope(|| self.execute(branch, releases))?;
            round.record("sluice.step", "led");
            return Ok(Step::Led);
        }
        info_span!("sluice.recover").in_scope(|| self.recover(branch, releases))?;
        let (unmet, parts) = match info_span!("sluice.decide").in_scope(|| self.decide(branch))? {
            Decided::Written(unmet, parts) => (unmet, parts),
            Decided::Contended => {
                round.record("sluice.step", "contended");
                return Ok(Step::Contended);
            }
            Decided::Held => {
                // 121: nothing new to start or land; an earlier round's
                // outcomes are still pushed and its places released.
                info_span!("sluice.perform").in_scope(|| {
                    self.perform_releases(branch, releases)?;
                    self.perform_outbox(branch)
                })?;
                round.record("sluice.step", "held");
                return Ok(Step::Held);
            }
        };
        info_span!("sluice.perform")
            .in_scope(|| self.perform(branch, Some(&unmet), parts, releases))?;
        round.record("sluice.step", "stepped");
        Ok(Step::Stepped)
    }

    /// 72, 73: whether this instance keeps `branch`, taking it when no one
    /// does, or when the instance that does has written nothing for the
    /// lease by this host's own observation and there is work to do.
    /// Taking is a write (62), after which the former lead's next write is
    /// refused. A branch in this host's own files has no lead. Intake asks
    /// it too, to take a branch's pull requests in only where this
    /// instance keeps the branch (137).
    pub fn keeps(&self, branch: &str) -> Result<bool> {
        let line = self.read_line(branch)?;
        // 133: the round's first read of the records, answered from what was
        // last fetched, refuses the round before anything is decided.
        if let Some(refused) = self.seams.state.stale(branch) {
            return Err(refused.into());
        }
        let Some(tip) = self.seams.branch.read(branch)? else {
            return Ok(true);
        };
        if !self.rules_at(branch, &tip)?.is_some_and(|r| r.signed) {
            return Ok(true);
        }
        let me = self.peer.name.clone();
        let from = match &line.doc.lead {
            Some(l) if *l == me => return Ok(true),
            None => None,
            Some(other) => {
                let now = self.now();
                let seen = self.seams.state.observed(branch)?;
                let since = match seen {
                    Some((v, at)) if v == line.version => at,
                    _ => {
                        self.seams.state.observe(branch, line.version, &now)?;
                        return Ok(false);
                    }
                };
                let waited = seconds_between(&since, &now).unwrap_or(0.0);
                if waited < self.peer.lease.as_secs_f64() || !has_work(&line) {
                    return Ok(false);
                }
                Some(other.clone())
            }
        };
        let mut taken = line;
        taken.doc.lead = Some(me.clone());
        let now = self.now();
        taken.log(&now, Event::Took { by: me, from });
        Ok(matches!(self.write(&taken)?, Written::Written(_)))
    }

    // -----------------------------------------------------------------------
    // 1. Recover
    // -----------------------------------------------------------------------

    fn recover(&self, branch: &str, releases: &mut Vec<ReleaseOutcome>) -> Result<()> {
        // A place decided by a step that stopped before confirming its run
        // started: release it by its name and open the run again, whatever
        // its release did (123); one that failed is tried again by the
        // rounds after.
        let line = self.read_line(branch)?;
        let stale: Vec<(String, String)> = line
            .doc
            .candidates
            .iter()
            .flat_map(|c| {
                c.runs
                    .iter()
                    .filter(|r| r.state == RunState::Starting && self.mine(r))
                    .filter_map(|r| r.place.clone().map(|p| (c.id.clone(), p)))
            })
            .collect();
        for (cand, place) in stale {
            let outcome = self.release_made(branch, &place)?;
            let retry = matches!(outcome.how, Releasing::Failed { .. });
            self.update(branch, |l| {
                if let Some(c) = l.candidate_mut(&cand) {
                    for r in &mut c.runs {
                        if r.state == RunState::Starting && r.place.as_deref() == Some(&place) {
                            r.state = RunState::Open;
                            r.place = None;
                            r.run = None;
                        }
                    }
                }
                if retry && !l.doc.release.contains(&place) {
                    l.doc.release.push(place.clone());
                }
                Ok(())
            })?;
            if outcome.how != Releasing::Released {
                releases.push(outcome);
            }
        }
        self.perform(branch, None, Parts::new(), releases)
    }

    // -----------------------------------------------------------------------
    // 3. Perform
    // -----------------------------------------------------------------------

    /// `unmet`, where the round has it, is what it looked at of the host
    /// before its claims (design D4). 71 (rule core.49): `parts`, the room
    /// it holds for the runs it marked starting, is kept through its
    /// landing, releases and outcomes, and handed to their starts.
    fn perform(
        &self,
        branch: &str,
        unmet: Option<&Unmet>,
        parts: Parts,
        releases: &mut Vec<ReleaseOutcome>,
    ) -> Result<()> {
        self.perform_landing(branch)?;
        self.perform_releases(branch, releases)?;
        self.perform_outbox(branch)?;
        self.perform_starts(branch, unmet, parts, releases)
    }

    /// 25, 34: release the places of candidates whose verdict was read or
    /// that were dropped; only places sluice made (rule core.16). 123: a
    /// release that fails holds its place, never the round, and is tried
    /// once a round: a place tried already in this round, held, or made
    /// by another host is passed over. A held place of this host's whose
    /// working copy has gone since is released once, as any gone place is.
    /// 81: then each place this host made for the branch that no claim of
    /// its own holds, so the lead releases what it left as every other
    /// instance does, after a landing left unfinished is settled and
    /// before the round claims ([`Sluice::release_left`]).
    fn perform_releases(&self, branch: &str, releases: &mut Vec<ReleaseOutcome>) -> Result<()> {
        let line = self.read_line(branch)?;
        let doc = &line.doc;
        let elsewhere = |f: &crate::records::FailedRelease| {
            f.host.as_deref().is_some_and(|h| h != self.peer.name)
        };
        let gone_since = doc
            .failed_releases
            .iter()
            .filter(|f| f.stopped && !elsewhere(f) && self.held_gone(&f.place))
            .map(|f| &f.place);
        for place in doc.release.iter().chain(gone_since) {
            let passed_over = releases.iter().any(|o| o.place == *place)
                || doc.failed_releases.iter().any(|f| {
                    f.place == *place && ((f.stopped && !self.held_gone(place)) || elsewhere(f))
                });
            if passed_over {
                continue;
            }
            let outcome = self.release_made(branch, place)?;
            if outcome.how != Releasing::Released {
                releases.push(outcome);
            }
        }
        self.release_left(&line, releases)
    }

    /// 81, 123: on every round of this host, the lead's and any other's,
    /// before it claims: each place it made for the branch of `line`, the
    /// branch's records as the round read them, that no claim of its own
    /// holds, its candidate's verdict read, the candidate obsolete or the
    /// claim lapsed, is released. A place whose release stopped is held
    /// and passed over while its working copy is there, as is a place
    /// tried already this round: a held place bars no claim.
    fn release_left(&self, line: &Line, releases: &mut Vec<ReleaseOutcome>) -> Result<()> {
        let branch = &line.branch;
        let held: BTreeSet<&str> = line
            .doc
            .candidates
            .iter()
            .flat_map(|c| &c.runs)
            .filter(|r| self.mine(r) && r.state.held())
            .filter_map(|r| r.place.as_deref())
            .collect();
        let prefix = format!("sluice/try/{branch}/");
        for p in self.seams.checkout.made()? {
            if !p.starts_with(&prefix) || held.contains(p.as_str()) {
                continue;
            }
            let stopped = line
                .doc
                .failed_releases
                .iter()
                .any(|f| f.place == p && f.stopped);
            if (stopped && !self.held_gone(&p)) || releases.iter().any(|o| o.place == p) {
                continue;
            }
            let outcome = self.release_made(branch, &p)?;
            if outcome.how != Releasing::Released {
                releases.push(outcome);
            }
        }
        Ok(())
    }

    /// 123: whether the working copy of the held place `place` is gone, so
    /// it is released once more; a place whose state cannot be read is
    /// left held.
    fn held_gone(&self, place: &str) -> bool {
        self.seams.checkout.gone(place).unwrap_or(false)
    }

    /// 24, 45, 46: move the branch from exactly the revision the landing
    /// was formed on; a branch found elsewhere refuses the landing, which
    /// is reported and never retried (rule core.5).
    fn perform_landing(&self, branch: &str) -> Result<()> {
        let line = self.read_line(branch)?;
        let Some(intent) = line.doc.landing.clone() else {
            return Ok(());
        };
        let span = info_span!(
            "sluice.land",
            sluice.branch = branch,
            sluice.submissions = ?intent.ids,
            sluice.from = %intent.from,
            sluice.to = %intent.to,
            sluice.outcome = "",
        );
        let _span = span.enter();
        let found = self.seams.branch.read(branch)?;
        let moved = match &found {
            // Moved by a step that stopped before recording it.
            Some(f) if *f == intent.to => Moved::Moved,
            Some(f) if *f == intent.from => {
                self.seams
                    .branch
                    .move_to(branch, &intent.from, &intent.to)?
            }
            // Moved, then moved again before the landing was recorded: it
            // is in the branch's history all the same.
            Some(f) if self.seams.branch.contains(f, &intent.to)? => Moved::Moved,
            _ => Moved::Elsewhere(found.clone()),
        };
        let now = self.now();
        // 55, 70: a landing that changes the gate leaves the branch covered
        // by no verdict under the gate it now carries.
        let same_gate = matches!(moved, Moved::Moved)
            && self.rules_at(branch, &intent.from)? == self.rules_at(branch, &intent.to)?;
        match moved {
            Moved::Moved => {
                let confirmed = self.update(branch, |l| {
                    if l.doc.landing.as_ref() != Some(&intent) {
                        return Ok(false);
                    }
                    l.doc.landing = None;
                    let mut heads = Vec::new();
                    for c in &intent.commits {
                        let s = l.sub_mut(c.id);
                        s.state = State::Landed;
                        s.landed_as = Some(c.commit.clone());
                        if intent.beside {
                            s.beside = Some(Beside::Landed);
                            heads.push(s.head.clone());
                        }
                        // 23, 140: work landed beside carries the verdict
                        // that covered it, as its landing commit names it
                        // (rule core.39).
                        let covered_by = if intent.beside {
                            s.covered_by.clone()
                        } else {
                            None
                        };
                        let item = OutboxItem {
                            id: format!("{}:landed", c.id),
                            submission: c.id,
                            outcome: DecidedRecord::Landed {
                                landed_as: c.commit.clone(),
                                beside: intent.beside,
                                covered_by,
                            },
                        };
                        l.push_outcome(item, &now, self.peer.expire_after)?;
                    }
                    l.log(
                        &now,
                        Event::Landed {
                            ids: intent.ids.clone(),
                            to: intent.to.clone(),
                            beside: intent.beside,
                        },
                    );
                    // 140, 10: every candidate formed before a landing
                    // beside is the same candidate on the revision it made,
                    // carrying its work; its runs and verdict stand (17).
                    if intent.beside {
                        for c in &mut l.doc.candidates {
                            if c.on == intent.from {
                                c.on = intent.to.clone();
                                c.carries.extend(heads.iter().cloned());
                            }
                        }
                    }
                    // The candidates behind what landed hold exactly the tree
                    // they held: they now extend the new revision (10). One
                    // that landed whole while under test, on a deeper
                    // candidate's verdict, has its place released (25).
                    let mut release = Vec::new();
                    l.doc.candidates.retain_mut(|c| {
                        if c.on == intent.from && c.submissions.starts_with(&intent.ids) {
                            c.submissions.drain(..intent.ids.len());
                            c.on = intent.to.clone();
                            if c.submissions.is_empty() {
                                if c.state.under_test() {
                                    release.extend(
                                        c.runs
                                            .iter()
                                            .filter(|r| self.mine(r))
                                            .filter_map(|r| r.place.clone()),
                                    );
                                }
                                return false;
                            }
                        }
                        true
                    });
                    l.doc.release.extend(release);
                    l.doc.base = Some(intent.to.clone());
                    l.doc.verified = same_gate;
                    // 23, 70, 140: the verdict that covers the revision
                    // landed, which each landing beside on it names; none
                    // where the gate changed, or the intent named none
                    // (record 0083; rule core.39).
                    l.doc.covered_by = if same_gate {
                        intent.covering.clone()
                    } else {
                        None
                    };
                    // 121: every later read of the branch must contain it.
                    l.doc.landed = Some(intent.to.clone());
                    Ok(true)
                })?;
                span.record("sluice.outcome", "landed");
                // 124: the copy follows the landing; one left behind is
                // recorded beside it and never fails it (122).
                if confirmed {
                    match self.level_copy(branch, &intent.to) {
                        Ok(levelled) => self.record_copy(branch, &intent.to, &levelled)?,
                        Err(e) => tracing::warn!(
                            sluice.branch = branch,
                            "reading this host's copy after a landing failed: {e:#}"
                        ),
                    }
                }
                if confirmed {
                    for id in &intent.ids {
                        let waited = line
                            .subs
                            .get(id)
                            .and_then(|s| seconds_between(&s.submitted, &now));
                        info!(
                            monotonic_counter.sluice.landed = 1u64,
                            sluice.branch = branch,
                        );
                        if let Some(secs) = waited {
                            info!(
                                histogram.sluice.submission.landed_after = secs,
                                sluice.branch = branch,
                            );
                        }
                    }
                }
                Ok(())
            }
            Moved::Elsewhere(found) => {
                span.record("sluice.outcome", "refused");
                info!(
                    monotonic_counter.sluice.refused = 1u64,
                    sluice.branch = branch
                );
                self.update(branch, |l| {
                    if l.doc.landing.as_ref() != Some(&intent) {
                        return Ok(());
                    }
                    l.doc.landing = None;
                    for id in &intent.ids {
                        let s = l.sub_mut(*id);
                        if s.state == State::Landing {
                            s.state = State::Queued;
                            s.tested = None;
                            s.result = None;
                            s.covered_by = None;
                        }
                    }
                    l.log(
                        &now,
                        Event::Refused {
                            ids: intent.ids.clone(),
                            expected: intent.from.clone(),
                            found: found.clone(),
                        },
                    );
                    Ok(())
                })
            }
        }
    }

    /// 50, 51: push each outcome's notice to its submission's session
    /// alone, at most once: only an outcome owed to its reader with no
    /// hand-over and no confirmation, as `owed` reads them, is pushed, and
    /// its item goes whatever the carrier answered (design D10). A push the
    /// carrier took is recorded as a hand-over by its way with weir's
    /// `hand`, confirming it only where the carrier says its reader
    /// confirms on receipt, and logged `handed`, and `confirmed` where it
    /// confirms; one it refused stays owed, logged `pending`, and is pushed
    /// by no later round (weir 103, 109, 111; record 0102; design D5). A
    /// step that stops between a push and its record pushes it again, under
    /// the same key.
    fn perform_outbox(&self, branch: &str) -> Result<()> {
        let line = self.read_line(branch)?;
        let now = self.now();
        let owed = weir_delivery::owed(line.deliveries(), &now)
            .map_err(|refused| anyhow!("reading {branch}'s deliveries: {refused}"))?;
        for item in line.doc.outbox.clone() {
            let sub = line.sub(item.submission)?;
            let reader = sub.from.worktree.as_str();
            // 51: no push of an outcome handed to its reader by any way, or
            // confirmed: owed with no hand-over alone, or one decided
            // before its delivery was recorded, which has none.
            let recorded = line.deliveries().iter().any(
                |r| matches!(r, weir_delivery::Record::Delivery(d) if d.key().as_str() == item.id),
            );
            let unhanded = owed
                .readers
                .iter()
                .filter(|o| o.reader.as_str() == reader)
                .flat_map(|o| &o.deliveries)
                .any(|d| d.delivery.key().as_str() == item.id && d.handovers.is_empty());
            let delivery = if recorded && !unhanded {
                None
            } else {
                // ADR 0044: the report as it stands, for a driver that shows
                // one; a read that fails pushes the outcome without it.
                let report = self.verdict_of(branch, item.submission).ok().flatten();
                Some(self.seams.driver.notify(&Notification {
                    id: item.id.clone(),
                    submission: item.submission,
                    branch: branch.into(),
                    to: sub.from.clone(),
                    decided: item.outcome.to_decided(),
                    report,
                })?)
            };
            let now = self.now();
            self.update(branch, |l| {
                let Some(n) = l.doc.outbox.iter().position(|o| o.id == item.id) else {
                    return Ok(());
                };
                l.doc.outbox.remove(n);
                match &delivery {
                    None => {}
                    Some(Delivery::Handed { way, to, confirms }) => {
                        l.hand(item.submission, &item.id, reader, way, to, *confirms, &now)?;
                    }
                    Some(Delivery::Undelivered { why }) => {
                        l.log(
                            &now,
                            Event::Pending {
                                id: item.submission,
                                notification: item.id.clone(),
                                why: why.clone(),
                            },
                        );
                    }
                }
                Ok(())
            })?;
        }
        Ok(())
    }

    /// 33, 37: make each decided run's place and start its checks
    /// ([`Sluice::start_run`]) in the part `parts` holds for it, reading
    /// the host where `unmet`, the round's look, has not (design D4). 71
    /// (rule core.49): a run marked starting with no part held, a stopped
    /// round's, goes back to open for a later round, as does one the check
    /// seam refuses at the bound and every run decided behind it; a place
    /// made for it is released as any candidate's is (123), into
    /// `releases` when its release was not plain. 84, 139 (rule core.38):
    /// the stopped round's run is read against the host first, as its
    /// start would have read it, so an engine gone since its claim leaves
    /// it a run whose tests could not run, with the reason. 88, S19: a run
    /// a lead marked starting under the claim it gave a pool's job is no
    /// stopped round's; the job's round holds its part as it starts it.
    fn perform_starts(
        &self,
        branch: &str,
        unmet: Option<&Unmet>,
        mut parts: Parts,
        releases: &mut Vec<ReleaseOutcome>,
    ) -> Result<()> {
        let line = self.read_line(branch)?;
        let starting: Vec<(Candidate, usize)> = line
            .doc
            .candidates
            .iter()
            .filter(|c| c.state.under_test())
            .flat_map(|c| {
                c.runs
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.state == RunState::Starting && self.mine(r))
                    .map(|(i, _)| (c.clone(), i))
                    .collect::<Vec<_>>()
            })
            .collect();
        if starting.is_empty() {
            return Ok(());
        }
        let Some(from) = self.start_from(branch, &line)? else {
            return Ok(());
        };
        let fresh;
        let unmet = match unmet {
            Some(u) => u,
            None => {
                fresh = Unmet::of(from.policy.limits.clone());
                &fresh
            }
        };
        let mut full = false;
        for (c, i) in starting {
            let name = c.runs[i].place.clone().expect("a starting run has a place");
            let held = parts.remove(&name);
            if held.is_none() && !given_out(&c.runs[i]) {
                // No room was held for it: a stopped round's (core.49).
                if self.meets(unmet) {
                    self.reopen_made(branch, &c.id, i, &name, releases)?;
                } else {
                    let checks = match c.runs[i].checks.is_empty() {
                        true => from.rules.required.clone(),
                        false => c.runs[i].checks.clone(),
                    };
                    let kind = from.policy.limits.executor.as_deref().unwrap_or("open");
                    let why = unmet.found().unwrap_or_default().to_string();
                    let report = unstarted(&checks, kind, why);
                    let home = from.home.as_deref();
                    self.not_started(branch, &c, i, &name, &report, home, releases)?;
                }
                continue;
            }
            // 71: once a start is refused at the bound, the host holds none
            // of the room this round counted on; each run decided behind it
            // is open again, its part given back, no place made for it.
            if full {
                drop(held);
                self.reopen_made(branch, &c.id, i, &name, releases)?;
                continue;
            }
            let part = match held {
                Some(part) => part,
                // 88, S19: a run a lead marked starting under the claim it
                // gave this pool's job holds its room at the job's start, as
                // `start_claimed` holds it (rule core.49).
                None => match self.seams.tests.hold(RunKind::Queue)? {
                    Some(part) => part,
                    None => {
                        self.reopen_made(branch, &c.id, i, &name, releases)?;
                        full = true;
                        continue;
                    }
                },
            };
            full = self.start_run(branch, &from, &c, i, unmet, part, releases)? == Started::AtBound;
        }
        Ok(())
    }

    /// 71, 123: run `i` of candidate `cand`, decided into `place` and not
    /// started, open again ([`Sluice::reopen`]), the checkout a stopped
    /// start made for it released first, into `releases` when its release
    /// was not plain, and released again by the rounds after where it
    /// failed.
    fn reopen_made(
        &self,
        branch: &str,
        cand: &str,
        i: usize,
        place: &str,
        releases: &mut Vec<ReleaseOutcome>,
    ) -> Result<()> {
        if !self.seams.checkout.made()?.iter().any(|p| p == place) {
            return self.reopen(branch, cand, i, place, false);
        }
        let outcome = self.release_made(branch, place)?;
        let retry = matches!(outcome.how, Releasing::Failed { .. });
        self.reopen(branch, cand, i, place, retry)?;
        if outcome.how != Releasing::Released {
            releases.push(outcome);
        }
        Ok(())
    }

    /// Design D4 (88, 114, 139): start the run of `branch` that `claim`
    /// holds and that waits to start, as a round starts its own: the host
    /// read afresh first, then its checkout, at its combination fetched
    /// here where this host lacks it, and its tests' start; a start that
    /// fails or a host that cannot meet what the run needs recorded as a
    /// run whose tests could not run, and a checkout that cannot be made
    /// as a run that never began (record 0118). `None` where no run of the
    /// claim waits to start. Under the branch's lock, waited for (rule
    /// core.14). 71 (rule core.49): it holds the run's part of the host's
    /// bound before its checkout is made; where the host has no room, the
    /// run is open again and [`Started::AtBound`] is answered.
    ///
    /// The one start of a claimed run: a round's starts come through it,
    /// and `sluice job run`, of the change `actions-pool`, is to start its
    /// pool job's run through it under the job's claim, so the job needs
    /// nothing of its own to refuse at its start.
    pub fn start_claimed(
        &self,
        branch: &str,
        claim: &str,
        releases: &mut Vec<ReleaseOutcome>,
    ) -> Result<Option<Started>> {
        let _lock = self.seams.state.wait_lock(branch)?;
        let line = self.read_line(branch)?;
        let held = line
            .doc
            .candidates
            .iter()
            .filter(|c| c.state.under_test())
            .find_map(|c| {
                c.runs
                    .iter()
                    .position(|r| r.state == RunState::Starting && r.host.as_deref() == Some(claim))
                    .map(|i| (c.clone(), i))
            });
        let Some((c, i)) = held else {
            return Ok(None);
        };
        let Some(from) = self.start_from(branch, &line)? else {
            return Ok(None);
        };
        let Some(part) = self.seams.tests.hold(RunKind::Queue)? else {
            let name = c.runs[i].place.clone().expect("a starting run has a place");
            self.reopen_made(branch, &c.id, i, &name, releases)?;
            return Ok(Some(Started::AtBound));
        };
        let unmet = Unmet::of(from.policy.limits.clone());
        self.start_run(branch, &from, &c, i, &unmet, part, releases)
            .map(Some)
    }

    /// What starting `branch`'s claimed runs reads of it, once: `None`
    /// where its records hold no revision yet. 29: a gate no run of its
    /// executor can satisfy starts nothing.
    fn start_from(&self, branch: &str, line: &Line) -> Result<Option<StartFrom>> {
        let Some(base) = line.doc.base.clone() else {
            return Ok(None);
        };
        let policy = self
            .policy_at(&base)?
            .ok_or_else(|| anyhow!("{branch}'s revision {base} has no {POLICY_PATH}"))?;
        let rules = self.gate(branch, &base)?;
        let kind = policy.limits.executor.as_deref().unwrap_or("open");
        if !rules.satisfiable_by(kind) {
            bail!(
                "{branch}'s gate trusts {:?}, which no run of [executor] kind = {kind:?} satisfies",
                rules.trust
            );
        }
        Ok(Some(StartFrom {
            home: self.home_system(&base)?,
            verified: line.doc.verified,
            base,
            policy,
            rules,
        }))
    }

    /// 33, 37 (design D1, D4): start run `i` of candidate `c`, decided into
    /// its place. The host is read first where `unmet` has not been this
    /// round; where it cannot meet what the run needs, no checkout is made
    /// and each check is not run with the reason, the executor's own
    /// report (139). Then the checkout, at the combination, fetched by its
    /// own ref first where this host lacks it, however the run's claim
    /// reached the host (82, C.6); a checkout that cannot be made is a run
    /// that never began, open again with no outcome and counting toward
    /// nothing (60, 114; record 0118). Then the tests' start; a start
    /// refused at the bound is open again, counting toward nothing (71),
    /// and any other start that fails, a remote's unanswered read aside,
    /// leaves each check not run with the error's words (114). Either is
    /// read as a run whose tests could not run. The run starts in `part`,
    /// its room in the host's bound (71; rule core.49); a part not started
    /// is dropped, its room given back at once.
    #[allow(clippy::too_many_arguments)]
    fn start_run(
        &self,
        branch: &str,
        from: &StartFrom,
        c: &Candidate,
        i: usize,
        unmet: &Unmet,
        part: Part,
        releases: &mut Vec<ReleaseOutcome>,
    ) -> Result<Started> {
        let r = &c.runs[i];
        let kind = from.policy.limits.executor.as_deref().unwrap_or("open");
        let _span = info_span!(
            "sluice.start",
            sluice.branch = branch,
            sluice.candidate = %c.revision,
            sluice.submissions = ?c.submissions,
            sluice.executor = kind,
            sluice.system = r.system.as_deref().unwrap_or(""),
        )
        .entered();
        let name = r.place.clone().expect("a starting run has a place");
        let checks = if r.checks.is_empty() {
            from.rules.required.clone()
        } else {
            r.checks.clone()
        };
        let home = from.home.as_deref();
        if !self.meets(unmet) {
            let why = unmet.found().unwrap_or_default().to_string();
            let report = unstarted(&checks, kind, why.clone());
            self.not_started(branch, c, i, &name, &report, home, releases)?;
            return Ok(Started::NotRun(why));
        }
        let since = (from.verified && c.on == from.base).then(|| c.on.clone());
        let place = self.fetch_lacking(branch, c).and_then(|()| {
            self.seams
                .checkout
                .materialize(&name, &c.revision, &from.policy.checkout)
        });
        let place = match place {
            Ok(place) => place,
            // 133, 134: a remote that could not answer says nothing of the
            // run; the round is refused as any such read.
            Err(e) if sluice_seams::unanswered(&e) || read_refused(&e).is_some() => return Err(e),
            Err(e) => {
                let why = could_not_start(&e);
                let report = unstarted(&checks, kind, why.clone());
                self.unmade(branch, c, i, &name, &report, releases)?;
                return Ok(Started::Unmade(why));
            }
        };
        let started = self.seams.tests.start(
            &RunRequest {
                place,
                revision: c.revision.clone(),
                required: checks.clone(),
                since,
                limit: from.rules.timeout,
                limits: from.policy.limits.clone(),
                tests: from.policy.tests.clone(),
                command: None,
                // 108: the cache supplies outputs; a replay counts only
                // through a trusted pass recorded for its key.
                replay: !r.no_replay,
                system: r.system.clone(),
            },
            part,
        );
        let e = match started {
            Ok(run) => {
                self.update(branch, |l| {
                    if let Some(x) = l.candidate_mut(&c.id) {
                        if let Some(xr) = x.runs.get_mut(i) {
                            if xr.state == RunState::Starting && xr.place.as_deref() == Some(&name)
                            {
                                xr.state = RunState::Testing;
                                xr.run = Some(run.clone());
                            }
                        }
                    }
                    Ok(())
                })?;
                return Ok(Started::Running(run));
            }
            // Refused before anything of it ran: no run, no outcome.
            Err(e) if e.downcast_ref::<AtBound>().is_some() => {
                let outcome = self.release_made(branch, &name)?;
                let retry = matches!(outcome.how, Releasing::Failed { .. });
                self.reopen(branch, &c.id, i, &name, retry)?;
                if outcome.how != Releasing::Released {
                    releases.push(outcome);
                }
                return Ok(Started::AtBound);
            }
            // 133, 134: a remote that could not answer says nothing of the
            // run; the round is refused as any such read.
            Err(e) if sluice_seams::unanswered(&e) || read_refused(&e).is_some() => return Err(e),
            Err(e) => e,
        };
        let why = could_not_start(&e);
        let report = unstarted(&checks, kind, why.clone());
        self.not_started(branch, c, i, &name, &report, home, releases)?;
        Ok(Started::NotRun(why))
    }

    /// Design D1 (16, 114, 115): run `i` of candidate `c`, decided into
    /// `place`, whose tests could not be started, read as `report`, a run
    /// whose every check was not run. Its checkout, where one was made, is
    /// released as a run refused at the bound has its released, a release
    /// that failed tried again by the rounds after (123); the run is
    /// written once, read as any run that left checks not run: open again
    /// for any host under the bound, past it each check named with why.
    #[allow(clippy::too_many_arguments)]
    fn not_started(
        &self,
        branch: &str,
        c: &Candidate,
        i: usize,
        place: &str,
        report: &RunReport,
        home: Option<&str>,
        releases: &mut Vec<ReleaseOutcome>,
    ) -> Result<()> {
        let made = self.seams.checkout.made()?.iter().any(|p| p == place);
        let outcome = match made {
            true => Some(self.release_made(branch, place)?),
            false => None,
        };
        let retry = outcome
            .as_ref()
            .is_some_and(|o| matches!(o.how, Releasing::Failed { .. }));
        self.update(branch, |l| {
            let now = self.now();
            let Some(xr) = l.candidate_mut(&c.id).and_then(|x| x.runs.get_mut(i)) else {
                return Ok(());
            };
            if xr.state != RunState::Starting || xr.place.as_deref() != Some(place) {
                return Ok(());
            }
            // Released here; to be released again only where that failed.
            if !retry {
                xr.place = None;
            }
            let system = xr.system.clone();
            let system = system.as_deref().or(home);
            let host = &self.peer.name;
            if let Some(named) = read_not_run(l, c, i, &report.not_run, false, system, host, &now) {
                let x = l.candidate_mut(&c.id).expect("a read candidate");
                let xr = &mut x.runs[i];
                xr.state = RunState::Failed;
                let left = xr.place.take();
                x.failed.extend(named);
                if let Some(p) = left.filter(|p| !l.doc.release.contains(p)) {
                    l.doc.release.push(p);
                }
            }
            Ok(())
        })?;
        if let Some(o) = outcome.filter(|o| o.how != Releasing::Released) {
            releases.push(o);
        }
        Ok(())
    }

    /// 82, C.6: candidate `c`'s combination, fetched by its own ref where
    /// this host lacks it, so its place is made at it however the run's
    /// claim reached the host: a round's claim after its fetch of every
    /// candidate, the lead's own start, or a pool's job (record 0118).
    fn fetch_lacking(&self, branch: &str, c: &Candidate) -> Result<()> {
        if self.seams.branch.holds(&c.revision)? {
            return Ok(());
        }
        self.seams.state.fetch_candidate(branch, &c.id)
    }

    /// 60, 114 (record 0118): run `i` of candidate `c`, decided into
    /// `place`, whose place could not be made, `report` naming each check
    /// with why. The run never began: it has no outcome, counts toward no
    /// runs in a row (115) and blames no one, and it is open again for any
    /// host, its record saying why (16) until its next run has a result.
    /// Whatever of the place was made is released, a release that failed
    /// tried again by the rounds after (123).
    fn unmade(
        &self,
        branch: &str,
        c: &Candidate,
        i: usize,
        place: &str,
        report: &RunReport,
        releases: &mut Vec<ReleaseOutcome>,
    ) -> Result<()> {
        let made = self.seams.checkout.made()?.iter().any(|p| p == place);
        let outcome = match made {
            true => Some(self.release_made(branch, place)?),
            false => None,
        };
        let retry = outcome
            .as_ref()
            .is_some_and(|o| matches!(o.how, Releasing::Failed { .. }));
        self.update(branch, |l| {
            let now = self.now();
            let Some(xr) = l.candidate_mut(&c.id).and_then(|x| x.runs.get_mut(i)) else {
                return Ok(());
            };
            if xr.state != RunState::Starting || xr.place.as_deref() != Some(place) {
                return Ok(());
            }
            xr.state = RunState::Open;
            xr.place = None;
            xr.run = None;
            xr.host = None;
            xr.lost = Some(LostRun {
                host: self.peer.name.clone(),
                system: xr.system.clone(),
                not_run: report.not_run.clone(),
            });
            if retry && !l.doc.release.iter().any(|p| p == place) {
                l.doc.release.push(place.to_string());
            }
            l.log(
                &now,
                Event::Lost {
                    combination: c.revision.clone(),
                    not_run: report.not_run.clone(),
                },
            );
            Ok(())
        })?;
        if let Some(o) = outcome.filter(|o| o.how != Releasing::Released) {
            releases.push(o);
        }
        Ok(())
    }

    /// 71: run `i` of candidate `cand`, decided into `place` and not
    /// started, is open again, its claim given up so any host with room
    /// may take it; `retry` when its place was made and its release
    /// failed, so the rounds after try it again (123).
    fn reopen(&self, branch: &str, cand: &str, i: usize, place: &str, retry: bool) -> Result<()> {
        self.update(branch, |l| {
            if let Some(r) = l.candidate_mut(cand).and_then(|c| c.runs.get_mut(i)) {
                if r.state == RunState::Starting && r.place.as_deref() == Some(place) {
                    r.state = RunState::Open;
                    r.place = None;
                    r.run = None;
                    r.host = None;
                }
            }
            if retry && !l.doc.release.iter().any(|p| p == place) {
                l.doc.release.push(place.to_string());
            }
            Ok(())
        })
    }

    // -----------------------------------------------------------------------
    // 2. Decide
    // -----------------------------------------------------------------------

    /// Everything a round decides, written in one write.
    fn decide(&self, branch: &str) -> Result<Decided> {
        let mut line = self.read_line(branch)?;
        if line.doc.landing.is_some() {
            bail!("{branch} has a landing left unfinished");
        }
        let now = self.now();
        // Design D2: before any run is read, and before a round that holds
        // the branch returns.
        self.drop_withdrawn(&mut line)?;
        let Some(tip) = self.seams.branch.read(branch)? else {
            bail!("{branch} does not exist");
        };
        // 121: records written before the revision a read must contain take
        // the log's last landing, or `base` when `base` lacks it: a branch
        // removed and made again, whose log holds the removed one's
        // landings, which bind no reading of it.
        if line.doc.landed.is_none() {
            line.doc.landed = match (line.last_landed()?, &line.doc.base) {
                (Some(last), Some(base)) if !self.seams.branch.contains(base, &last)? => {
                    Some(base.clone())
                }
                (last, _) => last,
            };
        }
        // 121: a read lacking the landing the branch must contain is a
        // branch whose reading and records disagree. Nothing is polled,
        // formed, started or landed, and `base` stays the revision last
        // formed on, so a landing put back keeps every candidate (10, 17).
        if let Some(lacks) = line.doc.landed.clone() {
            if tip != lacks && !self.seams.branch.contains(&tip, &lacks)? {
                let hold = line.doc.held.as_ref();
                if hold.is_none_or(|h| h.read != tip || h.lacks != lacks) {
                    line.log(
                        &now,
                        Event::Held {
                            read: tip.clone(),
                            lacks: lacks.clone(),
                        },
                    );
                    line.doc.held = Some(BranchHold {
                        read: tip,
                        lacks,
                        since: now,
                    });
                }
                return Ok(match self.write(&line)? {
                    Written::Written(_) => Decided::Held,
                    Written::Refused => Decided::Contended,
                });
            }
        }
        line.doc.held = None;
        // 124: the copy follows the landing this read holds, recorded in
        // this round's write when it is left behind.
        if let Some(landed) = line.doc.landed.clone() {
            let levelled = self.level_copy(branch, &landed)?;
            self.note_copy(&mut line, &landed, &levelled, &now);
        }
        if line.doc.base.as_ref() != Some(&tip) {
            // Moved by something other than this branch's landing (S8), or
            // never read: every candidate is formed again on it (10, 24).
            if let Some(old) = line.doc.base.clone() {
                line.log(
                    &now,
                    Event::Moved {
                        from: old,
                        to: tip.clone(),
                    },
                );
            }
            line.doc.base = Some(tip.clone());
            line.doc.verified = false;
            // 70, 140: nor names one (record 0083; rule core.39).
            line.doc.covered_by = None;
        }
        let rules = self.gate(branch, &tip)?;
        let home = self.home_system(&tip)?;
        let listed = self.key_listed(branch, &tip, &rules)?;
        let unmet = Unmet::of(self.policy_at(&tip)?.map(|p| p.limits).unwrap_or_default());
        // 71 (rule core.49): the room held for each run marked starting,
        // from this count to its start; given back where the write fails.
        let mut parts = Parts::new();
        // A new branch starts at its widest window (13).
        if line.doc.window == 0 {
            line.doc.window = rules.parallel;
        }
        line.doc.window = line.doc.window.clamp(1, rules.parallel);

        let adopted = self.adopt_left_behind(&mut line, &now)?;
        // 82: heads submitted from other hosts, fetched to be formed here;
        // and the heads of submissions out of the queue removed.
        self.seams.state.fetch_heads(branch)?;
        // A candidate another lead formed is fetched to be tested here.
        self.seams.state.fetch_candidates(branch)?;
        let queued: BTreeSet<&Rev> = line
            .subs
            .values()
            .filter(|s| !s.state.is_out())
            .map(|s| &s.head)
            .collect();
        let gone: BTreeSet<Rev> = line
            .subs
            .values()
            .filter(|s| s.state.is_out() && !queued.contains(&s.head))
            .map(|s| s.head.clone())
            .collect();
        for head in &gone {
            self.seams.state.unpublish_head(branch, head)?;
        }
        self.poll(
            &mut line,
            &rules,
            &unmet.limits,
            home.as_deref(),
            listed,
            &now,
        )?;
        // 122: work the branch already holds, landed by a landing this
        // queue did not make, is landed there; nothing is formed of it.
        self.land_found(&mut line, &tip, &now)?;
        // 140: work no required check reads lands beside the chain, before
        // anything is formed; a round that lands so does nothing else, and
        // the next forms on the revision it made.
        let beside = info_span!("sluice.beside")
            .in_scope(|| self.choose_beside(&mut line, &rules, &tip, &now))?;
        if !beside {
            let chain = loop {
                let (chain, removed) = self.arrange(&mut line, &rules, &now)?;
                if !removed {
                    break chain;
                }
            };
            if !self.choose_landing(&mut line, &rules, &chain, &now)? {
                parts = self.choose_starts(
                    &mut line,
                    &rules,
                    home.as_deref(),
                    listed,
                    &unmet,
                    &chain,
                    &now,
                )?;
                self.launch_on_pools(
                    &mut line,
                    &rules,
                    home.as_deref(),
                    listed,
                    &unmet,
                    &chain,
                    &now,
                )?;
                self.note_declined(&mut line, &rules, home.as_deref(), listed, &unmet, &now);
            }
            derive_states(&mut line, &chain);
        }
        self.keep_tasks(&line)?;
        let queued = line.subs.values().filter(|s| !s.state.is_out()).count() as u64;
        let written = matches!(self.write(&line)?, Written::Written(_));
        if written && !adopted.is_empty() {
            self.seams.state.forget_left_behind(branch, &adopted)?;
        }
        if written {
            info!(gauge.sluice.queue.depth = queued, sluice.branch = branch);
            info!(
                gauge.sluice.window = line.doc.window as u64,
                sluice.branch = branch
            );
        }
        Ok(if written {
            Decided::Written(unmet, parts)
        } else {
            Decided::Contended
        })
    }

    /// 122: every submission in the queue, not landing and in no candidate
    /// formed on `base`, whose work `base` already holds, as another
    /// queue's landing or a hand's left it there, is landed as the commit
    /// that landed it, logged as found so; its outcome is delivered as any
    /// landing's (51), and nothing is formed, run or landed of it (6, 22).
    /// Only the submissions a round is about to form are read, once each.
    fn land_found(&self, line: &mut Line, base: &Rev, now: &str) -> Result<()> {
        let unformed: Vec<(u64, Rev)> = line
            .subs
            .values()
            .filter(|s| !s.state.is_out() && s.state != State::Landing)
            .filter(|s| {
                !line
                    .doc
                    .candidates
                    .iter()
                    .any(|c| c.on == *base && c.submissions.contains(&s.id))
            })
            .map(|s| (s.id, s.head.clone()))
            .collect();
        let mut found: BTreeMap<Rev, Vec<u64>> = BTreeMap::new();
        for (id, head) in unformed {
            if let Some(to) = self.seams.branch.landed_by(base, &head)? {
                found.entry(to).or_default().push(id);
            }
        }
        for (to, ids) in found {
            for &id in &ids {
                let s = line.sub_mut(id);
                s.state = State::Landed;
                s.landed_as = Some(to.clone());
                s.landed_elsewhere = true;
                s.held = None;
                line.push_outcome(
                    OutboxItem {
                        id: format!("{id}:landed"),
                        submission: id,
                        outcome: DecidedRecord::Landed {
                            landed_as: to.clone(),
                            beside: false,
                            covered_by: None,
                        },
                    },
                    now,
                    self.peer.expire_after,
                )?;
            }
            line.log(now, Event::FoundLanded { ids, to });
        }
        Ok(())
    }

    /// Design D1, D2 (25, 32, 81; rules core.10, core.16, core.23): drop
    /// every combination under test that holds a withdrawn submission,
    /// whoever withdrew it, from `line` as read: obsolete, it is open to no
    /// claim, and the places of this host's runs of it are listed to
    /// release, which ends each run before anything is started or claimed.
    /// None of its runs is read, finished or not (D3; rule core.7). A
    /// combination whose entries ahead landed holds no withdrawn submission,
    /// and stands (rule core.17). Another host's run of it is that host's
    /// to end (81).
    fn drop_withdrawn(&self, line: &mut Line) -> Result<()> {
        let subs = &line.subs;
        let obsolete: BTreeSet<String> = line
            .doc
            .candidates
            .iter()
            .filter(|c| c.state.under_test())
            .filter(|c| {
                c.submissions
                    .iter()
                    .any(|id| subs.get(id).is_some_and(|s| s.state == State::Withdrawn))
            })
            .map(|c| c.id.clone())
            .collect();
        if obsolete.is_empty() {
            return Ok(());
        }
        let mut release = Vec::new();
        line.doc.candidates.retain(|c| {
            let keep = !obsolete.contains(&c.id);
            if !keep {
                release.extend(
                    c.runs
                        .iter()
                        .filter(|r| self.mine(r))
                        .filter_map(|r| r.place.clone()),
                );
            }
            keep
        });
        for id in &obsolete {
            self.seams.state.unpublish_candidate(&line.branch, id)?;
        }
        line.doc.release.extend(release);
        Ok(())
    }

    /// Design D5 (137; the obsolete-combinations spec): what a hand-over of
    /// `branch`'s pull requests to another host ends in its own pass,
    /// whether or not a round follows: the combinations its withdrawals made
    /// obsolete dropped, as a round drops them before it reads any run, and
    /// the places listed to release released, ending this host's runs of
    /// them. No round: no run is read, and nothing formed, started or
    /// landed. A branch whose lock another step holds is that step's, which
    /// began after the withdrawals and drops them itself (rule core.14).
    /// Each release not plainly released is returned (123).
    pub fn drop_obsolete(&self, branch: &str) -> Result<Vec<ReleaseOutcome>> {
        let Some(_lock) = self.seams.state.lock(branch)? else {
            return Ok(Vec::new());
        };
        let mut line = self.read_line(branch)?;
        let held = line.doc.candidates.len();
        self.drop_withdrawn(&mut line)?;
        if line.doc.candidates.len() != held {
            if let Written::Refused = self.write(&line)? {
                // 62: written since; the next round reads it again.
                return Ok(Vec::new());
            }
        } else if line.doc.release.is_empty() {
            return Ok(Vec::new());
        }
        let mut releases = Vec::new();
        self.perform_releases(branch, &mut releases)?;
        Ok(releases)
    }

    /// Design D10: what this host had queued on the branch in its own
    /// records, left behind when another host's queue reached the remote
    /// first, submitted again under new ids, each naming its old one.
    /// Returns the old ids, for the records seam to forget once written.
    fn adopt_left_behind(&self, line: &mut Line, now: &str) -> Result<Vec<u64>> {
        let left = self.seams.state.left_behind(&line.branch)?;
        let mut adopted = Vec::new();
        for bytes in left {
            let old: crate::records::Submission = serde_json::from_slice(&bytes)?;
            adopted.push(old.id);
            if line.subs.values().any(|s| s.was == Some(old.id)) {
                continue;
            }
            let id = self.seams.state.allocate_id(&line.branch)?;
            let head = old.head.clone();
            line.subs.insert(
                id,
                crate::records::Submission {
                    id,
                    submitted: now.to_string(),
                    after: Vec::new(),
                    attempt: 1,
                    state: State::Queued,
                    tested: None,
                    result: None,
                    failed: Vec::new(),
                    conflict: None,
                    withdrawn: None,
                    landed_as: None,
                    was: Some(old.id),
                    ..old
                },
            );
            line.log(now, Event::Submitted { id, head });
        }
        Ok(adopted)
    }

    /// ADR 0028: keep the tasks of each run still under test, as its
    /// runner tells them, and drop those of every candidate no longer under
    /// test. What is kept is what the runner says now. Beside the records,
    /// read by no verdict (rules core.7, core.9): a runner that cannot say
    /// leaves what was kept, and fails no round.
    fn keep_tasks(&self, line: &Line) -> Result<()> {
        let state = &self.seams.state;
        let kept = state.progress(&line.branch)?;
        let mut testing = BTreeSet::new();
        for c in &line.doc.candidates {
            let runs: Vec<&RunHandle> = c
                .runs
                .iter()
                .filter(|r| r.state == RunState::Testing && self.mine(r))
                .filter_map(|r| r.run.as_ref())
                .collect();
            if !c.state.under_test() || runs.is_empty() {
                continue;
            }
            testing.insert(c.id.clone());
            let tasks = runs.iter().try_fold(Vec::new(), |mut all, run| {
                all.extend(self.seams.tests.tasks(run)?);
                anyhow::Ok(all)
            });
            match tasks {
                Ok(tasks) if kept.get(&c.id).map(Vec::as_slice).unwrap_or_default() == tasks => {}
                // A run started again on the candidate says nothing yet: the
                // tasks of the run before go.
                Ok(tasks) if tasks.is_empty() => state.put_progress(&line.branch, &c.id, None)?,
                Ok(tasks) => state.put_progress(&line.branch, &c.id, Some(&tasks))?,
                Err(e) => tracing::warn!(
                    sluice.branch = %line.branch,
                    sluice.candidate = %c.revision,
                    "reading a run's tasks failed: {e:#}"
                ),
            }
        }
        for id in kept.keys().filter(|id| !testing.contains(*id)) {
            state.put_progress(&line.branch, id, None)?;
        }
        Ok(())
    }

    /// Read every run that finished: record each check, pass and fail
    /// alike, before the verdict is read from the records (16, 69; rules
    /// core.7, core.9). A candidate has its verdict once every run of it
    /// has one, over every check on every system its gate names (86).
    fn poll(
        &self,
        line: &mut Line,
        rules: &Rules,
        limits: &Limits,
        home: Option<&str>,
        listed: bool,
        now: &str,
    ) -> Result<()> {
        let pools = match &line.doc.base {
            Some(base) if rules.signed => {
                self.policy_at(base)?.map(|p| p.pools).unwrap_or_default()
            }
            _ => Vec::new(),
        };
        let testing: Vec<Candidate> = line
            .doc
            .candidates
            .iter()
            .filter(|c| c.state.under_test())
            .cloned()
            .collect();
        for c in testing {
            for (i, r) in c.runs.iter().enumerate() {
                if !self.mine(r) {
                    // 77: another instance's claim, open again once its
                    // host has shown no sign of life for the bound.
                    let host = r.host.clone().expect("another's run names its host");
                    if r.state.held() && self.lapsed(&host, now, &pools, rules.remote.as_deref())? {
                        let x = line.candidate_mut(&c.id).expect("polled candidate");
                        let xr = &mut x.runs[i];
                        xr.state = RunState::Open;
                        xr.host = None;
                        xr.place = None;
                        xr.run = None;
                        line.log(
                            now,
                            Event::Lapsed {
                                combination: c.revision.clone(),
                                host,
                            },
                        );
                    }
                    continue;
                }
                if r.state != RunState::Testing {
                    continue;
                }
                let run = r.run.clone().expect("a testing run has a handle");
                match self.seams.tests.progress(&run)? {
                    Progress::Running => {}
                    Progress::Lost => {
                        // 60: no outcome; the checks run again.
                        let x = line.candidate_mut(&c.id).expect("polled candidate");
                        let xr = &mut x.runs[i];
                        xr.state = RunState::Open;
                        xr.run = None;
                        xr.host = None;
                        if let Some(p) = xr.place.take() {
                            line.doc.release.push(p);
                        }
                        line.log(
                            now,
                            Event::Lost {
                                combination: c.revision.clone(),
                                not_run: Vec::new(),
                            },
                        );
                    }
                    Progress::Finished(report) => {
                        let report = self.after_run(&run, limits, report)?;
                        self.finish_run(line, &c, i, &run, report, rules, home, now)?;
                    }
                }
            }
            // 66, 87: a run no executor here takes, for a system this host
            // is not, one it turned down for what it lacks (record 0074) or,
            // on a branch whose records are on its remote, one whose result
            // signed here the gate would not count, is read as not passed
            // once the time limit has gone; each of its checks says who
            // turned it down and why, where any did.
            let x = line.candidate(&c.id).expect("polled candidate").clone();
            if let (Some(limit), Some(opened)) = (rules.timeout, x.opened.as_deref()) {
                let over = seconds_between(opened, now).is_some_and(|s| s >= limit.as_secs_f64());
                if over {
                    let me = self.peer.name.clone();
                    let x = line.candidate_mut(&c.id).expect("polled candidate");
                    for r in x.runs.iter_mut() {
                        if r.state == RunState::Open
                            && (!ours(r, home)
                                || r.declined.contains_key(&me)
                                || !self.counts_here(r, home, rules, listed))
                        {
                            r.state = RunState::TimedOut;
                            let checks = if r.checks.is_empty() {
                                rules.required.clone()
                            } else {
                                r.checks.clone()
                            };
                            let waited = (!r.declined.is_empty()).then(|| waited_for(&r.declined));
                            x.failed.extend(checks.into_iter().map(|task| FailedTask {
                                task,
                                hash: String::new(),
                                log: None,
                                flaky: None,
                                system: r.system.clone(),
                                not_run: waited.clone(),
                                failed_by: Vec::new(),
                                unsigned: None,
                            }));
                        }
                    }
                }
            }
            self.judge(line, &c.id, rules, now)?;
        }
        Ok(())
    }

    /// 114, 139; records 0072 (design D7 of the change
    /// `a-check-its-executor-refused-has-no-outcome`), 0074 (design D5) and
    /// 0097 (design D3): the finished run `run` under `limits`, read with
    /// what became of the host during it and what its executor refused it.
    /// Where a check did not pass in a run its time limit did not end, the
    /// check seam is asked, once, as the report is first taken: first
    /// whether the host's system was switched during the run, whatever the
    /// policy asks of the host; then what the executor reports refusing
    /// the run; then, where the run asks for the container engine, whether
    /// the engine has gone. The first that answers gives the reason, and
    /// none after it is asked: every check that failed, was failed by a
    /// task it depends on or was left without a result has no outcome, with
    /// why, the tasks it was failed by none either, and a check that passed
    /// keeps its pass (84, 149; rule core.35). A report of what was
    /// refused that could not be read is kept on the report, the run read
    /// as one refused nothing and the engine asked as before (design D8).
    /// A run whose checks all passed, or that its time limit ended, is
    /// never looked at, and a required check the runner keeps out of its
    /// CI mode stays missing.
    pub(crate) fn after_run(
        &self,
        run: &RunHandle,
        limits: &Limits,
        mut report: RunReport,
    ) -> Result<RunReport> {
        let unpassed = report.tasks.iter().any(|t| t.outcome == Outcome::Fail)
            || !report.failed_by.is_empty()
            || report.missing.iter().any(|m| !report.kept_out.contains(m));
        if report.timed_out || !unpassed {
            return Ok(report);
        }
        let reason = match self.seams.tests.switched(run)? {
            Some(why) => why,
            None => match self.seams.tests.refused(run)? {
                Refusals::Refused(why) => why,
                refusals => {
                    if let Refusals::Unread(why) = refusals {
                        report.refusals_unread = Some(why);
                    }
                    if !limits.docker {
                        return Ok(report);
                    }
                    let engine = Limits {
                        docker: true,
                        ..Limits::default()
                    };
                    match self.seams.tests.unmet(&engine)? {
                        Some(why) => format!("after the run, {why}"),
                        None => return Ok(report),
                    }
                }
            },
        };
        let (failed, passed) = std::mem::take(&mut report.tasks)
            .into_iter()
            .partition(|t| t.outcome == Outcome::Fail);
        let failed: Vec<sluice_seams::TaskResult> = failed;
        report.tasks = passed;
        let (kept_out, missing) = std::mem::take(&mut report.missing)
            .into_iter()
            .partition(|m| report.kept_out.contains(m));
        let missing: Vec<String> = missing;
        report.missing = kept_out;
        // 149: a task a check was failed by has no outcome either, and is
        // recorded nowhere.
        report.dependencies.clear();
        let failed_by = std::mem::take(&mut report.failed_by);
        report.not_run.extend(
            failed
                .into_iter()
                .map(|t| t.task)
                .chain(failed_by.into_iter().map(|f| f.task))
                .chain(missing)
                .map(|task| NotRun {
                    task,
                    reason: reason.clone(),
                    key: None,
                }),
        );
        Ok(report)
    }

    /// One run's report: its results recorded, then its own verdict read.
    #[allow(clippy::too_many_arguments)]
    fn finish_run(
        &self,
        line: &mut Line,
        c: &Candidate,
        i: usize,
        run: &RunHandle,
        mut report: sluice_seams::RunReport,
        rules: &Rules,
        home: Option<&str>,
        now: &str,
    ) -> Result<()> {
        let r = &c.runs[i];
        // Design D7 (114; record 0105): why its last run had no outcome
        // goes once its next report is read, whatever that says.
        if let Some(xr) = line.candidate_mut(&c.id).and_then(|x| x.runs.get_mut(i)) {
            xr.lost = None;
        }
        let system = r.system.as_deref().or(home);
        let span = info_span!(
            "sluice.verdict",
            sluice.branch = %line.branch,
            sluice.candidate = %c.revision,
            sluice.submissions = ?c.submissions,
            sluice.executor = %report.executor.kind,
            sluice.confined = report.executor.confined,
            sluice.system = system.unwrap_or(""),
            sluice.outcome = "",
            sluice.ran = 0u64,
            sluice.replayed = 0u64,
        );
        let _span = span.enter();
        let mut results = self.results(&line.branch)?;
        let mut fresh = Vec::new();
        // Design D9 (C.6): on a branch whose records are on its remote, a
        // result is recorded signed, and only for a key the candidate's
        // tree bears out, a task a check was failed by as a check (149).
        let (proven, refused): (Option<Vec<String>>, Vec<sluice_seams::Refused>) = if rules.signed {
            let proof = self.seams.tests.proven(run, &report)?;
            // 114, 16, record 0105 (design D5, D6 of the change
            // `a-pass-nothing-proves-blames-no-one`; rule core.35): a pass
            // the run kept no proof of has no outcome. It leaves the
            // checks the run judged for those it did not, with the
            // profile's words, before anything is recorded, so nothing is
            // recorded under its key and no verdict reads it; the reading
            // of what the run left not run does the rest. Its cause is
            // noted on the host that read the run, which ran it, before
            // anything returns early.
            let proved = report.tasks.iter().any(|t| {
                matches!(t.source, Source::Ran { .. })
                    && t.outcome == Outcome::Pass
                    && proof.proven.contains(&t.task)
            });
            for event in proof::note(
                &mut line.doc.proof_missing,
                &self.peer.name,
                &proof.unkept,
                proved,
                now,
            ) {
                line.log(now, event);
            }
            report
                .tasks
                .retain(|t| !proof.unkept.iter().any(|n| n.task == t.task));
            report.not_run.extend(proof.unkept);
            (Some(proof.proven), proof.refused)
        } else {
            (None, Vec::new())
        };
        // 149, 16: the run of a task a check was failed by is recorded
        // under its own key as any check's run is (rule core.7).
        for t in report.tasks.iter().chain(&report.dependencies) {
            if proven.as_ref().is_some_and(|p| !p.contains(&t.task)) {
                continue;
            }
            let event = match &t.source {
                Source::Ran { host } => Event::Ran {
                    combination: c.revision.clone(),
                    task: t.task.clone(),
                    hash: t.hash.clone(),
                    outcome: t.outcome,
                    host: host.clone(),
                },
                Source::Replayed { from_run_at } => Event::Replayed {
                    combination: c.revision.clone(),
                    task: t.task.clone(),
                    hash: t.hash.clone(),
                    outcome: t.outcome,
                    from_run_at: from_run_at.clone(),
                },
            };
            let l = ResultLine::of(
                now,
                t,
                &report.executor,
                &line.branch,
                &c.revision,
                None,
                system,
            );
            if !results.iter().any(|x| x.same_as(&l)) {
                fresh.push(l);
            }
            line.log(now, event);
        }
        // Clause 119: what the runner said of the run, kept for the
        // report, and, 139 (design D8), why its executor could not read
        // what it refused it; no verdict reads either.
        let detail = report.detail.clone().unwrap_or_default();
        if !detail.is_empty() || !report.kept_out.is_empty() || report.refusals_unread.is_some() {
            line.log(
                now,
                Event::Detail {
                    combination: c.revision.clone(),
                    system: system.map(str::to_string),
                    detail,
                    kept_out: report.kept_out.clone(),
                    refusals_unread: report.refusals_unread.clone(),
                },
            );
        }
        if !fresh.is_empty() {
            let mut bytes = Vec::new();
            for l in &fresh {
                bytes.extend(crate::records::to_line(l));
            }
            self.seams.state.record_results(&bytes)?;
            if rules.signed {
                // 78: what counts is what the records hold signed.
                results = self.results(&line.branch)?;
            } else {
                results.extend(fresh);
            }
        }
        // Design D9: a check, or a task a check was failed by, whose key
        // the tree did not bear out was signed by no one, so it is no pass
        // (15, 149): the verdict reads it failed under that key.
        let mut v = verdict::read(&report, &results, rules, system, home, proven.as_deref());
        // C.6, 20 (design D5 of the change
        // `a-signed-run-without-a-sandbox-proves-its-passes`): a check
        // failed under a key the check profile refused says why, in the
        // profile's words, on the submission blamed for it.
        for f in &mut v.failed {
            if let Some(r) = refused.iter().find(|r| r.task == f.task && r.key == f.hash) {
                f.unsigned = Some(r.why.clone());
            }
        }
        // 108: a replay no trusted pass proved counts for nothing, and the
        // checks run again with no replay.
        if !v.unproven.is_empty() && !r.no_replay {
            let x = line.candidate_mut(&c.id).expect("polled candidate");
            let xr = &mut x.runs[i];
            xr.state = RunState::Open;
            xr.run = None;
            xr.host = None;
            xr.no_replay = true;
            if let Some(p) = xr.place.take() {
                line.doc.release.push(p);
            }
            line.log(
                now,
                Event::Unproven {
                    combination: c.revision.clone(),
                    tasks: v.unproven.iter().map(|u| u.task.clone()).collect(),
                    keys: v.unproven.clone(),
                },
            );
            return Ok(());
        }
        // 87, S18: checks whose results this host signed with a key the
        // branch's policy does not list are missing, never failed. With
        // nothing failed on a result the gate counts, the run is open again
        // for a host whose results count, and the candidate's time limit
        // reads it not passed where none takes it (66).
        if !v.missing.is_empty() && v.failed.is_empty() && !v.timed_out {
            let x = line.candidate_mut(&c.id).expect("polled candidate");
            let xr = &mut x.runs[i];
            xr.state = RunState::Open;
            xr.run = None;
            xr.host = None;
            if let Some(p) = xr.place.take() {
                line.doc.release.push(p);
            }
            line.log(
                now,
                Event::Uncounted {
                    combination: c.revision.clone(),
                    tasks: v.missing.clone(),
                    host: self.peer.name.clone(),
                },
            );
            return Ok(());
        }
        let checks = if r.checks.is_empty() {
            &rules.required
        } else {
            &r.checks
        };
        // 70, 55: a revision with no verdict runs every required check; a
        // report holding none of them is no pass.
        if c.every_check && !checks.is_empty() && report.tasks.is_empty() {
            v.passed = false;
        }
        // 149, 114: a check every task it was failed by reads passed was
        // judged by nothing, and is read as one the run did not judge.
        let not_run: Vec<NotRun> = report.not_run.iter().chain(&v.unjudged).cloned().collect();
        if !not_run.is_empty() {
            let judged = !v.failed.is_empty() || v.timed_out;
            let host = &self.peer.name;
            let Some(named) = read_not_run(line, c, i, &not_run, judged, system, host, now) else {
                return Ok(());
            };
            v.passed = false;
            v.failed.extend(named);
        }
        // 20: what the failed tasks printed, kept while the place still
        // holds it; a check failed by a task it depends on never ran, and
        // printed nothing of its own (149).
        if !v.passed && !v.failed.is_empty() {
            let tasks: Vec<String> = v
                .failed
                .iter()
                .filter(|f| f.failed_by.is_empty())
                .map(|f| f.task.clone())
                .collect();
            let logs = self.seams.tests.logs(run, &tasks)?;
            let key = match system {
                Some(s) if c.runs.len() > 1 => format!("{}-{s}", c.id),
                _ => c.id.clone(),
            };
            let kept = self.seams.state.keep_logs(&line.branch, &key, &logs)?;
            for f in &mut v.failed {
                f.log = kept
                    .iter()
                    .find(|k| k.task == f.task)
                    .map(|k| k.text.clone());
            }
            // ADR 0022: the output, for telemetry to send as its level
            // says; the kept copy is the record.
            for l in &logs {
                let kept = kept.iter().find(|k| k.task == l.task);
                let replayed = report
                    .tasks
                    .iter()
                    .any(|t| t.task == l.task && matches!(t.source, Source::Replayed { .. }));
                tracing::event!(
                    target: OUTPUT,
                    tracing::Level::INFO,
                    sluice.branch = %line.branch,
                    sluice.candidate = %c.revision,
                    sluice.submissions = ?c.submissions,
                    sluice.task = %l.task,
                    sluice.kept = kept.map(|k| k.text.as_str()).unwrap_or(""),
                    sluice.bytes = l.text.len() as u64,
                    sluice.replayed = replayed,
                    "{}",
                    l.text
                );
            }
        }
        // 19, 20: where the run a flaky task contradicts is kept, when the
        // records name it; a pointer, so one that cannot be read holds up
        // no verdict. This run's time and host are those of its own line,
        // recorded before the verdict was read (rule core.7): the newest of
        // the kind it reported for the task on this combination and
        // system, the one written at `now` or the same result written
        // before it; neither for a result with no line.
        for f in &mut v.failed {
            if let Some(k) = &mut f.flaky {
                let kind = report
                    .tasks
                    .iter()
                    .chain(&report.dependencies)
                    .find(|t| t.task == f.task && t.hash == f.hash)
                    .map(|t| match t.source {
                        Source::Ran { .. } => ResultKind::Ran,
                        Source::Replayed { .. } => ResultKind::Replayed,
                    });
                let this = results
                    .iter()
                    .filter(|l| {
                        Some(l.kind) == kind
                            && l.task == f.task
                            && l.hash == f.hash
                            && l.branch == line.branch
                            && l.combination == c.revision
                            && l.worktree.is_none()
                            && l.system.as_deref() == system
                    })
                    .max_by(|a, b| a.at.cmp(&b.at));
                k.at = this.map(|l| l.at.clone());
                k.host = this.and_then(|l| l.host.clone());
                k.other.log = match self.kept_log(line, &f.task, &k.other) {
                    Ok(log) => log,
                    Err(e) => {
                        tracing::warn!(
                            sluice.branch = %line.branch,
                            sluice.candidate = %c.revision,
                            "reading where a flaky task's other run is kept failed: {e:#}"
                        );
                        None
                    }
                };
            }
        }
        let word = if v.passed {
            "passed"
        } else if v.timed_out {
            "timed out"
        } else {
            "failed"
        };
        span.record("sluice.outcome", word);
        span.record("sluice.ran", v.ran as u64);
        span.record("sluice.replayed", v.replayed as u64);
        let branch = line.branch.clone();
        info!(
            monotonic_counter.sluice.tasks.ran = v.ran as u64,
            sluice.branch = branch.as_str()
        );
        info!(
            monotonic_counter.sluice.tasks.replayed = v.replayed as u64,
            sluice.branch = branch.as_str(),
        );
        let x = line.candidate_mut(&c.id).expect("polled candidate");
        let xr = &mut x.runs[i];
        xr.state = if v.passed {
            RunState::Passed
        } else if v.timed_out {
            RunState::TimedOut
        } else {
            RunState::Failed
        };
        let place = xr.place.clone();
        x.failed.extend(v.failed);
        x.ran += v.ran;
        x.replayed += v.replayed;
        if let Some(p) = place {
            line.doc.release.push(p);
        }
        Ok(())
    }

    /// A candidate's verdict, once every run of it has its own: passed
    /// only when every run passed (86, 15).
    fn judge(&self, line: &mut Line, id: &str, rules: &Rules, now: &str) -> Result<()> {
        let c = line.candidate(id).expect("judged candidate").clone();
        if c.runs.is_empty() || !c.runs.iter().all(|r| r.state.done()) {
            return Ok(());
        }
        let state = if c.runs.iter().any(|r| r.state == RunState::Failed) {
            CandidateState::Failed
        } else if c.runs.iter().any(|r| r.state == RunState::TimedOut) {
            CandidateState::TimedOut
        } else {
            CandidateState::Passed
        };
        // 82: a candidate with a verdict is open to no claim.
        self.seams.state.unpublish_candidate(&line.branch, id)?;
        let x = line.candidate_mut(id).expect("judged candidate");
        x.state = state;
        x.passed_at = (state == CandidateState::Passed).then(|| now.to_string());
        let word = x.result_word().to_string();
        let branch = line.branch.clone();
        let branch = branch.as_str();
        info!(
            monotonic_counter.sluice.verdicts = 1u64,
            sluice.branch = branch,
            sluice.outcome = word.as_str(),
        );
        if tracing::enabled!(tracing::Level::INFO) {
            if let Some(secs) = self.tested_for(branch, &c.revision, now)? {
                info!(
                    histogram.sluice.candidate.tested_for = secs,
                    sluice.branch = branch,
                    sluice.outcome = word.as_str(),
                );
            }
        }
        line.log(
            now,
            Event::Result {
                combination: c.revision.clone(),
                result: word,
                failed: c.failed.iter().map(|f| f.task.clone()).collect(),
            },
        );
        if state == CandidateState::Passed && Some(&c.on) == line.doc.base.as_ref() {
            line.doc.window = (line.doc.window + 1).min(rules.parallel);
        }
        Ok(())
    }

    /// Order the queue into the branch's chain, form every candidate of
    /// it, drop candidates it does not hold, and attribute failures.
    /// Returns the chain's candidates, and whether a submission was
    /// removed, in which case the chain is arranged again (8, 12, 21).
    fn arrange(&self, line: &mut Line, rules: &Rules, now: &str) -> Result<(Vec<String>, bool)> {
        let base = line.doc.base.clone().expect("decide reads the base first");
        let ord = line.order(Some(&base), false);
        let mut removed = false;

        // Form (10, 12): reuse a candidate formed on this revision with
        // exactly these submissions, form the rest.
        let mut prev = base.clone();
        let mut prefix: Vec<u64> = Vec::new();
        let mut ahead: Vec<(u64, Rev)> = Vec::new();
        let mut ids: Vec<String> = Vec::new();
        for &id in &ord.chain {
            prefix.push(id);
            let reused = line
                .doc
                .candidates
                .iter()
                .find(|c| c.on == base && c.submissions == prefix);
            // 140: one whose carried work no longer forms on it is
            // obsolete, formed again and never landed without that work.
            let reused = match reused {
                Some(c) if self.carried(c)?.is_none() => None,
                found => found,
            };
            if let Some(c) = reused {
                prev = c.revision.clone();
                ids.push(c.id.clone());
                ahead.push((id, prev.clone()));
                continue;
            }
            let head = line.sub(id)?.head.clone();
            let _span = info_span!("sluice.form", sluice.submission = id).entered();
            match self.seams.branch.form(&prev, &head)? {
                Formed::Revision(rev) => {
                    let cid = format!("c{}", line.doc.next_candidate);
                    line.doc.next_candidate += 1;
                    line.doc.candidates.push(Candidate {
                        id: cid.clone(),
                        on: base.clone(),
                        submissions: prefix.clone(),
                        revision: rev.clone(),
                        state: CandidateState::Formed,
                        runs: Vec::new(),
                        opened: None,
                        failed: Vec::new(),
                        every_check: false,
                        ran: 0,
                        replayed: 0,
                        not_run_runs: 0,
                        passed_at: None,
                        carries: Vec::new(),
                    });
                    prev = rev.clone();
                    ids.push(cid);
                    ahead.push((id, rev));
                }
                Formed::Conflict { paths } => {
                    prefix.pop();
                    let work = |id| line.sub(id).map(|s| (s.head.clone(), s.from.clone()));
                    let with = self.contested(&work, &base, &ahead, &head)?;
                    self.remove_conflict(line, id, paths, with, now)?;
                    removed = true;
                }
            }
        }

        // Drop every candidate the chain does not hold: obsolete (10, 21,
        // 24).
        let held: BTreeSet<&String> = ids.iter().collect();
        let mut release = Vec::new();
        let mut dropped = Vec::new();
        line.doc.candidates.retain(|c| {
            let keep = held.contains(&c.id);
            if !keep && c.state.under_test() {
                release.extend(
                    c.runs
                        .iter()
                        .filter(|r| self.mine(r))
                        .filter_map(|r| r.place.clone()),
                );
                dropped.push(c.id.clone());
            }
            keep
        });
        for id in dropped {
            self.seams.state.unpublish_candidate(&line.branch, &id)?;
        }
        line.doc.release.extend(release);

        // Attribute (17, 18): the newest submission of the first failed
        // candidate, once every candidate ahead of it has a verdict.
        for cid in &ids {
            let c = line.candidate(cid).expect("a chain's candidate").clone();
            if c.state == CandidateState::Passed {
                continue;
            }
            if c.state.failed() {
                self.blame(line, &c, rules, now)?;
                removed = true;
            }
            break;
        }
        Ok((ids, removed))
    }

    /// 65: every submission ahead the head conflicts with alone, or, when
    /// none does, every one ahead up to the one at which the conflict
    /// appeared. `ahead` pairs each submission ahead with the candidate
    /// revision through it; `work` gives a submission's head and who
    /// submitted it, by its number, from the records or a reader's read of
    /// them (64).
    pub(crate) fn contested(
        &self,
        work: &dyn Fn(u64) -> Result<(Rev, Submitter)>,
        base: &Rev,
        ahead: &[(u64, Rev)],
        head: &Rev,
    ) -> Result<Vec<Contested>> {
        let conflicts = |onto: &Rev| -> Result<bool> {
            Ok(matches!(
                self.seams.branch.form(onto, head)?,
                Formed::Conflict { .. }
            ))
        };
        let mut ids = Vec::new();
        if !conflicts(base)? {
            for (id, _) in ahead {
                let alone = self.seams.branch.form(base, &work(*id)?.0)?;
                if let Formed::Revision(r) = alone {
                    if conflicts(&r)? {
                        ids.push(*id);
                    }
                }
            }
            if ids.is_empty() {
                for (n, (_, rev)) in ahead.iter().enumerate() {
                    if conflicts(rev)? {
                        ids = ahead[..=n].iter().map(|(i, _)| *i).collect();
                        break;
                    }
                }
            }
        }
        ids.into_iter()
            .map(|id| {
                Ok(Contested {
                    id,
                    from: work(id)?.1,
                })
            })
            .collect()
    }

    fn remove_conflict(
        &self,
        line: &mut Line,
        id: u64,
        paths: Vec<String>,
        with: Vec<Contested>,
        now: &str,
    ) -> Result<()> {
        info!(monotonic_counter.sluice.conflicts = 1u64, sluice.branch = %line.branch);
        let s = line.sub_mut(id);
        s.state = State::Conflict;
        s.conflict = Some(ConflictRecord {
            paths: paths.clone(),
            with: with.clone(),
        });
        line.log(
            now,
            Event::Conflict {
                id,
                paths: paths.clone(),
                with: with.iter().map(|c| c.id).collect(),
            },
        );
        line.push_outcome(
            OutboxItem {
                id: format!("{id}:conflict"),
                submission: id,
                outcome: DecidedRecord::Conflict { paths, with },
            },
            now,
            self.peer.expire_after,
        )
    }

    /// 17, 20, 21, 13: blame the newest submission of a failed
    /// candidate, tell its session alone, and test fewer at once.
    fn blame(&self, line: &mut Line, c: &Candidate, rules: &Rules, now: &str) -> Result<()> {
        let id = c.newest();
        let _span = info_span!(
            "sluice.blame",
            sluice.branch = %line.branch,
            sluice.submission = id,
            sluice.candidate = %c.revision,
        )
        .entered();
        info!(monotonic_counter.sluice.culprits = 1u64, sluice.branch = %line.branch);
        let s = line.sub_mut(id);
        s.state = State::Failed;
        s.tested = Some(Tested {
            combination: c.revision.clone(),
            on: c.on.clone(),
            submissions: c.submissions.clone(),
        });
        s.result = Some(c.result_word().into());
        s.failed = c.failed.clone();
        line.log(
            now,
            Event::Blamed {
                id,
                combination: c.revision.clone(),
            },
        );
        line.push_outcome(
            OutboxItem {
                id: format!("{id}:failed"),
                submission: id,
                outcome: DecidedRecord::Failed {
                    failed: c.failed.iter().map(|f| f.task.clone()).collect(),
                    combination: c.revision.clone(),
                    submissions: c.submissions.clone(),
                    timed_out: c.state == CandidateState::TimedOut,
                    logs: c
                        .failed
                        .iter()
                        .filter_map(|f| {
                            f.log.clone().map(|at| TaskLog {
                                task: f.task.clone(),
                                text: at,
                            })
                        })
                        .collect(),
                },
            },
            now,
            self.peer.expire_after,
        )?;
        line.doc.window = (line.doc.window / 2).clamp(1, rules.parallel);
        Ok(())
    }

    /// 140, 23: the revision whose tree `c` lands as: its tested revision
    /// with the work landed beside it since it was formed, in order; `None`
    /// when that work no longer forms on it.
    fn carried(&self, c: &Candidate) -> Result<Option<Rev>> {
        let mut tree = c.revision.clone();
        for head in &c.carries {
            match self.seams.branch.form(&tree, head)? {
                Formed::Revision(r) => tree = r,
                Formed::Conflict { .. } => return Ok(None),
            }
        }
        Ok(Some(tree))
    }

    /// 140: land beside the chain each queued entry in no candidate whose
    /// work no required check reads, at the branch's revision `base`,
    /// covered by a verdict (70), or at any candidate formed on it as it
    /// would land now; in submission order, each landed one standing as
    /// landed for those after it, so a group's leading heads that qualify
    /// land in the act's order (125). Each entry read and not landed so
    /// for a candidate's checks or a conflict with an entry ahead is told
    /// which entry it waits behind. Returns whether a landing beside was
    /// written; nothing is run, and no place or room taken (13, 71).
    fn choose_beside(&self, line: &mut Line, rules: &Rules, base: &Rev, now: &str) -> Result<bool> {
        // Only an entry in no candidate, never read so, is read.
        let unread = line.subs.values().any(|s| {
            !s.state.is_out()
                && s.state != State::Landing
                && s.beside.is_none()
                && !line
                    .doc
                    .candidates
                    .iter()
                    .any(|c| c.on == *base && c.submissions.contains(&s.id))
        });
        if !line.doc.verified || !unread {
            return Ok(false);
        }
        // 23, 70, 140: a revision whose covering verdict the records do not
        // name, as an older sluice left it, lands nothing beside: its work
        // takes the chain, whose landing names one (record 0083; rule
        // core.39).
        let Some(covering) = line.doc.covered_by.clone() else {
            return Ok(false);
        };
        let Some(policy) = self.policy_at(base)? else {
            return Ok(false);
        };
        let formed: Vec<&Candidate> = {
            let mut c: Vec<&Candidate> = line
                .doc
                .candidates
                .iter()
                .filter(|c| c.on == *base)
                .collect();
            c.sort_by_key(|c| c.submissions.len());
            c
        };
        let in_candidate: BTreeSet<u64> = formed
            .iter()
            .flat_map(|c| c.submissions.iter().copied())
            .collect();
        let mut reading: Vec<(u64, Rev)> = Vec::new();
        for c in &formed {
            match self.carried(c)? {
                Some(tree) => reading.push((c.newest(), tree)),
                // Formed again before anything lands beside it.
                None => return Ok(false),
            }
        }
        let landed = line.landed();
        let live: Vec<crate::records::Submission> = line
            .subs
            .values()
            .filter(|s| !s.state.is_out() && s.state != State::Landing)
            .cloned()
            .collect();
        let mut tip = base.clone();
        let mut chosen: Vec<(u64, Rev, Rev)> = Vec::new();
        for sub in &live {
            if in_candidate.contains(&sub.id) || sub.beside.is_some() {
                continue;
            }
            let chosen_ids: BTreeSet<u64> = chosen.iter().map(|c| c.0).collect();
            // 6, 7: an entry whose after-entries have not landed waits; and
            // 125, of an act's heads only the leading ones land beside, each
            // after the one before it.
            let before: Vec<u64> = line
                .subs
                .values()
                .filter(|o| sub.act.is_some() && o.act == sub.act && o.id < sub.id)
                .map(|o| o.id)
                .collect();
            if !sub
                .after
                .iter()
                .chain(&before)
                .all(|a| landed.contains(a) || chosen_ids.contains(a))
            {
                continue;
            }
            let ahead: Vec<&crate::records::Submission> = live
                .iter()
                .filter(|a| a.id != sub.id && !chosen_ids.contains(&a.id))
                .filter(|a| {
                    a.priority > sub.priority || (a.priority == sub.priority && a.id < sub.id)
                })
                .collect();
            let read = self.read_beside(sub, &tip, &reading, &ahead, rules, &policy.tests)?;
            match read {
                Read::Chain => {}
                Read::Behind(behind, why) => {
                    line.sub_mut(sub.id).beside = Some(Beside::Behind { behind, why });
                    info!(
                        sluice.branch = %line.branch,
                        sluice.submission = sub.id,
                        "waits behind #{behind} ({why:?}) beside the queue"
                    );
                }
                Read::Beside { formed, merged } => {
                    let commit = self.seams.branch.commit(&LandingCommit {
                        tree_of: formed.clone(),
                        parent: tip.clone(),
                        head: sub.head.clone(),
                        message: beside_message(
                            &policy.landing_subject,
                            &line.branch,
                            sub,
                            &covering,
                        ),
                    })?;
                    for (r, m) in reading.iter_mut().zip(merged) {
                        r.1 = m;
                    }
                    chosen.push((sub.id, commit.clone(), formed));
                    tip = commit;
                }
            }
        }
        if chosen.is_empty() {
            return Ok(false);
        }
        let ids: Vec<u64> = chosen.iter().map(|c| c.0).collect();
        let mut commits = Vec::new();
        for (id, commit, formed) in chosen {
            let s = line.sub_mut(id);
            s.state = State::Landing;
            s.tested = None;
            s.result = None;
            s.covered_by = Some(covering.clone());
            commits.push(LandingCommitRecord {
                id,
                commit,
                tested: formed,
            });
        }
        line.log(
            now,
            Event::Landing {
                ids: ids.clone(),
                from: base.clone(),
                to: tip.clone(),
            },
        );
        line.doc.landing = Some(LandingIntent {
            ids,
            from: base.clone(),
            to: tip,
            commits,
            beside: true,
            // 23, 140: the branch it lands as is covered by the verdict
            // that covered the revision it landed on (rule core.39).
            covering: Some(covering),
        });
        Ok(true)
    }

    /// 140: how `sub`'s work is read on the branch's revision as it would
    /// be now, `tip`, and on each formed candidate as it would land now,
    /// `formed` (each by its newest entry, in chain order), with the work
    /// of each entry `ahead` of it. A reading that fails takes the chain.
    fn read_beside(
        &self,
        sub: &crate::records::Submission,
        tip: &Rev,
        formed: &[(u64, Rev)],
        ahead: &[&crate::records::Submission],
        rules: &Rules,
        tests: &sluice_seams::Settings,
    ) -> Result<Read> {
        let branch = &self.seams.branch;
        // A conflict on the branch itself is found at forming (12).
        let Formed::Revision(on_tip) = branch.form(tip, &sub.head)? else {
            return Ok(Read::Chain);
        };
        // 56: the policy changes the gate; it takes the chain.
        if branch.read_file(tip, POLICY_PATH)? != branch.read_file(&on_tip, POLICY_PATH)? {
            return Ok(Read::Chain);
        }
        for a in ahead {
            if let Formed::Revision(alone) = branch.form(tip, &a.head)? {
                if let Formed::Conflict { .. } = branch.form(&alone, &sub.head)? {
                    return Ok(Read::Behind(a.id, BesideWhy::Conflict));
                }
            }
        }
        let mut merged = Vec::new();
        for (newest, tree) in formed {
            match branch.form(tree, &sub.head)? {
                Formed::Revision(m) => merged.push(m),
                Formed::Conflict { .. } => {
                    return Ok(Read::Behind(*newest, BesideWhy::Conflict));
                }
            }
        }
        let mut pairs = vec![(tip.clone(), on_tip.clone())];
        pairs.extend(
            formed
                .iter()
                .zip(&merged)
                .map(|((_, t), m)| (t.clone(), m.clone())),
        );
        let reads = match self.seams.tests.reads(&Reading {
            required: rules.required.clone(),
            tests: tests.clone(),
            pairs,
        }) {
            Ok(r) if r.len() == formed.len() + 1 => r,
            Ok(_) => return Ok(Read::Chain),
            Err(e) => {
                warn!(
                    sluice.submission = sub.id,
                    "the affected reading failed; the work takes the queue: {e:#}"
                );
                return Ok(Read::Chain);
            }
        };
        if !reads[0].none() {
            return Ok(Read::Chain);
        }
        if let Some(((newest, _), _)) = formed.iter().zip(&reads[1..]).find(|(_, r)| !r.none()) {
            return Ok(Read::Behind(*newest, BesideWhy::Read));
        }
        Ok(Read::Beside {
            formed: on_tip,
            merged,
        })
    }

    /// 22, 23: land the longest prefix of the chain whose verdicts allow it,
    /// one commit per submission, writing the intent before the branch
    /// moves (24). Returns whether a landing was chosen.
    fn choose_landing(
        &self,
        line: &mut Line,
        rules: &Rules,
        chain: &[String],
        now: &str,
    ) -> Result<bool> {
        let base = line.doc.base.clone().expect("decide reads the base first");
        {
            let cands: Vec<Candidate> = chain
                .iter()
                .map(|id| line.candidate(id).expect("a chain's candidate").clone())
                .collect();
            for s in line.subs.values_mut() {
                s.held = None;
            }
            let n = landable(&cands, rules.batch);
            if n == 0 {
                return Ok(false);
            }
            if let Some(held) = held(&cands, n, rules.hold, now) {
                for c in cands[..n]
                    .iter()
                    .filter(|c| c.state == CandidateState::Passed)
                {
                    line.sub_mut(c.newest()).held = Some(held.clone());
                }
                return Ok(false);
            }
            let deepest = &cands[n - 1];
            // Design D8: on a branch whose records any host may write, a
            // landing reads its verdict again from the signed results.
            if rules.signed {
                let results = self.results(&line.branch)?;
                let judged: BTreeSet<&String> = cands[..n]
                    .iter()
                    .map(|c| {
                        if c.state == CandidateState::Passed {
                            &c.id
                        } else {
                            &deepest.id
                        }
                    })
                    .collect();
                let mut unsigned = Vec::new();
                for id in judged {
                    let c = line.candidate(id).expect("a chain's candidate");
                    if !signed_pass(c, &results, rules) {
                        unsigned.push((c.id.clone(), c.revision.clone()));
                    }
                }
                if !unsigned.is_empty() {
                    for (id, combination) in unsigned {
                        let c = line.candidate_mut(&id).expect("a chain's candidate");
                        c.state = CandidateState::Formed;
                        c.runs.clear();
                        c.opened = None;
                        c.failed.clear();
                        c.passed_at = None;
                        line.log(now, Event::Unsigned { combination });
                    }
                    return Ok(false);
                }
            }
            let subject = self
                .policy_at(&base)?
                .map(|p| p.landing_subject)
                .unwrap_or_else(|| crate::policy::LANDING_SUBJECT.into());
            let mut parent = base.clone();
            let mut commits = Vec::new();
            for c in &cands[..n] {
                // Each names its own verdict, or under deepest first the
                // deepest candidate's, on which they land together (23).
                let verdict = if c.state == CandidateState::Passed {
                    c
                } else {
                    deepest
                };
                let sub = line.sub(c.newest())?.clone();
                // 140, 23: the tested tree with the work landed beside it.
                let tree_of = self.carried(c)?.ok_or_else(|| {
                    anyhow!(
                        "{} no longer forms with the work landed beside it",
                        c.revision
                    )
                })?;
                let commit = self.seams.branch.commit(&LandingCommit {
                    tree_of,
                    parent: parent.clone(),
                    head: sub.head.clone(),
                    message: landing_message(&subject, &line.branch, &sub, verdict),
                })?;
                let s = line.sub_mut(sub.id);
                s.state = State::Landing;
                s.tested = Some(Tested {
                    combination: verdict.revision.clone(),
                    on: verdict.on.clone(),
                    submissions: verdict.submissions.clone(),
                });
                s.result = Some("passed".into());
                s.failed.clear();
                commits.push(LandingCommitRecord {
                    id: sub.id,
                    commit: commit.clone(),
                    tested: verdict.revision.clone(),
                });
                parent = commit;
            }
            // 23, 70, 140: the branch it lands as is covered by the verdict
            // its last commit names, the deepest candidate's, at that commit
            // (core.17; record 0083; rule core.39).
            let covering = Some(CoveringVerdict {
                at: parent.clone(),
                result: deepest.result_word().into(),
                ran: deepest.ran as u64,
                replayed: deepest.replayed as u64,
            });
            let ids: Vec<u64> = commits.iter().map(|c| c.id).collect();
            line.log(
                now,
                Event::Landing {
                    ids: ids.clone(),
                    from: base.clone(),
                    to: parent.clone(),
                },
            );
            line.doc.landing = Some(LandingIntent {
                ids,
                from: base,
                to: parent,
                commits,
                beside: false,
                covering,
            });
            Ok(true)
        }
    }

    /// 71: how many runs of the host's bound are free now: the bound less
    /// the checks running on the host and the parts held for runs about to
    /// start, every repository's of both kinds (session-runs-1 clause 89).
    /// It decides no start, each of which holds its own part (rule
    /// core.49); a lead reads it only to tell whether it would take a run
    /// itself before it asks a pool (107).
    fn free_runs(&self) -> Result<usize> {
        let running = self.seams.tests.running()?.total();
        Ok(self.seams.tests.bound()?.saturating_sub(running))
    }

    /// Session-runs-1 clauses 89 and 91: how many session runs may start
    /// now: the bound less `reserved`, the part kept for submissions'
    /// runs, less the checks running on the host. A session run belongs
    /// to the host, not to a branch an instance serves, so every
    /// repository's runs count against it (71).
    pub fn session_capacity(&self, reserved: usize) -> Result<usize> {
        Ok(self.room(reserved)?.for_sessions())
    }

    /// 71, session-runs-1 clauses 89 and 91: the host's bound and what
    /// counts against it, every repository's runs, as `session_capacity`
    /// decides on it, for a person to read (ADR 0035).
    pub fn room(&self, reserved: usize) -> Result<Room> {
        let running = self.seams.tests.running()?;
        Ok(Room {
            bound: self.seams.tests.bound()?,
            queue: running.queue,
            sessions: running.sessions,
            reserve: reserved,
        })
    }

    /// 13, 14, 71: start candidates up to the window, and their runs this
    /// host takes as far as the host has room; first the runs of
    /// candidates under test opened again (60, 108). Rule core.49: a run is
    /// marked starting only with its part of the host's bound held, before
    /// the decision is written, and no further run is marked once the host
    /// holds none: the parts it holds, by place.
    #[allow(clippy::too_many_arguments)]
    fn choose_starts(
        &self,
        line: &mut Line,
        rules: &Rules,
        home: Option<&str>,
        listed: bool,
        unmet: &Unmet,
        chain: &[String],
        now: &str,
    ) -> Result<Parts> {
        let mut parts = Parts::new();
        let mut full = false;
        let reopened: Vec<(String, usize)> = chain
            .iter()
            .filter_map(|id| line.candidate(id))
            .filter(|c| c.state.under_test())
            .flat_map(|c| {
                c.runs
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| self.claimable(r, home, rules, listed, unmet))
                    .map(|(i, _)| (c.id.clone(), i))
                    .collect::<Vec<_>>()
            })
            .collect();
        for (id, i) in reopened {
            let Some(part) = self.seams.tests.hold(RunKind::Queue)? else {
                full = true;
                break;
            };
            let place = self.next_place(line);
            parts.insert(place.clone(), part);
            let host = rules.signed.then(|| self.peer.name.clone());
            let c = line.candidate_mut(&id).expect("a chain's candidate");
            c.runs[i].state = RunState::Starting;
            c.runs[i].place = Some(place.clone());
            c.runs[i].host = host;
            c.runs[i].declined.remove(&self.peer.name);
            let c = c.clone();
            line.log(
                now,
                Event::Testing {
                    combination: c.revision.clone(),
                    submissions: c.submissions.clone(),
                    place,
                },
            );
        }
        let running = line
            .doc
            .candidates
            .iter()
            .filter(|c| c.state.under_test())
            .count();
        let window = line.doc.window.saturating_sub(running);
        if window == 0 || chain.is_empty() {
            return Ok(parts);
        }
        let mut start: Vec<String> = Vec::new();
        {
            let cands: Vec<&Candidate> = chain
                .iter()
                .map(|id| line.candidate(id).expect("a chain's candidate"))
                .collect();
            let failed = cands.iter().position(|c| c.state.failed());
            let pick: Vec<String> = if !rules.batch {
                cands
                    .iter()
                    .filter(|c| c.state == CandidateState::Formed)
                    .map(|c| c.id.clone())
                    .collect()
            } else if cands.iter().any(|c| c.state.under_test())
                && (rules.hold.is_none() || failed.is_some())
            {
                // 117: only a branch with a hold tests the deepest beside
                // one under test, and never while a failure is found out.
                Vec::new()
            } else if let Some(f) = failed {
                // The deepest failed: test the candidates it skipped (14).
                cands[..f]
                    .iter()
                    .filter(|c| c.state == CandidateState::Formed)
                    .map(|c| c.id.clone())
                    .collect()
            } else {
                // Deepest first, as deep as the window reaches.
                let d = line.doc.window.min(cands.len());
                let c = cands[d - 1];
                if c.state == CandidateState::Formed {
                    vec![c.id.clone()]
                } else {
                    Vec::new()
                }
            };
            start.extend(pick.into_iter().take(window));
        }
        let every_known = line.doc.verified;
        for id in start {
            let mut runs = open_runs(rules, home);
            let mine = runs
                .iter()
                .position(|r| self.claimable(r, home, rules, listed, unmet));
            // A candidate this host would run starts only with room held
            // for it.
            let part = match mine {
                Some(_) if full => break,
                Some(_) => match self.seams.tests.hold(RunKind::Queue)? {
                    Some(part) => Some(part),
                    None => break,
                },
                None => None,
            };
            let place = match mine {
                Some(i) => {
                    let place = self.next_place(line);
                    if let Some(part) = part {
                        parts.insert(place.clone(), part);
                    }
                    runs[i].state = RunState::Starting;
                    runs[i].place = Some(place.clone());
                    runs[i].host = rules.signed.then(|| self.peer.name.clone());
                    place
                }
                None => String::new(),
            };
            if rules.signed {
                // 82: open to claims only once every host can fetch it.
                let rev = line
                    .candidate(&id)
                    .expect("a chain's candidate")
                    .revision
                    .clone();
                self.seams
                    .state
                    .publish_candidate(&line.branch, &id, &rev)?;
            }
            let c = line.candidate_mut(&id).expect("a chain's candidate");
            c.state = CandidateState::Testing;
            c.runs = runs;
            c.opened = Some(now.to_string());
            c.every_check = !every_known;
            let c = c.clone();
            line.sub_mut(c.newest()).tested = Some(Tested {
                combination: c.revision.clone(),
                on: c.on.clone(),
                submissions: c.submissions.clone(),
            });
            line.log(
                now,
                Event::Testing {
                    combination: c.revision.clone(),
                    submissions: c.submissions.clone(),
                    place,
                },
            );
        }
        Ok(parts)
    }

    /// Whether `run` is this instance's: its claim names it, or the
    /// branch keeps no claims (75).
    fn mine(&self, run: &SystemRun) -> bool {
        run.host.as_deref().is_none_or(|h| h == self.peer.name)
    }

    /// 75, 79, 87, 107, 84: whether this instance takes `run`: one it may
    /// take, on a host that meets what the branch's runs ask of it
    /// (record 0074), which is looked at only here, once a round.
    fn claimable(
        &self,
        run: &SystemRun,
        home: Option<&str>,
        rules: &Rules,
        listed: bool,
        unmet: &Unmet,
    ) -> bool {
        self.may_claim(run, home, rules, listed) && self.meets(unmet)
    }

    /// Design D3, D2 (84, 87): whether this host meets what `unmet`'s
    /// limits ask of it, what its executor holds and, where they ask for
    /// one, a container engine that answers, read from the host once a
    /// round. A look that fails says so as what it lacks: the host could
    /// not show it meets them.
    fn meets(&self, unmet: &Unmet) -> bool {
        unmet
            .looked
            .get_or_init(|| match self.seams.tests.unmet(&unmet.limits) {
                Ok(why) => why,
                Err(e) => Some(format!("looking at the host failed: {e:#}")),
            })
            .is_none()
    }

    /// Design D4: name this host, with what the round's look found it
    /// lacks, on each open run it may take, where it has not said so for
    /// that reason already, logging each once.
    fn note_declined(
        &self,
        line: &mut Line,
        rules: &Rules,
        home: Option<&str>,
        listed: bool,
        unmet: &Unmet,
        now: &str,
    ) {
        let Some(why) = unmet.found() else {
            return;
        };
        let me = &self.peer.name;
        let mut said = Vec::new();
        for c in line
            .doc
            .candidates
            .iter_mut()
            .filter(|c| c.state.under_test())
        {
            for r in &mut c.runs {
                if !self.may_claim(r, home, rules, listed)
                    || r.declined.get(me).is_some_and(|w| w == why)
                {
                    continue;
                }
                r.declined.insert(me.clone(), why.to_string());
                said.push(Event::Declined {
                    combination: c.revision.clone(),
                    system: r.system.clone(),
                    host: me.clone(),
                    why: why.to_string(),
                });
            }
        }
        for e in said {
            line.log(now, e);
        }
    }

    /// 75, 79, 87, 107: whether this instance may take `run`: open and
    /// unclaimed, of the system it is, and one whose result signed here
    /// the gate would count ([`Sluice::counts_here`]).
    fn may_claim(&self, run: &SystemRun, home: Option<&str>, rules: &Rules, listed: bool) -> bool {
        run.state == RunState::Open
            && run.host.is_none()
            && self.runs_system(run, home)
            && self.counts_here(run, home, rules, listed)
    }

    /// 78, 79, 87: whether the gate would count `run`'s result signed by
    /// this host: on a branch whose records are on its remote, it signs as
    /// a signer the gate trusts for what the gate needs, and `listed`, the
    /// records' word that the branch's policy at its revision lists the
    /// key it signs with for that signer ([`Sluice::key_listed`]). The
    /// signer's name alone takes nothing. On any other branch, any run.
    fn counts_here(
        &self,
        run: &SystemRun,
        home: Option<&str>,
        rules: &Rules,
        listed: bool,
    ) -> bool {
        if !rules.signed {
            return true;
        }
        let Some(signer) = &self.peer.signer else {
            return false;
        };
        let Some((may_confine, systems)) = rules.claims.get(signer) else {
            return false;
        };
        let system = run.system.as_deref().or(home);
        listed
            && (!rules.confined || *may_confine)
            && match (system, systems) {
                (Some(s), Some(list)) => list.iter().any(|x| x == s),
                _ => true,
            }
    }

    /// 78, 87: whether `branch`'s policy at `tip`, the revision the round
    /// read, lists the key this host signs with for the signer it signs
    /// as, asked of the records, which hold the keys, once where a round
    /// claims. A branch whose records are the host's own files, or a host
    /// that signs nothing, asks nothing.
    fn key_listed(&self, branch: &str, tip: &Rev, rules: &Rules) -> Result<bool> {
        Ok(rules.signed
            && self.peer.signer.is_some()
            && self.seams.state.verifies_own(branch, tip)?)
    }

    /// 87: whether this host runs `run`'s system: any run for no system,
    /// and one for the system it is, or one of those its `[runs] systems`
    /// names.
    fn runs_system(&self, run: &SystemRun, home: Option<&str>) -> bool {
        match (&self.peer.systems, run.system.as_deref()) {
            (_, None) => true,
            (Some(list), Some(s)) => list.iter().any(|x| x == s),
            (None, Some(_)) => ours(run, home),
        }
    }

    /// 87: refuse a host whose `[runs] systems` names a system it is not,
    /// by the policy of any branch with records. A branch whose tip or
    /// policy cannot be read is passed by (26, rule core.14): its own
    /// round reads the same policy once it reads.
    pub fn check_host_systems(&self) -> Result<()> {
        let Some(names) = &self.peer.systems else {
            return Ok(());
        };
        let p = self.seams.tests.platform()?;
        for b in self.seams.state.branches()? {
            let read = self
                .seams
                .branch
                .read(&b)
                .and_then(|tip| tip.map(|tip| self.policy_at(&tip)).transpose());
            let Ok(Some(Some(policy))) = read else {
                continue;
            };
            for n in names {
                if let Some(s) = policy.systems.get(n) {
                    if s.os != p.os || s.arch != p.arch {
                        bail!(
                            "[runs] systems names {n}, which is {} on {}; this host is {} on {}",
                            s.os,
                            s.arch,
                            p.os,
                            p.arch
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// 88, 107: start on a pool each open run of a candidate under test
    /// that no live instance able to take it has room for by its last sign
    /// of life, the policy's pools tried in order; the job's start is its
    /// claim.
    #[allow(clippy::too_many_arguments)]
    fn launch_on_pools(
        &self,
        line: &mut Line,
        rules: &Rules,
        home: Option<&str>,
        listed: bool,
        unmet: &Unmet,
        chain: &[String],
        now: &str,
    ) -> Result<()> {
        if !rules.signed {
            return Ok(());
        }
        let Some(base) = line.doc.base.clone() else {
            return Ok(());
        };
        let Some(policy) = self.policy_at(&base)? else {
            return Ok(());
        };
        if policy.pools.is_empty() {
            return Ok(());
        }
        let mut rooms: Vec<(Option<String>, Option<String>, u64)> = Vec::new();
        for (host, (_, beat)) in self.seams.state.beats()? {
            if host == self.peer.name
                || host.starts_with("pool/")
                || self.lapsed(&host, now, &[], None)?
            {
                continue;
            }
            let v: serde_json::Value = serde_json::from_slice(&beat).unwrap_or_default();
            let room = v["bound"]
                .as_u64()
                .unwrap_or(0)
                .saturating_sub(v["held"].as_u64().unwrap_or(0));
            rooms.push((
                v["signer"].as_str().map(str::to_string),
                v["system"].as_str().map(str::to_string),
                room,
            ));
        }
        let trusted = |signer: &str| {
            rules
                .claims
                .get(signer)
                .is_some_and(|(confined, _)| !rules.confined || *confined)
        };
        let mut lead_room = self.free_runs()?;
        let open: Vec<(String, usize)> = chain
            .iter()
            .filter_map(|id| line.candidate(id))
            .filter(|c| c.state.under_test())
            .flat_map(|c| {
                c.runs
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.state == RunState::Open && r.host.is_none())
                    .map(|(i, _)| (c.id.clone(), i))
                    .collect::<Vec<_>>()
            })
            .collect();
        for (id, i) in open {
            let run = line.candidate(&id).expect("a chain's candidate").runs[i].clone();
            // 107: a live instance that may take it, with room, comes first.
            if self.claimable(&run, home, rules, listed, unmet) && lead_room > 0 {
                lead_room -= 1;
                continue;
            }
            let taker = rooms.iter_mut().find(|(signer, system, room)| {
                *room > 0
                    && signer.as_deref().is_some_and(&trusted)
                    && (run.system.is_none() || *system == run.system)
            });
            if let Some(t) = taker {
                t.2 -= 1;
                continue;
            }
            let pool = policy.pools.iter().find_map(|p| {
                if !trusted(&p.signer) {
                    return None;
                }
                let runner = match &run.system {
                    Some(s) => p.runners.get(s).cloned(),
                    None => p.runners.values().next().cloned(),
                }?;
                Some((p, runner))
            });
            let Some((pool, runner)) = pool else {
                continue;
            };
            let place = self.next_place(line);
            let claim = format!("pool/{}/{}", pool.name, line.doc.next_place - 1);
            let c = line.candidate(&id).expect("a chain's candidate").clone();
            let signer = policy
                .signers
                .get(&pool.signer)
                .map(|s| s.identity.clone())
                .expect("a pool's signer is one the policy names");
            let launched = self.seams.tests.launch(&sluice_seams::Launch {
                pool: pool.name.clone(),
                kind: pool.kind.clone(),
                signer,
                runner,
                remote: rules.remote.clone(),
                branch: line.branch.clone(),
                candidate: c.id.clone(),
                combination: c.revision.clone(),
                system: run.system.clone(),
                claim: claim.clone(),
            });
            // 134: a dispatch whose answer was not read is never sent
            // again: its run is claimed as dispatched, the claim's event
            // carrying the forge's words, and the next round reads whether
            // a job of the claim runs, lapsing it when none does (77, 88).
            let unanswered = match launched {
                Ok(()) => None,
                Err(e) if sluice_seams::unanswered(&e) => Some(format!("{e:#}")),
                Err(e) => return Err(e),
            };
            let x = line.candidate_mut(&id).expect("a chain's candidate");
            let r = &mut x.runs[i];
            r.state = RunState::Starting;
            r.host = Some(claim.clone());
            r.place = Some(place);
            line.log(
                now,
                Event::Claimed {
                    combination: c.revision.clone(),
                    system: run.system.clone(),
                    host: claim,
                    unanswered,
                },
            );
        }
        Ok(())
    }

    /// 77: whether `host` has shown no sign of life for the bound, by this
    /// host's own observation of its beats; a pool's job, by its pool
    /// (88).
    fn lapsed(
        &self,
        host: &str,
        now: &str,
        pools: &[crate::policy::Pool],
        remote: Option<&str>,
    ) -> Result<bool> {
        if let Some(rest) = host.strip_prefix("pool/") {
            let name = rest.split('/').next().unwrap_or_default();
            return match pools.iter().find(|p| p.name == name) {
                Some(p) => Ok(!self.seams.tests.job_alive(&p.kind, host, remote)?),
                // A pool the policy no longer names runs nothing for it.
                None => Ok(true),
            };
        }
        let marker = self
            .seams
            .state
            .beats()?
            .remove(host)
            .map(|(m, _)| m)
            .unwrap_or_default();
        match self.seams.state.observed_beat(host)? {
            Some((m, since)) if m == marker => Ok(
                seconds_between(&since, now).is_some_and(|s| s >= self.peer.alive.as_secs_f64())
            ),
            _ => {
                self.seams.state.observe_beat(host, &marker, now)?;
                Ok(false)
            }
        }
    }

    /// 75, 77, 81: what an instance that does not keep `branch` does for
    /// it: shows it is alive, releases the places it made that no claim of
    /// its own holds ([`Sluice::release_left`], as the lead does too),
    /// reports its runs that ended, and claims open runs it
    /// may take, earliest first, up to its bound, then starts them. 81,
    /// 123: it claims once each place it left has been released, gone or
    /// held, whatever their releases did: a held place bars no claim, and
    /// keeps its ports and disk beside the places the claims are run in.
    /// Only a failure to write the records stops its claims for the round.
    fn execute(&self, branch: &str, releases: &mut Vec<ReleaseOutcome>) -> Result<()> {
        let Some(tip) = self.seams.branch.read(branch)? else {
            return Ok(());
        };
        let Some(rules) = self.rules_at(branch, &tip)? else {
            return Ok(());
        };
        if !rules.signed {
            return Ok(());
        }
        let home = self.home_system(&tip)?;
        let listed = self.key_listed(branch, &tip, &rules)?;
        let unmet = Unmet::of(self.policy_at(&tip)?.map(|p| p.limits).unwrap_or_default());
        let me = self.peer.name.clone();
        let line = self.read_line(branch)?;
        // 107, 71: the room a lead reads is the host's, every repository's
        // runs counted.
        let beat = serde_json::json!({
            "host": me,
            "signer": self.peer.signer,
            "system": home,
            "held": self.seams.tests.running()?.total(),
            "bound": self.seams.tests.bound()?,
            "at": self.now(),
        });
        self.seams.state.beat(&me, beat.to_string().as_bytes())?;

        // 124: this host's copy follows the landings its rounds read, the
        // lead's among them, unless the read lacks the last (121).
        if let Some(landed) = line.doc.landed.clone() {
            if tip == landed || self.seams.branch.contains(&tip, &landed)? {
                let levelled = self.level_copy(branch, &landed)?;
                self.record_copy(branch, &landed, &levelled)?;
            }
        }

        self.release_left(&line, releases)?;

        // This host's runs that ended, reported while its claim holds.
        let mine: Vec<(Candidate, usize)> = line
            .doc
            .candidates
            .iter()
            .filter(|c| c.state.under_test())
            .flat_map(|c| {
                c.runs
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| {
                        r.state == RunState::Testing && r.host.as_deref() == Some(me.as_str())
                    })
                    .map(|(i, _)| (c.clone(), i))
                    .collect::<Vec<_>>()
            })
            .collect();
        for (c, i) in mine {
            let handle = c.runs[i].run.clone().expect("a testing run has a handle");
            let place = c.runs[i].place.clone();
            match self.seams.tests.progress(&handle)? {
                Progress::Running => {}
                Progress::Lost => {
                    self.update(branch, |l| {
                        let now = self.now();
                        if let Some(x) = l.candidate_mut(&c.id) {
                            let r = &mut x.runs[i];
                            if r.host.as_deref() == Some(me.as_str())
                                && r.state == RunState::Testing
                            {
                                r.state = RunState::Open;
                                r.host = None;
                                r.place = None;
                                r.run = None;
                                l.log(
                                    &now,
                                    Event::Lost {
                                        combination: c.revision.clone(),
                                        not_run: Vec::new(),
                                    },
                                );
                            }
                        }
                        Ok(())
                    })?;
                    if let Some(p) = place {
                        let outcome = self.release_made(branch, &p)?;
                        if outcome.how != Releasing::Released {
                            releases.push(outcome);
                        }
                    }
                }
                Progress::Finished(report) => {
                    let report = self.after_run(&handle, &unmet.limits, report)?;
                    self.update(branch, |l| {
                        let now = self.now();
                        let Some(x) = l.candidate(&c.id).cloned() else {
                            return Ok(());
                        };
                        let r = &x.runs[i];
                        // 77: a report under a claim this host no longer
                        // holds is never written.
                        if r.host.as_deref() != Some(me.as_str()) || r.state != RunState::Testing {
                            return Ok(());
                        }
                        self.finish_run(
                            l,
                            &x,
                            i,
                            &handle,
                            report.clone(),
                            &rules,
                            home.as_deref(),
                            &now,
                        )?;
                        // This host releases its own place below.
                        if let Some(p) = &place {
                            l.doc.release.retain(|q| q != p);
                        }
                        Ok(())
                    })?;
                    if let Some(p) = place {
                        let outcome = self.release_made(branch, &p)?;
                        if outcome.how != Releasing::Released {
                            releases.push(outcome);
                        }
                    }
                }
            }
        }

        // 75, 76, 71: claims, earliest first, up to this host's bound, of
        // candidates fetched from the remote (82); 81, 123: whatever the
        // releases above did.
        // Rule core.49: each claim holds its run's part of the host's bound
        // first, and none is made once the host holds none.
        self.seams.state.fetch_candidates(branch)?;
        let mut parts = Parts::new();
        while let Some(part) = self.seams.tests.hold(RunKind::Queue)? {
            let mut part = Some(part);
            let claimed = self.update(branch, |l| {
                let now = self.now();
                let next = l
                    .doc
                    .candidates
                    .iter()
                    .filter(|c| c.state.under_test())
                    .flat_map(|c| {
                        c.runs
                            .iter()
                            .enumerate()
                            .find(|(_, r)| {
                                self.claimable(r, home.as_deref(), &rules, listed, &unmet)
                            })
                            .map(|(i, _)| (c.id.clone(), i))
                    })
                    .next();
                let Some((id, i)) = next else {
                    return Ok(false);
                };
                let place = self.next_place(l);
                if let Some(part) = part.take() {
                    parts.insert(place.clone(), part);
                }
                let c = l.candidate_mut(&id).expect("a candidate");
                let r = &mut c.runs[i];
                r.state = RunState::Starting;
                r.host = Some(me.clone());
                r.place = Some(place);
                r.declined.remove(&me);
                let system = r.system.clone();
                let combination = c.revision.clone();
                l.log(
                    &now,
                    Event::Claimed {
                        combination,
                        system,
                        host: me.clone(),
                        unanswered: None,
                    },
                );
                Ok(true)
            })?;
            if !claimed {
                break;
            }
        }
        // Design D4: the runs it turned down for what it lacks say so.
        if unmet.found().is_some() {
            self.update(branch, |l| {
                let now = self.now();
                self.note_declined(l, &rules, home.as_deref(), listed, &unmet, &now);
                Ok(())
            })?;
        }
        self.perform_starts(branch, Some(&unmet), parts, releases)
    }

    /// 124: level this host's copy of `branch` to `landed`, the landing
    /// its records hold, from exactly the value the copy holds (rule
    /// core.5). A levelling that fails, or finds the copy moved meanwhile,
    /// leaves it behind for the next round to try again; it never fails
    /// the round (122).
    fn level_copy(&self, branch: &str, landed: &Rev) -> Result<Levelled> {
        let HostCopy::At(copy) = self.seams.branch.copy(branch)? else {
            return Ok(Levelled::Itself);
        };
        if copy.as_ref() == Some(landed) {
            return Ok(Levelled::Holds);
        }
        Ok(
            match self.seams.branch.level(branch, copy.as_ref(), landed) {
                Ok(Moved::Moved) => Levelled::Holds,
                Ok(Moved::Elsewhere(at)) => Levelled::Behind {
                    at,
                    why: "it moved while sluice levelled it".into(),
                },
                Err(e) => Levelled::Behind {
                    at: copy,
                    why: format!("{e:#}"),
                },
            },
        )
    }

    /// 124, S64: what `levelled` says of this host's copy, in `line`: a
    /// copy left behind is this host's entry in `copies`, logged once for
    /// each value, landing and why; one that holds the landing takes the
    /// entry out, logged when there was one. Whether `line` changed.
    fn note_copy(&self, line: &mut Line, landed: &Rev, levelled: &Levelled, now: &str) -> bool {
        let host = self.peer.name.clone();
        match levelled {
            Levelled::Itself => false,
            Levelled::Holds => {
                if line.doc.copies.remove(&host).is_none() {
                    return false;
                }
                line.log(
                    now,
                    Event::CopyLevelled {
                        host,
                        at: landed.clone(),
                    },
                );
                true
            }
            Levelled::Behind { at, why } => {
                let since = match line.doc.copies.get(&host) {
                    Some(e) if e.at == *at && e.lacks == *landed && e.why == *why => return false,
                    Some(e) => e.since.clone(),
                    None => now.to_string(),
                };
                line.log(
                    now,
                    Event::CopyBehind {
                        host: host.clone(),
                        at: at.clone(),
                        lacks: landed.clone(),
                        why: why.clone(),
                    },
                );
                line.doc.copies.insert(
                    host,
                    CopyBehind {
                        at: at.clone(),
                        lacks: landed.clone(),
                        why: why.clone(),
                        since,
                    },
                );
                true
            }
        }
    }

    /// [`Sluice::note_copy`] in a write of its own, made only when it
    /// changes the records.
    fn record_copy(&self, branch: &str, landed: &Rev, levelled: &Levelled) -> Result<()> {
        let now = self.now();
        if !self.note_copy(&mut self.read_line(branch)?, landed, levelled, &now) {
            return Ok(());
        }
        self.update(branch, |l| Ok(self.note_copy(l, landed, levelled, &now)))?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 133: a read of the branch's remote the round could not make
    // -----------------------------------------------------------------------

    /// 133: a round `refused` refused, counted in this host's refusal of
    /// the branch's remote, its cause added when no round of the refusal met
    /// it before, and its lines written when the records can be (design D4,
    /// D5). The round is kept before any write is tried, and a refusal that
    /// ended with lines still unwritten is kept inside the one that opens
    /// anew until a write carries them. The round's error stands whatever
    /// this does.
    fn note_refused(&self, branch: &str, refused: &ReadRefused) {
        let noted = (|| {
            let r = Refusal::refused(self.refusal(branch)?, refused, &self.now());
            self.keep_refusal(branch, Some(&r))?;
            self.write_refusal(branch)
        })();
        if let Err(e) = noted {
            warn!(
                sluice.branch = branch,
                "keeping the refused read of {} in the log failed: {e:#}", refused.remote
            );
        }
    }

    /// 133: a round that read the branch and its records ends this host's
    /// refusal of the remote, and writes its end (design D5).
    fn note_read_again(&self, branch: &str) {
        let noted = (|| {
            let Some(mut r) = self.refusal(branch)? else {
                return Ok(());
            };
            r.read_again(&self.now());
            self.keep_refusal(branch, Some(&r))?;
            self.write_refusal(branch)
        })();
        if let Err(e) = noted {
            warn!(
                sluice.branch = branch,
                "keeping the end of a refused read in the log failed: {e:#}"
            );
        }
    }

    /// The lines of this host's refusal not yet in the branch's log, in a
    /// write holding nothing else, when the round's read of the records
    /// reached them; otherwise they wait for the first write that can.
    fn write_refusal(&self, branch: &str) -> Result<()> {
        if self.seams.state.stale(branch).is_some() {
            return Ok(());
        }
        self.write_refusal_lines(branch)
    }

    /// The name of the next place on the branch.
    fn next_place(&self, line: &mut Line) -> String {
        let place = format!("sluice/try/{}/{}", line.branch, line.doc.next_place);
        line.doc.next_place += 1;
        place
    }

    /// The policy's name for the system this host's executor is, at the
    /// branch's revision `rev`; `None` when the policy names none (85).
    pub(crate) fn home_system(&self, rev: &Rev) -> Result<Option<String>> {
        let Some(policy) = self.policy_at(rev)? else {
            return Ok(None);
        };
        if policy.systems.is_empty() {
            return Ok(None);
        }
        let p = self.seams.tests.platform()?;
        Ok(policy
            .systems
            .iter()
            .find(|(_, s)| s.os == p.os && s.arch == p.arch)
            .map(|(n, _)| n.clone()))
    }
}

/// Could-not-run-1 114, 115 (design D1): the checks run `i` of `c` left
/// not run, `not_run`, read from its report alone and no run handle, so a
/// run that came back and one whose tests could not be started are read
/// alike; a check judged by nothing among them (149).
/// `judged` says a check failed on its own result or the time limit ended
/// the run. Unjudged and under the bound, the run is open again for any
/// host, its place to be released and its loss logged, and nobody is
/// blamed: `None`; the run keeps, until its next report is read, that
/// `host`, the host reading it, had no outcome of it, on the run's system,
/// and each check not run with why (design D7 of the change
/// `a-pass-nothing-proves-blames-no-one`; record 0105). Past the bound,
/// or judged, each check is returned named with why, to be read as
/// failed.
#[allow(clippy::too_many_arguments)]
fn read_not_run(
    line: &mut Line,
    c: &Candidate,
    i: usize,
    not_run: &[NotRun],
    judged: bool,
    system: Option<&str>,
    host: &str,
    now: &str,
) -> Option<Vec<FailedTask>> {
    let x = line.candidate_mut(&c.id).expect("a read candidate");
    if !judged {
        x.not_run_runs += 1;
        if x.not_run_runs < NOT_RUN_BOUND {
            let xr = &mut x.runs[i];
            xr.state = RunState::Open;
            xr.run = None;
            xr.host = None;
            xr.lost = Some(LostRun {
                host: host.to_string(),
                system: xr.system.clone(),
                not_run: not_run.to_vec(),
            });
            if let Some(p) = xr.place.take() {
                line.doc.release.push(p);
            }
            line.log(
                now,
                Event::Lost {
                    combination: c.revision.clone(),
                    not_run: not_run.to_vec(),
                },
            );
            return None;
        }
    }
    Some(
        not_run
            .iter()
            .map(|n| FailedTask {
                task: n.task.clone(),
                hash: String::new(),
                log: None,
                flaky: None,
                system: system.map(str::to_string),
                not_run: Some(n.reason.clone()),
                failed_by: Vec::new(),
                unsigned: None,
            })
            .collect(),
    )
}

/// Design D1: how much of what a failing program printed a start's reason
/// keeps, from its end.
const START_SAID: usize = 2048;

/// Design D1 (114): why a run's tests could not be started, in the
/// error's words, its whole chain; what a failing program printed among
/// them, past the first line, which names what failed, kept to its last
/// [`START_SAID`] bytes.
fn could_not_start(e: &anyhow::Error) -> String {
    let text = format!("{e:#}");
    let (head, rest) = match text.split_once('\n') {
        Some((head, rest)) if head.len() <= START_SAID => (Some(head), rest),
        _ => (None, text.as_str()),
    };
    if rest.len() <= START_SAID {
        return format!("could not start: {text}");
    }
    let mut cut = rest.len() - START_SAID;
    while !rest.is_char_boundary(cut) {
        cut += 1;
    }
    match head {
        Some(head) => format!("could not start: {head}\n…{}", &rest[cut..]),
        None => format!("could not start: …{}", &rest[cut..]),
    }
}

/// What a run whose tests never started is named by where neither it nor
/// its gate names a check: the tests its policy's `[tests]` would have
/// run.
const UNNAMED_TESTS: &str = "[tests]";

/// Design D1 (114, 139): the report of a run of `checks`, under the
/// executor `kind` the policy names, whose tests never started: no task,
/// nothing missing, each check not run for `reason`, or, where it names
/// none, its tests as one, so the reason is never lost.
fn unstarted(checks: &[String], kind: &str, reason: String) -> RunReport {
    let unnamed = [UNNAMED_TESTS.to_string()];
    let checks = if checks.is_empty() { &unnamed } else { checks };
    RunReport {
        tasks: Vec::new(),
        executor: sluice_seams::Enforced {
            kind: kind.into(),
            confined: false,
            processors: None,
            memory: None,
            network: None,
            docker: false,
            grant: None,
        },
        missing: Vec::new(),
        not_run: checks
            .iter()
            .map(|task| NotRun {
                task: task.clone(),
                reason: reason.clone(),
                key: None,
            })
            .collect(),
        dependencies: Vec::new(),
        failed_by: Vec::new(),
        timed_out: false,
        detail: None,
        kept_out: Vec::new(),
        refusals_unread: None,
    }
}

/// Record 0074 (design D3): what a run no host took before its time
/// limit waited for, each host that turned it down, for its engine or a
/// limit its executor cannot hold, named with why (66, 87).
fn waited_for(declined: &BTreeMap<String, String>) -> String {
    let hosts: Vec<String> = declined.iter().map(|(h, w)| format!("{h}: {w}")).collect();
    format!(
        "no host that could run its tests as its policy asks took them: {}",
        hosts.join("; ")
    )
}

/// Whether this host's executor takes `run`: one for any system, or for
/// the system it is (87).
fn ours(run: &SystemRun, home: Option<&str>) -> bool {
    match run.system.as_deref() {
        None => true,
        Some(s) => Some(s) == home,
    }
}

/// 86: a candidate's runs, one per system its gate names for some
/// required check, each holding the checks that need it; the checks that
/// need no system ride in this host's run where it has one, else the
/// first, or make a run of their own for any system when no check names
/// one.
fn open_runs(rules: &Rules, home: Option<&str>) -> Vec<SystemRun> {
    let mut by: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut any = Vec::new();
    for check in &rules.required {
        let systems = rules.systems_of(check);
        if systems.is_empty() {
            any.push(check.clone());
        }
        for s in systems {
            by.entry(s.clone()).or_default().push(check.clone());
        }
    }
    let run = |system: Option<String>, checks: Vec<String>| SystemRun {
        system,
        checks,
        state: RunState::Open,
        place: None,
        run: None,
        no_replay: false,
        host: None,
        declined: BTreeMap::new(),
        lost: None,
    };
    if by.is_empty() {
        return vec![run(None, any)];
    }
    if !any.is_empty() {
        let at = home
            .filter(|h| by.contains_key(*h))
            .map(str::to_string)
            .unwrap_or_else(|| by.keys().next().expect("a system").clone());
        let checks = by.get_mut(&at).expect("a system");
        for c in any {
            if !checks.contains(&c) {
                checks.push(c);
            }
        }
    }
    by.into_iter().map(|(s, c)| run(Some(s), c)).collect()
}

impl Sluice<'_> {
    /// Where `task`'s output in the recorded run `r` is kept, as its
    /// branch's records name it: on the candidate that ran it, or on the
    /// submission blamed for it, live or finished (20). A session run's is
    /// on its place, not here.
    fn kept_log(&self, line: &Line, task: &str, r: &Recorded) -> Result<Option<String>> {
        if r.worktree.is_some() {
            return Ok(None);
        }
        let on = &r.combination;
        let other;
        let line = if r.branch == line.branch {
            line
        } else {
            other = self.read_line(&r.branch)?;
            &other
        };
        let kept = |failed: &[FailedTask]| {
            failed
                .iter()
                .find(|f| f.task == task)
                .and_then(|f| f.log.clone())
        };
        // The same head on the same revision is the same combination, so
        // a candidate of it may be this run's own, with nothing kept yet.
        let held = line
            .doc
            .candidates
            .iter()
            .filter(|c| &c.revision == on)
            .find_map(|c| kept(&c.failed));
        if held.is_some() {
            return Ok(held);
        }
        let log = Line::read_log(&self.seams.state.read(&line.branch)?)?;
        let blamed = log.iter().rev().find_map(|l| match &l.event {
            Event::Blamed { id, combination } if combination == on => Some(*id),
            _ => None,
        });
        let Some(id) = blamed else {
            return Ok(None);
        };
        Ok(match line.subs.get(&id) {
            Some(s) => kept(&s.failed),
            None => self
                .finished_sub(&line.branch, id)?
                .and_then(|s| kept(&s.failed)),
        })
    }

    /// How long a candidate was under test: from the log's last `Testing`
    /// of it to `now`.
    fn tested_for(&self, branch: &str, revision: &Rev, now: &str) -> Result<Option<f64>> {
        let snap = self.seams.state.read(branch)?;
        let log = Line::read_log(&snap)?;
        Ok(log
            .iter()
            .rev()
            .find(|l| matches!(&l.event, Event::Testing { combination, .. } if combination == revision))
            .and_then(|l| seconds_between(&l.at, now)))
    }
}

/// Design D8: whether the signed results bear out that `c` passed: every
/// run of it has a result a signer the gate trusts signed for this very
/// combination, each read as passing, a replay only through a trusted pass
/// signed for its key on its system (108).
fn signed_pass(c: &Candidate, results: &[ResultLine], rules: &Rules) -> bool {
    use crate::records::ResultKind;
    let trusted = |r: &ResultLine| {
        r.signer.as_deref().is_some_and(|s| {
            let confined = r.executor.as_ref().is_some_and(|e| e.confined);
            rules.trusts_signed(s, confined, r.system.as_deref())
        })
    };
    let proven = |task: &str, hash: &str, system: Option<&str>| {
        results.iter().any(|p| {
            p.kind == ResultKind::Ran
                && p.outcome == sluice_seams::Outcome::Pass
                && p.task == task
                && p.hash == hash
                && p.system.as_deref() == system
                && trusted(p)
        })
    };
    c.runs.iter().all(|run| {
        let mine: Vec<&ResultLine> = results
            .iter()
            .filter(|r| r.combination == c.revision && trusted(r))
            .filter(|r| run.system.is_none() || r.system == run.system)
            .collect();
        !mine.is_empty()
            && mine.iter().all(|r| match (r.kind, r.outcome) {
                (ResultKind::Replayed, _) => proven(&r.task, &r.hash, r.system.as_deref()),
                (ResultKind::Ran, sluice_seams::Outcome::Pass) => true,
                (ResultKind::Ran, sluice_seams::Outcome::Fail) => {
                    rules.flaky == crate::policy::Flaky::Pass
                        && proven(&r.task, &r.hash, r.system.as_deref())
                }
            })
    })
}

/// Whether a branch has anything a lead would do: a submission in the
/// queue, a run under test, a landing, an outcome or a place to release.
fn has_work(line: &Line) -> bool {
    line.subs.values().any(|s| !s.state.is_out())
        || line.doc.candidates.iter().any(|c| c.state.under_test())
        || line.doc.landing.is_some()
        || !line.doc.outbox.is_empty()
        || !line.doc.release.is_empty()
}

/// Seconds from one record time to another, for a reader.
pub(crate) fn seconds_since(from: &str, to: &str) -> Option<f64> {
    seconds_between(from, to)
}

/// Seconds from one record time to another.
fn seconds_between(from: &str, to: &str) -> Option<f64> {
    let from = chrono::DateTime::parse_from_rfc3339(from).ok()?;
    let to = chrono::DateTime::parse_from_rfc3339(to).ok()?;
    Some((to - from).num_milliseconds() as f64 / 1000.0)
}

/// How many candidates from the front of a chain may land (22): those
/// that passed, in order; under deepest first, everything up to the
/// deepest that passed with no failure ahead of it (14; answers 12).
fn landable(cands: &[Candidate], batch: bool) -> usize {
    if !batch {
        return cands
            .iter()
            .take_while(|c| c.state == CandidateState::Passed)
            .count();
    }
    let mut n = 0;
    for (i, c) in cands.iter().enumerate() {
        if c.state.failed() {
            break;
        }
        if c.state == CandidateState::Passed {
            n = i + 1;
        }
    }
    n
}

/// 117: whether the first `n` candidates, which may land, wait instead
/// for the deepest candidate holding them that is under test, the hold
/// not yet run from the earliest of their verdicts; and if so, for what
/// and until when.
fn held(
    cands: &[Candidate],
    n: usize,
    hold: Option<std::time::Duration>,
    now: &str,
) -> Option<Held> {
    let hold = chrono::Duration::from_std(hold?).ok()?;
    let by = cands[n..].iter().rev().find(|c| c.state.under_test())?;
    let since = cands[..n]
        .iter()
        .filter_map(|c| c.passed_at.as_deref())
        .min()?;
    let until = chrono::DateTime::parse_from_rfc3339(since).ok()? + hold;
    let now = chrono::DateTime::parse_from_rfc3339(now).ok()?;
    (now < until).then(|| Held {
        by: by.submissions.clone(),
        until: until
            .with_timezone(&chrono::Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    })
}

/// Each queued submission's state, read from the chain (26, 49).
fn derive_states(line: &mut Line, chain: &[String]) {
    let base = line.doc.base.clone();
    let mut under_test: BTreeSet<u64> = BTreeSet::new();
    let mut own: BTreeMap<u64, CandidateState> = BTreeMap::new();
    for c in &line.doc.candidates {
        if Some(&c.on) != base.as_ref() {
            continue;
        }
        if c.state.under_test() {
            under_test.extend(c.submissions.iter().copied());
        }
        own.insert(c.newest(), c.state);
    }
    let in_chain: BTreeSet<u64> = chain
        .iter()
        .filter_map(|cid| line.candidate(cid).map(|c| c.newest()))
        .collect();
    for s in line.subs.values_mut() {
        if s.state.is_out() || s.state == State::Landing {
            continue;
        }
        // Its own candidate passed: passed, though a deeper one holding it
        // is under test, as when a hold keeps it (117).
        s.state = if !in_chain.contains(&s.id) {
            State::Waiting
        } else if own.get(&s.id) == Some(&CandidateState::Passed) {
            State::Passed
        } else if under_test.contains(&s.id) {
            State::Testing
        } else {
            match own.get(&s.id) {
                Some(st) if st.failed() => State::Testing,
                _ => State::Queued,
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use sluice_fakes::Fakes;

    use super::*;
    use crate::{Clock, Requester, Seams, SubmitHead, SubmitRequest};

    const POLICY: &str = "[branches.main]\nrequired = [\"#gate\"]\nparallel = 4\n";

    struct Noon;

    impl Clock for Noon {
        fn now(&self) -> chrono::DateTime<Utc> {
            Utc.with_ymd_and_hms(2026, 10, 30, 12, 0, 0).unwrap()
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

    /// Whether the branch's log holds a result for the combination `rev`.
    fn result_read(s: &Sluice<'_>, rev: &Rev) -> bool {
        let log = Line::read_log(&s.seams.state.read("main").unwrap()).unwrap();
        log.into_iter()
            .any(|l| matches!(l.event, Event::Result { combination, .. } if combination == *rev))
    }

    fn holding(line: &Line, ids: &[u64]) -> Option<Candidate> {
        line.doc
            .candidates
            .iter()
            .find(|c| c.submissions == ids)
            .cloned()
    }

    /// Design D1, D2 (25, 81; rules core.7, core.17): a round drops the
    /// combination holding a withdrawn submission before it reads any run,
    /// its place listed to release and its finished run read for nothing,
    /// while the combination whose entry ahead landed as tested stands.
    #[test]
    fn a_withdrawn_submissions_combination_is_dropped_before_any_run_is_read() {
        let f = Fakes::new();
        let tip = f.world.init(
            "main",
            &[
                (POLICY_PATH, POLICY),
                ("a/lib", "1"),
                ("b/lib", "1"),
                ("c/lib", "1"),
            ],
        );
        for p in ["a", "b", "c"] {
            f.world.task(&format!("{p}:test"), &format!("{p}/"));
            let head = f.world.work(&tip, &[(&format!("{p}/lib"), Some("2"))]);
            f.world.session(&format!("unit/{p}"), "agent", &head);
        }
        f.world.hold_runs(true);
        let s = sluice(&f);
        let (one, two, three) = (
            submit(&s, "unit/a"),
            submit(&s, "unit/b"),
            submit(&s, "unit/c"),
        );
        s.step("main").unwrap();
        let line = s.read_line("main").unwrap();
        let first = holding(&line, &[one]).expect("#1 under test");
        let last = holding(&line, &[one, two, three]).expect("#3 under test");
        let place = last.runs[0].place.clone().expect("a run of #3 here");
        f.world.complete(&first.revision);
        f.world.complete(&last.revision);
        s.withdraw(three, &Requester::Operator).unwrap().unwrap();

        // The round's decision, written before anything of it is done (62),
        // under the branch's lock as a round's is (rule core.14).
        let lock = s.seams.state.lock("main").unwrap().expect("the lock");
        let Decided::Written(unmet, parts) = s.decide("main").unwrap() else {
            panic!("the round's decision is written");
        };
        let line = s.read_line("main").unwrap();
        assert!(line
            .doc
            .candidates
            .iter()
            .all(|c| !c.submissions.contains(&three)));
        // Listed before #1's run, read after it, listed its own.
        let first_place = first.runs[0].place.clone().expect("a run of #1 here");
        assert_eq!(line.doc.release, [place.clone(), first_place]);
        assert!(result_read(&s, &first.revision), "#1's run was read");
        assert!(!result_read(&s, &last.revision), "#3's run was read");
        assert_eq!(f.world.live_runs(), 2, "#2's and #3's");

        let mut releases = Vec::new();
        s.perform("main", Some(&unmet), parts, &mut releases)
            .unwrap();
        drop(lock);
        assert!(releases.is_empty(), "{releases:?}");
        assert!(f.world.released_places().contains(&place));
        assert_eq!(f.world.live_runs(), 1, "#3's run is ended");
        let line = s.read_line("main").unwrap();
        assert!(line.doc.release.is_empty());
        assert_eq!(s.state(one).unwrap().unwrap().state, State::Landed);
        let stands = holding(&line, &[two]).expect("#2's combination stands");
        assert!(stands.state.under_test());
        assert_eq!(Some(&stands.on), line.doc.base.as_ref());
        assert!(!result_read(&s, &last.revision));
    }
}
