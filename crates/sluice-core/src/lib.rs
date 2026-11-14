//! The core of sluice: the policy, the records, the chain's order, forming,
//! verdicts, attribution, the window and the landing, as one step
//! function over a branch's records (ADR 0005).
//!
//! The core does no I/O but through the five seams of `sluice-seams`
//! (rule core.2), names no tool (rule core.1), and holds nothing between
//! steps outside the records (rule core.14).

pub mod branches;
pub mod equip;
pub mod home;
pub mod line;
pub mod notice;
pub mod ops;
pub mod order;
pub mod overview;
pub mod owed;
pub mod policy;
pub mod preview;
pub mod proof;
pub mod records;
pub mod refused;
pub mod refused_writes;
pub mod release;
pub mod session;
pub mod step;
pub mod text;
pub mod verdict;
pub mod verdict_view;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use sluice_seams::{Branch, Checkout, Driver, ReadRefused, Rev, State, Tests, Written};

use crate::line::Line;
use crate::policy::{Policy, Rules, POLICY_PATH};
use crate::records::{parse_lines, to_line, ResultLine, SetAside, SET_ASIDE_SCHEMA};
use crate::refused::Refusal;

pub use notice::notice;
pub use ops::{
    Accepted, AmendRefusal, CopyDiffers, CopyHow, Forecast, Forecasted, GroupRequest, KeptOut,
    Lookup, Refused, Requester, Several, Status, SubmitHead, SubmitRefusal, SubmitRequest,
    SubmittedHead, Unread, WithdrawRefusal, Withdrew,
};
pub use owed::{
    Collect, ConfirmAs, Confirmation, Confirmations, DeliveryView, Owed, OwedOutcome, OwedView,
    ReaderView,
};
pub use step::{Round, Started, Step};

/// The `tracing` target of a failure's output, one event per failed task
/// with the output as its message: telemetry sends it as its level says,
/// and nothing reads it back (ADR 0022). The kept copy is the record (20).
pub const OUTPUT: &str = "sluice::output";

/// The time, for the records' timestamps. Not a seam: the environment.
pub trait Clock {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// The five seams, each bound by one profile (52).
#[derive(Clone, Copy)]
pub struct Seams<'a> {
    pub checkout: &'a dyn Checkout,
    pub tests: &'a dyn Tests,
    pub branch: &'a dyn Branch,
    pub driver: &'a dyn Driver,
    pub state: &'a dyn State,
}

/// 64, 129 (design D16): what a forecast read as the host holds the
/// repository could not form from, since the host process's reading did not
/// bring it to the host. A forecast asks a reading afresh once; where that
/// reading is current and still brings no revision of a branch, the branch
/// is no branch, refused as one (64); anything else still lacking is said
/// as a read it could not make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unheld {
    /// The branch named: the host holds no revision of it.
    Branch(String),
    /// The work queued on the branch named, or a combination formed on it.
    Work(String),
}

impl std::fmt::Display for Unheld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = match self {
            Unheld::Branch(b) => format!("a revision of {b}"),
            Unheld::Work(b) => format!("the work queued on {b} or a combination formed on it"),
        };
        write!(
            f,
            "the host process's reading did not bring {what} to this host"
        )
    }
}

impl std::error::Error for Unheld {}

pub struct Sluice<'a> {
    seams: Seams<'a>,
    clock: &'a dyn Clock,
    peer: Peer,
    /// 124: whether it reads the policy only as far as guarding needs.
    guarding: bool,
    /// 129 (design D16): whether it reads every branch as the host holds
    /// it since the host process's last ask, asking no remote: a one-shot
    /// read's. A round never reads held.
    held: bool,
    /// 129: each policy read, by the revision it was read at.
    policies: Keeping<'a>,
}

/// 129: each policy a reader read, by the revision it was read at and
/// whether it was read whole or only as far as guarding needs: what the
/// policy's bytes decided there, the policy, no policy file, or the words
/// it was refused with. A read that failed, an object missing or git's own
/// I/O, decided nothing and is not kept. Each pass of `sluice run`, each
/// watch tick and each wait's read marks the revisions it used, and
/// [`Policies::next_pass`] drops those the last did not use, so what is
/// kept is at most a policy per branch and worktree in sight. A policy at
/// a revision is a function of that revision's bytes and of this sluice:
/// keeping it holds nothing the core decides between steps (rule core.14).
#[derive(Default)]
pub struct Policies {
    kept: std::cell::RefCell<std::collections::BTreeMap<(Rev, bool), Decided>>,
    used: std::cell::RefCell<std::collections::BTreeSet<(Rev, bool)>>,
}

/// What a policy's bytes decided at a revision.
#[derive(Clone)]
enum Decided {
    Read(Option<Policy>),
    /// The words it was refused with, beneath the revision's.
    Refused(String),
}

impl Policies {
    /// Start a pass, a tick or a wait's read: forget every policy the last
    /// did not use.
    pub fn next_pass(&self) {
        let used = std::mem::take(&mut *self.used.borrow_mut());
        self.kept.borrow_mut().retain(|k, _| used.contains(k));
    }

    fn get(&self, key: &(Rev, bool)) -> Option<Decided> {
        let found = self.kept.borrow().get(key).cloned();
        if found.is_some() {
            self.used.borrow_mut().insert(key.clone());
        }
        found
    }

    fn keep(&self, key: (Rev, bool), decided: Decided) {
        self.used.borrow_mut().insert(key.clone());
        self.kept.borrow_mut().insert(key, decided);
    }
}

/// The policies a sluice keeps: its own, or a store it was given that
/// outlives it.
enum Keeping<'a> {
    Own(Policies),
    Shared(&'a Policies),
}

impl std::ops::Deref for Keeping<'_> {
    type Target = Policies;

    fn deref(&self) -> &Policies {
        match self {
            Keeping::Own(p) => p,
            Keeping::Shared(p) => p,
        }
    }
}

/// 72 to 74: who this instance is among the hosts that keep a branch
/// whose records are on its remote, and the lease it waits out before it
/// takes one another instance keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub name: String,
    pub lease: std::time::Duration,
    /// 77: how long a host's sign of life may stand before its claims
    /// lapse.
    pub alive: std::time::Duration,
    /// 78: the signer this host's results are signed by, where it has a
    /// signing key; it claims runs only of branches that trust it.
    pub signer: Option<String>,
    /// 87: the systems this host runs, by the policy's names; `None` for
    /// the one system it is.
    pub systems: Option<Vec<String>>,
    /// 51: the span an outcome this host's rounds record is owed before it
    /// expires, the host's `[deliveries] expire_after` (weir 104; record
    /// 0102; design D9).
    pub expire_after: std::time::Duration,
}

/// 51: an outcome's span where the host sets no `[deliveries]
/// expire_after`: a week (design D9).
pub const EXPIRE_AFTER: std::time::Duration = std::time::Duration::from_secs(168 * 60 * 60);

impl Default for Peer {
    fn default() -> Self {
        Peer {
            name: "this-host".into(),
            lease: std::time::Duration::from_secs(300),
            alive: std::time::Duration::from_secs(120),
            signer: None,
            systems: None,
            expire_after: EXPIRE_AFTER,
        }
    }
}

/// How many times a write refused because the records moved is tried
/// again from a fresh read before the call gives up (62).
const WRITE_TRIES: usize = 16;

impl<'a> Sluice<'a> {
    pub fn new(seams: Seams<'a>, clock: &'a dyn Clock) -> Self {
        Sluice {
            seams,
            clock,
            peer: Peer::default(),
            guarding: false,
            held: false,
            policies: Keeping::Own(Policies::default()),
        }
    }

    /// The same sluice, keeping the policies it reads in `policies`, which
    /// outlive it: a reader that makes a sluice per pass keeps them across
    /// its passes (129).
    pub fn keeping(self, policies: &'a Policies) -> Self {
        Sluice {
            policies: Keeping::Shared(policies),
            ..self
        }
    }

    /// 129: start a pass of `sluice run`, a watch tick or a wait's read:
    /// the policies the last did not use are forgotten.
    pub fn next_pass(&self) {
        self.policies.next_pass();
    }

    /// The same sluice, as `peer` among the hosts of a branch whose
    /// records are on its remote (72 to 74).
    pub fn as_peer(self, peer: Peer) -> Self {
        Sluice { peer, ..self }
    }

    /// The same sluice, for branch protection: it reads the policy only as
    /// far as guarding needs, [`Policy::guarding`] (124).
    pub fn guarding(self) -> Self {
        Sluice {
            guarding: true,
            ..self
        }
    }

    /// The seams this sluice is bound to, for a reader of the records.
    pub fn seams(&self) -> Seams<'a> {
        self.seams
    }

    fn now(&self) -> String {
        self.clock.now().to_rfc3339_opts(SecondsFormat::Secs, true)
    }

    fn read_line(&self, branch: &str) -> Result<Line> {
        Line::load(branch, self.state_read(branch)?)
    }

    /// The branch's live records: as the host holds them since the last
    /// ask where this sluice reads held, and read afresh otherwise (129;
    /// design D16).
    fn state_read(&self, branch: &str) -> Result<sluice_seams::Snapshot> {
        match self.held {
            true => self.seams.state.read_held(branch),
            false => self.seams.state.read(branch),
        }
    }

    /// The branch's revision: as the host holds it, with no fetch, where
    /// this sluice reads held, and read as a round reads it otherwise (129;
    /// design D16).
    pub fn branch_read(&self, branch: &str) -> Result<Option<Rev>> {
        match self.held {
            true => self.seams.branch.held(branch),
            false => self.seams.branch.read(branch),
        }
    }

    /// Whether it reads every branch as the host holds it (129; design
    /// D16).
    pub fn is_held(&self) -> bool {
        self.held
    }

    /// The same sluice, reading every branch as the host holds it since the
    /// host process's last ask, asking no remote: a one-shot read's, once
    /// the host process's reading is current (129; design D16). A round
    /// refuses to run under it, and its binding asks no remote from here
    /// on.
    pub fn holding(self) -> Self {
        self.seams.state.hold();
        Sluice { held: true, ..self }
    }

    /// Write `line`'s changes. 133: the lines of this host's refusal of
    /// the branch's remote not yet in its log go in the same write, each at
    /// the time it happened, and once written are marked so (design D5).
    /// 148: a write that changes something ends the causes of refused
    /// writes past their day, logging each end (design D4).
    fn write(&self, line: &Line) -> Result<Written> {
        self.carry(line, false)
    }

    /// [`Sluice::write`]; with `always`, the refusal's lines are written
    /// even when `line` changes nothing else, and no cause of refused
    /// writes is ended unless it does.
    fn carry(&self, line: &Line, always: bool) -> Result<Written> {
        let own = line.changes();
        if own.is_empty() && !always {
            return Ok(Written::Written(line.version));
        }
        // The time is asked only where a cause could end, so a write of a
        // branch with none reads no clock.
        let ending = match own.is_empty() || line.doc.refused_writes.is_empty() {
            true => None,
            false => Some(self.now()).filter(|now| {
                line.doc
                    .refused_writes
                    .iter()
                    .any(|c| !refused_writes::stands(c, now))
            }),
        };
        let refusal = self.refusal(&line.branch)?;
        // 133 (design D27): this host's refusal of the branch's intake, its
        // lines marked so, written beside the round's.
        let intake = self.intake_refusal(&line.branch)?;
        let carried: Vec<(String, crate::records::Event)> = refusal
            .as_ref()
            .map(|r| r.unwritten(&self.peer.name))
            .unwrap_or_default()
            .into_iter()
            .chain(
                intake
                    .as_ref()
                    .map(|r| r.unwritten_as(&self.peer.name, true))
                    .unwrap_or_default(),
            )
            .collect();
        let changes = if carried.is_empty() && ending.is_none() {
            own
        } else {
            let mut with = line.clone();
            for (at, event) in carried.iter().cloned() {
                with.log(&at, event);
            }
            if let Some(now) = &ending {
                with.end_refused_writes(now);
            }
            with.changes()
        };
        if changes.is_empty() {
            return Ok(Written::Written(line.version));
        }
        let written = self
            .seams
            .state
            .write(&line.branch, line.version, changes)?;
        if let Written::Written(_) = written {
            // The write stands whatever this does: a line not marked is
            // written again by the next write, which decides nothing.
            if let Some(r) = refusal.filter(|_| !carried.is_empty()) {
                if let Err(e) = self.keep_refusal(&line.branch, r.written().as_ref()) {
                    tracing::warn!(
                        sluice.branch = line.branch,
                        "noting the refusal's lines as written failed: {e:#}"
                    );
                }
            }
            if let Some(r) = intake.filter(|_| !carried.is_empty()) {
                if let Err(e) = self.keep_intake_refusal(&line.branch, r.written().as_ref()) {
                    tracing::warn!(
                        sluice.branch = line.branch,
                        "noting the intake refusal's lines as written failed: {e:#}"
                    );
                }
            }
        }
        Ok(written)
    }

    /// 133: the lines of this host's refusal of `branch`'s remote not yet in
    /// its log, in a write holding nothing else, read again when the write
    /// is refused (62).
    fn write_refusal_lines(&self, branch: &str) -> Result<()> {
        for _ in 0..WRITE_TRIES {
            let line = self.read_line(branch)?;
            if let Written::Written(_) = self.carry(&line, true)? {
                return Ok(());
            }
        }
        bail!("{branch}'s records kept moving under {WRITE_TRIES} writes")
    }

    /// 133: this host's refusal of `branch`'s remote, as the records seam
    /// keeps it beside the branch's records.
    fn refusal(&self, branch: &str) -> Result<Option<Refusal>> {
        self.seams
            .state
            .refusal(branch)?
            .map(|b| Refusal::parse(&b))
            .transpose()
    }

    /// Keep `refusal` as this host's refusal of `branch`'s remote, or, with
    /// `None`, remove it.
    fn keep_refusal(&self, branch: &str, refusal: Option<&Refusal>) -> Result<()> {
        let bytes = refusal.map(Refusal::to_bytes);
        self.seams.state.keep_refusal(branch, bytes.as_deref())
    }

    /// The remote `branch`'s table names, read from its policy at its
    /// revision as the host holds it, whatever the table refuses (core.27).
    pub fn remote_of(&self, branch: &str) -> Result<Option<String>> {
        let Some(rev) = self.seams.branch.held(branch)? else {
            return Ok(None);
        };
        match self.policy_at(&rev)? {
            Some(policy) => policy.remote_of(branch),
            None => Ok(None),
        }
    }

    /// 133 (design D27): this host's refusal of `branch`'s intake, as the
    /// records seam keeps it beside its refusal of the remote.
    pub fn intake_refusal(&self, branch: &str) -> Result<Option<Refusal>> {
        self.seams
            .state
            .intake_refusal(branch)?
            .map(|b| Refusal::parse(&b))
            .transpose()
    }

    /// Keep `refusal` as this host's refusal of `branch`'s intake, or, with
    /// `None`, remove it.
    fn keep_intake_refusal(&self, branch: &str, refusal: Option<&Refusal>) -> Result<()> {
        let bytes = refusal.map(Refusal::to_bytes);
        self.seams
            .state
            .keep_intake_refusal(branch, bytes.as_deref())
    }

    /// 133, rule core.28 (design D27): an intake pass of `branch` met
    /// `causes`, each a read of the forge it could not make: counted in this
    /// host's refusal of the branch's intake, each cause no pass of the
    /// refusal met before added, and a `read-refused` line marked `intake`
    /// written for each, in a write of its own where the branch's lock is
    /// free, or the next write of the branch's log otherwise. The causes
    /// whose lines this note wrote, which a pass says in the host process's
    /// log; none it met before, and none a restarted process, a step or a
    /// submit meets again.
    pub fn note_intake_refused(
        &self,
        branch: &str,
        causes: &[ReadRefused],
    ) -> Result<Vec<ReadRefused>> {
        let Some(first) = causes.first() else {
            return Ok(Vec::new());
        };
        let now = self.now();
        let standing = self.intake_refusal(branch)?.filter(|r| r.ended.is_none());
        let rounds = standing.as_ref().map_or(0, |r| r.rounds);
        let mut r = Refusal::refused(self.intake_refusal(branch)?, first, &now);
        for c in &causes[1..] {
            r = Refusal::refused(Some(r), c, &now);
        }
        r.rounds = rounds + 1;
        let unlogged = |r: &Refusal| -> Vec<(String, String)> {
            r.causes
                .iter()
                .filter(|c| !c.logged)
                .map(|c| (c.asked.clone(), c.said.clone()))
                .collect()
        };
        let before = unlogged(&r);
        self.keep_intake_refusal(branch, Some(&r))?;
        // A write that fails leaves the lines for the next write (73).
        if let Err(e) = self.write_intake_lines(branch) {
            tracing::warn!(
                sluice.branch = branch,
                "writing the intake refusal's lines failed: {e:#}"
            );
        }
        let after = self
            .intake_refusal(branch)?
            .map(|r| unlogged(&r))
            .unwrap_or_default();
        Ok(causes
            .iter()
            .filter(|c| {
                let key = (c.asked.clone(), c.said.clone());
                before.contains(&key) && !after.contains(&key)
            })
            .cloned()
            .collect())
    }

    /// 133 (design D27): an intake pass of `branch` met no failure: this
    /// host's refusal of its intake, where one stands, ends, its
    /// `read-again` line marked `intake` written as the refusal's are.
    pub fn note_intake_read(&self, branch: &str) -> Result<()> {
        let Some(mut r) = self.intake_refusal(branch)? else {
            return Ok(());
        };
        if r.ended.is_none() {
            r.read_again(&self.now());
            self.keep_intake_refusal(branch, Some(&r))?;
        }
        self.write_intake_lines(branch)
    }

    /// The lines of this host's refusals of `branch` not yet in its log, in
    /// a write holding nothing else, where the branch's lock is free;
    /// otherwise the next write carries them (rule core.14).
    fn write_intake_lines(&self, branch: &str) -> Result<()> {
        let Some(_lock) = self.seams.state.lock(branch)? else {
            return Ok(());
        };
        self.write_refusal_lines(branch)
    }

    /// Read, change and write a branch's records, reading again when the
    /// write is refused (62). `f` must be safe to apply to a fresh read.
    fn update<T>(&self, branch: &str, mut f: impl FnMut(&mut Line) -> Result<T>) -> Result<T> {
        for _ in 0..WRITE_TRIES {
            let mut line = self.read_line(branch)?;
            let out = f(&mut line)?;
            if let Written::Written(_) = self.write(&line)? {
                return Ok(out);
            }
        }
        bail!("{branch}'s records kept moving under {WRITE_TRIES} writes")
    }

    /// 56: the policy, read from a revision of the branch; for guarding,
    /// only as far as guarding needs (124). A policy it refuses names the
    /// revision, which is the one a push fixes (record 0069).
    ///
    /// 129: read from git once per revision while the reader keeps it,
    /// its refusal included, in the same words; a read that failed is made
    /// again.
    pub fn policy_at(&self, rev: &Rev) -> Result<Option<Policy>> {
        let key = (rev.clone(), self.guarding);
        let decided = match self.policies.get(&key) {
            Some(d) => d,
            None => {
                let decided = match self.seams.branch.read_file(rev, POLICY_PATH)? {
                    None => Decided::Read(None),
                    Some(bytes) => {
                        let read = || -> Result<Policy> {
                            let text = std::str::from_utf8(&bytes)?;
                            match self.guarding {
                                true => Policy::guarding(text),
                                false => Policy::parse(text),
                            }
                        };
                        match read() {
                            Ok(p) => Decided::Read(Some(p)),
                            Err(e) => Decided::Refused(format!("{e:#}")),
                        }
                    }
                };
                self.policies.keep(key, decided.clone());
                decided
            }
        };
        match decided {
            Decided::Read(p) => Ok(p),
            Decided::Refused(words) => {
                Err(anyhow!(words).context(format!("{POLICY_PATH} at {rev}")))
            }
        }
    }

    /// 27, 56: the gate of `branch`, read from its revision `rev`; a policy
    /// refused there names the branch and the revision (record 0069).
    pub fn rules_at(&self, branch: &str, rev: &Rev) -> Result<Option<Rules>> {
        let rules = || -> Result<Option<Rules>> {
            Ok(match self.policy_at(rev)? {
                None => None,
                Some(p) => p.rules_for(branch)?.cloned(),
            })
        };
        rules().with_context(|| format!("{branch}'s policy"))
    }

    fn gate(&self, branch: &str, rev: &Rev) -> Result<Rules> {
        self.rules_at(branch, rev)?.ok_or_else(|| {
            anyhow!(
                "no [branches] table in {POLICY_PATH} at {branch}'s revision {rev} names {branch}"
            )
        })
    }

    /// 69 (design D17): the results `ask` names, as `branch`'s gate reads
    /// them, its signed runs verified with the signers `branch`'s own
    /// policy names (78; rule core.22), less every result set aside
    /// (could-not-run-1 116); the lines themselves stay (rule core.7). An
    /// ask for nothing reads nothing.
    fn results_for(&self, branch: &str, ask: &sluice_seams::ResultAsk) -> Result<Vec<ResultLine>> {
        if ask.is_empty() {
            return Ok(Vec::new());
        }
        let all: Vec<ResultLine> = parse_lines(&self.seams.state.results_for(branch, ask)?)?;
        let aside: Vec<SetAside> = parse_lines(&self.seams.state.set_asides()?)?;
        Ok(all
            .into_iter()
            .filter(|r| !aside.iter().any(|a| r.set_aside_by(a)))
            .collect())
    }

    /// Could-not-run-1 116: set aside the one result of `task` recorded at
    /// `at` (and whose key starts with `hash`, when given), for `reason`,
    /// by `by`. More than one such result is an error listing them, so the
    /// operator names one.
    pub fn set_aside(
        &self,
        task: &str,
        at: &str,
        hash: Option<&str>,
        reason: &str,
        by: &str,
    ) -> Result<SetAside> {
        if reason.trim().is_empty() {
            bail!("a result is set aside with why: give a reason");
        }
        // Design D17: each branch with records asked for `task`'s results
        // alone, a line two branches read kept once. A branch whose
        // results cannot be read is passed by into the unread list with
        // its words (26; rule core.11).
        let ask = sluice_seams::ResultAsk::default().task(task);
        let mut seen = std::collections::BTreeSet::new();
        let mut every = Vec::new();
        let mut unread = Vec::new();
        for b in self.seams.state.branches()? {
            match self.results_for(&b, &ask) {
                Ok(lines) => every.extend(lines.into_iter().filter(|r| seen.insert(to_line(r)))),
                Err(e) => unread.push(Unread::of(&b, &e)),
            }
        }
        let found: Vec<ResultLine> = every
            .into_iter()
            .filter(|r| r.task == task && r.at == at)
            .filter(|r| match hash {
                Some(h) => r.hash.starts_with(h),
                None => true,
            })
            .collect();
        let r = match found.as_slice() {
            [] if unread.is_empty() => {
                bail!("no result of {task} recorded at {at} that is not already set aside")
            }
            [] => bail!(
                "no result of {task} recorded at {at} that is not already set aside, \
                 where these branches' results could be read; these could not:\n{}",
                unread
                    .iter()
                    .map(|u| format!("  {}: {}", u.branch, u.error))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            [r] => r.clone(),
            many => bail!(
                "{} results of {task} were recorded at {at}; name one by its key:\n{}",
                many.len(),
                many.iter()
                    .map(|r| format!(
                        "  {}  {:?} on {} {}",
                        r.hash,
                        r.outcome,
                        r.branch,
                        r.combination.short()
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        };
        let aside = SetAside {
            schema: SET_ASIDE_SCHEMA.into(),
            at: self.now(),
            task: r.task,
            hash: r.hash,
            run_at: r.at,
            branch: r.branch,
            combination: r.combination,
            reason: reason.trim().into(),
            by: by.into(),
        };
        self.seams.state.set_aside(&to_line(&aside))?;
        Ok(aside)
    }

    /// The branch that holds submission `id`, live or finished. With no
    /// branch named, every branch with records is read, in the records'
    /// order, and none stops the reading: one whose records cannot be
    /// read, or whose finished record of `id` cannot be, is passed by into
    /// the unread list with the words it was refused with. A number one
    /// branch holds is found on it; one that more than one holds names
    /// none of them alone, and is answered with each, and each branch that
    /// could not be read (26; record 0114; rule core.11). With `on`
    /// named, that branch alone is read, so no other is ever named.
    pub fn find(&self, id: u64, on: Option<&str>) -> Result<Found> {
        let branches = match on {
            Some(b) => vec![b.to_string()],
            None => self.seams.state.branches()?,
        };
        let mut held = Vec::new();
        let mut unread = Vec::new();
        for b in branches {
            let holds = self.read_line(&b).and_then(|line| {
                if line.subs.contains_key(&id) {
                    return Ok(true);
                }
                if line.finished.contains_key(&id) {
                    self.finished_sub(&b, id)?;
                    return Ok(true);
                }
                Ok(false)
            });
            match holds {
                Ok(true) => held.push(b),
                Ok(false) => {}
                Err(e) => unread.push(Unread::of(&b, &e)),
            }
        }
        Ok(match held.len() {
            0 => Found::Unfound(unread),
            1 => Found::On(held.remove(0)),
            _ => Found::Several(Several {
                branches: held,
                unread,
            }),
        })
    }
}

/// Where a lookup by number found a submission (26; record 0114).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// On this branch, the one of those read that holds it.
    On(String),
    /// On none of the branches read; the branches that could not be read,
    /// none when every branch was.
    Unfound(Vec<Unread>),
    /// On more than one branch, so it names none of them alone.
    Several(Several),
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use sluice_fakes::Fakes;

    use super::*;

    struct Noon;

    impl Clock for Noon {
        fn now(&self) -> DateTime<Utc> {
            Utc.with_ymd_and_hms(2026, 10, 29, 12, 0, 0).unwrap()
        }
    }

    fn seams(f: &Fakes) -> Seams<'_> {
        Seams {
            checkout: &f.checkout,
            tests: &f.tests,
            branch: &f.branch,
            driver: &f.driver,
            state: &f.state,
        }
    }

    /// 129, reader-tick "A branch's policy is read once per revision": six
    /// passes at one revision read the policy from git once; a second
    /// revision is read once more; a revision a whole pass left unused is
    /// forgotten, and read again when asked.
    #[test]
    fn a_policy_is_read_once_per_revision() {
        let f = Fakes::new();
        let at = f.world.init("main", &[(POLICY_PATH, "[branches.main]\n")]);
        let s = Sluice::new(seams(&f), &Noon);
        let reads = f.world.file_reads();
        for _ in 0..6 {
            s.next_pass();
            assert!(s.policy_at(&at).unwrap().is_some());
            assert!(s.rules_at("main", &at).unwrap().is_some());
        }
        assert_eq!(f.world.file_reads(), reads + 1);
        let moved = f.world.hand_commit("main", &[("a", Some("1"))]);
        s.next_pass();
        assert!(s.policy_at(&moved).unwrap().is_some());
        assert!(s.policy_at(&moved).unwrap().is_some());
        assert_eq!(f.world.file_reads(), reads + 2);
        s.next_pass();
        s.next_pass();
        assert!(s.policy_at(&at).unwrap().is_some());
        assert_eq!(f.world.file_reads(), reads + 3);
    }

    /// 129, reader-tick "A refused policy stays refused at its revision":
    /// the refusal is kept, in the same words, and read from git once.
    #[test]
    fn a_refused_policy_is_kept_with_its_words() {
        let f = Fakes::new();
        let at = f.world.init("main", &[(POLICY_PATH, "[branches.main\n")]);
        let s = Sluice::new(seams(&f), &Noon);
        let reads = f.world.file_reads();
        let first = format!("{:#}", s.rules_at("main", &at).unwrap_err());
        assert!(
            first.starts_with(&format!("main's policy: {POLICY_PATH} at {at}: ")),
            "{first}"
        );
        for _ in 0..3 {
            s.next_pass();
            assert_eq!(format!("{:#}", s.rules_at("main", &at).unwrap_err()), first);
        }
        assert_eq!(f.world.file_reads(), reads + 1);
    }

    /// 129: a read that failed decided nothing, and is made again.
    #[test]
    fn a_failed_read_is_made_again() {
        let f = Fakes::new();
        f.world.init("main", &[(POLICY_PATH, "[branches.main]\n")]);
        let s = Sluice::new(seams(&f), &Noon);
        let reads = f.world.file_reads();
        let missing = Rev::new("missing");
        assert!(s.policy_at(&missing).is_err());
        assert!(s.policy_at(&missing).is_err());
        assert_eq!(f.world.file_reads(), reads + 2);
    }

    /// 129: a sluice made per pass keeps the policies it reads in a store
    /// that outlives it.
    #[test]
    fn a_kept_store_outlives_the_sluice() {
        let f = Fakes::new();
        let at = f.world.init("main", &[(POLICY_PATH, "[branches.main]\n")]);
        let kept = Policies::default();
        let reads = f.world.file_reads();
        for _ in 0..3 {
            kept.next_pass();
            let s = Sluice::new(seams(&f), &Noon).keeping(&kept);
            assert!(s.policy_at(&at).unwrap().is_some());
        }
        assert_eq!(f.world.file_reads(), reads + 1);
    }
}

#[cfg(test)]
mod intake_refusals {
    use chrono::{DateTime, TimeZone, Utc};
    use sluice_fakes::Fakes;

    use super::*;
    use crate::records::Event;

    struct Noon;

    impl Clock for Noon {
        fn now(&self) -> DateTime<Utc> {
            Utc.with_ymd_and_hms(2026, 11, 13, 12, 0, 0).unwrap()
        }
    }

    fn cause(asked: &str, said: &str) -> ReadRefused {
        ReadRefused {
            remote: "origin".into(),
            asked: asked.into(),
            said: said.into(),
            note: None,
        }
    }

    /// The branch's `read-refused` and `read-again` lines: whether each is
    /// of its intake, its event and what it asked.
    fn lines(s: &Sluice<'_>) -> Vec<(bool, &'static str, String)> {
        let snapshot = s.seams.state.read("main").unwrap();
        Line::read_log(&snapshot)
            .unwrap()
            .into_iter()
            .filter_map(|l| match l.event {
                Event::ReadRefused { intake, asked, .. } => Some((intake, "read-refused", asked)),
                Event::ReadAgain { intake, .. } => Some((intake, "read-again", String::new())),
                _ => None,
            })
            .collect()
    }

    /// 133, rule core.28 (design D27): an intake pass's causes are kept in
    /// this host's refusal of the branch's intake, apart from the round's
    /// refusal of its remote, and logged once for each cause, marked
    /// `intake`: a pass that meets them again logs nothing and says
    /// nothing, a new cause is logged alone, and a pass that meets none
    /// ends the refusal with a `read-again` so marked.
    #[test]
    fn an_intake_refusal_is_logged_once_per_cause_apart_from_the_rounds() {
        let f = Fakes::new();
        f.world
            .init("main", &[(crate::policy::POLICY_PATH, "[branches.main]\n")]);
        let seams = Seams {
            checkout: &f.checkout,
            tests: &f.tests,
            branch: &f.branch,
            driver: &f.driver,
            state: &f.state,
        };
        let s = Sluice::new(seams, &Noon);
        let listed = cause("main's pull requests", "GitHub said 401 Bad credentials");
        let headless = cause("pull/12", "pull/12 has no head");
        let noted = s
            .note_intake_refused("main", &[listed.clone(), headless.clone()])
            .unwrap();
        assert_eq!(noted, [listed.clone(), headless.clone()]);
        let two = vec![
            (true, "read-refused", "main's pull requests".to_string()),
            (true, "read-refused", "pull/12".to_string()),
        ];
        assert_eq!(lines(&s), two);
        // Met again: nothing logged, nothing to say.
        let noted = s
            .note_intake_refused("main", &[listed.clone(), headless.clone()])
            .unwrap();
        assert!(noted.is_empty(), "{noted:?}");
        assert_eq!(lines(&s), two);
        // A new cause alone.
        let other = cause("pull/13", "GitHub said 502");
        let noted = s
            .note_intake_refused("main", &[listed.clone(), other.clone()])
            .unwrap();
        assert_eq!(noted, [other]);
        assert_eq!(lines(&s).len(), 3);
        let kept = s.intake_refusal("main").unwrap().unwrap();
        assert_eq!(kept.rounds, 3);
        // The round's own refusal of the remote is apart.
        assert_eq!(s.refusal("main").unwrap(), None);
        // A pass that met none ends it.
        s.note_intake_read("main").unwrap();
        let all = lines(&s);
        assert_eq!(
            all.last(),
            Some(&(true, "read-again", String::new())),
            "{all:?}"
        );
        assert_eq!(s.intake_refusal("main").unwrap(), None);
        s.note_intake_read("main").unwrap();
        assert_eq!(lines(&s).len(), 4, "ended once");
    }
}
