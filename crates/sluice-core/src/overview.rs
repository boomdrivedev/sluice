//! What a reader sees of a repository's queues at a glance: every
//! branch with records, its candidates, its submissions still in play and
//! its latest landings, blames and conflicts (26, 49; ADR 0023).
//!
//! Read from the records alone (61; rule core.11): it takes no lock,
//! holds no decision and writes nothing (rule core.14). A branch's policy
//! is read at its revision as the host holds it, and at the revision its
//! records last saw only where the host holds none, never fetched but by
//! the tick's one ask of its remote, so a reader that asks every second
//! asks a remote no more than for its records (56, 64, 124, 129).

use std::cell::OnceCell;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize, Serializer};
use sluice_seams::{Rev, Snapshot, Submitter, TaskProgress};

use crate::line::Line;
use crate::policy::{Policy, Rules};
use crate::records::{
    parse_lines, BranchHold, CandidateState, Event, FailedRelease, FailedTask, LogLine, LostRun,
    RefusedWrite, RunState, Submission, LOG_FILE,
};
use crate::Sluice;

pub const OVERVIEW_SCHEMA: &str = "sluice/overview@1";

/// How many of a branch's latest landings, blames and conflicts are
/// shown.
pub const RECENT: usize = 10;

/// The log events a reader is shown as recent: what became of
/// submissions.
const RECENT_EVENTS: [&str; 3] = ["landed", "blamed", "conflict"];

/// One branch as the overview is computed from: its records, and the
/// gate and executor the policy names at the revision they last saw.
#[derive(Clone, Debug)]
pub struct BranchRead {
    pub branch: String,
    pub snapshot: Snapshot,
    pub rules: Option<Rules>,
    /// The executor `[executor] kind` names; `None` names none.
    pub executor: Option<String>,
    /// Each candidate's tasks as last kept beside the records (ADR 0028).
    pub progress: BTreeMap<String, Vec<TaskProgress>>,
}

/// One branch's part of the overview: read back from a reader's document
/// as it was written (129).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BranchView {
    pub branch: String,
    /// The branch's revision as its records last saw it.
    pub at: Option<Rev>,
    /// 64: the branch's revision as the read found it, the remote's for a
    /// branch on a remote, which a forecast forms on; `None` where the
    /// read could not ask the branch's remote or the remote holds no such
    /// branch, for a view built from records alone, and in a document from
    /// before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<Rev>,
    /// 105: the branch itself is gone: the host holds no revision of it,
    /// and the read of its records was not refused. Its records stay, and
    /// are read as any branch's; left out while the branch is there, and
    /// in a document from before it (record 0091).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gone: bool,
    /// The records' version: it moves on every write, so a reader redraws
    /// only when it does.
    pub version: u64,
    /// How many candidates are under test at once (13).
    pub window: usize,
    /// The window's upper bound, as the policy states it.
    pub parallel: Option<usize>,
    /// The git remote the policy lands the branch on (C.4, C.7), by its
    /// name, never its URL, which can carry credentials: a reader resolves
    /// it and decides what it links to, so the core names no forge (rule
    /// core.1). `None` for a branch kept in this host's repository.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    /// Submissions still in a chain: queued, waiting, testing, passed or
    /// landing.
    pub depth: usize,
    pub candidates: Vec<CandidateView>,
    /// The submissions still in a chain, as `sluice status <id> --json`
    /// prints each.
    pub submissions: Vec<Submission>,
    /// 7, 64: the submissions a queued submission's `after` names that
    /// have landed, in number order: with the queue, all a forecast orders
    /// it by. `[]` in a document from before it.
    #[serde(default)]
    pub after_landed: Vec<u64>,
    /// The latest `landed`, `blamed` and `conflict` lines of the branch's
    /// log, oldest first, exactly as written.
    pub recent: Vec<serde_json::Value>,
    /// The worktree each submission the recent lines name was submitted
    /// from, as its `from.worktree` says (26, 49, 61): live, or moved to
    /// the finished index, which keeps it (130).
    pub recent_from: BTreeMap<u64, String>,
    /// The subject each submission the recent lines name was given at its
    /// submit (23, 26), live or moved to the finished index, which keeps
    /// it; none for a submission whose submit gave none, which a reader
    /// names by `recent_from`, and `{}` in a document from before it.
    #[serde(default)]
    pub recent_subjects: BTreeMap<u64, String>,
    /// 121: the branch was last read at a revision lacking its last
    /// landing, and nothing is formed or landed on it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held: Option<BranchHold>,
    /// 123: the branch's checkouts whose release failed, from its state,
    /// so every host's overview lists them (26, 61): held once `stopped`,
    /// tried again by the next round until then.
    #[serde(serialize_with = "held_checkouts")]
    pub held_checkouts: Vec<FailedRelease>,
    /// 148: the causes of writes the protection refused, from the branch's
    /// state: those standing when the view was read, newest last refusal
    /// first, as `sluice status <branch> --json` lists them; `[]` in a
    /// document from before it (design D6).
    #[serde(default)]
    pub refused_writes: Vec<RefusedWrite>,
    /// 26: what refused this host's read of the branch, its policy, its
    /// revision or its records, in the words it was refused with (133).
    /// Nothing else of the branch is read, and no other branch is hidden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unread: Option<String>,
    /// 80, 129: the read of the branch's records on its remote that could
    /// not be made, in the remote's words: what is shown is what was last
    /// fetched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale: Option<String>,
    /// Record 0122 (design D11): where the branch's queue lives, and its
    /// move while one is written; `None` for records with no home yet, and
    /// in a document from before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<HomeView>,
}

/// Record 0122 (design D11): a branch's home as the overview and a status
/// give it: `at`, `here` for this host's own files or a remote's name, and
/// while a move is written, where to, since when, and how many combinations
/// under test it waits for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeView {
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moving: Option<MovingView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MovingView {
    pub to: String,
    pub since: String,
    /// How many combinations under test the move waits for.
    pub waiting: usize,
}

impl HomeView {
    /// Record 0122 (design D11): `line`'s home, `None` where its records
    /// hold none.
    pub fn of(line: &Line) -> Option<HomeView> {
        let home = line.home.as_ref()?;
        let moving = home
            .moving
            .as_ref()
            .filter(|_| home.moving_to().is_some())
            .map(|m| MovingView {
                to: m.to.clone(),
                since: m.since.clone(),
                waiting: line
                    .doc
                    .candidates
                    .iter()
                    .filter(|c| c.state.under_test())
                    .count(),
            });
        Some(HomeView {
            at: home.at.clone(),
            moving,
        })
    }

    /// Design D11: the line a status says it in: `on origin`, `in this
    /// host's own files`, or `moving to origin since <time>, waiting for
    /// <n> combinations under test`.
    pub fn line(&self) -> String {
        match &self.moving {
            Some(m) => format!(
                "moving to {} since {}, waiting for {} combination{} under test",
                crate::home::describe(&m.to),
                m.since,
                m.waiting,
                if m.waiting == 1 { "" } else { "s" }
            ),
            None if self.at == crate::records::HERE => "in this host's own files".into(),
            None => format!("on {}", self.at),
        }
    }
}

impl BranchView {
    /// 26: `branch`, which could not be read, shown with the refusal
    /// `why` and nothing read of it.
    pub fn unread(branch: &str, why: &anyhow::Error) -> BranchView {
        BranchView {
            branch: branch.to_string(),
            at: None,
            revision: None,
            gone: false,
            version: 0,
            window: 0,
            parallel: None,
            remote: None,
            depth: 0,
            candidates: Vec::new(),
            submissions: Vec::new(),
            after_landed: Vec::new(),
            recent: Vec::new(),
            recent_from: BTreeMap::new(),
            recent_subjects: BTreeMap::new(),
            held: None,
            held_checkouts: Vec::new(),
            refused_writes: Vec::new(),
            unread: Some(format!("{why:#}")),
            stale: None,
            home: None,
        }
    }
}

/// 123: each held checkout as `sluice status <branch> --json` lists it,
/// `stopped` said either way, and `host` where the state names one.
fn held_checkouts<S: Serializer>(held: &[FailedRelease], s: S) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct Entry<'a> {
        place: &'a str,
        failed: &'a str,
        tries: u32,
        at: &'a str,
        stopped: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        host: Option<&'a str>,
    }
    s.collect_seq(held.iter().map(|f| Entry {
        place: &f.place,
        failed: &f.failed,
        tries: f.tries,
        at: &f.at,
        stopped: f.stopped,
        host: f.host.as_deref(),
    }))
}

/// One candidate: a combination under test or judged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandidateView {
    pub id: String,
    pub submissions: Vec<u64>,
    pub revision: Rev,
    /// 10, 64: the branch's revision it was formed on, which a forecast
    /// on that revision takes it from; `None` in a document from before
    /// it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<Rev>,
    pub state: CandidateState,
    /// When its run started, from the branch's log.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started: Option<String>,
    /// How long its checks may run, in seconds (66).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    /// The checks the gate requires (27).
    pub targets: Vec<String>,
    /// The executor the policy names.
    pub executor: Option<String>,
    pub ran: usize,
    pub replayed: usize,
    /// Each failed check, with where its output is kept (20).
    pub failed: Vec<FailedTask>,
    /// Where each task of its run has got, while it is under test and
    /// its runner says (ADR 0028). Never a result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<Vec<TaskProgress>>,
    /// 84, record 0074: each host that turned down a run of it still
    /// open, for what it lacks of what the run asks of it, with why: what
    /// the combination waits for.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub waiting: BTreeMap<String, String>,
    /// 114, record 0105 (design D7 of the change
    /// `a-pass-nothing-proves-blames-no-one`): each of its runs whose last
    /// run had no outcome, until that run's next report is read: the host
    /// that read it, its system, and each check it left not run with why,
    /// whatever the cause. None in a document from before it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lost: Vec<LostRun>,
}

/// One branch's part of the overview, from what was read of it.
pub fn branch_view(read: BranchRead) -> Result<BranchView> {
    let BranchRead {
        branch,
        snapshot,
        rules,
        executor,
        mut progress,
    } = read;
    let log = snapshot.files.get(LOG_FILE).cloned().unwrap_or_default();
    let line = Line::load(&branch, snapshot)?;
    let events: Vec<LogLine> = parse_lines(&log)?;
    let started = |revision: &Rev| {
        events.iter().rev().find_map(|l| match &l.event {
            Event::Testing { combination, .. } if combination == revision => Some(l.at.clone()),
            _ => None,
        })
    };
    let candidates = line
        .doc
        .candidates
        .iter()
        .map(|c| CandidateView {
            id: c.id.clone(),
            submissions: c.submissions.clone(),
            revision: c.revision.clone(),
            on: Some(c.on.clone()),
            state: c.state,
            started: (c.state != CandidateState::Formed)
                .then(|| started(&c.revision))
                .flatten(),
            limit: rules.as_ref().and_then(|r| r.timeout).map(|t| t.as_secs()),
            targets: rules
                .as_ref()
                .map(|r| r.required.clone())
                .unwrap_or_default(),
            executor: executor.clone(),
            ran: c.ran,
            replayed: c.replayed,
            failed: c.failed.clone(),
            tasks: matches!(c.state, CandidateState::Starting | CandidateState::Testing)
                .then(|| progress.remove(&c.id))
                .flatten(),
            waiting: c
                .runs
                .iter()
                .filter(|r| r.state == RunState::Open && r.host.is_none())
                .flat_map(|r| r.declined.clone())
                .collect(),
            lost: c.runs.iter().filter_map(|r| r.lost.clone()).collect(),
        })
        .collect();
    let submissions: Vec<Submission> = in_queue(line.subs.values()).cloned().collect();
    let after_landed = after_landed(&line);
    let recent = recent(&log, RECENT)?;
    // An id in neither the live records nor the finished index, which only
    // a hand-edited record gives, is left out.
    let recent_from = recent
        .iter()
        .flat_map(named)
        .filter_map(|id| {
            let worktree = match line.subs.get(&id) {
                Some(s) => s.from.worktree.clone(),
                None => line.finished.get(&id)?.worktree.clone(),
            };
            Some((id, worktree))
        })
        .collect();
    let recent_subjects = recent
        .iter()
        .flat_map(named)
        .filter_map(|id| {
            let subject = match line.subs.get(&id) {
                Some(s) => s.subject.clone(),
                None => line.finished.get(&id)?.subject.clone(),
            };
            Some((id, subject?))
        })
        .collect();
    Ok(BranchView {
        branch,
        at: line.doc.base.clone(),
        revision: None,
        gone: false,
        version: line.version,
        window: line.doc.window,
        parallel: rules.as_ref().map(|r| r.parallel),
        remote: rules.as_ref().and_then(|r| r.remote.clone()),
        depth: submissions.len(),
        candidates,
        submissions,
        after_landed,
        recent,
        recent_from,
        recent_subjects,
        held: line.doc.held.clone(),
        held_checkouts: line.doc.failed_releases.clone(),
        refused_writes: line.doc.refused_writes.clone(),
        unread: None,
        stale: None,
        home: HomeView::of(&line),
    })
}

/// 7: the submissions a queued submission's `after` names that have
/// landed, in number order.
pub fn after_landed(line: &Line) -> Vec<u64> {
    let landed = line.landed();
    let named: BTreeSet<u64> = in_queue(line.subs.values())
        .flat_map(|s| s.after.iter().copied())
        .filter(|a| landed.contains(a))
        .collect();
    named.into_iter().collect()
}

/// The submissions a recent line names: a landing's `ids`, a blame's
/// `id`, and a conflict's `id` and the submissions it is `with`.
fn named(line: &serde_json::Value) -> Vec<u64> {
    let ids = |key: &str| -> Vec<u64> {
        line[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_u64)
            .collect()
    };
    match line["event"].as_str() {
        Some("landed") => ids("ids"),
        Some("blamed") => line["id"].as_u64().into_iter().collect(),
        Some("conflict") => line["id"].as_u64().into_iter().chain(ids("with")).collect(),
        _ => Vec::new(),
    }
}

/// The last `n` lines of a log whose event is one a reader is shown as
/// recent, oldest first, each as written.
pub fn recent(log: &[u8], n: usize) -> Result<Vec<serde_json::Value>> {
    let mut out: Vec<serde_json::Value> = Vec::new();
    for l in std::str::from_utf8(log)?.lines() {
        if l.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(l)?;
        if v["event"]
            .as_str()
            .is_some_and(|e| RECENT_EVENTS.contains(&e))
        {
            out.push(v);
        }
    }
    let skip = out.len().saturating_sub(n);
    Ok(out.split_off(skip))
}

/// A worktree that has checked out a branch sluice lands on. sluice
/// moves the branch under it and leaves its files as they were, so it is
/// reported, never moved, and never stops a landing (25; ADR 0024).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Held {
    pub worktree: PathBuf,
    pub branch: String,
}

/// Whether sluice lands on `branch`: it has records, or `policy`, read
/// from the branch's own revision (rule core.15), gives it a gate.
pub fn lands_on(branch: &str, recorded: &[String], policy: Option<&Policy>) -> bool {
    recorded.iter().any(|b| b == branch)
        || policy.is_some_and(|p| matches!(p.rules_for(branch), Ok(Some(_))))
}

impl Sluice<'_> {
    /// Every worktree that has checked out a branch sluice lands on, as
    /// the place seam lists them. A read: a policy it cannot read marks
    /// nothing.
    pub fn held(&self) -> Result<Vec<Held>> {
        let recorded = self.seams.state.branches()?;
        let mut out = Vec::new();
        for c in self.seams.checkout.checked_out()? {
            let policy = if recorded.contains(&c.branch) {
                None
            } else {
                self.policy_at(&c.head).ok().flatten()
            };
            if lands_on(&c.branch, &recorded, policy.as_ref()) {
                out.push(Held {
                    worktree: c.worktree,
                    branch: c.branch,
                });
            }
        }
        Ok(out)
    }

    /// 26, 61: every branch with records, as last written, each as
    /// [`Sluice::branch_view_of`] reads it, in the order `from`'s policy
    /// names them (ADR 0036). A branch whose policy, revision or records
    /// could not be read is shown with the refusal and hides no other
    /// (26; 129, 133): only the records' own list fails it whole. Each
    /// branch's view names its revision as the tick's ask found it, so a
    /// forecast can form from what a reader wrote (64, 129).
    ///
    /// ADR 0036, 56, 129: the order is read from `from`'s policy at the
    /// revision its view reads its policy at, and at `from`'s own revision
    /// only where it has no records, never fetching, so one revision gives
    /// everything the overview reads of that branch's policy (ADR 0046 as
    /// amended 2026-11-06; rule core.15).
    pub fn overview(&self, from: Option<&OrderFrom>) -> Result<Vec<BranchView>> {
        let mut out = Vec::new();
        let mut order_at = from.and_then(|f| f.local.clone());
        for branch in self.seams.state.branches()? {
            let read = self.view_read(&branch);
            if from.is_some_and(|f| f.branch == branch) {
                if let Ok(read) = &read {
                    order_at = read.policy_at.clone().or(order_at);
                }
            }
            let view = read
                .and_then(|read| self.view_from(&branch, read))
                .unwrap_or_else(|e| BranchView::unread(&branch, &e));
            out.push(view);
        }
        // The order decides nothing, so a policy it cannot read leaves the
        // branches in name order rather than failing the overview.
        let policy = order_at.and_then(|rev| self.policy_at(&rev).ok().flatten());
        order(&mut out, policy.as_ref());
        Ok(out)
    }

    /// 26, 61: one branch's part of the overview, as last written, read
    /// at the revision its records last saw: what `sluice status` and a
    /// waiting command show of its combinations (ADR 0042).
    ///
    /// 129: the records as the host holds them since the tick's ask, with
    /// no fetch, and, where that ask was refused, its words; and the
    /// branch's revision as the host holds it, none where that ask was
    /// refused, so a forecast can form from what a reader wrote (64).
    ///
    /// 56, 124, 129: its policy at the branch's revision as this host holds
    /// it, whether or not that ask was refused, and at the revision its
    /// records last saw only where the host holds none, so a policy mended
    /// outside sluice ends the branch's refusal with no round run (ADR 0046
    /// as amended 2026-11-06; rule core.15).
    pub fn branch_view_of(&self, branch: &str) -> Result<BranchView> {
        let read = self.view_read(branch)?;
        self.view_from(branch, read)
    }

    /// What a branch's view is read from, before its policy is: its
    /// records as the host holds them, its revision as the host holds it,
    /// and the revision its policy is read at, that one, then the one its
    /// records last saw.
    fn view_read(&self, branch: &str) -> Result<ViewRead> {
        let snapshot = self.seams.state.read_held(branch)?;
        let base = Line::load(branch, snapshot.clone())?.doc.base;
        let held = self.seams.branch.held(branch)?;
        Ok(ViewRead {
            policy_at: held.clone().or(base),
            snapshot,
            held,
        })
    }

    fn view_from(&self, branch: &str, read: ViewRead) -> Result<BranchView> {
        let ViewRead {
            snapshot,
            held,
            policy_at,
        } = read;
        let policy = match policy_at {
            Some(rev) => self
                .policy_at(&rev)
                .with_context(|| format!("{branch}'s policy at {rev}"))?,
            None => None,
        };
        let rules = match &policy {
            Some(p) => p.rules_for(branch)?.cloned(),
            None => None,
        };
        let mut view = branch_view(BranchRead {
            executor: policy.and_then(|p| p.limits.executor),
            progress: self.seams.state.progress(branch)?,
            branch: branch.to_string(),
            snapshot,
            rules,
        })?;
        let stale = self.seams.state.stale(branch);
        view.revision = match &stale {
            Some(_) => None,
            None => held,
        };
        // 105: a branch the host holds no revision of, read with no
        // refusal, is gone; its records are shown all the same.
        view.gone = stale.is_none() && view.revision.is_none();
        view.stale = stale.map(|r| r.to_string());
        view.refused_writes = crate::refused_writes::standing(&view.refused_writes, || self.now());
        Ok(view)
    }
}

/// A branch's records and revisions as its view reads them.
struct ViewRead {
    snapshot: Snapshot,
    held: Option<Rev>,
    policy_at: Option<Rev>,
}

/// 77, 107: a host serving a remote, by its last sign of life.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostView {
    pub host: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub held: u64,
    /// The runs it has room for by its last sign of life; none once it
    /// has lapsed.
    pub room: u64,
    /// Its sign of life has stood still for the bound, by this host's
    /// observation (77).
    pub lapsed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

impl Sluice<'_> {
    /// Every host serving a remote, by its last sign of life. A reader:
    /// what this host has observed of each beat is read, never written.
    /// 129: after a tick's ask, the signs of life it fetched.
    pub fn hosts(&self) -> Result<Vec<HostView>> {
        let now = self.now();
        let mut out = Vec::new();
        for (host, (marker, beat)) in self.seams.state.beats()? {
            let v: serde_json::Value = serde_json::from_slice(&beat).unwrap_or_default();
            let lapsed = match self.seams.state.observed_beat(&host)? {
                Some((m, since)) if m == marker => crate::step::seconds_since(&since, &now)
                    .is_some_and(|s| s >= self.peer.alive.as_secs_f64()),
                _ => false,
            };
            let held = v["held"].as_u64().unwrap_or(0);
            let bound = v["bound"].as_u64().unwrap_or(0);
            out.push(HostView {
                signer: v["signer"].as_str().map(str::to_string),
                system: v["system"].as_str().map(str::to_string),
                held,
                room: if lapsed {
                    0
                } else {
                    bound.saturating_sub(held)
                },
                lapsed,
                at: v["at"].as_str().map(str::to_string),
                host,
            });
        }
        Ok(out)
    }
}

/// The host's runs against its bound (71; session-runs-1 clauses 89, 91;
/// ADR 0035): the bound, the submissions' runs running on the host, every
/// repository's, the host's session runs, and the part of the bound
/// session runs never take. It decides nothing; `session_capacity` reads
/// the same numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Room {
    pub bound: usize,
    pub queue: usize,
    pub sessions: usize,
    pub reserve: usize,
}

impl Room {
    /// How many more session runs may start now.
    pub fn for_sessions(&self) -> usize {
        self.bound
            .saturating_sub(self.reserve)
            .saturating_sub(self.queue + self.sessions)
    }

    /// ADR 0035: how many runs agents' checks hold while they hold a
    /// branch's queue back: the host has no free run, a session run holds
    /// one, and the branch has `queued` submissions while fewer than its
    /// `window` of candidates are under test. `None` when they hold
    /// nothing back.
    pub fn held_by_checks(&self, queued: usize, testing: usize, window: usize) -> Option<usize> {
        (self.sessions > 0
            && self.queue + self.sessions >= self.bound
            && queued > 0
            && testing < window)
            .then_some(self.sessions)
    }
}

/// The branch whose policy orders the overview, the repository's default
/// (ADR 0036): its name, and its revision for when it has no records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderFrom {
    pub branch: String,
    pub local: Option<Rev>,
}

/// ADR 0036: `branches` in the order `policy` writes the tables their
/// gates come from, the branches one pattern gives a gate together in name
/// order, and those no table names last in name order. No round, gate or
/// landing reads it (rule core.15).
pub fn order(branches: &mut [BranchView], policy: Option<&Policy>) {
    branches.sort_by_cached_key(|b| {
        let table = policy.and_then(|p| p.table_for(&b.branch).ok().flatten());
        (table.unwrap_or(usize::MAX), b.branch.clone())
    });
}

pub const HOME_SCHEMA: &str = "sluice/home@1";

/// One branch as the home view counts it: the overview's own counts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BranchCount {
    pub branch: String,
    pub depth: usize,
    pub testing: usize,
    pub parallel: Option<usize>,
    /// 26: the refusal the branch could not be read for, as the
    /// overview shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unread: Option<String>,
}

/// Every branch's counts, as the overview gives them.
pub fn counts(branches: &[BranchView]) -> Vec<BranchCount> {
    branches
        .iter()
        .map(|b| BranchCount {
            branch: b.branch.clone(),
            depth: b.depth,
            testing: b.candidates.iter().filter(|c| c.state.under_test()).count(),
            parallel: b.parallel,
            unread: b.unread.clone(),
        })
        .collect()
}

/// The submissions still in a chain, queued or under test, whose count is
/// a branch's `depth`: what `sluice status <branch>` shows by default.
pub fn in_queue<'a>(
    subs: impl IntoIterator<Item = &'a Submission>,
) -> impl Iterator<Item = &'a Submission> {
    subs.into_iter().filter(|s| !s.state.is_out())
}

/// The worktree a reading of whose a submission is is for: the branch it
/// is on, empty when it is on none, and its path where its own
/// submissions are read by it, as the home view reads them.
#[derive(Debug)]
pub struct Here<'a> {
    path: Option<&'a str>,
    branch: &'a str,
    /// Whether sluice lands on `branch`, read once, when a pull request
    /// opened from it is first found.
    lands_on: OnceCell<bool>,
}

impl<'a> Here<'a> {
    /// The worktree at `path` on `branch`, its own submissions read by
    /// its path: the home view's.
    pub fn at(path: &'a str, branch: &'a str) -> Here<'a> {
        Here {
            path: Some(path),
            branch,
            lands_on: OnceCell::new(),
        }
    }

    /// The worktree on `branch`, its own submissions read by the work
    /// they name: the inbox's and a wait's delivery (51, 135).
    pub fn on(branch: &'a str) -> Here<'a> {
        Here {
            path: None,
            branch,
            lands_on: OnceCell::new(),
        }
    }

    /// The branch the worktree is on, empty when it is on none.
    pub fn branch(&self) -> &str {
        self.branch
    }

    /// Design D6 (51, 124; ADR 0025; rule core.50): whether `from` is
    /// this worktree's. Never a submission by name, which no worktree
    /// made. Its own: the path it was submitted from where the worktree
    /// is read by its path, else a submission naming no head whose work is
    /// the worktree's branch. A pull request's opened from the worktree's
    /// branch, where sluice lands on that branch nowhere, as `lands_on`
    /// says, asked only when such a request is found and once for this
    /// worktree: no worktree pushes a branch sluice lands on (1, 124).
    pub fn made(&self, from: &Submitter, lands_on: impl FnOnce(&str) -> bool) -> bool {
        if from.by_name() {
            return false;
        }
        let own = match self.path {
            Some(path) => from.path.as_deref() == Some(path),
            None => from.head.is_none() && from.worktree == self.branch,
        };
        own || (!self.branch.is_empty()
            && from.head.as_deref() == Some(self.branch)
            && !*self.lands_on.get_or_init(|| lands_on(self.branch)))
    }
}

/// The latest submission still in a chain that `made` says the worktree
/// made.
pub fn in_play(branches: &[BranchView], made: impl Fn(&Submitter) -> bool) -> Option<&Submission> {
    branches
        .iter()
        .flat_map(|b| &b.submissions)
        .filter(|s| made(&s.from))
        .max_by_key(|s| s.id)
}

/// The submissions the branches' recent lines name, newest first, each
/// with its branch.
pub fn recent_ids(branches: &[BranchView]) -> Vec<(&str, u64)> {
    let mut named: Vec<(&str, &str, u64)> = Vec::new();
    for b in branches {
        let branch = b.branch.as_str();
        for r in &b.recent {
            let at = r["at"].as_str().unwrap_or("");
            match r["event"].as_str() {
                Some("landed") => named.extend(
                    r["ids"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(serde_json::Value::as_u64)
                        .map(|id| (at, branch, id)),
                ),
                Some(_) => named.extend(r["id"].as_u64().map(|id| (at, branch, id))),
                None => {}
            }
        }
    }
    named.sort_by(|a, b| (b.0, b.2).cmp(&(a.0, a.2)));
    named.into_iter().map(|(_, b, id)| (b, id)).collect()
}

impl Sluice<'_> {
    /// Design D6 (51, 124; rule core.50): whether `from` is the worktree
    /// `here`'s, by the one reading of [`Here::made`].
    pub fn made(&self, here: &Here<'_>, from: &Submitter) -> bool {
        here.made(from, |branch| self.lands_on_held(branch))
    }

    /// Design D6 (124; rule core.27): whether sluice lands on `branch`, as
    /// the protection reads it: the policy at the branch's revision as
    /// the host holds it, with no fetch. A branch the host holds no
    /// revision of, or whose policy cannot be read, is one it lands on
    /// nowhere.
    pub fn lands_on_held(&self, branch: &str) -> bool {
        match self.seams.branch.held(branch) {
            Ok(Some(at)) => self.protects(branch, &at).unwrap_or(false),
            _ => false,
        }
    }

    /// The worktree `here`'s submission, as the home view shows it: the
    /// latest still in a chain, else the latest the branches' recent
    /// lines name. A read, as the overview is, of each branch's records
    /// once at most, however many recent lines name it.
    pub fn home(&self, branches: &[BranchView], here: &Here<'_>) -> Result<Option<Submission>> {
        if let Some(s) = in_play(branches, |from| self.made(here, from)) {
            return Ok(Some(s.clone()));
        }
        let mut read: BTreeMap<&str, crate::line::Line> = BTreeMap::new();
        for (branch, id) in recent_ids(branches) {
            let line = match read.entry(branch) {
                Entry::Occupied(e) => e.into_mut(),
                Entry::Vacant(e) => e.insert(self.read_line(branch)?),
            };
            // Live, or moved to the finished records: one file then (131).
            let s = match line.subs.get(&id) {
                Some(s) => Some(s.clone()),
                None if line.finished.contains_key(&id) => self.finished_sub(branch, id)?,
                None => None,
            };
            if let Some(s) = s.filter(|s| self.made(here, &s.from)) {
                return Ok(Some(s));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(event: &str, rest: &str) -> String {
        format!("{{\"schema\":\"sluice/log@1\",\"at\":\"2026-10-16T00:00:00Z\",\"event\":\"{event}\"{rest}}}\n")
    }

    #[test]
    fn recent_keeps_the_last_landings_blames_and_conflicts_verbatim() {
        let mut log = String::new();
        log += &line("submitted", ",\"id\":1,\"head\":\"a\"");
        for id in 1..=12 {
            log += &line("landed", &format!(",\"ids\":[{id}],\"to\":\"b\""));
        }
        log += &line("blamed", ",\"id\":13,\"combination\":\"c\"");
        log += &line("ran", ",\"combination\":\"c\",\"task\":\"t\",\"hash\":\"h\",\"outcome\":\"fail\",\"host\":\"x\"");
        log += &line("conflict", ",\"id\":14,\"paths\":[\"p\"],\"with\":[]");

        let r = recent(log.as_bytes(), RECENT).unwrap();
        assert_eq!(r.len(), RECENT);
        assert_eq!(r[0]["ids"][0], 5);
        assert_eq!(r[8]["event"], "blamed");
        assert_eq!(r[9]["event"], "conflict");
        assert_eq!(r[9]["paths"][0], "p");
        assert_eq!(r[9]["schema"], "sluice/log@1");
    }

    #[test]
    fn a_branch_with_records_or_a_gate_is_one_sluice_lands_on() {
        let policy = Policy::parse(
            "[branches.main]\nrequired = [\"#gate\"]\n[branches.\"bolt/*\"]\nrequired = [\"#gate\"]\n",
        )
        .unwrap();
        let recorded = vec!["unit/queued".to_string()];
        assert!(lands_on("main", &[], Some(&policy)));
        assert!(lands_on("bolt/units", &[], Some(&policy)));
        assert!(lands_on("unit/queued", &recorded, None));
        assert!(!lands_on("unit/mine", &recorded, Some(&policy)));
        assert!(!lands_on("main", &[], None));
    }

    fn view(branch: &str, recent: Vec<serde_json::Value>) -> BranchView {
        BranchView {
            branch: branch.into(),
            at: None,
            revision: None,
            gone: false,
            version: 0,
            window: 0,
            parallel: None,
            remote: None,
            depth: 0,
            candidates: Vec::new(),
            submissions: Vec::new(),
            after_landed: Vec::new(),
            recent,
            recent_from: BTreeMap::new(),
            recent_subjects: BTreeMap::new(),
            held: None,
            held_checkouts: Vec::new(),
            refused_writes: Vec::new(),
            unread: None,
            stale: None,
            home: None,
        }
    }

    /// A branch's view from its record files, under `policy`'s gate for
    /// it.
    fn read(policy: &str, files: &[(&str, String)]) -> BranchView {
        let rules = Policy::parse(policy)
            .unwrap()
            .rules_for("main")
            .unwrap()
            .cloned();
        branch_view(BranchRead {
            branch: "main".into(),
            snapshot: Snapshot {
                version: 1,
                files: files
                    .iter()
                    .map(|(p, b)| (p.to_string(), b.clone().into_bytes()))
                    .collect(),
            },
            rules,
            executor: None,
            progress: BTreeMap::new(),
        })
        .unwrap()
    }

    /// C.4, C.7: a branch names the remote its policy lands it on, and
    /// a branch kept in this host's repository names none.
    #[test]
    fn a_branch_names_its_remote_by_the_policys_name() {
        let on = read("[branches.main]\nrequired = []\nremote = \"origin\"\n", &[]);
        assert_eq!(on.remote.as_deref(), Some("origin"));
        assert_eq!(serde_json::to_value(&on).unwrap()["remote"], "origin");
        let local = read("[branches.main]\nrequired = []\n", &[]);
        assert_eq!(local.remote, None);
        let json = serde_json::to_value(&local).unwrap();
        assert!(json.get("remote").is_none(), "{json}");
    }

    /// 123, 26: a branch lists every checkout its state names whose
    /// release failed, held or still tried, each as `sluice status
    /// --json` names it; one with none an empty list.
    #[test]
    fn a_branch_lists_its_checkouts_whose_release_failed() {
        const GATE: &str = "[branches.main]\nrequired = []\n";
        let mut doc = crate::records::BranchDoc::new("main");
        doc.failed_releases = vec![
            FailedRelease {
                place: "sluice/try/main/48".into(),
                failed: "preview: down\nusage: scripts/preview up\n".into(),
                tries: 4,
                at: "2026-10-23T09:00:00Z".into(),
                stopped: true,
                host: Some("deneb".into()),
            },
            FailedRelease {
                place: "sluice/try/main/51".into(),
                failed: "busy".into(),
                tries: 1,
                at: "2026-10-23T09:05:00Z".into(),
                stopped: false,
                host: None,
            },
        ];
        let branch = String::from_utf8(crate::records::to_document(&doc)).unwrap();
        let v = read(GATE, &[(crate::records::BRANCH_FILE, branch)]);
        assert_eq!(v.held_checkouts, doc.failed_releases);
        assert_eq!(
            serde_json::to_value(&v).unwrap()["held_checkouts"],
            serde_json::json!([
                {"place": "sluice/try/main/48", "failed": "preview: down\nusage: scripts/preview up\n",
                 "tries": 4, "at": "2026-10-23T09:00:00Z", "stopped": true, "host": "deneb"},
                {"place": "sluice/try/main/51", "failed": "busy",
                 "tries": 1, "at": "2026-10-23T09:05:00Z", "stopped": false},
            ])
        );

        let none = read(GATE, &[]);
        assert_eq!(
            serde_json::to_value(&none).unwrap()["held_checkouts"],
            serde_json::json!([])
        );
    }

    fn submission(id: u64, worktree: &str, state: &str) -> (String, String) {
        (
            format!("submissions/{id}.json"),
            format!(
                "{{\"schema\":\"sluice/submission@1\",\"id\":{id},\"branch\":\"main\",\
                 \"head\":\"h{id}\",\"from\":{{\"agent\":\"a\",\"worktree\":\"{worktree}\"}},\
                 \"submitted\":\"t\",\"after\":[],\"priority\":0,\"attempt\":1,\
                 \"state\":\"{state}\"}}"
            ),
        )
    }

    /// `id`'s record in `state`, queued after `after`.
    fn after(id: u64, state: &str, after: &[u64]) -> (String, String) {
        let (path, doc) = submission(id, "unit/a", state);
        let after = format!("\"after\":{after:?}");
        (path, doc.replace("\"after\":[]", &after))
    }

    /// 7, 64: a branch names the landed submissions a queued one's
    /// `after` names, live or moved to the finished index, in number
    /// order; one no queued submission names, or one still queued, is no
    /// part of it.
    #[test]
    fn a_branch_names_the_landed_submissions_its_queue_waits_behind() {
        const GATE: &str = "[branches.main]\nrequired = []\n";
        let subs = [
            after(1, "landed", &[]),
            after(3, "landed", &[]),
            after(5, "queued", &[4, 3, 6]),
            after(6, "queued", &[]),
            after(7, "failed", &[1]),
        ];
        let mut files: Vec<(&str, String)> =
            subs.iter().map(|(p, d)| (p.as_str(), d.clone())).collect();
        files.push((
            crate::records::FINISHED_INDEX,
            "{\"id\":4,\"state\":\"landed\",\"worktree\":\"unit/a\"}\n".into(),
        ));
        let v = read(GATE, &files);
        assert_eq!(v.after_landed, [3, 4]);
        assert_eq!(
            serde_json::to_value(&v).unwrap()["after_landed"],
            serde_json::json!([3, 4])
        );
        let none = read(GATE, &[]);
        assert_eq!(
            serde_json::to_value(&none).unwrap()["after_landed"],
            serde_json::json!([])
        );
    }

    /// 10, 64: each combination names the branch's revision it was formed
    /// on beside its own; a document from before it reads with none.
    #[test]
    fn a_combination_names_the_revision_it_was_formed_on() {
        const GATE: &str = "[branches.main]\nrequired = []\n";
        let mut doc = crate::records::BranchDoc::new("main");
        doc.candidates = vec![serde_json::from_value(serde_json::json!({
            "id": "c1", "on": "r0", "submissions": [1, 2], "revision": "c12",
            "state": "testing",
        }))
        .unwrap()];
        let branch = String::from_utf8(crate::records::to_document(&doc)).unwrap();
        let v = read(GATE, &[(crate::records::BRANCH_FILE, branch)]);
        assert_eq!(v.candidates[0].on, Some(Rev::new("r0")));
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["candidates"][0]["on"], "r0");
        assert_eq!(json["candidates"][0]["revision"], "c12");
        assert_eq!(
            json["candidates"][0]["submissions"],
            serde_json::json!([1, 2])
        );

        let mut before = json.clone();
        before["candidates"][0]
            .as_object_mut()
            .unwrap()
            .remove("on");
        before.as_object_mut().unwrap().remove("after_landed");
        let read: BranchView = serde_json::from_value(before).unwrap();
        assert_eq!(read.candidates[0].on, None);
        assert_eq!(read.after_landed, Vec::<u64>::new());
    }

    /// 64: a branch read from its records alone names no revision of the
    /// branch, and says nothing of one.
    #[test]
    fn a_branch_read_alone_names_no_revision() {
        let v = read("[branches.main]\nrequired = []\n", &[]);
        assert_eq!(v.revision, None);
        assert!(serde_json::to_value(&v).unwrap().get("revision").is_none());
    }

    /// 26, 49, 61: each submission a recent line names, by the worktree
    /// it came from, whether still live or moved to the finished index;
    /// the recent lines themselves as written.
    #[test]
    fn a_branch_names_where_its_recent_submissions_came_from() {
        const GATE: &str = "[branches.main]\nrequired = []\n";
        let mut log = String::new();
        log += &line("landed", ",\"ids\":[1],\"to\":\"b\"");
        log += &line("blamed", ",\"id\":2,\"combination\":\"c\"");
        log += &line("conflict", ",\"id\":3,\"paths\":[\"p\"],\"with\":[1]");
        log += &line("conflict", ",\"id\":9,\"paths\":[\"p\"],\"with\":[]");
        let (two, two_doc) = submission(2, "unit/b", "failed");
        let (three, three_doc) = submission(3, "pull/41", "conflict");
        let files = [
            (LOG_FILE, log.clone()),
            (
                crate::records::FINISHED_INDEX,
                "{\"id\":1,\"state\":\"landed\",\"worktree\":\"unit/a\"}\n".to_string(),
            ),
            (two.as_str(), two_doc),
            (three.as_str(), three_doc),
        ];
        let v = read(GATE, &files);
        assert_eq!(
            serde_json::to_value(&v).unwrap()["recent_from"],
            serde_json::json!({"1": "unit/a", "2": "unit/b", "3": "pull/41"}),
            "an id in no record is left out"
        );
        assert_eq!(v.recent, recent(log.as_bytes(), RECENT).unwrap());

        let (one, one_doc) = submission(1, "unit/a", "queued");
        let queued = read(GATE, &[(one.as_str(), one_doc)]);
        assert_eq!(
            serde_json::to_value(&queued).unwrap()["recent_from"],
            serde_json::json!({})
        );
    }

    /// 23, 26: the subject of each submission a recent line names whose
    /// submit gave one, live or from the finished index; none for one with
    /// no subject or an index line written before subjects were kept.
    #[test]
    fn a_branch_names_the_subjects_of_its_recent_submissions() {
        const GATE: &str = "[branches.main]\nrequired = []\n";
        let subject = |id: u64, worktree: &str, given: &str| {
            let (path, doc) = submission(id, worktree, "landed");
            let doc = doc.replacen(
                "\"state\":",
                &format!("\"subject\":{given:?},\"state\":"),
                1,
            );
            (path, doc)
        };
        let log = line("landed", ",\"ids\":[1,2],\"to\":\"b\"");
        let (one, one_doc) = subject(1, "unit/a", "Batch review cards");
        let (two, two_doc) = submission(2, "unit/b", "landed");
        let live = read(
            GATE,
            &[
                (LOG_FILE, log.clone()),
                (one.as_str(), one_doc),
                (two.as_str(), two_doc),
            ],
        );
        let json = serde_json::to_value(&live).unwrap();
        assert_eq!(
            json["recent_subjects"],
            serde_json::json!({"1": "Batch review cards"})
        );
        assert_eq!(
            json["recent_from"],
            serde_json::json!({"1": "unit/a", "2": "unit/b"})
        );

        let finished = read(
            GATE,
            &[
                (LOG_FILE, log.clone()),
                (
                    crate::records::FINISHED_INDEX,
                    "{\"id\":1,\"state\":\"landed\",\"worktree\":\"unit/a\",\
                     \"subject\":\"Batch review cards\"}\n\
                     {\"id\":2,\"state\":\"landed\",\"worktree\":\"unit/b\"}\n"
                        .to_string(),
                ),
            ],
        );
        assert_eq!(
            serde_json::to_value(&finished).unwrap()["recent_subjects"],
            serde_json::json!({"1": "Batch review cards"})
        );
        assert_eq!(finished.recent_from, live.recent_from);
        assert_eq!(finished.recent, live.recent);

        let (one, one_doc) = submission(1, "unit/a", "queued");
        let queued = read(GATE, &[(one.as_str(), one_doc)]);
        assert_eq!(
            serde_json::to_value(&queued).unwrap()["recent_subjects"],
            serde_json::json!({})
        );
    }

    /// A reader's document from a sluice before `recent_subjects` reads
    /// with none.
    #[test]
    fn a_branch_view_without_recent_subjects_reads() {
        let mut json = serde_json::to_value(view("main", Vec::new())).unwrap();
        json.as_object_mut().unwrap().remove("recent_subjects");
        let back: BranchView = serde_json::from_value(json).unwrap();
        assert!(back.recent_subjects.is_empty());
    }

    #[test]
    fn recent_ids_are_newest_first_across_branches() {
        let at = |t: &str, rest: &str| -> serde_json::Value {
            serde_json::from_str(&format!("{{\"at\":\"{t}\"{rest}}}")).unwrap()
        };
        let a = view(
            "a",
            vec![
                at(
                    "2026-10-16T01:00:00Z",
                    ",\"event\":\"landed\",\"ids\":[1,2]",
                ),
                at("2026-10-16T03:00:00Z", ",\"event\":\"blamed\",\"id\":5"),
            ],
        );
        let b = view(
            "b",
            vec![at(
                "2026-10-16T02:00:00Z",
                ",\"event\":\"conflict\",\"id\":3",
            )],
        );
        assert_eq!(
            recent_ids(&[a, b]),
            [("a", 5), ("b", 3), ("a", 2), ("a", 1)]
        );
    }

    fn from(worktree: &str, path: Option<&str>, head: Option<&str>) -> Submitter {
        Submitter {
            agent: "a".into(),
            worktree: worktree.into(),
            path: path.map(Into::into),
            head: head.map(Into::into),
            host: None,
            session: None,
        }
    }

    /// Design D6 (ADR 0025): a worktree made what names its path, or its
    /// branch as a pull request's head where sluice lands on that branch
    /// nowhere; read by its branch, what names its branch as its work and
    /// no head.
    #[test]
    fn a_worktree_made_what_names_its_path_or_its_branch_as_a_pull_requests_head() {
        let nowhere = |_: &str| false;
        let here = Here::at("/w/unit", "unit");
        assert!(here.made(&from("unit", Some("/w/unit"), None), nowhere));
        assert!(here.made(&from("pull/9", None, Some("unit")), nowhere));
        assert!(!here.made(&from("unit", None, None), nowhere));
        assert!(!here.made(&from("pull/9", None, Some("other")), nowhere));
        let detached = Here::at("/w/unit", "");
        assert!(!detached.made(&from("pull/9", None, Some("")), nowhere));

        let on = Here::on("unit");
        assert!(on.made(&from("unit", None, None), nowhere));
        assert!(on.made(&from("pull/9", None, Some("unit")), nowhere));
        assert!(!on.made(&from("unit-b", None, None), nowhere));
        assert!(!on.made(&from("pull/9", None, Some("other")), nowhere));
    }

    /// Design D6 (124, 1; rule core.50): a submission by name is no
    /// worktree's, and a pull request from a branch sluice lands on is no
    /// worktree's either; whether sluice lands on the branch is asked once,
    /// and only when such a request is found.
    #[test]
    fn a_submission_by_name_or_a_request_from_a_branch_sluice_lands_on_is_no_worktrees() {
        let asked = std::cell::Cell::new(0);
        let lands_on = |branch: &str| {
            assert_eq!(branch, "bolt/x");
            asked.set(asked.get() + 1);
            true
        };
        let on = Here::on("bolt/x");
        assert!(!on.made(&from("bolt/x", None, Some("bolt/x")), lands_on));
        assert!(on.made(&from("bolt/x", None, None), lands_on), "its own");
        assert_eq!(asked.get(), 0, "asked only for a request");
        assert!(!on.made(&from("pull/9", None, Some("bolt/x")), lands_on));
        assert!(!on.made(&from("pull/10", None, Some("bolt/x")), lands_on));
        assert_eq!(asked.get(), 1, "asked once");
        let at = Here::at("/w/bolt-x", "bolt/x");
        assert!(!at.made(&from("bolt/x", None, Some("bolt/x")), lands_on));
        assert!(!at.made(&from("pull/9", None, Some("bolt/x")), lands_on));
        assert!(at.made(&from("bolt/x", Some("/w/bolt-x"), None), lands_on));
    }

    /// 124, design D6 (rule core.50), scenario "The home view on the
    /// branch": a submission by name of the branch a worktree has checked
    /// out is not shown as the worktree's, queued or landed, nor a request
    /// from that branch, which sluice lands on; the worktree's own is.
    #[test]
    fn the_home_view_takes_no_submission_by_name() {
        use chrono::{DateTime, TimeZone, Utc};
        use sluice_fakes::Fakes;

        use crate::{Clock, SubmitHead, SubmitRequest};

        struct Noon;

        impl Clock for Noon {
            fn now(&self) -> DateTime<Utc> {
                Utc.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap()
            }
        }

        let f = Fakes::new();
        let tip = f.world.init(
            "main",
            &[
                (
                    crate::policy::POLICY_PATH,
                    "[tests]\nrunner = \"fake\"\n\n[checkout]\n\n\
                     [branches.main]\nrequired = []\n\
                     [branches.\"bolt/*\"]\nrequired = []\n",
                ),
                ("a/lib", "a1"),
            ],
        );
        let at = f.world.work(&tip, &[("a/lib", Some("a2"))]);
        f.world.set_branch("bolt/x", &at);
        let s = Sluice::new(
            crate::Seams {
                checkout: &f.checkout,
                tests: &f.tests,
                branch: &f.branch,
                driver: &f.driver,
                state: &f.state,
            },
            &Noon,
        );
        let submit = |head: SubmitHead| {
            s.submit(&SubmitRequest {
                head,
                branch: "main".into(),
                after: Vec::new(),
                priority: 0,
                subject: None,
            })
            .unwrap()
            .unwrap()
            .id
        };
        let by_name = submit(SubmitHead::Branch("bolt/x".into()));
        let here = Here::at("/w/bolt-x", "bolt/x");
        let branches = s.overview(None).unwrap();
        assert!(branches[0].submissions.iter().any(|x| x.id == by_name));
        assert_eq!(s.home(&branches, &here).unwrap(), None, "queued");
        for _ in 0..24 {
            s.step("main").unwrap();
        }
        assert_eq!(
            s.state(by_name).unwrap().unwrap().state,
            crate::records::State::Landed
        );
        let branches = s.overview(None).unwrap();
        assert!(recent_ids(&branches).contains(&("main", by_name)));
        assert_eq!(s.home(&branches, &here).unwrap(), None, "landed");

        // A request from the branch, which sluice lands on, is no
        // worktree's; one from a branch sluice lands on nowhere is.
        let mut request = s.state(by_name).unwrap().unwrap();
        request.from = from("pull/9", None, Some("bolt/x"));
        assert!(!s.made(&here, &request.from));
        f.world.set_branch("unit", &at);
        assert!(s.made(
            &Here::at("/w/unit", "unit"),
            &from("pull/9", None, Some("unit"))
        ));
        // The worktree's own, by its path.
        assert!(s.made(&here, &from("bolt/x", Some("/w/bolt-x"), None)));
    }

    /// ADR 0036: the policy's tables in the order written, a pattern's
    /// branches together in name order, unnamed branches last.
    #[test]
    fn orders_branches_by_the_policy() {
        let policy = Policy::parse(
            "[branches.main]\nrequired = []\n\
             [branches.\"bolt/*\"]\nrequired = []\n\
             [branches.release]\nrequired = []\n",
        )
        .unwrap();
        let names = |v: &[BranchView]| v.iter().map(|b| b.branch.clone()).collect::<Vec<_>>();
        let mut v: Vec<BranchView> = ["scratch", "release", "bolt/b", "main", "bolt/a", "aside"]
            .iter()
            .map(|b| view(b, Vec::new()))
            .collect();
        order(&mut v, Some(&policy));
        assert_eq!(
            names(&v),
            ["main", "bolt/a", "bolt/b", "release", "aside", "scratch"]
        );
        order(&mut v, None);
        assert_eq!(
            names(&v),
            ["aside", "bolt/a", "bolt/b", "main", "release", "scratch"]
        );
    }

    /// ADR 0035: checks hold a branch back only with the host full, a
    /// check holding a run, and the branch with room in its window for a
    /// submission queued.
    #[test]
    fn checks_hold_a_queue_back_only_when_they_take_its_room() {
        let room = |bound, queue, sessions| Room {
            bound,
            queue,
            sessions,
            reserve: 0,
        };
        assert_eq!(room(3, 2, 1).held_by_checks(1, 2, 3), Some(1));
        assert_eq!(room(4, 2, 2).held_by_checks(1, 2, 3), Some(2));
        assert_eq!(room(4, 2, 1).held_by_checks(1, 2, 3), None, "a free run");
        assert_eq!(room(3, 3, 0).held_by_checks(1, 3, 4), None, "no check");
        assert_eq!(room(3, 2, 1).held_by_checks(0, 2, 3), None, "none queued");
        assert_eq!(room(3, 2, 1).held_by_checks(1, 2, 2), None, "window full");
    }

    #[test]
    fn recent_of_no_log_is_empty() {
        assert!(recent(b"", RECENT).unwrap().is_empty());
    }
}
