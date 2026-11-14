//! `sluice preview`, `sluice previews` and `sluice release <preview>`: a
//! running copy of a revision a branch has landed, held until its caller
//! asks for its release (previews-1 clauses 98 to 103). Its command counts
//! against the host's bound while it runs, as a session run does, and
//! never takes the reserve (102).

use std::io::Write;

use anyhow::Result;
use sluice_core::preview::{Dropped, PreviewRefusal, PreviewState, PreviewView};
use sluice_core::{Clock, Sluice};
use sluice_seams::{PreviewPlace, Rev};

use crate::exec::Host;
use crate::sessions::SessionRun;
use crate::told::{self, Kind};
use crate::{Pause, CONFLICT, FAILED, LANDED, STOPPED};

pub struct Ask<'a> {
    pub branch: String,
    pub revision: String,
    /// Return once the command has ended, not once it has started.
    pub wait: bool,
    pub json: bool,
    pub host: &'a dyn Host,
    pub clock: &'a dyn Clock,
    pub pause: &'a dyn Pause,
}

/// Write human text: to stderr when stdout is the JSON document's.
fn say(out: &mut dyn Write, json: bool, text: &str) -> Result<()> {
    if json {
        writeln!(std::io::stderr(), "{text}")?;
    } else {
        writeln!(out, "{text}")?;
    }
    Ok(())
}

/// Say what was released because nobody could use it (101).
pub fn tell_dropped(out: &mut dyn Write, json: bool, dropped: &[Dropped]) -> Result<()> {
    for d in dropped {
        let text = match (&d.error, &d.failed) {
            (Some(e), _) if d.held => format!(
                "{name} is held: its release failed: {e}. `sluice release {name}` runs it again",
                name = d.name
            ),
            (Some(e), _) => format!("could not release {}: {e}", d.name),
            (None, Some(_)) => format!(
                "released {}: its command failed; `sluice previews` shows what it printed",
                d.name
            ),
            (None, None) => {
                let gone = d.gone.clone().unwrap_or_default();
                format!(
                    "freed {}: its checkout no longer exists; {}; its ports and names free",
                    d.name,
                    crate::exec::gone_release(gone.ran, gone.failed.as_deref())
                )
            }
        };
        say(out, json, &text)?;
    }
    Ok(())
}

/// What was released because nobody could use it, as a JSON document
/// lists it.
pub fn dropped_json(dropped: &[Dropped]) -> Vec<serde_json::Value> {
    dropped
        .iter()
        .map(|d| {
            let mut doc = serde_json::json!({
                "name": d.name,
                "branch": d.branch,
                "released": d.error.is_none(),
                "failed": d.failed.is_some(),
                "error": d.error,
                "held": d.held,
            });
            if let Some(g) = &d.gone {
                doc["gone"] = serde_json::json!({"ran": g.ran, "failed": g.failed});
            }
            doc
        })
        .collect()
}

/// One preview as `--json` prints it.
pub(crate) fn preview_json(v: &PreviewView) -> serde_json::Value {
    let p = &v.place;
    let mut doc = serde_json::json!({
        "name": p.name,
        "branch": p.branch,
        "revision": p.revision,
        "state": v.state.word(),
        "made": p.made,
        "checkout": p.path,
        "addresses": p.addresses,
        "names": p.names,
        "ports": p.ports,
    });
    if let PreviewState::Failed(output) = &v.state {
        doc["output"] = output.clone().into();
    }
    if let Some(f) = &p.release_failed {
        doc["release_failed"] = serde_json::json!(f);
    }
    doc
}

/// One preview as text: its name, revision and state, then each address.
fn write_preview(v: &PreviewView, out: &mut dyn Write) -> Result<()> {
    let p = &v.place;
    writeln!(
        out,
        "{} of {} at {}: {}",
        p.name,
        p.branch,
        p.revision.short(),
        v.state.word()
    )?;
    let width = p.addresses.keys().map(String::len).max().unwrap_or(0);
    for (label, address) in &p.addresses {
        writeln!(out, "  {label:width$}  {address}")?;
    }
    if !p.ports.is_empty() {
        let ports: Vec<String> = p.ports.iter().map(|(k, v)| format!("{k}={v}")).collect();
        writeln!(out, "  ports: {}", ports.join(" "))?;
    }
    if let PreviewState::Failed(output) = &v.state {
        for line in output.trim_end().lines() {
            writeln!(out, "  | {line}")?;
        }
    }
    // 123: kept until it is released by name, or its failure accepted.
    if let Some(f) = &p.release_failed {
        let tries = match f.tries {
            1 => "1 try".to_string(),
            n => format!("{n} tries"),
        };
        let held = if f.stopped { ", held" } else { "" };
        writeln!(
            out,
            "  its release failed ({tries}{held}): sluice release {} runs it again, with --accept-failure frees it without",
            p.name
        )?;
        for line in f.failed.trim_end().lines() {
            writeln!(out, "  | {line}")?;
        }
    }
    Ok(())
}

/// A preview's command no longer runs: it counts against nothing (102).
fn uncount(host: &dyn Host, s: &Sluice<'_>, p: &PreviewPlace) -> Result<PreviewState> {
    let state = s.preview_state(p)?;
    if !matches!(state, PreviewState::Starting | PreviewState::Waiting) {
        host.end_session(&p.path.to_string_lossy())?;
    }
    Ok(state)
}

impl Ask<'_> {
    pub fn run(&self, s: &Sluice<'_>, out: &mut dyn Write) -> Result<i32> {
        let seams = s.seams();
        let json = self.json;
        let Some(revision) = self.host.revision(&self.revision)? else {
            let text = format!("refused: no revision {}", self.revision);
            told::tell(out, json, Kind::Refused, &text, &[])?;
            return Ok(CONFLICT);
        };
        let revision = Rev::new(revision);
        tell_dropped(out, json, &s.release_unusable_previews()?)?;
        let plan = match s.preview_plan(&self.branch, &revision)? {
            Ok(p) => p,
            Err(r) => {
                let next = match r {
                    PreviewRefusal::NoTable => vec![],
                    _ => vec!["sluice overview".to_string()],
                };
                let why = match r {
                    PreviewRefusal::NoBranch => format!("no branch {}", self.branch),
                    PreviewRefusal::NotManaged => format!(
                        "no [branches] table in .config/sluice.toml on {} names it",
                        self.branch
                    ),
                    PreviewRefusal::NotLanded => format!(
                        "{} has not landed {}: only what a branch has landed is previewed",
                        self.branch,
                        revision.short()
                    ),
                    PreviewRefusal::NoTable => format!(
                        "no [preview] table in .config/sluice.toml at {}",
                        revision.short()
                    ),
                };
                told::tell(out, json, Kind::Refused, &format!("refused: {why}"), &next)?;
                return Ok(CONFLICT);
            }
        };

        // 100: a preview whose command has started is given back as it is;
        // one whose asker stopped while it waited is taken up.
        let existing = s
            .preview_of(&revision)?
            .filter(|p| p.run.is_some() || p.failed.is_some());
        if existing.is_none() {
            let (place, preview) = {
                let _lock = seams.state.wait_lock(&self.branch)?;
                s.make_preview(&plan)?
            };
            // 102, 71: counted from its start to its end, against every
            // repository's runs on the host, never in the reserve; a start
            // the check seam refuses at the bound waits as one the count
            // refused.
            let mut told_waiting = false;
            loop {
                let guard = seams.state.lock(&self.branch)?;
                let part = match guard {
                    Some(_) => seams.tests.hold(sluice_seams::RunKind::Session)?,
                    None => None,
                };
                if let Some(part) = part {
                    match s.start_preview(&plan, place.clone(), part) {
                        Ok(run) => {
                            let held = self.host.lock_sessions()?;
                            self.host.begin_session(
                                held.as_ref(),
                                &SessionRun {
                                    worktree: preview.path.to_string_lossy().into_owned(),
                                    place: preview.name.clone(),
                                    branch: self.branch.clone(),
                                    pid: std::process::id(),
                                    started: self
                                        .clock
                                        .now()
                                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                                    run_dir: self.host.run_dir(&run).unwrap_or_default(),
                                    command: None,
                                    tasks: Vec::new(),
                                },
                            )?;
                            break;
                        }
                        Err(e) if e.downcast_ref::<sluice_seams::AtBound>().is_some() => {}
                        Err(e) => return Err(e),
                    }
                }
                drop(guard);
                if !told_waiting {
                    say(out, json, "waiting: the host is at its bound")?;
                    told_waiting = true;
                }
                if !self.pause.pause() {
                    say(
                        out,
                        json,
                        &format!(
                            "stopped: {} waits; ask for it again to start it",
                            preview.name
                        ),
                    )?;
                    return Ok(STOPPED);
                }
            }
        }

        let mut view = self.view(s, &revision)?;
        while self.wait && view.state == PreviewState::Starting {
            if !self.pause.pause() {
                say(out, json, "stopped: the preview's command goes on")?;
                return Ok(STOPPED);
            }
            view = self.view(s, &revision)?;
        }
        let code = match (&view.state, self.wait) {
            (PreviewState::Failed(_), true) => FAILED,
            _ => LANDED,
        };
        if matches!(view.state, PreviewState::Failed(_)) {
            tell_dropped(out, json, &s.release_unusable_previews()?)?;
            view = self.view(s, &revision)?;
        }
        if json {
            let mut doc = preview_json(&view);
            doc["exit"] = code.into();
            writeln!(out, "{doc}")?;
        } else {
            write_preview(&view, out)?;
        }
        Ok(code)
    }

    fn view(&self, s: &Sluice<'_>, revision: &Rev) -> Result<PreviewView> {
        let place = s
            .preview_of(revision)?
            .ok_or_else(|| anyhow::anyhow!("the preview of {revision} was released meanwhile"))?;
        let state = uncount(self.host, s, &place)?;
        Ok(PreviewView { place, state })
    }
}

/// `sluice previews`: every preview on the host, with its state (101).
pub fn list(s: &Sluice<'_>, host: &dyn Host, json: bool, out: &mut dyn Write) -> Result<i32> {
    tell_dropped(out, json, &s.release_unusable_previews()?)?;
    let mut views = Vec::new();
    for place in s.seams().checkout.previews()? {
        let state = uncount(host, s, &place)?;
        views.push(PreviewView { place, state });
    }
    if json {
        let docs: Vec<serde_json::Value> = views.iter().map(preview_json).collect();
        writeln!(out, "{}", serde_json::to_string_pretty(&docs)?)?;
    } else if views.is_empty() {
        writeln!(out, "no previews")?;
    } else {
        for v in &views {
            write_preview(v, out)?;
        }
    }
    Ok(LANDED)
}

/// `sluice release <preview>`: release a preview at its caller's asking
/// (101; rule core.16). The name may be given whole, `preview/<rev>`, or
/// as the revision. A release that fails exits 1, saying what failed, and
/// leaves the preview held (123); with `accept`, it is freed with nothing
/// run.
pub fn release(
    s: &Sluice<'_>,
    host: &dyn Host,
    preview: &str,
    accept: bool,
    json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let name = if preview.starts_with(sluice_core::preview::PREVIEW) {
        preview.to_string()
    } else {
        match host.revision(preview)? {
            Some(rev) => Sluice::preview_name(&Rev::new(rev)),
            None => format!("{}{preview}", sluice_core::preview::PREVIEW),
        }
    };
    let place = s
        .seams()
        .checkout
        .previews()?
        .into_iter()
        .find(|p| p.name == name);
    let released = match accept {
        true => s.accept_preview(&name),
        false => s.release_preview(&name),
    };
    let (released, failed) = match released {
        Ok(r) => (r, None),
        Err(e) => (false, Some(format!("{e:#}"))),
    };
    if released {
        if let Some(p) = place {
            host.end_session(&p.path.to_string_lossy())?;
        }
    }
    let text = match (&failed, released) {
        (Some(e), _) => format!(
            "could not release {name}: {e}\n\
             `sluice release {name}` runs it again; `sluice release {name} --accept-failure` frees it without"
        ),
        (None, true) if accept => format!(
            "freed {name} without its release command: its command's run ended, its checkout removed, its ports and names free"
        ),
        (None, true) => format!(
            "released {name}: its command's run ended, the release command run, its ports and names free"
        ),
        (None, false) => "nothing to release".into(),
    };
    if json {
        if failed.is_some() {
            writeln!(std::io::stderr(), "{text}")?;
        }
        let mut doc = serde_json::json!({ "preview": name, "released": released });
        if accept {
            doc["accepted"] = released.into();
        }
        if let Some(f) = &failed {
            doc["failed"] = f.clone().into();
        }
        writeln!(out, "{doc}")?;
    } else {
        writeln!(out, "{text}")?;
    }
    Ok(if failed.is_some() { FAILED } else { LANDED })
}
