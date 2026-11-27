//! A pass of one repository (129, 135; rule core.14; design D5, D6): what
//! `sluice run` ran each tick, moved whole into the host process. The
//! remote's one ask, the abandoned checkouts and unusable previews
//! released, the intake, a round of each branch in play, each branch it
//! could not read passed by with its words, and whether the repository
//! has sluice. A
//! step's round and a pass's are one round (rule core.14).

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sluice_core::{Clock, Sluice, Step};
use sluice_seams::Rev;

use crate::overview;
use crate::{exec, preview, telemetry, PullRequests, INTERRUPTED};

/// What a repository's passes keep from one to the next: what was last
/// said, so each thing is said once while it stands (26, 121), and what
/// was last asked of whether the repository has sluice (129).
#[derive(Default)]
pub struct Kept {
    pub(crate) asked: Option<overview::Asked>,
    /// 121: each branch's hold as last said.
    pub(crate) said: BTreeMap<String, Option<(Rev, Rev)>>,
    /// 26, 121: the words each branch passed by was last said with.
    pub(crate) passed: BTreeMap<String, String>,
    /// 152, record 0124 as amended 2026-11-23 (design D3): when each
    /// branch whose one work is a held landing last had its round.
    pub(crate) held: BTreeMap<String, chrono::DateTime<chrono::Utc>>,
}

impl Kept {
    /// What a `sluice run` started afresh was handed of what its last run
    /// said (135; record 0107).
    pub fn handed(
        said: BTreeMap<String, Option<(Rev, Rev)>>,
        passed: BTreeMap<String, String>,
    ) -> Kept {
        Kept {
            asked: None,
            said,
            passed,
            held: BTreeMap::new(),
        }
    }

    /// 121: each branch's hold as last said.
    pub fn said(&self) -> &BTreeMap<String, Option<(Rev, Rev)>> {
        &self.said
    }

    /// 26: the words each branch passed by was last said with.
    pub fn passed(&self) -> &BTreeMap<String, String> {
        &self.passed
    }
}

/// What a pass came to, as `sluice run` printed it and a follower is told.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// Each line the pass said, as `sluice run` printed it on stdout.
    pub lines: Vec<String>,
    /// 97, 123, previews-1 101: what it freed or released of the host's,
    /// whatever branch: worktrees and previews gone, said as they were.
    #[serde(default)]
    pub freed: Vec<String>,
    /// 123: what each branch's round said of the checkouts it released,
    /// by branch: what a wait on the branch says, as its own rounds did.
    #[serde(default)]
    pub released: BTreeMap<String, Vec<String>>,
    /// 26: each branch this pass passed by, with its words.
    pub failed: BTreeMap<String, String>,
    /// Whether any branch still has work in play once the pass ended: a
    /// submission queued or under test, a checkout to release, or a
    /// preview to drop.
    pub in_play: bool,
    /// 152, record 0124 as amended 2026-11-23 (design D3): a branch holds
    /// a landing the forge refused, which keeps the host process while its
    /// rounds push it again once a minute.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub held: bool,
    /// 129: the reader found the repository has no sluice.
    pub no_sluice: bool,
    /// 129, 133 (design D16): the words a remote refused the pass's one
    /// ask with, where one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
    /// Its number among the repository's passes whose reports `sluice
    /// host` sent, from one (122, 123); 0 from a host that numbers none.
    #[serde(default)]
    pub pass: u64,
    /// The branches its binding reads as last fetched, each with the
    /// remote's refusal (the change `sluice-host-answers-every-read`,
    /// design D3). It stays in the host process.
    #[serde(skip)]
    pub stale: BTreeMap<String, sluice_seams::ReadRefused>,
}

/// One pass of the repository `s` and `host` bind (64, 129). It asks
/// each remote that keeps branches' records where `asks`, and otherwise
/// reads them as the binding's last ask fetched them, which a binding that
/// has asked nothing must not do (129 as amended 2026-11-19). A branch
/// whose read or round fails is passed by with its words, and the pass
/// goes on to every other (26; rule core.14).
#[allow(clippy::too_many_arguments)]
pub fn pass(
    s: &Sluice<'_>,
    host: &dyn exec::Host,
    pulls: &dyn PullRequests,
    clock: &dyn Clock,
    asks: bool,
    kept: &mut Kept,
) -> Result<Report> {
    let mut out: Vec<u8> = Vec::new();
    let mut report = Report::default();
    // 129: what the last pass read by revision and this one does not use
    // is forgotten.
    s.next_pass();
    host.next_pass();
    let seams = s.seams();
    // 129: each remote that keeps branches' records asked once, first,
    // where the pass asks; a refusal is its branches' alone, read as last
    // fetched. Where nothing is in play, it asks once a minute at most, and
    // reads them as last fetched between (129 as amended 2026-11-19).
    if asks {
        if let Err(e) = seams.state.refresh() {
            telemetry::warn(&format!("sluice: warning: {e:#}"));
            report.refused = Some(format!("{e:#}"));
        }
    }
    // 97, previews-1 101: every pass, whether or not a branch has a queue.
    let mut freed: Vec<u8> = Vec::new();
    exec::tell_abandoned(&mut freed, false, &s.release_abandoned()?)?;
    preview::tell_dropped(&mut freed, false, &s.release_unusable_previews()?)?;
    out.extend_from_slice(&freed);
    // 133: records it cannot list are its document, in the words it was
    // refused with, read again at its next pass.
    let mut branches = match seams.state.branches() {
        Ok(b) => b,
        Err(_) => {
            report.lines = lines_of(&out);
            report.freed = lines_of(&freed);
            report.in_play = true;
            return Ok(report);
        }
    };
    let records = branches.clone();
    match pulls.branches(seams) {
        Ok(b) => branches.extend(b),
        Err(e) => telemetry::warn(&format!("sluice: warning: {e:#}")),
    }
    branches.sort();
    branches.dedup();
    for branch in &branches {
        // 133 (design D27): only the causes its note logged are said, read
        // from the kept document, so a restarted host process, a step and
        // a submit say none again.
        let intaken = crate::intake(pulls, s, clock, branch, false, &mut out);
        for c in &intaken.logged {
            telemetry::warn(&format!("sluice: warning: {}", c.line(branch)));
        }
        // 26, rule core.14: a branch whose records' read or round fails is
        // that branch's alone; the pass goes on to every other branch. 129:
        // one with nothing in play costs the pass its records' read alone.
        // 152, record 0124 as amended 2026-11-23 (the change
        // `sluice-host-makes-each-change`, design D3): one whose one work
        // is a held landing has its round, which pushes it again, once a
        // minute at most, as a branch with nothing in play has its remote
        // asked.
        let every = chrono::TimeDelta::from_std(HELD_EVERY).unwrap_or_default();
        let held_due = s.holds_refused_landing(branch).unwrap_or(false)
            && kept
                .held
                .get(branch)
                .is_none_or(|at| clock.now() - *at >= every);
        let held_due = |_: &str| held_due;
        let ran = s.in_play(branch).and_then(|in_play| match in_play {
            false if held_due(branch) => {
                kept.held.insert(branch.clone(), clock.now());
                s.round(branch).map(Some)
            }
            false => Ok(None),
            true => s.round(branch).map(Some),
        });
        let round = match ran {
            Ok(round) => {
                kept.passed.remove(branch);
                round
            }
            // Ctrl-C reaching the git a branch's read runs is no refusal of
            // that branch's.
            Err(_) if INTERRUPTED.load(Ordering::SeqCst) => None,
            Err(e) => {
                let words = format!("{e:#}");
                crate::tell_passed(&mut out, branch, &words, &mut kept.passed)?;
                report.failed.insert(branch.clone(), words);
                None
            }
        };
        if let Some(round) = round {
            // 120, 128, 136 (design D5): a submit's pull request the round
            // sent back is marked a draft and shown held before any
            // follower reads the pass, with nothing asked of the forge.
            if round.step == Step::Stepped {
                let again = crate::settle(pulls, s, clock, branch);
                for c in &again.logged {
                    telemetry::warn(&format!("sluice: warning: {}", c.line(branch)));
                }
            }
            let mut gone: Vec<u8> = Vec::new();
            exec::tell_abandoned(&mut gone, false, &round.released)?;
            preview::tell_dropped(&mut gone, false, &round.previews)?;
            freed.extend_from_slice(&gone);
            out.extend_from_slice(&gone);
            let mut released: Vec<u8> = Vec::new();
            exec::tell_releases(&mut released, false, &round.releases)?;
            out.extend_from_slice(&released);
            report.released.insert(branch.clone(), lines_of(&released));
            if matches!(round.step, Step::Stepped | Step::Held) {
                let said = kept.said.entry(branch.clone()).or_default();
                crate::tell_hold(&mut out, branch, round.held.as_ref(), said)?;
            }
        }
    }
    // 129: whether the repository has sluice, read as a one-shot overview
    // reads it; 137 (design D16, D25): the pass reads each request this
    // host follows into a branch it does not keep, as it falls due at the
    // host's one bound. It builds no overview for them: a branch's view,
    // its log and its policy are read when a reader asks. A read it could
    // not make says nothing of it.
    if let Ok(none) = overview::pass_reads(s, host, clock, pulls, &records, &mut kept.asked) {
        report.no_sluice = none;
    }
    // What is left in play once the pass ended keeps its repository held
    // (135; design D6): a branch that cannot be read counts as in play, and
    // so does a preview to drop.
    report.in_play = branches
        .iter()
        .any(|b| report.failed.contains_key(b) || s.in_play(b).unwrap_or(true))
        || s.previews_in_play().unwrap_or(true);
    report.held = held(s);
    report.lines = lines_of(&out);
    report.freed = lines_of(&freed);
    Ok(report)
}

/// `said`, a pass's output, as its lines.
fn lines_of(said: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(said)
        .lines()
        .map(str::to_string)
        .collect()
}

/// Whether the repository has work in play (135): a submission queued or
/// under test or a checkout to release on any branch, read from the
/// records alone, or a preview to drop, read from the previews' places
/// and runs, so an idle tick reads it with no process started (129). A
/// branch whose records cannot be read counts, as do previews that cannot
/// be listed.
pub fn in_play(s: &Sluice<'_>) -> bool {
    let branches = match s.seams().state.branches() {
        Ok(branches) => branches.iter().any(|b| s.in_play(b).unwrap_or(true)),
        Err(_) => true,
    };
    branches || s.previews_in_play().unwrap_or(true)
}

/// How often a branch whose one work is a held landing has its round: a
/// minute, the pace of a repository with nothing in play (152, record 0124
/// as amended 2026-11-23; design D3).
pub const HELD_EVERY: std::time::Duration = super::IDLE_ASK;

/// Whether any branch of the repository holds a landing the forge refused
/// (152; design D3), read from the records alone.
pub fn held(s: &Sluice<'_>) -> bool {
    s.seams().state.branches().is_ok_and(|b| {
        b.iter()
            .any(|b| s.holds_refused_landing(b).unwrap_or(false))
    })
}

/// Each round of the branches of the repository `s` binds that hold a
/// landing the forge refused, as a look of a repository with nothing else
/// in play takes them, once a minute (152, record 0124 as amended
/// 2026-11-23; design D3). A round that fails is the branch's alone.
pub fn push_held(s: &Sluice<'_>) {
    let Ok(branches) = s.seams().state.branches() else {
        return;
    };
    for branch in branches {
        if s.holds_refused_landing(&branch).unwrap_or(false) {
            if let Err(e) = s.round(&branch) {
                telemetry::warn(&format!("sluice: warning: {branch}: {e:#}"));
            }
        }
    }
}

/// `sluice step`'s round of `branch`, as the host process runs it between
/// its passes (rule core.14; design D10): the remote's one ask, its intake
/// as today, then the round, then what the round sent back of a pull
/// request shown where it stepped, as a pass's are, so a step reads each
/// signer's runs as a pass does,
/// answered as the document `sluice step --json` prints, with what the
/// round said beside it, each warning its intakes met, once each, which
/// the step prints on stderr (128; design D26), and the line a step prints
/// without `--json`.
pub fn step(
    s: &Sluice<'_>,
    pulls: &dyn PullRequests,
    clock: &dyn Clock,
    branch: &str,
) -> Result<serde_json::Value> {
    let mut said: Vec<u8> = Vec::new();
    // 129 (design D10, D17): each remote that keeps branches' records asked
    // once, first, its signers' runs among it, as a pass asks it; a
    // refusal is said, and the round reads the remote as last fetched.
    let refused = s.seams().state.refresh().err().map(|e| format!("{e:#}"));
    // 128 (design D26): each cause its intake met, which the step prints
    // on stderr, every step.
    let mut warnings: Vec<String> = refused.into_iter().collect();
    warnings.extend(
        crate::intake(pulls, s, clock, branch, false, &mut said)
            .causes
            .iter()
            .map(|c| c.line(branch)),
    );
    let round = s.round(branch)?;
    if round.step == Step::Stepped {
        for c in &crate::settle(pulls, s, clock, branch).causes {
            let line = c.line(branch);
            if !warnings.contains(&line) {
                warnings.push(line);
            }
        }
    }
    exec::tell_abandoned(&mut said, false, &round.released)?;
    preview::tell_dropped(&mut said, false, &round.previews)?;
    exec::tell_releases(&mut said, false, &round.releases)?;
    let outcome = match round.step {
        Step::Stepped => "stepped",
        Step::Busy => "busy",
        Step::Led => "elsewhere",
        Step::Contended => "contended",
        Step::Held => "held",
    };
    let mut document = serde_json::json!({
        "branch": branch,
        "outcome": outcome,
        "released": exec::abandoned_json(&round.released),
        "previews_released": preview::dropped_json(&round.previews),
        "releases": exec::releases_json(&round.releases),
    });
    let line = match &round.held {
        Some(hold) => {
            document["held"] = crate::held_json(hold);
            crate::held_line(branch, hold)
        }
        None => {
            let word = match round.step {
                Step::Stepped => "stepped",
                Step::Busy => "busy: another round holds the lock",
                Step::Led => {
                    "elsewhere: another host runs the branch's queue, and its lease has not passed"
                }
                Step::Contended => "contended: the records moved; nothing done",
                Step::Held => "held",
            };
            format!("{branch}: {word}")
        }
    };
    Ok(serde_json::json!({
        "document": document,
        "said": lines_of(&said),
        "warnings": warnings,
        "line": line,
    }))
}
