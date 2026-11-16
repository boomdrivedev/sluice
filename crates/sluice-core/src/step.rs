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
    read_refused, AtBound, Contested, CoveringVerdict, Delivery, Formed, HostCopy, Job,
    LandingCommit, Limits, Moved, NotRun, Notification, Outcome, Part, Progress, ReadRefused,
    Reading, Refusals, Rev, RunHandle, RunKind, RunReport, RunRequest, Source, Submitter, TaskLog,
    Written,
};
use tracing::{info, info_span, warn};

use crate::line::Line;
use crate::overview::Room;
use crate::policy::{Rules, POLICY_PATH};
use crate::records::{
    Beside, BesideWhy, BranchHold, Candidate, CandidateState, CheckOutcome, Claim, ConflictRecord,
    CopyBehind, DecidedRecord, Ended, Event, FailedTask, Held, HomeState, LandingCommitRecord,
    LandingIntent, LandingRefused, LostRun, Moving, OutboxItem, Recorded, ResultKind, ResultLine,
    RunState, State, SystemRun, Tested, HERE,
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

/// 152, record 0124 (design D10): what the operator's push of a branch's
/// held landing came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushedHeld {
    /// The branch holds no landing the forge refused: nothing is pushed.
    Nothing,
    /// The forge took it: its submissions landed as the same landing.
    Landed { ids: Vec<u64>, to: Rev },
    /// The forge refused it again: held with its new words.
    Refused { ids: Vec<u64>, words: String },
    /// The branch had moved: the landing is dropped and its submissions
    /// form again, as any landing that finds the branch elsewhere (24).
    Elsewhere { ids: Vec<u64>, found: Option<Rev> },
    /// Submissions `left` of it had left the queue: nothing is pushed, the
    /// landing is dropped and `back` form again (24; rule core.48).
    Dropped { left: Vec<u64>, back: Vec<u64> },
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
    /// 88 (design D2): the claim a pool's job starts its run under, where
    /// it is not this instance's own.
    claim: Option<String>,
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
    run.holder().is_some_and(|h| h.starts_with("pool/"))
}

/// Record 0119 (design D11): one check's result as a pool's sign job
/// signed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedResult {
    pub check: String,
    pub outcome: Outcome,
    pub replayed: bool,
}

/// Design D4, record 0119 (design D11, D12): whether `run` waits for its
/// holder to start it: claimed, or a pool's run the lead wrote running from
/// its run job's start, which the job's `sluice job run` reads after it.
fn waits_for_its_job(run: &SystemRun) -> bool {
    run.state == RunState::Claimed || (run.state == RunState::Running && given_out(run))
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
    /// 152, record 0124 (design D10): the branch's landing the forge
    /// refused is held: nothing was formed, started or landed.
    Refused,
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
        // Record 0122, C.6 (design D4): before the branch is read, what this
        // host owes a move is carried, wherever its home is not moving: the
        // submissions it made left in a remote home closed toward the hosts'
        // own files come into its own the first time a round of its own
        // reads that home. A moving home is carried only by the round that
        // keeps it, once nothing is under test ([`Sluice::decide`]). As
        // there, a round that carries does nothing more: the next reads the
        // branch where its queue is now.
        let held = self.seams.state.read_held(branch)?;
        let moving = match held.files.get(crate::records::HOME_FILE) {
            Some(bytes) => serde_json::from_slice::<crate::records::HomeDoc>(bytes)
                .is_ok_and(|h| h.state == HomeState::Moving),
            None => false,
        };
        if !moving && self.seams.state.carry(branch)?.to.is_some() {
            round.record("sluice.step", "carried");
            return Ok(Step::Stepped);
        }
        if !self.keeps(branch)? {
            // 75: another instance keeps the branch; this one takes, runs
            // and reports runs of its own.
            info_span!("sluice.claim").in_scope(|| self.execute(branch, releases))?;
            round.record("sluice.step", "led");
            return Ok(Step::Led);
        }
        // Record 0122 (design D5; 73, 77): the lead shows its sign of life
        // each round, as every claim holder does.
        self.alive_on(branch)?;
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
            Decided::Refused => {
                // 152, record 0124 (design D10): the held landing pushes
                // nothing; outcomes are still pushed and places released.
                info_span!("sluice.perform").in_scope(|| {
                    self.perform_releases(branch, releases)?;
                    self.perform_outbox(branch)
                })?;
                round.record("sluice.step", "refused");
                return Ok(Step::Stepped);
            }
        };
        info_span!("sluice.perform")
            .in_scope(|| self.perform(branch, Some(&unmet), parts, releases))?;
        round.record("sluice.step", "stepped");
        Ok(Step::Stepped)
    }

    /// 72, 137 (design D25): the instance `branch`'s records name as
    /// keeping it, as read, taking nothing; `None` where none does yet, or
    /// where its records are this host's own.
    pub fn lead_of(&self, branch: &str) -> Result<Option<String>> {
        Ok(self.read_line(branch)?.doc.lead)
    }

    /// 72, 73: whether this instance keeps `branch`, taking it when no one
    /// does, or when the instance that does has written nothing for the
    /// lease by this host's own observation and there is work to do.
    /// Taking is a write (62), after which the former lead's next write is
    /// refused. Intake asks it too, to take a branch's pull requests in
    /// only where this instance keeps the branch (137).
    ///
    /// Record 0122, 137, record 0093 as amended 2026-11-08 (design D8): the
    /// home decides first. A queue open in this host's own files is this
    /// host's, with no lead. A remote home is taken or kept only by an
    /// instance the rules at the branch's revision accept: it signs as a
    /// signer they trust, and the records say they list the key it signs
    /// with; where they name no signer, as for a branch coming home, any
    /// instance may lead it, through the lease, so one carries its queue
    /// home. An instance they do not accept never writes itself as lead,
    /// and a lead they no longer accept keeps nothing from its next round,
    /// so an accepted one takes the branch once the lease passes (73).
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
        // 56, S64, 124 (record 0069): the rules at the branch's revision are
        // read before anything is decided, wherever its home is, so a policy
        // refused there refuses the round before it levels this host's copy,
        // lands or runs anything.
        let rules = self.rules_at(branch, &tip)?;
        if line.home.as_ref().is_some_and(|h| h.at == HERE) {
            return Ok(true);
        }
        let signed = rules.as_ref().is_some_and(|r| r.signed);
        // Records with no home yet go where the rules at the head send them.
        if line.home.is_none() && !signed {
            return Ok(true);
        }
        if signed && !self.accepted(branch, &tip, rules.as_ref())? {
            return Ok(false);
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
        taken.doc.kept = Some(now.clone());
        taken.log(&now, Event::Took { by: me, from });
        Ok(matches!(self.write(&taken)?, Written::Written(_)))
    }

    /// 72, 58, record 0122 (design D9): in a round this instance keeps as
    /// the lead of a remote home, while a run of the branch is claimed or
    /// under test, or open on a combination under test, `kept` is written
    /// once half a lease has passed by this instance's own clock since its
    /// last, so the branch's state never stands a lease while the lead is
    /// alive; a round that writes anything else writes it anyway
    /// ([`Sluice::write`]), and with no such run nothing is written for it.
    fn keep_lead(&self, line: &mut Line, now: &str) {
        if line.doc.lead.as_deref() != Some(self.peer.name.as_str()) {
            return;
        }
        if !line.doc.candidates.iter().any(|c| c.state.under_test()) {
            return;
        }
        let half = self.peer.lease.as_secs_f64() / 2.0;
        let due = line
            .doc
            .kept
            .as_deref()
            .and_then(|k| seconds_between(k, now))
            .is_none_or(|s| s >= half);
        if due {
            line.doc.kept = Some(now.to_string());
        }
    }

    /// 137, record 0093 as amended 2026-11-08 (design D8): whether the
    /// rules at `tip`, a branch whose records are on its remote, accept this
    /// instance: it signs as a signer they trust, and the records say they
    /// list the key it signs with ([`Sluice::key_listed`]).
    fn accepted(&self, branch: &str, tip: &Rev, rules: Option<&Rules>) -> Result<bool> {
        let Some(rules) = rules else {
            return Ok(false);
        };
        let trusted = self
            .peer
            .signer
            .as_ref()
            .is_some_and(|s| rules.claims.contains_key(s));
        Ok(trusted && self.key_listed(branch, tip, rules)?)
    }

    // -----------------------------------------------------------------------
    // 1. Recover
    // -----------------------------------------------------------------------

    /// Record 0122 (design D3): what was written and not yet done is done
    /// from what is written, each act checked against its proof first: a
    /// claimed run whose checks a stopped step started is written running
    /// from their own start, and one whose checkout it made is started in
    /// it ([`Sluice::perform_starts`]); nothing of a claimed run is released
    /// or opened again.
    fn recover(&self, branch: &str, releases: &mut Vec<ReleaseOutcome>) -> Result<()> {
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
        // Record 0122 (design D3): an ended run's checkout is released, and
        // the run closed, before its combination's landing takes its record
        // out of the branch's.
        self.perform_closes(branch, releases)?;
        self.perform_landing(branch)?;
        self.perform_releases(branch, releases)?;
        self.perform_outbox(branch)?;
        self.perform_starts(branch, unmet, parts, releases)?;
        self.perform_dispatches(branch)
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
        self.perform_closes(branch, releases)?;
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
            .filter(|r| self.mine(r) && r.state != RunState::Open && r.state != RunState::Closed)
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
        // 152, rule core.5, record 0124 (design D10): a landing the forge
        // refused is never pushed again by itself.
        if intent.refused.is_some() {
            return Ok(());
        }
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
                                release.extend(self.unclosed(c));
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
            Moved::Refused(words) => {
                // 152, rule core.5, record 0124 (design D10): the forge
                // refused the landing with the branch where it expected
                // it: held with the forge's words, its submissions refused,
                // logged once, and pushed again only on the operator's word.
                span.record("sluice.outcome", "forge_refused");
                self.update(branch, |l| {
                    if l.doc.landing.as_ref() != Some(&intent) {
                        return Ok(());
                    }
                    if let Some(landing) = l.doc.landing.as_mut() {
                        landing.refused = Some(LandingRefused {
                            words: words.clone(),
                            at: now.clone(),
                        });
                    }
                    for id in &intent.ids {
                        let s = l.sub_mut(*id);
                        if s.state == State::Landing {
                            s.state = State::Refused;
                        }
                        // 50, 51, rule core.52: its work is owed word of the
                        // hold, which decides no outcome.
                        let item = OutboxItem {
                            id: format!("{id}:refused"),
                            submission: *id,
                            outcome: DecidedRecord::Refused {
                                words: words.clone(),
                            },
                        };
                        l.push_outcome(item, &now, self.peer.expire_after)?;
                    }
                    l.log(
                        &now,
                        Event::ForgeRefused {
                            ids: intent.ids.clone(),
                            words: words.clone(),
                        },
                    );
                    Ok(())
                })
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

    /// 152, rule core.5, record 0124 (design D10): the operator's word that
    /// `branch`'s landing the forge refused be pushed again: under the
    /// branch's lock its refusal is cleared, its submissions landing again,
    /// and the landing is run once, as a round runs it: taken, they land as
    /// the same landing; refused again, it is held with the forge's new
    /// words. A branch with no landing held pushes nothing, and a held
    /// landing one of whose submissions has left the queue is dropped,
    /// pushing nothing, the rest back in the queue (24; rule core.48).
    pub fn push_held(&self, branch: &str) -> Result<PushedHeld> {
        let _lock = self.seams.state.wait_lock(branch)?;
        let line = self.read_line(branch)?;
        let Some(intent) = line.doc.landing.clone().filter(|l| l.refused.is_some()) else {
            return Ok(PushedHeld::Nothing);
        };
        let left = self.update(branch, |l| Ok(l.drop_left_landing()))?;
        if let Some(left) = left {
            let back = intent
                .ids
                .iter()
                .copied()
                .filter(|id| !left.contains(id))
                .collect();
            return Ok(PushedHeld::Dropped { left, back });
        }
        self.update(branch, |l| {
            if let Some(landing) = l.doc.landing.as_mut() {
                landing.refused = None;
            }
            for id in &intent.ids {
                let s = l.sub_mut(*id);
                if s.state == State::Refused {
                    s.state = State::Landing;
                }
            }
            Ok(())
        })?;
        self.perform_landing(branch)?;
        let line = self.read_line(branch)?;
        let ids = intent.ids.clone();
        Ok(match &line.doc.landing {
            Some(l) => match &l.refused {
                Some(r) => PushedHeld::Refused {
                    ids,
                    words: r.words.clone(),
                },
                None => bail!("{branch}'s landing is still unsettled after its push"),
            },
            None if ids
                .iter()
                .all(|id| line.subs.get(id).is_none_or(|s| s.state == State::Landed)) =>
            {
                PushedHeld::Landed { ids, to: intent.to }
            }
            None => PushedHeld::Elsewhere {
                ids,
                found: self.seams.branch.read(branch)?,
            },
        })
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

    /// 33, 37: make each claimed run's place and start its checks
    /// ([`Sluice::start_run`]) in the part `parts` holds for it, reading
    /// the host where `unmet`, the round's look, has not (design D4).
    /// Record 0122 (design D3): each is acted on from its written state, its
    /// proof read first: a run whose checks were started is written running
    /// from their own start and started no second time. 71 (rule core.49):
    /// a run claimed with no part held, a stopped round's, is started once
    /// this host holds room for it, in the checkout that round made, and
    /// waits claimed otherwise; one the check seam refuses at the bound,
    /// and every run decided behind it, goes back to open for a later
    /// round, a place made for it released as any candidate's is (123),
    /// into `releases` when its release was not plain. 84, 139 (rule core.38):
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
                    .filter(|(_, r)| r.state == RunState::Claimed && self.mine(r))
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
            let mut held = parts.remove(&name);
            // Record 0122 (design D3): a start found done is not done again:
            // the run is written running from its own start, nothing is
            // started, and the room held for it goes back.
            if let Some(found) = self.seams.tests.found(&name)? {
                drop(held);
                self.write_running(branch, &c.id, i, &name, None, &found.run, &found.since)?;
                continue;
            }
            if held.is_none() && !given_out(&c.runs[i]) {
                // No room was held for it: a stopped round's (core.49).
                // Record 0122 (design D3): it is started, in the checkout
                // that round made where it made one, once this host holds
                // room for it, and waits claimed for a round with room.
                if self.meets(unmet) {
                    if full {
                        continue;
                    }
                    match self.seams.tests.hold(RunKind::Queue)? {
                        Some(part) => held = Some(part),
                        None => {
                            full = true;
                            continue;
                        }
                    }
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
                    continue;
                }
            }
            // 71: once a start is refused at the bound, the host holds none
            // of the room this round counted on; each run decided behind it
            // is open again, its part given back, no place made for it.
            let given = given_checks(&c.runs[i], &from.rules);
            if full {
                drop(held);
                self.reopen_made(branch, &c.id, i, &name, &given, releases)?;
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
                        self.reopen_made(branch, &c.id, i, &name, &given, releases)?;
                        full = true;
                        continue;
                    }
                },
            };
            full = self.start_run(branch, &from, &c, i, unmet, part, releases)? == Started::AtBound;
        }
        Ok(())
    }

    /// 71, 123: run `i` of candidate `cand`, claimed into `place` and not
    /// started for want of room, ended with no outcome for `checks`
    /// ([`Sluice::reopen`]); the checkout a start made for it, where one
    /// was made, is released only once that end is written (record 0122,
    /// design D3; rule core.61), [`Sluice::release_ended`].
    fn reopen_made(
        &self,
        branch: &str,
        cand: &str,
        i: usize,
        place: &str,
        checks: &[String],
        releases: &mut Vec<ReleaseOutcome>,
    ) -> Result<()> {
        let made = self.seams.checkout.made()?.iter().any(|p| p == place);
        self.reopen(branch, cand, i, place, made, checks)?;
        if made {
            self.release_ended(branch, cand, i, place, releases)?;
        }
        Ok(())
    }

    /// Record 0122 (design D3; 123; rule core.61): the checkout `place` of
    /// run `i` of candidate `cand`, released now that the run's end is
    /// written, and the run closed once the release frees it; a release
    /// that was not plain goes into `releases`, and one that failed or is
    /// held leaves the run ended, its checkout tried by the rounds after
    /// as any ended run's is ([`Sluice::perform_closes`]).
    fn release_ended(
        &self,
        branch: &str,
        cand: &str,
        i: usize,
        place: &str,
        releases: &mut Vec<ReleaseOutcome>,
    ) -> Result<()> {
        let outcome = self.release_made(branch, place)?;
        let freed = outcome.how.freed();
        if outcome.how != Releasing::Released {
            releases.push(outcome);
        }
        if freed {
            self.write_closed(branch, cand, i, Some(place))?;
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
    /// and `sluice job run` starts its pool job's run through it under the
    /// job's claim, so the job needs nothing of its own to refuse at its
    /// start (record 0119; design D11).
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
                    .position(|r| r.holder() == Some(claim) && waits_for_its_job(r))
                    .map(|i| (c.clone(), i))
            });
        let Some((c, i)) = held else {
            return Ok(None);
        };
        let Some(mut from) = self.start_from(branch, &line)? else {
            return Ok(None);
        };
        from.claim = Some(claim.to_string());
        let Some(part) = self.seams.tests.hold(RunKind::Queue)? else {
            let name = c.runs[i].place.clone().expect("a starting run has a place");
            let given = given_checks(&c.runs[i], &from.rules);
            self.reopen_made(branch, &c.id, i, &name, &given, releases)?;
            return Ok(Some(Started::AtBound));
        };
        let unmet = Unmet::of(from.policy.limits.clone());
        self.start_run(branch, &from, &c, i, &unmet, part, releases)
            .map(Some)
    }

    /// Record 0119 (design D11; 75, 77, 109): the run `claim` holds on
    /// `branch`, ended as its pool's sign job ends it: each check the run
    /// was given read from what the job signed, `signed`, a pass it ran
    /// passed, a fail failed and a replay unproven, since a replay counts
    /// only through a pass signed for its key (108; rule core.9); each
    /// check of the run job's report the sign job left out, `left`, with
    /// no outcome for its own why, so a key the combination's tree does not
    /// bear out is left without a result and its follow-up holds it, as a
    /// host's run reads a key it cannot count (78; design D7); and a check
    /// it signed nothing of with no outcome, for `why`. The run is written
    /// ended and closed in one write, its checkout the job's runner's,
    /// which went with it. `false`, with nothing written, where the claim
    /// no longer holds the run (77).
    pub fn end_claimed(
        &self,
        branch: &str,
        claim: &str,
        signed: &[SignedResult],
        left: &[NotRun],
        why: &str,
    ) -> Result<bool> {
        let _lock = self.seams.state.wait_lock(branch)?;
        let line = self.read_line(branch)?;
        let Some(base) = line.doc.base.clone() else {
            return Ok(false);
        };
        let rules = self.gate(branch, &base)?;
        self.update(branch, |l| {
            let now = self.now();
            let held = l.doc.candidates.iter().find_map(|c| {
                c.runs
                    .iter()
                    .position(|r| r.holder() == Some(claim) && r.state.held())
                    .map(|i| (c.id.clone(), i))
            });
            let Some((cand, i)) = held else {
                return Ok(false);
            };
            let x = l.candidate_mut(&cand).expect("the candidate read");
            let given = given_checks(&x.runs[i], &rules);
            let mut outcomes: BTreeMap<String, CheckOutcome> = signed
                .iter()
                .map(|s| {
                    let outcome = match () {
                        _ if s.replayed => CheckOutcome::Unproven,
                        _ if s.outcome == Outcome::Pass => CheckOutcome::Passed,
                        _ => CheckOutcome::Failed,
                    };
                    (s.check.clone(), outcome)
                })
                .collect();
            for n in left {
                outcomes
                    .entry(n.task.clone())
                    .or_insert_with(|| CheckOutcome::None {
                        why: n.reason.clone(),
                    });
            }
            // A tag given is read by the checks it names, as a run's own
            // report is read (outcomes_of), each check it names that the
            // job left out undecided beside it; a check given that the job
            // signed nothing of has no outcome.
            for check in &given {
                outcomes.entry(check.clone()).or_insert_with(|| {
                    match check.starts_with('#') && !signed.is_empty() {
                        true => CheckOutcome::Passed,
                        false => CheckOutcome::None { why: why.into() },
                    }
                });
            }
            let undecided = outcomes
                .values()
                .any(|o| matches!(o, CheckOutcome::None { .. }));
            x.runs[i].place = None;
            let ended = Ended {
                at: now.clone(),
                outcomes,
                timed_out: false,
                why: undecided.then(|| why.to_string()),
                untaken: false,
            };
            end_run(l, &cand, i, ended, &now);
            Ok(true)
        })
    }

    /// 113, record 0119 (design D11): start a run of `head`, a pull
    /// request's head onto `branch`, as `sluice-pr`'s run job runs it,
    /// under no claim: its checkout at the head, the branch's gate, tests
    /// and limits read at the branch's current revision (rule core.15), on
    /// `system` where one is named. Nothing is written: a head's run is no
    /// combination's, and its keys count by key alone once signed (15). A
    /// checkout that cannot be made is [`Started::Unmade`], and a start
    /// that fails [`Started::NotRun`].
    pub fn start_head(&self, branch: &str, head: &Rev, system: Option<&str>) -> Result<Started> {
        let base = self
            .seams
            .branch
            .read(branch)?
            .ok_or_else(|| anyhow!("no branch {branch}"))?;
        let policy = self
            .policy_at(&base)?
            .ok_or_else(|| anyhow!("{branch}'s revision {base} has no {POLICY_PATH}"))?;
        let rules = self.gate(branch, &base)?;
        let Some(part) = self.seams.tests.hold(RunKind::Queue)? else {
            return Ok(Started::AtBound);
        };
        let name = format!("sluice/head/{head}");
        let place = match self
            .seams
            .checkout
            .materialize(&name, head, &policy.checkout)
        {
            Ok(place) => place,
            Err(e) if passes_through(&e) => return Err(e),
            Err(e) => return Ok(Started::Unmade(could_not_start(&e))),
        };
        let request = RunRequest {
            place,
            revision: head.clone(),
            required: rules.required.clone(),
            since: None,
            limit: rules.timeout,
            limits: policy.limits.clone(),
            tests: policy.tests.clone(),
            command: None,
            replay: true,
            system: system.map(str::to_string),
        };
        match self.seams.tests.start(&request, part) {
            Ok(run) => Ok(Started::Running(run)),
            Err(e) if e.downcast_ref::<AtBound>().is_some() => Ok(Started::AtBound),
            Err(e) if passes_through(&e) => Err(e),
            Err(e) => Ok(Started::NotRun(could_not_start(&e))),
        }
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
            claim: None,
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
            Err(e) if passes_through(&e) => return Err(e),
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
                // Record 0122 (design D3): written running from the run's own
                // start, as the supervisor's files give it.
                let since = match self.seams.tests.found(&name)? {
                    Some(found) => found.since,
                    None => self.now(),
                };
                let claim = from.claim.as_deref();
                self.write_running(branch, &c.id, i, &name, claim, &run, &since)?;
                return Ok(Started::Running(run));
            }
            // Refused before anything of it ran: no run, no outcome, its
            // end written before its checkout is released (design D3).
            Err(e) if e.downcast_ref::<AtBound>().is_some() => {
                self.reopen_made(branch, &c.id, i, &name, &checks, releases)?;
                return Ok(Started::AtBound);
            }
            // 133, 134: a remote that could not answer says nothing of the
            // run; the round is refused as any such read.
            Err(e) if passes_through(&e) => return Err(e),
            Err(e) => e,
        };
        let why = could_not_start(&e);
        let report = unstarted(&checks, kind, why.clone());
        self.not_started(branch, c, i, &name, &report, home, releases)?;
        Ok(Started::NotRun(why))
    }

    /// Record 0122 (design D3): run `i` of candidate `cand`, claimed into
    /// `place`, written running under `run` from its own start `since`;
    /// nothing where it is no longer claimed into that place under `claim`,
    /// a pool's job's, or this instance's own where it is `None`.
    #[allow(clippy::too_many_arguments)]
    fn write_running(
        &self,
        branch: &str,
        cand: &str,
        i: usize,
        place: &str,
        claim: Option<&str>,
        run: &RunHandle,
        since: &str,
    ) -> Result<()> {
        self.update(branch, |l| {
            let now = self.now();
            let Some(x) = l.candidate_mut(cand) else {
                return Ok(());
            };
            let combination = x.revision.clone();
            let Some(xr) = x.runs.get_mut(i) else {
                return Ok(());
            };
            // 75 (design D2, D9): only under the claim the act is made under.
            let held = match claim {
                Some(c) => xr.holder() == Some(c),
                None => self.mine(xr),
            };
            if xr.state != RunState::Claimed || xr.place.as_deref() != Some(place) || !held {
                return Ok(());
            }
            xr.state = RunState::Running;
            xr.run = Some(run.clone());
            xr.running = Some(since.to_string());
            let system = xr.system.clone();
            l.log(
                &now,
                Event::Running {
                    combination,
                    system,
                    run: i,
                },
            );
            Ok(())
        })
    }

    /// Record 0122 (design D3): the checkout of each ended run this host
    /// holds released, its proof the place profile no longer listing it,
    /// then the run written closed, logged `closed`; a checkout the place
    /// profile no longer lists is closed with no release command run, and
    /// an ended run that made none is closed as it is. A release that
    /// fails, or that is held, leaves its run ended, tried by the rounds
    /// after as any release is (123); one tried already this round, in
    /// `releases`, is passed over.
    fn perform_closes(&self, branch: &str, releases: &mut Vec<ReleaseOutcome>) -> Result<()> {
        let line = self.read_line(branch)?;
        let ended: Vec<(String, usize, Option<String>)> = line
            .doc
            .candidates
            .iter()
            .flat_map(|c| {
                c.runs
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.state == RunState::Ended && self.mine(r))
                    .map(|(i, r)| (c.id.clone(), i, r.place.clone()))
                    .collect::<Vec<_>>()
            })
            .collect();
        if ended.is_empty() {
            return Ok(());
        }
        let made = self.seams.checkout.made()?;
        for (cand, i, place) in ended {
            if let Some(p) = &place {
                let tried = releases.iter().any(|o| o.place == *p);
                let held = line
                    .doc
                    .failed_releases
                    .iter()
                    .any(|f| f.place == *p && f.stopped && !self.held_gone(p));
                if tried || held {
                    continue;
                }
                if made.contains(p) {
                    let outcome = self.release_made(branch, p)?;
                    let freed = outcome.how.freed();
                    if outcome.how != Releasing::Released {
                        releases.push(outcome);
                    }
                    if !freed {
                        continue;
                    }
                }
            }
            self.write_closed(branch, &cand, i, place.as_deref())?;
        }
        Ok(())
    }

    /// Record 0122 (design D1, D3): run `i` of candidate `cand`, ended with
    /// its checkout `place` released, written closed and logged; nothing
    /// where it is not ended so. Its checkout leaves the branch's places to
    /// release.
    fn write_closed(&self, branch: &str, cand: &str, i: usize, place: Option<&str>) -> Result<()> {
        self.update(branch, |l| {
            let now = self.now();
            let Some(x) = l.candidate_mut(cand) else {
                return Ok(());
            };
            let combination = x.revision.clone();
            let Some(xr) = x.runs.get_mut(i) else {
                return Ok(());
            };
            // 75 (design D2, D9): only under this host's own claim.
            if xr.state != RunState::Ended || xr.place.as_deref() != place || !self.mine(xr) {
                return Ok(());
            }
            xr.state = RunState::Closed;
            xr.closed = Some(now.clone());
            let system = xr.system.clone();
            if let Some(p) = place {
                l.doc.release.retain(|q| q != p);
            }
            l.log(
                &now,
                Event::Closed {
                    combination,
                    system,
                    run: i,
                },
            );
            Ok(())
        })
    }

    /// Design D1 (16, 114, 115): run `i` of candidate `c`, decided into
    /// `place`, whose tests could not be started, read as `report`, a run
    /// whose every check was not run. The run is written ended once, read
    /// as any run that left checks not run, its follow-up holding them
    /// under the bound and past it each check named with why; its checkout,
    /// where one was made, is released only once that end is written
    /// (record 0122, design D3; rule core.61), as a run refused at the
    /// bound has its released ([`Sluice::release_ended`]).
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
        self.update(branch, |l| {
            let now = self.now();
            let Some(xr) = l.candidate_mut(&c.id).and_then(|x| x.runs.get_mut(i)) else {
                return Ok(());
            };
            if xr.state != RunState::Claimed || xr.place.as_deref() != Some(place) {
                return Ok(());
            }
            // Design D3: a checkout made stays the run's until its release,
            // after this end; with none made, the run closes as it ends.
            if !made {
                xr.place = None;
            }
            let system = xr.system.clone();
            let system = system.as_deref().or(home);
            let host = &self.peer.name;
            // Record 0122 (design D7): ended, failed past the bound, and
            // otherwise with no outcome, its follow-up holding its checks.
            let ended = match read_not_run(l, c, i, &report.not_run, false, system, host, &now) {
                Some(named) => {
                    let ended = failing(&named, &now);
                    l.candidate_mut(&c.id)
                        .expect("a read candidate")
                        .failed
                        .extend(named);
                    ended
                }
                None => with_none(&report.not_run, &now),
            };
            end_run(l, &c.id, i, ended, &now);
            Ok(())
        })?;
        if made {
            self.release_ended(branch, &c.id, i, place, releases)?;
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
    /// runs in a row (115) and blames no one, and its follow-up is open
    /// for any host, its record saying why (16) until its next run has a
    /// result. Whatever of the place was made is released once the run's
    /// end is written (record 0122, design D3; rule core.61).
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
        self.update(branch, |l| {
            let now = self.now();
            let Some(xr) = l.candidate_mut(&c.id).and_then(|x| x.runs.get_mut(i)) else {
                return Ok(());
            };
            if xr.state != RunState::Claimed || xr.place.as_deref() != Some(place) {
                return Ok(());
            }
            // Design D3: a checkout made stays the run's until its release,
            // after this end; with none made, the run closes as it ends.
            if !made {
                xr.place = None;
            }
            xr.lost = Some(LostRun {
                host: self.peer.name.clone(),
                system: xr.system.clone(),
                not_run: report.not_run.clone(),
            });
            l.log(
                &now,
                Event::Lost {
                    combination: c.revision.clone(),
                    not_run: report.not_run.clone(),
                },
            );
            // Record 0122 (design D7): ended with no outcome, counting toward
            // nothing, its follow-up holding its checks.
            end_run(l, &c.id, i, with_none(&report.not_run, &now), &now);
            Ok(())
        })?;
        if made {
            self.release_ended(branch, &c.id, i, place, releases)?;
        }
        Ok(())
    }

    /// 71, record 0122 (design D7): run `i` of candidate `cand`, claimed
    /// into `place` and not started for want of room, ends with no outcome
    /// for each of `checks`, counting toward nothing, and its follow-up,
    /// open to any host with room, holds them; `made` when its checkout
    /// was made, which stays the run's until its release after this end
    /// (design D3), and otherwise it is closed in the same write.
    fn reopen(
        &self,
        branch: &str,
        cand: &str,
        i: usize,
        place: &str,
        made: bool,
        checks: &[String],
    ) -> Result<()> {
        self.update(branch, |l| {
            let now = self.now();
            let Some(r) = l.candidate_mut(cand).and_then(|c| c.runs.get_mut(i)) else {
                return Ok(());
            };
            if r.state != RunState::Claimed || r.place.as_deref() != Some(place) {
                return Ok(());
            }
            if !made {
                r.place = None;
            }
            let none: Vec<NotRun> = checks
                .iter()
                .map(|task| NotRun {
                    task: task.clone(),
                    reason: AT_BOUND.into(),
                    key: None,
                })
                .collect();
            end_run(l, cand, i, with_none(&none, &now), &now);
            Ok(())
        })
    }

    // -----------------------------------------------------------------------
    // 2. Decide
    // -----------------------------------------------------------------------

    /// Everything a round decides, written in one write.
    fn decide(&self, branch: &str) -> Result<Decided> {
        let mut line = self.read_line(branch)?;
        let now = self.now();
        // Design D2: before any run is read, and before a round that holds
        // the branch returns; record 0124 (design D10): a landing the forge
        // refused goes once one of its submissions has left.
        self.drop_withdrawn(&mut line)?;
        if let Some(landing) = &line.doc.landing {
            if landing.refused.is_none() {
                bail!("{branch} has a landing left unfinished");
            }
            // 152, record 0124 (design D10): while the forge's refusal of the
            // branch's landing stands, nothing is formed, started or landed
            // on the branch, and the submissions behind the landing wait.
            let held: BTreeSet<u64> = landing.ids.iter().copied().collect();
            let mut changed = false;
            for s in line.subs.values_mut() {
                if !s.state.is_out() && !held.contains(&s.id) && s.state != State::Waiting {
                    s.state = State::Waiting;
                    changed = true;
                }
            }
            if !changed {
                return Ok(Decided::Refused);
            }
            return Ok(match self.write(&line)? {
                Written::Written(_) => Decided::Refused,
                Written::Refused => Decided::Contended,
            });
        }
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
        // Record 0122 (design D3): the home the rules at the head name,
        // weighed against the one the queue is open in.
        let moving = self.weigh_home(&mut line, &crate::home::named(&rules), &tip, &now);
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
        // Record 0122, 59 (design D3): a moving home with no combination
        // under test or judged, and no landing unsettled, is carried under
        // this round's lock: the round's decisions written first, then every
        // live submission carried to the home the move names, logged
        // `carried` there, and the old home closed with its pointer. The
        // round ends there; the next forms in the new home.
        if moving && !line.doc.candidates.iter().any(|c| c.state.pins()) {
            if let Written::Refused = self.write(&line)? {
                return Ok(Decided::Contended);
            }
            self.seams.state.carry(branch)?;
            return Ok(Decided::Written(unmet, Parts::new()));
        }

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
        self.poll(&mut line, &rules, &unmet, home.as_deref(), listed, &now)?;
        // 122: work the branch already holds, landed by a landing this
        // queue did not make, is landed there; nothing is formed of it.
        self.land_found(&mut line, &tip, &now)?;
        // 140: work no required check reads lands beside the chain, before
        // anything is formed; a round that lands so does nothing else, and
        // the next forms on the revision it made.
        // Record 0122 (design D3): nothing lands beside a queue whose move
        // is written, which waits for no landing but its chain's.
        let beside = !moving
            && info_span!("sluice.beside")
                .in_scope(|| self.choose_beside(&mut line, &rules, &tip, &now))?;
        if !beside {
            let chain = loop {
                let (chain, removed) = self.arrange(&mut line, &rules, moving, &now)?;
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
            derive_states(&mut line, &chain, moving);
        }
        self.keep_tasks(&line)?;
        self.keep_lead(&mut line, &now);
        let queued = line.subs.values().filter(|s| !s.state.is_out()).count() as u64;
        let written = matches!(self.write(&line)?, Written::Written(_));
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

    /// Record 0122, 58 (design D3): in a round that keeps the queue's open
    /// home, the home the rules at the head `tip` name, `named`, against it.
    /// Where they differ, the move is written in the home, logged `moving`;
    /// a head naming the open home again ends a move written, logged
    /// `stayed`; one naming a third home writes the move there instead.
    /// Whether the queue's move stands written after it. Records with no
    /// home yet have the one their first write writes, and move nothing.
    fn weigh_home(&self, line: &mut Line, named: &str, tip: &Rev, now: &str) -> bool {
        let Some(home) = line.home.as_mut() else {
            return false;
        };
        let event = match home.state {
            HomeState::Closed => return false,
            HomeState::Open if home.at == named => return false,
            HomeState::Moving if home.at == named => {
                home.state = HomeState::Open;
                home.moving = None;
                Event::Stayed {
                    at: Some(tip.clone()),
                }
            }
            HomeState::Moving if home.moving_to() == Some(named) => return true,
            HomeState::Open | HomeState::Moving => {
                home.state = HomeState::Moving;
                home.moving = Some(Moving {
                    to: named.to_string(),
                    chosen_at: Some(tip.clone()),
                    since: now.to_string(),
                });
                Event::Moving {
                    to: named.to_string(),
                    at: Some(tip.clone()),
                }
            }
        };
        let moving = matches!(event, Event::Moving { .. });
        line.log(now, event);
        moving
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
        // 152, 24, record 0124 (design D10): a landing the forge refused
        // goes once one of its submissions has left the queue.
        line.drop_left_landing();
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
                .filter(|r| r.state == RunState::Running && self.mine(r))
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
        unmet: &Unmet,
        home: Option<&str>,
        listed: bool,
        now: &str,
    ) -> Result<()> {
        let limits = &unmet.limits;
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
                    // 77, record 0122 (design D5, D7): another instance's
                    // claim lapses only once its own holder has shown no
                    // sign of life for the bound, by this instance's own
                    // observation; its run ends with no outcome, naming
                    // the claim, closed in the same write, and its holder
                    // releases its checkout (81). Its follow-up holds its
                    // checks.
                    let host = r
                        .holder()
                        .expect("another's run names its host")
                        .to_string();
                    // 88, record 0122 (design D12): a pool's job running is
                    // written running, from its run job's own start.
                    if r.state == RunState::Claimed {
                        if let Some(since) =
                            self.job_started(&host, &pools, rules.remote.as_deref())?
                        {
                            let x = line.candidate_mut(&c.id).expect("polled candidate");
                            let xr = &mut x.runs[i];
                            xr.state = RunState::Running;
                            xr.running = Some(since);
                            let system = xr.system.clone();
                            line.log(
                                now,
                                Event::Running {
                                    combination: c.revision.clone(),
                                    system,
                                    run: i,
                                },
                            );
                            continue;
                        }
                    }
                    let claimed = r.claim.as_ref().map(|k| k.at.as_str());
                    if r.state.held()
                        && self.lapsed(&host, now, &pools, rules.remote.as_deref(), claimed)?
                    {
                        line.log(
                            now,
                            Event::Lapsed {
                                combination: c.revision.clone(),
                                host: host.clone(),
                            },
                        );
                        let why = format!("{host}'s claim lapsed");
                        let none: Vec<NotRun> = given_checks(r, rules)
                            .into_iter()
                            .map(|task| NotRun {
                                task,
                                reason: why.clone(),
                                key: None,
                            })
                            .collect();
                        line.candidate_mut(&c.id).expect("polled candidate").runs[i].place = None;
                        end_run(line, &c.id, i, with_none(&none, now), now);
                    }
                    continue;
                }
                // 60, 114, record 0122 (design D8): a report is read only
                // for a running run under this host's own claim, through
                // the handle its running wrote.
                if r.state != RunState::Running {
                    continue;
                }
                let Some(run) = r.run.clone() else {
                    continue;
                };
                match self.seams.tests.progress(&run)? {
                    Progress::Running => {}
                    Progress::Lost => {
                        // 60, record 0122 (design D7): no outcome: the run
                        // ends with none for its checks, its checkout
                        // released after, and its follow-up runs them again.
                        line.log(
                            now,
                            Event::Lost {
                                combination: c.revision.clone(),
                                not_run: Vec::new(),
                            },
                        );
                        let ended = with_none(&lost(&given_checks(r, rules)), now);
                        end_run(line, &c.id, i, ended, now);
                    }
                    Progress::Finished(report) => {
                        let report = self.after_run(&run, limits, report)?;
                        self.finish_run(line, &c, i, &run, report, rules, home, now)?;
                    }
                }
            }
            // 66, 87, record 0122 (design D6): an open run no live executor
            // may take, by its system, by the branch's signers or because
            // each live host of it turned it down for what it lacks (record
            // 0074), is read as not passed once the time limit has gone
            // since it opened; each of its checks says who turned it down
            // and why, where any did. A run a live executor may take is
            // under no limit while it waits: its limit counts from its own
            // running, as its executor holds it (66).
            let x = line.candidate(&c.id).expect("polled candidate").clone();
            let mut ending = BTreeSet::new();
            if let Some(limit) = rules.timeout {
                for (n, r) in x.runs.iter().enumerate() {
                    let opened = r.opened.as_deref().or(x.opened.as_deref());
                    let over = r.state == RunState::Open
                        && opened
                            .and_then(|o| seconds_between(o, now))
                            .is_some_and(|s| s >= limit.as_secs_f64());
                    if over && !self.takeable(r, home, rules, listed, unmet, &pools, now)? {
                        ending.insert(n);
                    }
                }
            }
            {
                if !ending.is_empty() {
                    let mut closing = Vec::new();
                    let x = line.candidate_mut(&c.id).expect("polled candidate");
                    for (n, r) in x.runs.iter_mut().enumerate() {
                        if ending.contains(&n) {
                            let checks = given_checks(r, rules);
                            let why = match r.declined.is_empty() {
                                false => waited_for(&r.declined),
                                true => NO_LIVE_HOST.to_string(),
                            };
                            r.state = RunState::Closed;
                            r.ended = Some(Ended {
                                at: now.to_string(),
                                outcomes: checks
                                    .iter()
                                    .map(|c| (c.clone(), CheckOutcome::None { why: why.clone() }))
                                    .collect(),
                                timed_out: false,
                                why: Some(why.clone()),
                                untaken: true,
                            });
                            r.closed = Some(now.to_string());
                            closing.push((r.system.clone(), n, why));
                        }
                    }
                    // Record 0122 (design D1): ended and closed in one write,
                    // having made no checkout.
                    for (system, n, why) in closing {
                        for event in [
                            Event::Ended {
                                combination: c.revision.clone(),
                                system: system.clone(),
                                run: n,
                                why: Some(why),
                            },
                            Event::Closed {
                                combination: c.revision.clone(),
                                system,
                                run: n,
                            },
                        ] {
                            line.log(now, event);
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
        // Design D17: the results of the keys this run reported and of its
        // combination alone.
        let ask = report.tasks.iter().chain(&report.dependencies).fold(
            sluice_seams::ResultAsk::default().combination(&c.revision),
            |ask, t| ask.key(&t.task, &t.hash),
        );
        let mut results = self.results_for(&line.branch, &ask)?;
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
                results = self.results_for(&line.branch, &ask)?;
            } else {
                results.extend(fresh);
            }
        }
        // Design D9: a check, or a task a check was failed by, whose key
        // the tree did not bear out was signed by no one, so it is no pass
        // (15, 149): the verdict reads it failed under that key.
        let checks = &if r.checks.is_empty() {
            rules.required.clone()
        } else {
            r.checks.clone()
        };
        let mut v = verdict::read(
            &report,
            &results,
            rules,
            system,
            home,
            proven.as_deref(),
            checks,
        );
        // C.6, 20 (design D5 of the change
        // `a-signed-run-without-a-sandbox-proves-its-passes`): a check
        // failed under a key the check profile refused says why, in the
        // profile's words, on the submission blamed for it.
        for f in &mut v.failed {
            if let Some(r) = refused.iter().find(|r| r.task == f.task && r.key == f.hash) {
                f.unsigned = Some(r.why.clone());
            }
        }
        // 108, record 0122 (design D7): a replay no trusted pass proved
        // counts for nothing: the run ends with it unproven, every other
        // outcome kept, and its follow-up runs it again with no replay.
        let mut why: Option<String> = None;
        if !v.unproven.is_empty() {
            why = Some(UNPROVEN.into());
            line.log(
                now,
                Event::Unproven {
                    combination: c.revision.clone(),
                    tasks: v.unproven.iter().map(|u| u.task.clone()).collect(),
                    keys: v.unproven.clone(),
                },
            );
        }
        // 87, S18, record 0122 (design D7): checks whose results this host
        // signed with a key the branch's policy does not list are missing,
        // never failed: with nothing failed on a result the gate counts,
        // the run ends with no outcome for them, and its follow-up is open
        // for a host whose results count.
        if !v.missing.is_empty() && v.failed.is_empty() && !v.timed_out {
            why.get_or_insert_with(|| UNCOUNTED.into());
            line.log(
                now,
                Event::Uncounted {
                    combination: c.revision.clone(),
                    tasks: v.missing.clone(),
                    host: self.peer.name.clone(),
                },
            );
        }
        // 70, 55: a revision with no verdict runs every required check; a
        // report holding none of them is no pass.
        if c.every_check && !checks.is_empty() && report.tasks.is_empty() {
            v.passed = false;
        }
        // 60, 114, record 0122 (design D8): a check the report holds nothing
        // for has no outcome, counting toward nothing.
        let mut none: Vec<NotRun> = v.none.clone();
        if let Some(n) = none.first() {
            why.get_or_insert_with(|| n.reason.clone());
        }
        // 149, 114: a check every task it was failed by reads passed was
        // judged by nothing, and is read as one the run did not judge;
        // under the bound, with none failed on its own result, the run ends
        // with no outcome for it (record 0122; design D7).
        let not_run: Vec<NotRun> = report.not_run.iter().chain(&v.unjudged).cloned().collect();
        if !not_run.is_empty() {
            let judged = !v.failed.is_empty() || v.timed_out;
            let host = &self.peer.name;
            match read_not_run(line, c, i, &not_run, judged, system, host, now) {
                Some(named) => {
                    v.passed = false;
                    v.failed.extend(named);
                }
                None => {
                    why.get_or_insert_with(|| not_run[0].reason.clone());
                    none.extend(not_run);
                }
            }
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
        let ended = Ended {
            at: now.to_string(),
            outcomes: outcomes_of(checks, &v, &report, &none),
            timed_out: v.timed_out,
            why: why.filter(|_| !v.timed_out && v.failed.is_empty()),
            untaken: false,
        };
        let ended_why = ended.why.clone();
        // Record 0122 (design D3): its end, after its results and before
        // its checkout's release, which [`Sluice::perform_closes`] makes.
        let x = line.candidate_mut(&c.id).expect("polled candidate");
        let xr = &mut x.runs[i];
        xr.state = RunState::Ended;
        xr.ended = Some(ended);
        let system = xr.system.clone();
        x.failed.extend(v.failed);
        x.ran += v.ran;
        x.replayed += v.replayed;
        line.log(
            now,
            Event::Ended {
                combination: c.revision.clone(),
                system,
                run: i,
                why: ended_why,
            },
        );
        Ok(())
    }

    /// A candidate's verdict, once every run of it has its own: passed
    /// only when every run passed (86, 15).
    fn judge(&self, line: &mut Line, id: &str, rules: &Rules, now: &str) -> Result<()> {
        let c = line.candidate(id).expect("judged candidate").clone();
        if c.runs.is_empty() {
            return Ok(());
        }
        let read = read_runs(&c.runs);
        let state = if read.failed {
            CandidateState::Failed
        } else if read.timed_out {
            CandidateState::TimedOut
        } else if read.untaken {
            // 66, 87, record 0122 (design D6): a run no live executor may
            // take ended at the limit: not passed, and nobody blamed.
            CandidateState::NotPassed
        } else if read.waiting {
            // Record 0122 (design D7): each run ended with checks undecided
            // is followed up, holding only those; the verdict waits for it.
            self.follow_up(line, id, &read.follow, now);
            return Ok(());
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

    /// Record 0122 (design D7; 108, S42): for each run of candidate `id` in
    /// `follow`, by its index, ended with checks undecided, one follow-up
    /// run of its system holding only those, `follows` naming it, with no
    /// replay where any was unproven or the run it follows had none; why
    /// that run had no outcome goes with it, until its own report is read.
    fn follow_up(
        &self,
        line: &mut Line,
        id: &str,
        follow: &[(usize, Vec<String>, bool)],
        now: &str,
    ) {
        for (last, checks, unproven) in follow {
            let x = line.candidate_mut(id).expect("judged candidate");
            let prev = &mut x.runs[*last];
            let lost = prev.lost.take();
            let run = SystemRun {
                system: prev.system.clone(),
                checks: checks.clone(),
                state: RunState::Open,
                place: None,
                run: None,
                no_replay: *unproven || prev.no_replay,
                declined: BTreeMap::new(),
                lost,
                opened: Some(now.to_string()),
                claim: None,
                running: None,
                ended: None,
                closed: None,
                follows: Some(*last),
            };
            let system = run.system.clone();
            x.runs.push(run);
            let n = x.runs.len() - 1;
            let combination = x.revision.clone();
            line.log(
                now,
                Event::Opened {
                    combination,
                    system,
                    run: n,
                },
            );
        }
    }

    /// Order the queue into the branch's chain, form every candidate of
    /// it, drop candidates it does not hold, and attribute failures.
    /// Returns the chain's candidates, and whether a submission was
    /// removed, in which case the chain is arranged again (8, 12, 21).
    ///
    /// Record 0122 (design D3): while the queue's move is written, nothing is
    /// formed: the chain holds only the combinations under test or judged
    /// on the base, and those formed and not under test are dropped.
    fn arrange(
        &self,
        line: &mut Line,
        rules: &Rules,
        moving: bool,
        now: &str,
    ) -> Result<(Vec<String>, bool)> {
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
            // Record 0122 (design D6): so is one read not passed for a run no
            // live executor took, whose submissions form again from the round
            // after the one that read it, which shows it.
            let reused = match reused {
                Some(c)
                    if c.state == CandidateState::NotPassed
                        && !c.runs.iter().any(|r| {
                            r.ended.as_ref().is_some_and(|e| e.untaken && e.at == now)
                        }) =>
                {
                    None
                }
                Some(c) if self.carried(c)?.is_none() => None,
                found => found,
            };
            if moving && !reused.is_some_and(|c| c.state.pins()) {
                break;
            }
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
            if !keep {
                release.extend(self.unclosed(c));
                if c.state.under_test() {
                    dropped.push(c.id.clone());
                }
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
            refused: None,
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
                // Design D17: the results of the combinations it lands,
                // and of the keys they replayed or failed, which a pass
                // elsewhere may prove.
                let landed = cands[..n]
                    .iter()
                    .fold(sluice_seams::ResultAsk::default(), |ask, c| {
                        ask.combination(&c.revision)
                    });
                let mine = self.results_for(&line.branch, &landed)?;
                let ask = mine.iter().fold(landed, |ask, r| ask.key(&r.task, &r.hash));
                let results = self.results_for(&line.branch, &ask)?;
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
                    let missing = unsigned_checks(c, &results, rules);
                    if !missing.is_empty() {
                        unsigned.push((c.id.clone(), c.revision.clone(), missing));
                    }
                }
                if !unsigned.is_empty() {
                    // 78, rule core.22, record 0122 (design D9): its runs
                    // stand as written; it is under test again, open to
                    // claims, and a follow-up holds each check with no
                    // counted pass behind it.
                    for (id, combination, missing) in unsigned {
                        let c = line.candidate_mut(&id).expect("a chain's candidate");
                        c.state = CandidateState::Testing;
                        c.passed_at = None;
                        line.log(
                            now,
                            Event::Unsigned {
                                combination: combination.clone(),
                            },
                        );
                        self.follow_up(line, &id, &missing, now);
                        self.seams
                            .state
                            .publish_candidate(&line.branch, &id, &combination)?;
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
                refused: None,
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
            let claim = rules.signed.then(|| Claim::new(&self.peer.name, now));
            let c = line.candidate_mut(&id).expect("a chain's candidate");
            c.runs[i].state = RunState::Claimed;
            c.runs[i].place = Some(place.clone());
            c.runs[i].claim = claim;
            c.runs[i].declined.remove(&self.peer.name);
            let c = c.clone();
            line.log(
                now,
                Event::Claimed {
                    combination: c.revision.clone(),
                    system: c.runs[i].system.clone(),
                    host: self.peer.name.clone(),
                    unanswered: None,
                },
            );
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
            // for it. Design D8 (75): on a branch whose records are on its
            // remote, a lead with no room opens it all the same, its runs
            // open to every host the branch accepts.
            let part = match mine {
                Some(_) if full && rules.signed => None,
                Some(_) if full => break,
                Some(_) => match self.seams.tests.hold(RunKind::Queue)? {
                    Some(part) => Some(part),
                    None if rules.signed => {
                        full = true;
                        None
                    }
                    None => break,
                },
                None => None,
            };
            let mine = mine.filter(|_| part.is_some() || !rules.signed);
            let place = match mine {
                Some(i) => {
                    let place = self.next_place(line);
                    if let Some(part) = part {
                        parts.insert(place.clone(), part);
                    }
                    runs[i].state = RunState::Claimed;
                    runs[i].place = Some(place.clone());
                    runs[i].claim = rules.signed.then(|| Claim::new(&self.peer.name, now));
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
            for r in &mut runs {
                r.opened = Some(now.to_string());
            }
            c.runs = runs;
            c.opened = Some(now.to_string());
            c.every_check = !every_known;
            let c = c.clone();
            // Record 0122 (design D1): each run opened, and the one this
            // host took claimed.
            for (n, r) in c.runs.iter().enumerate() {
                line.log(
                    now,
                    Event::Opened {
                        combination: c.revision.clone(),
                        system: r.system.clone(),
                        run: n,
                    },
                );
                if r.state == RunState::Claimed {
                    line.log(
                        now,
                        Event::Claimed {
                            combination: c.revision.clone(),
                            system: r.system.clone(),
                            host: self.peer.name.clone(),
                            unanswered: None,
                        },
                    );
                }
            }
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

    /// Record 0122 (design D3): the checkouts of `c`'s runs this host holds
    /// and has not closed, which go to the branch's places to release when
    /// its record goes.
    fn unclosed(&self, c: &Candidate) -> Vec<String> {
        c.runs
            .iter()
            .filter(|r| self.mine(r) && r.state != RunState::Closed)
            .filter_map(|r| r.place.clone())
            .collect()
    }

    /// Whether `run` is this instance's: its claim names it, or the
    /// branch keeps no claims (75).
    fn mine(&self, run: &SystemRun) -> bool {
        run.holder().is_none_or(|h| h == self.peer.name)
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
    /// that reason already, logging each once. 78, 87, record 0122 (design
    /// D6): a host signing as a signer the gate trusts with a key the
    /// branch's policy does not list says so too, on each open run it would
    /// take with a listed key, so no lead waits on it.
    fn note_declined(
        &self,
        line: &mut Line,
        rules: &Rules,
        home: Option<&str>,
        listed: bool,
        unmet: &Unmet,
        now: &str,
    ) {
        let Some(why) = self.declines(rules, listed, unmet) else {
            return;
        };
        let why = why.as_str();
        let me = &self.peer.name;
        let mut said = Vec::new();
        for c in line
            .doc
            .candidates
            .iter_mut()
            .filter(|c| c.state.under_test())
        {
            for r in &mut c.runs {
                if !self.may_claim(r, home, rules, true)
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

    /// Design D4, record 0122 (design D6): why this host turns down the
    /// open runs it would otherwise take: what the round's look found it
    /// lacks, or, on a branch whose records are on its remote, that it
    /// signs as a signer the gate trusts with a key the branch's policy
    /// does not list. `None` where it turns down none.
    fn declines(&self, rules: &Rules, listed: bool, unmet: &Unmet) -> Option<String> {
        if let Some(why) = unmet.found() {
            return Some(why.to_string());
        }
        let signer = self.peer.signer.as_deref()?;
        (rules.signed && !listed && rules.claims.contains_key(signer)).then(|| {
            format!("the branch's policy lists no key this host signs with for the signer {signer}")
        })
    }

    /// 75, 79, 87, 107: whether this instance may take `run`: open and
    /// unclaimed, of the system it is, and one whose result signed here
    /// the gate would count ([`Sluice::counts_here`]).
    fn may_claim(&self, run: &SystemRun, home: Option<&str>, rules: &Rules, listed: bool) -> bool {
        run.state == RunState::Open
            && run.claim.is_none()
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

    /// 88, 107, record 0122 (design D3, D12): claim for a pool each open
    /// run of a candidate under test that no live instance able to take it
    /// has room for by its last sign of life, the policy's pools tried in
    /// order; its job is dispatched by [`Sluice::perform_dispatches`].
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
                || self.lapsed(&host, now, &[], None, None)?
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
                    .filter(|(_, r)| r.state == RunState::Open && r.claim.is_none())
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
            // A pool with a runner of the run's system, whose signer the
            // gate trusts.
            let pool = policy.pools.iter().find(|p| {
                trusted(&p.signer)
                    && match &run.system {
                        Some(s) => p.runners.contains_key(s),
                        None => !p.runners.is_empty(),
                    }
            });
            let Some(pool) = pool else {
                continue;
            };
            let place = self.next_place(line);
            let claim = format!("pool/{}/{}", pool.name, line.doc.next_place - 1);
            // 88, record 0122 (design D3, D12): the claim is written first;
            // the job is dispatched by the round's perform, once the pool
            // answers that no job of the claim is there.
            let x = line.candidate_mut(&id).expect("a chain's candidate");
            let r = &mut x.runs[i];
            r.state = RunState::Claimed;
            r.claim = Some(Claim::new(&claim, now));
            r.place = Some(place);
        }
        Ok(())
    }

    /// 88, 107, 134, record 0122 (design D3, D12): each run of `branch`
    /// written claimed by a pool's claim whose job is not yet dispatched
    /// has it dispatched once the pool answers that no job of the claim is
    /// there, and is marked dispatched, as it is where the pool answers
    /// one is, sending nothing; the claim's event is logged with it,
    /// carrying the forge's words where the dispatch's answer was not read,
    /// which is never sent again. A dispatch the forge refused ends the run
    /// with no outcome, naming the refusal, closed, its follow-up the lead's
    /// to open, and fails the round with the forge's words.
    fn perform_dispatches(&self, branch: &str) -> Result<()> {
        let line = self.read_line(branch)?;
        let undispatched: Vec<(Candidate, usize)> = line
            .doc
            .candidates
            .iter()
            .filter(|c| c.state.under_test())
            .flat_map(|c| {
                c.runs
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| {
                        r.state == RunState::Claimed
                            && r.claim.as_ref().is_some_and(|k| {
                                k.by.starts_with("pool/") && k.dispatched.is_none()
                            })
                    })
                    .map(|(i, _)| (c.clone(), i))
                    .collect::<Vec<_>>()
            })
            .collect();
        if undispatched.is_empty() {
            return Ok(());
        }
        let Some(base) = line.doc.base.clone() else {
            return Ok(());
        };
        let Some(policy) = self.policy_at(&base)? else {
            return Ok(());
        };
        let rules = self.gate(branch, &base)?;
        for (c, i) in undispatched {
            let run = &c.runs[i];
            let claim = run.holder().expect("a pool's claim").to_string();
            let name = claim.split('/').nth(1).unwrap_or_default();
            let Some(pool) = policy.pools.iter().find(|p| p.name == name) else {
                continue;
            };
            let job = self
                .seams
                .tests
                .job(&pool.kind, &claim, rules.remote.as_deref())?;
            let unanswered = match job {
                Job::None => {
                    let runner = match &run.system {
                        Some(s) => pool.runners.get(s).cloned(),
                        None => pool.runners.values().next().cloned(),
                    }
                    .unwrap_or_default();
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
                        branch: branch.to_string(),
                        candidate: c.id.clone(),
                        combination: c.revision.clone(),
                        system: run.system.clone(),
                        claim: claim.clone(),
                    });
                    match launched {
                        Ok(()) => None,
                        Err(e) if sluice_seams::unanswered(&e) => Some(format!("{e:#}")),
                        Err(e) if sluice_seams::stopped(&e) => return Err(e),
                        Err(e) => {
                            let why = format!("the forge refused its dispatch: {e:#}");
                            self.update(branch, |l| {
                                let now = self.now();
                                let Some(xr) =
                                    l.candidate_mut(&c.id).and_then(|x| x.runs.get_mut(i))
                                else {
                                    return Ok(());
                                };
                                if xr.state != RunState::Claimed || xr.holder() != Some(&claim) {
                                    return Ok(());
                                }
                                xr.place = None;
                                let none: Vec<NotRun> = given_checks(xr, &rules)
                                    .into_iter()
                                    .map(|task| NotRun {
                                        task,
                                        reason: why.clone(),
                                        key: None,
                                    })
                                    .collect();
                                end_run(l, &c.id, i, with_none(&none, &now), &now);
                                Ok(())
                            })?;
                            return Err(e);
                        }
                    }
                }
                Job::Queued | Job::Running { .. } | Job::Ended { .. } => None,
            };
            self.update(branch, |l| {
                let now = self.now();
                let Some(x) = l.candidate_mut(&c.id) else {
                    return Ok(());
                };
                let combination = x.revision.clone();
                let Some(xr) = x.runs.get_mut(i) else {
                    return Ok(());
                };
                let Some(k) = xr.claim.as_mut().filter(|k| k.by == claim) else {
                    return Ok(());
                };
                k.dispatched = Some(now.clone());
                let system = xr.system.clone();
                l.log(
                    &now,
                    Event::Claimed {
                        combination,
                        system,
                        host: claim.clone(),
                        unanswered: unanswered.clone(),
                    },
                );
                Ok(())
            })?;
        }
        Ok(())
    }

    /// 88, record 0122 (design D12): the start of the run job of the pool's
    /// job under `claim`, where it runs; `None` for any claim that is no
    /// pool's, or whose job does not run.
    fn job_started(
        &self,
        claim: &str,
        pools: &[crate::policy::Pool],
        remote: Option<&str>,
    ) -> Result<Option<String>> {
        let Some(rest) = claim.strip_prefix("pool/") else {
            return Ok(None);
        };
        let name = rest.split('/').next().unwrap_or_default();
        let Some(p) = pools.iter().find(|p| p.name == name) else {
            return Ok(None);
        };
        Ok(match self.seams.tests.job(&p.kind, claim, remote)? {
            Job::Running { since } => Some(since),
            _ => None,
        })
    }

    /// 66, 87, 107, record 0122 (design D6): whether a live executor may take
    /// `run`: this host, where it may claim it and meets what the branch's
    /// runs ask of it; a host whose sign of life has not lapsed by this
    /// host's own observation, that takes the run's system, signs as a
    /// signer the gate trusts and has not turned the run down; or a pool the
    /// policy lists for the run's system, `pools`, whose signer the gate
    /// trusts.
    #[allow(clippy::too_many_arguments)]
    fn takeable(
        &self,
        run: &SystemRun,
        home: Option<&str>,
        rules: &Rules,
        listed: bool,
        unmet: &Unmet,
        pools: &[crate::policy::Pool],
        now: &str,
    ) -> Result<bool> {
        if self.may_claim(run, home, rules, listed) && self.meets(unmet) {
            return Ok(true);
        }
        let trusted = |signer: &str| {
            rules
                .claims
                .get(signer)
                .is_some_and(|(confined, _)| !rules.confined || *confined)
        };
        if !rules.signed {
            return Ok(false);
        }
        if pools.iter().any(|p| {
            trusted(&p.signer)
                && match &run.system {
                    Some(s) => p.runners.contains_key(s),
                    None => !p.runners.is_empty(),
                }
        }) {
            return Ok(true);
        }
        for (host, (_, beat)) in self.seams.state.beats()? {
            if host == self.peer.name
                || host.starts_with("pool/")
                || run.declined.contains_key(&host)
            {
                continue;
            }
            let v: serde_json::Value = serde_json::from_slice(&beat).unwrap_or_default();
            if !v["signer"].as_str().is_some_and(trusted) {
                continue;
            }
            let takes = match &run.system {
                None => true,
                Some(s) => {
                    v["system"].as_str() == Some(s.as_str())
                        || v["systems"]
                            .as_array()
                            .is_some_and(|l| l.iter().any(|x| x.as_str() == Some(s.as_str())))
                }
            };
            if takes && !self.lapsed(&host, now, &[], None, None)? {
                return Ok(true);
            }
        }
        Ok(false)
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
        claimed: Option<&str>,
    ) -> Result<bool> {
        if let Some(rest) = host.strip_prefix("pool/") {
            let name = rest.split('/').next().unwrap_or_default();
            let Some(p) = pools.iter().find(|p| p.name == name) else {
                // A pool the policy no longer names runs nothing for it.
                return Ok(true);
            };
            // Record 0122 (design D12): a job that ended with no record, or
            // none of the claim's title past the sign-of-life bound since
            // the claim, is a lapsed claim.
            return Ok(match self.seams.tests.job(&p.kind, host, remote)? {
                Job::Ended { recorded } => !recorded,
                Job::None => claimed
                    .and_then(|at| seconds_between(at, now))
                    .is_none_or(|s| s >= self.peer.alive.as_secs_f64()),
                Job::Queued | Job::Running { .. } => false,
            });
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
        self.show_alive(home.as_deref())?;

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
                        r.state == RunState::Running && r.holder() == Some(me.as_str())
                    })
                    .map(|(i, _)| (c.clone(), i))
                    .collect::<Vec<_>>()
            })
            .collect();
        for (c, i) in mine {
            // 60, 114, record 0122 (design D8): through the handle its
            // running wrote, and only that.
            let Some(handle) = c.runs[i].run.clone() else {
                continue;
            };
            let place = c.runs[i].place.clone();
            match self.seams.tests.progress(&handle)? {
                Progress::Running => {}
                Progress::Lost => {
                    // 60, record 0122 (design D7): ended with no outcome
                    // under this host's own claim, its checkout released and
                    // the run closed below.
                    self.update(branch, |l| {
                        let now = self.now();
                        let Some(r) = l.candidate(&c.id).map(|x| x.runs[i].clone()) else {
                            return Ok(());
                        };
                        if r.holder() != Some(me.as_str()) || r.state != RunState::Running {
                            return Ok(());
                        }
                        l.log(
                            &now,
                            Event::Lost {
                                combination: c.revision.clone(),
                                not_run: Vec::new(),
                            },
                        );
                        let ended = with_none(&lost(&given_checks(&r, &rules)), &now);
                        end_run(l, &c.id, i, ended, &now);
                        Ok(())
                    })?;
                }
                Progress::Finished(report) => {
                    let report = self.after_run(&handle, &unmet.limits, report)?;
                    let mut release_now = false;
                    self.update(branch, |l| {
                        let now = self.now();
                        let Some(x) = l.candidate(&c.id).cloned() else {
                            return Ok(());
                        };
                        let r = &x.runs[i];
                        // 77: a report under a claim this host no longer
                        // holds is never written.
                        if r.holder() != Some(me.as_str()) || r.state != RunState::Running {
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
                        // A run opened again leaves its place for this host
                        // to release below; an ended one is released, then
                        // closed, by [`Sluice::perform_closes`].
                        let reopened = l
                            .candidate(&c.id)
                            .is_some_and(|x| x.runs[i].state == RunState::Open);
                        release_now = false;
                        if let Some(p) = place.as_ref().filter(|_| reopened) {
                            l.doc.release.retain(|q| q != p);
                            release_now = true;
                        }
                        Ok(())
                    })?;
                    if let Some(p) = place.filter(|_| release_now) {
                        let outcome = self.release_made(branch, &p)?;
                        if outcome.how != Releasing::Released {
                            releases.push(outcome);
                        }
                    }
                }
            }
        }
        // Record 0122 (design D3): this host's ended runs released, then
        // closed.
        self.perform_closes(branch, releases)?;

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
                r.state = RunState::Claimed;
                r.claim = Some(Claim::new(&me, &now));
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
        if self.declines(&rules, listed, &unmet).is_some() {
            self.update(branch, |l| {
                let now = self.now();
                self.note_declined(l, &rules, home.as_deref(), listed, &unmet, &now);
                Ok(())
            })?;
        }
        self.perform_starts(branch, Some(&unmet), parts, releases)
    }

    /// 77, 107, record 0122 (design D5): this host's sign of life, with the
    /// room a lead reads, the host's, every repository's runs counted (71).
    fn show_alive(&self, home: Option<&str>) -> Result<()> {
        let beat = serde_json::json!({
            "host": self.peer.name,
            "signer": self.peer.signer,
            "system": home,
            "systems": self.peer.systems,
            "held": self.seams.tests.running()?.total(),
            "bound": self.seams.tests.bound()?,
            "at": self.now(),
        });
        self.seams
            .state
            .beat(&self.peer.name, beat.to_string().as_bytes())
    }

    /// Record 0122 (design D5; 73, 77): on a branch whose records are on its
    /// remote, this host's sign of life, written in every round of its own,
    /// the lead's among them; nothing on a branch in its own files.
    fn alive_on(&self, branch: &str) -> Result<()> {
        let Some(tip) = self.seams.branch.read(branch)? else {
            return Ok(());
        };
        if !self.rules_at(branch, &tip)?.is_some_and(|r| r.signed) {
            return Ok(());
        }
        let home = self.home_system(&tip)?;
        self.show_alive(home.as_deref())
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
                Ok(Moved::Refused(words)) => Levelled::Behind {
                    at: copy,
                    why: words,
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
/// the run. Unjudged and under the bound, the checks have no outcome, the
/// run's loss is logged, and nobody is blamed: `None`, and the caller ends
/// the run with no outcome for them, which its follow-up runs again
/// (record 0122; design D7); the run keeps that `host`, the host reading
/// it, had no outcome of it, on the run's system, and each check not run
/// with why, until its follow-up's report is read (design D7 of the change
/// `a-pass-nothing-proves-blames-no-one`; record 0105). Past the bound, or
/// judged, each check is returned named with why, to be read as failed.
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
            xr.lost = Some(LostRun {
                host: host.to_string(),
                system: xr.system.clone(),
                not_run: not_run.to_vec(),
            });
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

/// 133, 134, record 0122 (design D3): an error a start reads nothing of
/// and passes on, ending the round: a remote that could not answer, a read
/// of it refused, or the process stopped part way through the act.
fn passes_through(e: &anyhow::Error) -> bool {
    sluice_seams::unanswered(e) || read_refused(e).is_some() || sluice_seams::stopped(e)
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

/// The checks run `r` was given: its own, or where it names none, every
/// required check of the gate.
fn given_checks(r: &SystemRun, rules: &Rules) -> Vec<String> {
    match r.checks.is_empty() {
        true => rules.required.clone(),
        false => r.checks.clone(),
    }
}

/// Record 0122 (design D7): a candidate's runs read together, each run and
/// its follow-ups as one, each check by its last outcome.
#[derive(Default)]
struct RunsRead {
    /// A check failed in some run: read at once.
    failed: bool,
    /// The time limit ended some run (66): read at once.
    timed_out: bool,
    /// Some run ended with no live executor that may take it (66, 87;
    /// design D6): read at once, with no follow-up.
    untaken: bool,
    /// Some run has not ended, or ended with checks undecided.
    waiting: bool,
    /// Each run, by its index, ended with checks undecided and followed by
    /// none: those checks, and whether any of them was unproven.
    follow: Vec<(usize, Vec<String>, bool)>,
}

/// Record 0122 (design D7): `runs` read over each run and its follow-ups.
fn read_runs(runs: &[SystemRun]) -> RunsRead {
    let mut read = RunsRead::default();
    for root in (0..runs.len()).filter(|n| runs[*n].follows.is_none()) {
        let mut chain = vec![root];
        let mut cur = root;
        while let Some(next) = runs.iter().position(|r| r.follows == Some(cur)) {
            chain.push(next);
            cur = next;
        }
        let mut last: BTreeMap<&String, &CheckOutcome> = BTreeMap::new();
        let mut done = true;
        for &n in &chain {
            match &runs[n].ended {
                Some(e) if runs[n].state.done() => {
                    // 66: a run its time limit ended reads as timed out,
                    // whatever of it failed.
                    read.timed_out |= e.timed_out;
                    read.untaken |= e.untaken;
                    for (check, outcome) in &e.outcomes {
                        read.failed |= *outcome == CheckOutcome::Failed && !e.timed_out;
                        last.insert(check, outcome);
                    }
                }
                _ => done = false,
            }
        }
        if !done {
            read.waiting = true;
            continue;
        }
        let undecided: Vec<(&String, &CheckOutcome)> = last
            .into_iter()
            .filter(|(_, o)| **o != CheckOutcome::Passed)
            .collect();
        if !undecided.is_empty() {
            read.waiting = true;
            let unproven = undecided.iter().any(|(_, o)| **o == CheckOutcome::Unproven);
            let checks = undecided.into_iter().map(|(c, _)| c.clone()).collect();
            read.follow.push((cur, checks, unproven));
        }
    }
    read
}

/// 71, record 0122 (design D7): why a run claimed and not started for want
/// of the host's room ended with no outcome.
const AT_BOUND: &str = "the host had no room for it";

/// Record 0122 (design D1, D7): run `i` of candidate `cand` in `l` written
/// ended with `ended`, and closed in the same write where it holds no
/// checkout, each logged.
fn end_run(l: &mut Line, cand: &str, i: usize, ended: Ended, now: &str) {
    let Some(x) = l.candidate_mut(cand) else {
        return;
    };
    let combination = x.revision.clone();
    let Some(xr) = x.runs.get_mut(i) else {
        return;
    };
    let why = ended.why.clone();
    xr.state = RunState::Ended;
    xr.ended = Some(ended);
    let system = xr.system.clone();
    let closes = xr.place.is_none();
    if closes {
        xr.state = RunState::Closed;
        xr.closed = Some(now.to_string());
    }
    l.log(
        now,
        Event::Ended {
            combination: combination.clone(),
            system: system.clone(),
            run: i,
            why,
        },
    );
    if closes {
        l.log(
            now,
            Event::Closed {
                combination,
                system,
                run: i,
            },
        );
    }
}

/// 60, record 0122 (design D7): each of `checks` with no outcome, the
/// run's executor having stopped before it reported.
fn lost(checks: &[String]) -> Vec<NotRun> {
    checks
        .iter()
        .map(|task| NotRun {
            task: task.clone(),
            reason: LOST.into(),
            key: None,
        })
        .collect()
}

/// Record 0122 (design D7): the end of a run with no outcome for each
/// check of `none`, with why, the first its cause.
fn with_none(none: &[NotRun], now: &str) -> Ended {
    Ended {
        at: now.to_string(),
        outcomes: none
            .iter()
            .map(|n| {
                (
                    n.task.clone(),
                    CheckOutcome::None {
                        why: n.reason.clone(),
                    },
                )
            })
            .collect(),
        timed_out: false,
        why: none.first().map(|n| n.reason.clone()),
        untaken: false,
    }
}

/// Record 0122 (design D1): the end of a run whose checks `named` are read
/// failed, each failed.
fn failing(named: &[FailedTask], now: &str) -> Ended {
    Ended {
        at: now.to_string(),
        outcomes: named
            .iter()
            .map(|f| (f.task.clone(), CheckOutcome::Failed))
            .collect(),
        timed_out: false,
        why: None,
        untaken: false,
    }
}

/// 108, record 0122 (design D7): why a run whose replays no counted pass
/// proved ended with checks undecided.
const UNPROVEN: &str = "a replay no counted pass proved";

/// 87, S18, record 0122 (design D7): why a run whose results count for
/// nothing for the branch's gate ended with no outcome for them.
const UNCOUNTED: &str = "no result of it counts for the branch's gate";

/// 60, record 0122 (design D7): why a run whose executor stopped before
/// it reported ended with no outcome.
const LOST: &str = "its executor stopped before it reported";

/// Record 0122 (design D1, D7, D8): each check of a run by its outcome in
/// its end: every task its report names, passed unless `v` read it failed
/// or unproven; each check with none, `none`, with why; a task whose
/// results count for nothing here with none (87, S18); and each check the
/// run was given, `given`, a tag among them, passed where the run passed or
/// its report named a task of it, and otherwise none, its report holding
/// nothing for it (15, 60, 108).
fn outcomes_of(
    given: &[String],
    v: &verdict::Verdict,
    report: &RunReport,
    none: &[NotRun],
) -> BTreeMap<String, CheckOutcome> {
    let mut outcomes: BTreeMap<String, CheckOutcome> = BTreeMap::new();
    for t in &report.tasks {
        outcomes.insert(t.task.clone(), CheckOutcome::Passed);
    }
    for n in none {
        outcomes.insert(
            n.task.clone(),
            CheckOutcome::None {
                why: n.reason.clone(),
            },
        );
    }
    for task in &v.missing {
        outcomes.insert(
            task.clone(),
            CheckOutcome::None {
                why: UNCOUNTED.into(),
            },
        );
    }
    for u in &v.unproven {
        outcomes.insert(u.task.clone(), CheckOutcome::Unproven);
    }
    for f in &v.failed {
        outcomes.insert(f.task.clone(), CheckOutcome::Failed);
    }
    let named = !report.tasks.is_empty() || !v.failed.is_empty();
    for check in given {
        outcomes
            .entry(check.clone())
            .or_insert_with(|| match v.passed || named {
                true => CheckOutcome::Passed,
                false => CheckOutcome::None {
                    why: verdict::NO_OUTCOME.into(),
                },
            });
    }
    // A run that did not pass reads as one that did not, whatever it was
    // given.
    if !v.passed && !v.timed_out && outcomes.values().all(|o| *o == CheckOutcome::Passed) {
        outcomes.insert(
            UNNAMED_TESTS.into(),
            CheckOutcome::None {
                why: verdict::NO_OUTCOME.into(),
            },
        );
    }
    outcomes
}

/// 66, 87, record 0122 (design D6): why a run no host turned down, with no
/// live executor that may take it, ended at the time limit.
const NO_LIVE_HOST: &str = "no host that may take it was live";

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
        declined: BTreeMap::new(),
        lost: None,
        opened: None,
        claim: None,
        running: None,
        ended: None,
        closed: None,
        follows: None,
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

/// Design D8, record 0122 (design D9): the checks of `c` the signed results
/// do not bear out, by the last run of each run and its follow-ups that
/// read them passed, as `follow_up` takes them: a check passes only through
/// a result a signer the gate trusts signed for this very combination,
/// read as passing, a replay only through a trusted pass signed for its key
/// on its system (108); a tag, through every such result of its runs, of
/// which there is one at least. Empty where they bear out every pass.
fn unsigned_checks(
    c: &Candidate,
    results: &[ResultLine],
    rules: &Rules,
) -> Vec<(usize, Vec<String>, bool)> {
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
    let counted = |r: &ResultLine| match (r.kind, r.outcome) {
        (ResultKind::Replayed, _) => proven(&r.task, &r.hash, r.system.as_deref()),
        (ResultKind::Ran, sluice_seams::Outcome::Pass) => true,
        (ResultKind::Ran, sluice_seams::Outcome::Fail) => {
            rules.flaky == crate::policy::Flaky::Pass
                && proven(&r.task, &r.hash, r.system.as_deref())
        }
    };
    let mut unsigned = Vec::new();
    for root in (0..c.runs.len()).filter(|n| c.runs[*n].follows.is_none()) {
        let system = c.runs[root].system.clone();
        let mut chain = vec![root];
        let mut cur = root;
        while let Some(next) = c.runs.iter().position(|r| r.follows == Some(cur)) {
            chain.push(next);
            cur = next;
        }
        let mut last: BTreeMap<&String, &CheckOutcome> = BTreeMap::new();
        for &n in &chain {
            if let Some(e) = &c.runs[n].ended {
                last.extend(e.outcomes.iter());
            }
        }
        let mine: Vec<&ResultLine> = results
            .iter()
            .filter(|r| r.combination == c.revision && trusted(r))
            .filter(|r| system.is_none() || r.system == system)
            .collect();
        let bears = |check: &str| match check.starts_with('#') {
            true => !mine.is_empty() && mine.iter().all(|r| counted(r)),
            false => mine.iter().any(|r| r.task == check && counted(r)),
        };
        let missing: Vec<String> = last
            .into_iter()
            .filter(|(check, o)| **o == CheckOutcome::Passed && !bears(check))
            .map(|(check, _)| check.clone())
            .collect();
        if !missing.is_empty() {
            unsigned.push((cur, missing, false));
        }
    }
    unsigned
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
/// Record 0122 (design D3): while the queue's move is written, a
/// submission no combination holds is queued, never waiting: the chain
/// stops at what is under test, not at what it waits on (7).
fn derive_states(line: &mut Line, chain: &[String], moving: bool) {
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
            match moving {
                true => State::Queued,
                false => State::Waiting,
            }
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
        // Listed before #1's run is read; #1's run ended, its checkout
        // released after its end is written (record 0122; design D3).
        let first_place = first.runs[0].place.clone().expect("a run of #1 here");
        assert_eq!(line.doc.release, std::slice::from_ref(&place));
        let read = holding(&line, &[one]).expect("#1's combination");
        assert_eq!(read.runs[0].state, RunState::Ended);
        assert!(result_read(&s, &first.revision), "#1's run was read");
        assert!(!result_read(&s, &last.revision), "#3's run was read");
        assert_eq!(f.world.live_runs(), 2, "#2's and #3's");

        let mut releases = Vec::new();
        s.perform("main", Some(&unmet), parts, &mut releases)
            .unwrap();
        drop(lock);
        assert!(releases.is_empty(), "{releases:?}");
        assert!(f.world.released_places().contains(&place));
        assert!(f.world.released_places().contains(&first_place));
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
