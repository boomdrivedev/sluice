//! `sluice overview`: every branch's queue at a glance, for a person, a
//! pane or a status line (26, 49; ADR 0023). A reader only: it releases,
//! counts and writes nothing (rules core.11, core.14), so `sluice
//! previews`' own tidying is left to it. A watch writes nothing: it
//! follows the reader's document sluice's host process writes (129, 135),
//! and ends where the repository has no sluice (ADR 0056).

use std::collections::BTreeSet;
use std::io::Write;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Result;
use chrono::SecondsFormat;
use sluice_core::overview::{BranchView, Room, OVERVIEW_SCHEMA};
use sluice_core::records::{CandidateState, RefusedWrite, State};
use sluice_core::{Clock, Sluice};

use crate::exec::Host;
use crate::preview::preview_json;
use crate::reader::{no_sluice, untimed, Follow};
use crate::{
    hashes, held_line, progress, sessions_json, telemetry, Across, Names, Pause, PullRequests,
    INTERRUPTED, STOPPED,
};

/// 137, design D5: how the overview reads the pull requests into each
/// branch this host follows and another host takes in.
#[derive(Clone, Copy)]
pub enum Follows<'a> {
    /// As `sluice status` reads them, each one due asked of GitHub at the
    /// host's one bound: a one-shot overview's.
    Asked(&'a dyn PullRequests),
    /// As the host last kept them, GitHub asked nothing: a reader's
    /// document's, read again only once what the host keeps of them moved
    /// (129; rules core.25, core.31).
    Kept(&'a dyn PullRequests),
}

/// Every branch's part, and the whole document as `--json` prints it,
/// asking no remote: every branch read as the host holds it since the last
/// ask (129), with the pull requests into it the host follows, read as
/// `follows` says (137).
fn document_held(
    s: &Sluice<'_>,
    host: &dyn Host,
    clock: &dyn Clock,
    follows: Follows<'_>,
) -> Result<(Vec<BranchView>, serde_json::Value)> {
    let branches = s.overview(host.default_branch()?.as_ref())?;
    let mut views = serde_json::to_value(&branches)?;
    // 137, design D5: each branch with pull requests the host follows and
    // another host takes in carries them, as `sluice status` gives them; one
    // this host has no records of is listed with its empty queue, as status
    // shows it, after the others.
    for (branch, a) in followed(s, clock, follows, &branches) {
        let views = views.as_array_mut().expect("the branches are a list");
        let at = match views.iter().position(|v| v["branch"] == branch.as_str()) {
            Some(at) => at,
            None => {
                let view = s
                    .branch_view_of(&branch)
                    .unwrap_or_else(|e| BranchView::unread(&branch, &e));
                views.push(serde_json::to_value(&view)?);
                views.len() - 1
            }
        };
        views[at]["taken_in_by"] = serde_json::to_value(&a.taken_in_by)?;
        views[at]["pull_requests"] = serde_json::to_value(&a.pull_requests)?;
    }
    let mut sessions = sessions_json(&s.session_places()?, &s.held()?);
    // ADR 0035: each worktree's session run going, from the host's state;
    // a run in another repository counts only in `room`.
    let runs = s.seams().tests.session_runs()?;
    for w in &mut sessions {
        if let Some(r) = runs.iter().find(|r| w["worktree"] == r.worktree.as_str()) {
            let mut run = serde_json::json!({ "started": r.started });
            if let Some(c) = &r.command {
                run["command"] = c.as_str().into();
            }
            if !r.tasks.is_empty() {
                run["tasks"] = serde_json::json!(r.tasks);
            }
            w["run"] = run;
        }
    }
    let previews: Vec<serde_json::Value> = s.previews()?.iter().map(preview_json).collect();
    let hosts = s.hosts()?;
    let (room, process) = room_and_process(s, host, clock)?;
    let doc = serde_json::json!({
        "schema": OVERVIEW_SCHEMA,
        "host": host.name(),
        "repository": host.repository()?,
        "at": clock.now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "branches": views,
        "sessions": sessions,
        "previews": previews,
        "hosts": hosts,
        "room": room,
        "host_process": process,
    });
    Ok((branches, doc))
}

/// 71, 150 (record 0123; design D12, D13): the host's room as the overview
/// gives it, the runs waiting by kind, whose turn the next contested run
/// is and each repository's runs under test and waiting beside the count,
/// and sluice's host process, `{version, pid, since}`, both read from the
/// host process where one answers: where none does, nothing waits and the
/// process is `null`.
pub(crate) fn room_and_process(
    s: &Sluice<'_>,
    host: &dyn Host,
    clock: &dyn Clock,
) -> Result<(serde_json::Value, serde_json::Value)> {
    let mut room = serde_json::to_value(s.room(host.reserved())?)?;
    let here = crate::host_process::Here {
        s,
        host,
        pulls: &(),
        clock,
    };
    let answered = host.host_process().room(&here).unwrap_or(None);
    Ok(match answered {
        Some((named, view)) => {
            room["waiting"] = serde_json::to_value(view.waiting)?;
            room["turn"] = serde_json::to_value(view.turn)?;
            room["repositories"] = serde_json::to_value(&view.repositories)?;
            let process = serde_json::json!({
                "version": named.version,
                "pid": named.pid,
                "since": named.since,
            });
            (room, process)
        }
        None => {
            room["waiting"] = serde_json::json!({ "queue": 0, "sessions": 0 });
            room["turn"] = "queue".into();
            room["repositories"] = serde_json::json!({});
            (room, serde_json::Value::Null)
        }
    })
}

/// The host's room on one line, as the overview, the home view and
/// `sluice status` say it, from the room as the overview gives it; `None`
/// while nothing runs or waits.
pub(crate) fn room_line(room: &serde_json::Value) -> Option<String> {
    let n = |v: &serde_json::Value| v.as_u64().unwrap_or(0);
    let (queue, sessions) = (n(&room["queue"]), n(&room["sessions"]));
    let waiting = &room["waiting"];
    let (wq, ws) = (n(&waiting["queue"]), n(&waiting["sessions"]));
    if queue + sessions + wq + ws == 0 {
        return None;
    }
    let mut line = format!("host: {} of {} runs", queue + sessions, n(&room["bound"]));
    if queue > 0 {
        line.push_str(&format!(" · {queue} queue"));
    }
    match sessions {
        0 => {}
        1 => line.push_str(" · 1 agent"),
        n => line.push_str(&format!(" · {n} agents")),
    }
    if wq + ws > 0 {
        let mut waits = Vec::new();
        if wq > 0 {
            waits.push(format!(
                "{wq} combination{}",
                if wq == 1 { "" } else { "s" }
            ));
        }
        if ws > 0 {
            waits.push(format!("{ws} agent{}", if ws == 1 { "" } else { "s" }));
        }
        line.push_str(&format!(" · waiting: {}", waits.join(", ")));
        if room["turn"] == "sessions" {
            line.push_str(" · an agent's next");
        }
    }
    Some(line)
}

/// 137, design D5: each branch's pull requests the host follows and
/// another host takes in, by branch, read as `follows` says: the overview's
/// branches and every branch the default branch's policy takes pull
/// requests on. A one-shot read that fails is warned of, as `sluice status`
/// warns of it, and its branch shown without them; a reader's tick shows
/// it without them and says nothing, since the next one-shot read says it.
fn followed(
    s: &Sluice<'_>,
    clock: &dyn Clock,
    follows: Follows<'_>,
    views: &[BranchView],
) -> Vec<(String, Across)> {
    let (pulls, asked) = match follows {
        Follows::Asked(pulls) => (pulls, true),
        Follows::Kept(pulls) => (pulls, false),
    };
    let warn = |e: anyhow::Error| {
        if asked {
            telemetry::warn(&format!("sluice: warning: {e:#}"));
        }
    };
    let mut branches: BTreeSet<String> = views.iter().map(|b| b.branch.clone()).collect();
    match pulls.branches(s.seams()) {
        Ok(b) => branches.extend(b),
        Err(e) => warn(e),
    }
    let mut out = Vec::new();
    for branch in branches {
        let read = match follows {
            Follows::Asked(_) => pulls.across(s.seams(), clock, &branch),
            Follows::Kept(_) => pulls.kept_across(s.seams(), clock, &branch),
        };
        match read {
            Ok(Some(a)) => out.push((branch, a)),
            Ok(None) => {}
            Err(e) => warn(e),
        }
    }
    out
}

/// `tick` is how long `pause` pauses, which a watch that reads names in
/// its documents (64, 129).
#[allow(clippy::too_many_arguments)]
pub fn run(
    s: &Sluice<'_>,
    host: &dyn Host,
    clock: &dyn Clock,
    pulls: &dyn PullRequests,
    json: bool,
    watch: bool,
    pause: &dyn Pause,
    out: &mut dyn Write,
) -> Result<i32> {
    if !watch {
        // 129 (design D16): once the host process's reading is current, the
        // records as the host holds them, asking no remote, and the pull
        // requests it follows as the host last kept them.
        let (branches, doc) = document_held(s, host, clock, Follows::Kept(pulls))?;
        if json {
            writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
        } else {
            write_overview(&branches, &doc, out)?;
        }
        return Ok(0);
    }
    // 129, 135: sluice's host process is the repository's reader on the
    // host; the watch follows it, starting it where none answers, and its
    // document, reading nothing itself and taking no lock or lease (rule
    // core.25). A host process that stopped is reached again at the next
    // tick, and the last document stands until the next is written.
    let (readers, repository) = (host.readers()?, host.repository()?);
    let follow = Follow::of(&readers, &repository, host.version());
    let here = crate::host_process::Here {
        s,
        host,
        pulls,
        clock,
    };
    let mut follower = crate::host_process::ask::Follower::new(
        &here,
        crate::host_process::protocol::FollowKind::Watch,
    );
    // Once at the start, then each time anything but the time it was read
    // changes: what reads the lines finds what happened by comparing
    // `recent`.
    let mut last: Option<serde_json::Value> = None;
    // 129: it ends only when stopped, when its output is closed, when its
    // repository is gone, or where the repository has no sluice; anything
    // else that fails is tried again at the next tick, which is its cadence
    // and no retry (rule core.29).
    loop {
        follower.tick(&here);
        if INTERRUPTED.load(Ordering::SeqCst) {
            return Ok(STOPPED);
        }
        // 129: a repository gone ends the watch as a failing overview
        // ends, with the host process's read's words where it wrote them.
        if host.repository_gone(&repository) {
            let said = follow
                .read()
                .and_then(|d| d["error"]["message"].as_str().map(str::to_string));
            match said {
                Some(words) => anyhow::bail!("{words}"),
                None => anyhow::bail!("the repository at {repository} is gone"),
            }
        }
        if let Some(doc) = follow.read() {
            let now = untimed(&doc);
            if last.as_ref() != Some(&now) {
                writeln!(out, "{}", serde_json::to_string(&doc)?)?;
                out.flush()?;
                last = Some(now);
            }
            // 129: the repository has no sluice, as the host process read
            // it: the document saying so is the last line.
            if no_sluice(&doc) {
                return Ok(0);
            }
        }
        if !pause.pause() {
            return Ok(STOPPED);
        }
    }
}

/// `doc` as a reader writes it: naming its tick, the pause between its
/// reads, in whole seconds, so a forecast can tell whether it keeps it
/// (64, 129).
pub(crate) fn ticked(mut doc: serde_json::Value, tick: Duration) -> serde_json::Value {
    doc["tick"] = tick.as_secs().into();
    doc
}

/// What a watch last asked of whether the repository has sluice: the tips
/// the host held, and whether any held the policy (129).
pub(crate) struct Asked {
    tips: Vec<sluice_seams::Rev>,
    holds: bool,
}

/// The reader's document of one pass, asking no remote: every branch
/// read as the host holds it since the pass's own ask (129, 135). The
/// overview says `"set_up": false` where the records list no branch and
/// no branch the host holds, its own or a remote's as last fetched, has
/// the policy. The policy is looked for again only once the tips the host
/// holds, or the records' list of branches, changed since `asked`: a list
/// with a branch in it forgets what was asked, so its emptying asks again.
/// A check of the branches that fails is a read it could not make (133).
pub(crate) fn read_held(
    s: &Sluice<'_>,
    host: &dyn Host,
    clock: &dyn Clock,
    follows: Follows<'_>,
    asked: &mut Option<Asked>,
) -> Result<serde_json::Value> {
    let (branches, mut doc) = document_held(s, host, clock, follows)?;
    if !branches.is_empty() {
        *asked = None;
        return Ok(doc);
    }
    let tips = host.tips()?;
    let holds = match asked.take() {
        Some(a) if a.tips == tips => a.holds,
        _ => host.holds_policy(&tips)?,
    };
    *asked = Some(Asked { tips, holds });
    if !holds {
        doc["set_up"] = false.into();
    }
    Ok(doc)
}

/// The overview as text: a line per branch, its combinations under test,
/// and its latest landings, blames and conflicts, newest first.
fn write_overview(
    branches: &[BranchView],
    doc: &serde_json::Value,
    out: &mut dyn Write,
) -> Result<()> {
    if branches.is_empty() {
        writeln!(out, "no branch has records here")?;
    }
    let room: Option<Room> = serde_json::from_value(doc["room"].clone()).ok();
    let now = doc["at"]
        .as_str()
        .and_then(|at| at.parse().ok())
        .unwrap_or_default();
    let mut gone = Vec::new();
    for b in branches {
        // 26: a branch that could not be read is shown with the refusal,
        // in its words, and nothing read of it.
        if let Some(why) = &b.unread {
            writeln!(
                out,
                "{} could not be read: {}",
                b.branch,
                why.replace('\n', "\n  ")
            )?;
            write_held_places(&held_places(&b.branch, Some(b), doc), out)?;
            write_across(doc, &b.branch, out)?;
            continue;
        }
        let places = held_places(&b.branch, Some(b), doc);
        // 105, record 0091: a branch gone with nothing of it to watch is
        // named apart, after the branches it lands on.
        if b.gone && nothing_to_watch(b, &places) {
            gone.push(b.branch.as_str());
            continue;
        }
        let at = b.at.as_ref().map_or("nothing", |r| r.short());
        let testing = b.candidates.iter().filter(|c| c.state.under_test()).count();
        let up_to = b
            .parallel
            .map_or(String::new(), |p| format!(" (up to {p})"));
        let held = held_back(room.as_ref(), b);
        if b.gone {
            writeln!(out, "{}", gone_line(&b.branch, b.at.as_ref()))?;
        } else {
            writeln!(
                out,
                "{} at {at} · {} in the queue · testing {testing} at once{up_to}{held}",
                b.branch, b.depth
            )?;
        }
        if let Some(hold) = &b.held {
            writeln!(out, "  {}", held_line(&b.branch, hold))?;
        }
        // 80, 129: its records as last fetched, and why.
        if let Some(why) = &b.stale {
            writeln!(out, "  {why}: shown as last fetched")?;
        }
        write_held_places(&places, out)?;
        write_refused_writes(&b.branch, &b.refused_writes, out)?;
        let names = branch_names(b);
        for c in &b.candidates {
            if !matches!(c.state, CandidateState::Starting | CandidateState::Testing) {
                continue;
            }
            // ADR 0042: how far it has got, as of when the overview was read.
            writeln!(
                out,
                "  {}",
                progress::testing_line(&b.branch, c, now, Some(&names))
            )?;
        }
        for r in b.recent.iter().rev().take(3) {
            writeln!(out, "  {}", recent_line(r, &names))?;
        }
        write_across(doc, &b.branch, out)?;
    }
    if !gone.is_empty() {
        writeln!(out, "gone, nothing queued or recent: {}", gone.join(", "))?;
    }
    // 123: a worktree's or a preview's held place whose branch has no
    // records is named all the same; 137: so are the pull requests into it
    // the host follows.
    for branch in unlisted_branches(branches, doc) {
        writeln!(out, "{branch} · no records")?;
        write_held_places(&held_places(&branch, None, doc), out)?;
        write_across(doc, &branch, out)?;
    }
    // 77, 107: the hosts serving a remote, by their last sign of life.
    for h in doc["hosts"].as_array().map_or(&[][..], Vec::as_slice) {
        let mut line = format!(
            "host {}  {}  {}  {} held",
            h["host"].as_str().unwrap_or(""),
            h["signer"].as_str().unwrap_or("signs nothing"),
            h["system"].as_str().unwrap_or("any system"),
            h["held"].as_u64().unwrap_or(0),
        );
        if h["lapsed"].as_bool() == Some(true) {
            line.push_str(" · lapsed: no sign of life for the bound");
        } else {
            line.push_str(&format!(" · room for {}", h["room"].as_u64().unwrap_or(0)));
        }
        writeln!(out, "{line}")?;
    }
    let sessions = doc["sessions"].as_array().map_or(&[][..], Vec::as_slice);
    // ADR 0035: agents' checks, as load on the host, never as submissions.
    for w in sessions {
        let run = &w["run"];
        if run.is_null() {
            continue;
        }
        let what = match (run["command"].as_str(), run["tasks"].as_array()) {
            (Some(c), _) => format!(" {c}"),
            (None, Some(t)) => {
                let done = t
                    .iter()
                    .filter(|t| {
                        matches!(t["state"].as_str(), Some("passed" | "failed" | "replayed"))
                    })
                    .count();
                format!(" {done}/{}", t.len())
            }
            (None, None) => String::new(),
        };
        writeln!(
            out,
            "checking {} → {}{what} since {}",
            w["worktree"].as_str().unwrap_or(""),
            w["branch"].as_str().unwrap_or(""),
            run["started"].as_str().unwrap_or("")
        )?;
    }
    if let Some(line) = room_line(&doc["room"]) {
        writeln!(out, "{line}")?;
    }
    let held = sessions.iter().filter(|w| w.get("ports").is_some()).count();
    let previews = doc["previews"].as_array().map_or(0, Vec::len);
    if held + previews > 0 {
        let worktrees = if held == 1 { "worktree" } else { "worktrees" };
        writeln!(out, "{held} {worktrees} held · {previews} previews")?;
    }
    for w in sessions {
        if let Some(b) = w["holds_queued_branch"].as_str() {
            writeln!(
                out,
                "warning: {} has {b} checked out: sluice moves {b} under it",
                w["worktree"].as_str().unwrap_or("")
            )?;
        }
    }
    Ok(())
}

/// 137, design D5: the pull requests into `branch` the host follows and
/// another host takes in, as the document carries them; `None` where it
/// carries none.
fn across_in(doc: &serde_json::Value, branch: &str) -> Option<Across> {
    let view = doc["branches"]
        .as_array()?
        .iter()
        .find(|v| v["branch"] == branch)?;
    Some(Across {
        taken_in_by: serde_json::from_value(view["taken_in_by"].clone()).ok()?,
        pull_requests: serde_json::from_value(view.get("pull_requests")?.clone()).ok()?,
    })
}

/// 137, design D5: under `branch`, the pull requests the host follows and
/// another host takes in, as `sluice status <branch>` words them.
fn write_across(doc: &serde_json::Value, branch: &str, out: &mut dyn Write) -> Result<()> {
    if let Some(a) = across_in(doc, branch) {
        writeln!(out, "  {}", a.heading(branch))?;
        for row in &a.pull_requests {
            writeln!(out, "    {}", a.line(row))?;
        }
    }
    Ok(())
}

/// 105, record 0091: a branch that is gone, by the revision its records
/// last saw, as the overview and `sluice status` head it.
pub fn gone_line(branch: &str, at: Option<&sluice_seams::Rev>) -> String {
    match at {
        Some(at) => format!("{branch} is gone, last at {}", at.short()),
        None => format!("{branch} is gone"),
    }
}

/// 105, record 0091: nothing of a branch a person watches its queue for:
/// nothing queued, under test or recent, no hold, no held place, no
/// refused write and no read refused.
fn nothing_to_watch(b: &BranchView, places: &[HeldPlace]) -> bool {
    b.depth == 0
        && b.submissions.is_empty()
        && b.candidates.is_empty()
        && b.recent.is_empty()
        && b.held.is_none()
        && b.stale.is_none()
        && b.refused_writes.is_empty()
        && places.is_empty()
}

/// 123: a place of a branch held because its release failed and tried
/// no more, as the overview names it.
struct HeldPlace {
    /// `sluice/try/main/48`, `worktree /work/app/unit-a` or `preview docs`.
    name: String,
    /// What `sluice release` is given to release it.
    release: String,
    /// How many times its release failed.
    tries: u64,
    /// The last line of what failed.
    failed: String,
    /// Another host's, on which alone it is released.
    host: Option<String>,
}

/// `branch`'s held places: its checkouts held, then the worktrees and the
/// previews of it whose release failed and is tried no more.
fn held_places(branch: &str, b: Option<&BranchView>, doc: &serde_json::Value) -> Vec<HeldPlace> {
    let here = doc["host"].as_str();
    let checkouts = b
        .into_iter()
        .flat_map(|b| &b.held_checkouts)
        .filter(|f| f.stopped)
        .map(|f| HeldPlace {
            name: f.place.clone(),
            release: f.place.clone(),
            tries: f.tries.into(),
            failed: last_line(&f.failed),
            host: f.host.clone().filter(|h| Some(h.as_str()) != here),
        });
    let of = |key: &str| doc[key].as_array().map_or(&[][..], Vec::as_slice);
    let stopped = |v: &&serde_json::Value| {
        v["branch"] == branch && v["release_failed"]["stopped"].as_bool() == Some(true)
    };
    let failure = |v: &serde_json::Value| {
        (
            v["release_failed"]["tries"].as_u64().unwrap_or(0),
            last_line(v["release_failed"]["failed"].as_str().unwrap_or("")),
        )
    };
    let worktrees = of("sessions").iter().filter(stopped).map(|w| {
        let path = w["worktree"].as_str().unwrap_or("");
        let (tries, failed) = failure(w);
        HeldPlace {
            name: format!("worktree {path}"),
            release: format!("--worktree {path}"),
            tries,
            failed,
            host: None,
        }
    });
    let previews = of("previews").iter().filter(stopped).map(|p| {
        let name = p["name"].as_str().unwrap_or("");
        let (tries, failed) = failure(p);
        HeldPlace {
            name: format!("preview {name}"),
            release: name.to_string(),
            tries,
            failed,
            host: None,
        }
    });
    checkouts.chain(worktrees).chain(previews).collect()
}

/// The branches that have no records of held worktrees and previews, and
/// of the pull requests the host follows, in name order.
fn unlisted_branches(branches: &[BranchView], doc: &serde_json::Value) -> BTreeSet<String> {
    let of = |key: &str| doc[key].as_array().map_or(&[][..], Vec::as_slice);
    let held = ["sessions", "previews"]
        .iter()
        .flat_map(|key| of(key))
        .filter(|v| v["release_failed"]["stopped"].as_bool() == Some(true));
    let followed = of("branches")
        .iter()
        .filter(|v| v.get("pull_requests").is_some());
    held.chain(followed)
        .filter_map(|v| v["branch"].as_str())
        .filter(|b| !branches.iter().any(|v| v.branch == *b))
        .map(str::to_string)
        .collect()
}

/// The last line of `text` with anything on it.
fn last_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

/// 123: each held place in two lines, as the round says a hold: what
/// failed, then how to run its release again or free it without.
fn write_held_places(places: &[HeldPlace], out: &mut dyn Write) -> Result<()> {
    for p in places {
        let on = p
            .host
            .as_ref()
            .map_or(String::new(), |h| format!(" on {h}"));
        let times = match p.tries {
            1 => "once".to_string(),
            n => format!("{n} times"),
        };
        let failed = match p.failed.as_str() {
            "" => String::new(),
            f => format!(": {f}"),
        };
        writeln!(
            out,
            "  {}{on} is held: its release failed {times}{failed}",
            p.name
        )?;
        let run = p
            .host
            .as_ref()
            .map_or(String::new(), |h| format!("on {h}: "));
        writeln!(
            out,
            "    {run}sluice release {r} runs it again; sluice release {r} --accept-failure frees it without",
            r = p.release
        )?;
    }
    Ok(())
}

/// How many of a branch's refused writes the overview shows under it.
const REFUSED_SHOWN: usize = 3;

/// 148: a line for each of the branch's standing causes of refused writes,
/// newest first and at most three, then how many more and where they are
/// (design D6).
fn write_refused_writes(branch: &str, causes: &[RefusedWrite], out: &mut dyn Write) -> Result<()> {
    for c in causes.iter().take(REFUSED_SHOWN) {
        let writes = match c.times {
            1 => "1 write".to_string(),
            n => format!("{n} writes"),
        };
        writeln!(
            out,
            "  sluice refused {writes} on {}, last {}: {}",
            c.host,
            c.last,
            command_line(&c.command)
        )?;
    }
    if causes.len() > REFUSED_SHOWN {
        writeln!(
            out,
            "  and {} more: sluice status {branch}",
            causes.len() - REFUSED_SHOWN
        )?;
    }
    Ok(())
}

/// How many characters of a refused write's command a line shows.
const COMMAND_SHOWN: usize = 100;

/// 148: a refused write's command as a line shows it: its first line, cut
/// after a hundred characters, `…` where anything of it is left out
/// (design D5, D6). `--json` carries it whole.
pub(crate) fn command_line(command: &str) -> String {
    let mut lines = command.lines();
    let first = lines.next().unwrap_or_default();
    let mut shown: String = first.chars().take(COMMAND_SHOWN).collect();
    if shown.len() < first.len() || lines.next().is_some() {
        shown.push('…');
    }
    shown
}

/// ADR 0035: the clause a branch's counts line ends with when agents'
/// checks hold its queue back; empty otherwise.
pub fn held_back(room: Option<&Room>, b: &BranchView) -> String {
    let queued = b
        .submissions
        .iter()
        .filter(|s| s.state == State::Queued)
        .count();
    let testing = b.candidates.iter().filter(|c| c.state.under_test()).count();
    held_clause(room, queued, testing, b.window)
}

/// The clause for `queued` submissions, `testing` candidates and a
/// `window`, as [`Room::held_by_checks`] decides it.
pub fn held_clause(room: Option<&Room>, queued: usize, testing: usize, window: usize) -> String {
    match room.and_then(|r| r.held_by_checks(queued, testing, window)) {
        None => String::new(),
        Some(1) => " · 1 slot to an agent's checks".into(),
        Some(n) => format!(" · {n} slots to agents' checks"),
    }
}

/// Each submission a branch's lines may name (26): one in its chain by
/// its record, one its recent lines name by its subject, else the
/// worktree it came from; one a document gives neither for, by its
/// number alone.
fn branch_names(b: &BranchView) -> Names {
    let mut names = Names::default();
    for (id, from) in &b.recent_from {
        names.insert(*id, from);
    }
    for (id, subject) in &b.recent_subjects {
        names.insert(*id, subject);
    }
    for s in &b.submissions {
        names.insert(s.id, s.name());
    }
    names
}

/// One recent log line, as text, its submissions by `names` (26).
fn recent_line(r: &serde_json::Value, names: &Names) -> String {
    let at = r["at"].as_str().unwrap_or("");
    let ids = |v: &serde_json::Value| -> Vec<u64> {
        v.as_array()
            .map(|a| a.iter().filter_map(serde_json::Value::as_u64).collect())
            .unwrap_or_default()
    };
    let short =
        |v: &serde_json::Value| v.as_str().unwrap_or("").chars().take(7).collect::<String>();
    let named = |ids: &[u64]| hashes(ids, Some(names), &mut BTreeSet::new());
    let id = |v: &serde_json::Value| named(&[v.as_u64().unwrap_or(0)]);
    match r["event"].as_str() {
        Some("landed") => format!(
            "{at}  landed {} as {}{}",
            named(&ids(&r["ids"])),
            short(&r["to"]),
            // 140: with no check run.
            if r["beside"] == true {
                " beside the queue"
            } else {
                ""
            }
        ),
        Some("blamed") => format!(
            "{at}  blamed {} on {}",
            id(&r["id"]),
            short(&r["combination"])
        ),
        Some("conflict") => {
            let paths: Vec<&str> = r["paths"]
                .as_array()
                .map(|a| a.iter().filter_map(serde_json::Value::as_str).collect())
                .unwrap_or_default();
            format!("{at}  conflict {} on {}", id(&r["id"]), paths.join(", "))
        }
        _ => at.to_string(),
    }
}
