//! `sluice` with no command: where this worktree stands, for a person, an
//! agent or a status line that asks every few seconds. Its submission,
//! the branches' counts and its place, computed from the overview and
//! cut to the worktree by the path it submits from (ADR 0023). A reader
//! only, as the overview is (rules core.11, core.14).

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use sluice_core::overview::{counts, BranchCount, Here, HOME_SCHEMA};
use sluice_core::records::{State, Submission};
use sluice_core::Sluice;
use sluice_seams::{SessionPlace, Submitter};

use crate::exec::Host;
use crate::{hashes, Names};

/// The commands that follow from where the worktree stands.
fn next(from: Option<&Submitter>, sub: Option<&Submission>) -> Vec<String> {
    let Some(from) = from else {
        return vec!["sluice overview".into()];
    };
    match sub {
        Some(s) if !s.state.is_out() => vec![
            format!("sluice status {}", s.id),
            format!("sluice withdraw {} --as {}", s.id, from.agent),
        ],
        Some(s) if matches!(s.state, State::Failed | State::Conflict) => {
            vec![format!("sluice submit {}", s.branch)]
        }
        Some(s) => vec![
            format!("sluice submit {}", s.branch),
            "sluice overview".into(),
        ],
        None => vec![
            "sluice submit <branch>".into(),
            "sluice submit <branch> --status".into(),
            "sluice overview".into(),
        ],
    }
}

fn submission_json(s: &Submission) -> serde_json::Value {
    let failed: Vec<serde_json::Value> = s
        .failed
        .iter()
        .map(|f| serde_json::json!({ "task": f.task, "log": f.log }))
        .collect();
    serde_json::json!({
        "id": s.id,
        "branch": s.branch,
        "state": s.state.word(),
        "failed": failed,
        "landed_as": s.landed_as,
        "subject": s.subject,
    })
}

pub fn run(
    s: &Sluice<'_>,
    host: &dyn Host,
    worktree: &str,
    json: bool,
    documented: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    // 129 (design D16): from the reader's document of the host process's
    // current reading, where its reader keeps its tick and the look did not
    // make the reading; otherwise from the records as the host holds them,
    // asking no remote.
    let read = documented
        .then(|| crate::overview::reader_document(host))
        .flatten();
    let (branches, room) = match read {
        Some((branches, doc)) => match serde_json::from_value(doc["room"].clone()) {
            Ok(room) => (branches, room),
            Err(_) => (branches, s.room(host.reserved())?),
        },
        None => (
            s.overview(host.default_branch()?.as_ref())?,
            s.room(host.reserved())?,
        ),
    };
    let counts = counts(&branches);
    let held: Vec<String> = branches
        .iter()
        .map(|b| crate::overview::held_back(Some(&room), b))
        .collect();
    // Outside a worktree the driver knows, there is no session to show.
    let from = s.seams().driver.session(worktree).ok().map(|x| x.from);
    let path = from.as_ref().and_then(|f| f.path.clone());
    let sub = match (&from, &path) {
        (Some(f), Some(p)) => s.home(&branches, &Here::at(p, &f.worktree))?,
        _ => None,
    };
    let place: Option<SessionPlace> = match &path {
        Some(p) => s
            .session_places()?
            .into_iter()
            .find(|x| !x.gone && x.worktree == Path::new(p)),
        None => None,
    };
    let next = next(from.as_ref(), sub.as_ref());
    if json {
        let doc = serde_json::json!({
            "schema": HOME_SCHEMA,
            "worktree": from.as_ref().map(|f| serde_json::json!({
                "path": f.path,
                "branch": f.worktree,
                "agent": f.agent,
            })),
            "submission": sub.as_ref().map(submission_json),
            "branches": counts,
            "place": place.as_ref().map(|p| serde_json::json!({
                "ports": p.ports,
                "last_run": p.last_run,
            })),
            "next": next,
        });
        writeln!(out, "{doc}")?;
    } else {
        write_home(
            from.as_ref(),
            sub.as_ref(),
            &counts,
            &held,
            place.as_ref(),
            &next,
            out,
        )?;
    }
    Ok(0)
}

fn write_home(
    from: Option<&Submitter>,
    sub: Option<&Submission>,
    counts: &[BranchCount],
    held: &[String],
    place: Option<&SessionPlace>,
    next: &[String],
    out: &mut dyn Write,
) -> Result<()> {
    match from {
        Some(f) => writeln!(
            out,
            "worktree {} ({}) at {}",
            f.worktree,
            f.agent,
            f.path.as_deref().unwrap_or("an unknown path")
        )?,
        None => writeln!(
            out,
            "not in a worktree sluice can submit from: the agent working in a worktree submits \
             its work from there"
        )?,
    }
    match sub {
        Some(s) => {
            let landed = s
                .landed_as
                .as_ref()
                .map_or(String::new(), |r| format!(" as {}", r.short()));
            // 26: by its number and its subject, or its branch.
            let named = hashes(&[s.id], Some(&Names::of([s])), &mut BTreeSet::new());
            writeln!(out, "{named} to {} · {}{landed}", s.branch, s.state.word())?;
            for f in &s.failed {
                let log = f.log.as_deref().unwrap_or("no log");
                writeln!(out, "  failed {}: {log}", f.task)?;
            }
        }
        None if from.is_some() => writeln!(out, "nothing submitted from this worktree")?,
        None => {}
    }
    if counts.is_empty() {
        writeln!(out, "no branch has records here")?;
    }
    for (c, held) in counts.iter().zip(held) {
        // 26: as the overview shows a branch that could not be read.
        if let Some(why) = &c.unread {
            writeln!(
                out,
                "{} could not be read: {}",
                c.branch,
                why.replace('\n', "\n  ")
            )?;
            continue;
        }
        let up_to = c
            .parallel
            .map_or(String::new(), |p| format!(" (up to {p})"));
        writeln!(
            out,
            "{}: {} in the queue · testing {} at once{up_to}{held}",
            c.branch, c.depth, c.testing
        )?;
    }
    if let Some(p) = place {
        let ports: Vec<String> = p.ports.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let last = p.last_run.as_deref().unwrap_or("never");
        writeln!(out, "place: {} · last run {last}", ports.join(" "))?;
    }
    for n in next {
        writeln!(out, "next: {n}")?;
    }
    Ok(())
}
