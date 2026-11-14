//! `sluice exec`: a session's own run of its checks, or of a command, in
//! its worktree (session-runs-1 clauses 89 to 93). It counts against
//! the host's bound, the worktree's ports and the branch's executor and
//! limits, and ends with its process group; a run of the checks in a
//! clean worktree is recorded under its keys.

use std::any::Any;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use sluice_core::overview::OrderFrom;
use sluice_core::release::{ReleaseOutcome, Releasing};
use sluice_core::session::{Abandoned, Recorded, SessionPlan, SessionRefusal, SessionStart};
use sluice_core::{Clock, Forecast, Sluice};
use sluice_seams::{
    AtBound, Outcome, Progress, Released, Rev, RunHandle, RunOutcome, RunReport, TaskProgress,
};

use crate::sessions::SessionRun;
use crate::told::{self, Kind};
use crate::words;
use crate::{Pause, Pushers, CONFLICT, FAILED, LANDED, STOPPED};

/// What `sluice exec` needs of the host beyond the seams: the reserve,
/// the host's records of its session runs and their lock, and a run's
/// directory. The count a run starts under is the check seam's (71).
pub trait Host {
    /// The part of the bound session runs never use (91).
    fn reserved(&self) -> usize;

    /// 135 (design D2, D14): sluice's host process, as this command asks
    /// it: through its socket, or, in the host process's own tasks and the
    /// tests' host, in place.
    fn host_process(&self) -> &dyn crate::host_process::HostProcess;
    /// 72 to 74: this host among the hosts of a branch whose records are
    /// on its remote: its name and the lease it waits out.
    fn peer(&self) -> sluice_core::Peer {
        sluice_core::Peer::default()
    }
    /// `dir` as the worktree's canonical path, the key of its session runs.
    fn worktree(&self, dir: &str) -> Result<String>;
    /// 147: the top of the git worktree that holds `dir`, canonical as
    /// [`Host::worktree`] gives a path, where the repository keeps what
    /// equips it; `None` where `dir` is in no worktree.
    fn working_copy(&self, dir: &str) -> Result<Option<String>> {
        self.worktree(dir).map(Some)
    }
    /// The host's lock over its session runs' records, held until dropped.
    fn lock_sessions(&self) -> Result<Box<dyn Any>>;
    /// Record `run` as running, under the lock `held`.
    fn begin_session(&self, held: &dyn Any, run: &SessionRun) -> Result<()>;
    /// The worktree's session run has ended.
    fn end_session(&self, worktree: &str) -> Result<()>;
    /// Keep `tasks` on the worktree's running session run (ADR 0035).
    fn session_tasks(&self, worktree: &str, tasks: &[TaskProgress]) -> Result<()>;
    /// Where a run's output and result are kept, when on this host.
    fn run_dir(&self, run: &RunHandle) -> Option<PathBuf>;
    /// End a run's process group now.
    fn end_run(&self, run: &RunHandle) -> Result<()>;
    /// `rev` as the repository's full revision, or `None` when it names
    /// none.
    fn revision(&self, rev: &str) -> Result<Option<String>>;
    /// The host's name, as its runs are recorded under.
    fn name(&self) -> String;
    /// The repository, as the path of its common git directory.
    fn repository(&self) -> Result<String>;
    /// Whether `repository`, a common git directory as [`Host::repository`]
    /// names it, is gone from the host: what ends a watch of it (129).
    fn repository_gone(&self, repository: &str) -> bool;
    /// Every branch tip the host holds, its own and each remote's tracking
    /// branch as last fetched, each once, with nothing fetched and no
    /// process started: what whether the repository has sluice rests on
    /// (129; rule core.27).
    fn tips(&self) -> Result<Vec<Rev>>;
    /// Whether any of `tips`, as [`Host::tips`] read them, has the policy
    /// file, with no process started: where none has and no branch has
    /// records, the repository has no sluice (129; rule core.27).
    fn holds_policy(&self, tips: &[Rev]) -> Result<bool>;
    /// 129: start a pass of `sluice run`, a watch tick or a wait's read:
    /// what the host's profiles keep by revision and the last did not use
    /// is forgotten.
    fn next_pass(&self) {}
    /// Where each repository's reader of its queues keeps its lock and the
    /// overview it last read (129; ADR 0056).
    fn readers(&self) -> Result<PathBuf>;
    /// The version of sluice this process is, which a reader names (129).
    fn version(&self) -> &str {
        crate::VERSION
    }
    /// The repository's default branch, its HEAD, with its revision here:
    /// whose policy orders the overview (ADR 0036). `None` when HEAD names
    /// no branch.
    fn default_branch(&self) -> Result<Option<OrderFrom>>;
}

pub struct Exec<'a> {
    pub branch: String,
    pub worktree: String,
    /// The command after `--`; empty runs the checks.
    pub command: Vec<String>,
    pub json: bool,
    /// Release the worktree once the run ends (95).
    pub release: bool,
    pub host: &'a dyn Host,
    /// Where each branch lives, to say so when it cannot be read.
    pub pushers: &'a dyn Pushers,
    pub clock: &'a dyn Clock,
    pub pause: &'a dyn Pause,
}

/// The next step after a refusal over the branch: see which there are.
fn overview() -> [String; 1] {
    ["sluice overview".to_string()]
}

/// Write human text: to stderr when stdout is the JSON document's.
fn say(out: &mut dyn Write, json: bool, text: &str) -> Result<()> {
    if json {
        let mut e = std::io::stderr();
        writeln!(e, "{text}")?;
    } else {
        writeln!(out, "{text}")?;
    }
    Ok(())
}

/// 123: how the release of a place whose working copy was gone went, as
/// a line says it.
pub fn gone_release(ran: bool, failed: Option<&str>) -> String {
    match (ran, failed) {
        (_, Some(f)) => format!("its release failed once from its scratch directory: {f}"),
        (true, None) => "its release ran once from its scratch directory".into(),
        (false, None) => "it had no release to run".into(),
    }
}

/// Say what a round or a run freed because its worktree no longer exists,
/// and how its stored release went (session-runs-1 clause 97; 123).
pub fn tell_abandoned(out: &mut dyn Write, json: bool, released: &[Abandoned]) -> Result<()> {
    for a in released {
        let text = match &a.error {
            Some(e) => format!(
                "could not free {}, whose worktree no longer exists: {e}",
                a.worktree
            ),
            None => format!(
                "freed {}: it no longer exists; {}; its runs ended, its ports free",
                a.worktree,
                gone_release(a.ran, a.failed.as_deref())
            ),
        };
        say(out, json, &text)?;
    }
    Ok(())
}

/// What a round or a run freed because its worktree no longer exists, as
/// a JSON document lists it.
pub fn abandoned_json(released: &[Abandoned]) -> Vec<serde_json::Value> {
    released
        .iter()
        .map(|a| {
            serde_json::json!({
                "worktree": a.worktree,
                "branch": a.branch,
                "freed": a.freed,
                "ran": a.ran,
                "failed": a.failed,
                "error": a.error,
            })
        })
        .collect()
}

/// 123: say what a round did with each combination's checkout whose
/// release failed, was held, or whose working copy was gone. A held
/// checkout is said once, on the round that held it; the rounds after try
/// it no more and say nothing of it.
pub fn tell_releases(out: &mut dyn Write, json: bool, releases: &[ReleaseOutcome]) -> Result<()> {
    for r in releases {
        let name = &r.place;
        let text = match &r.how {
            Releasing::Released | Releasing::Accepted(_) => continue,
            Releasing::Gone { ran, failed } => format!(
                "freed {name}: it no longer exists; {}; its runs ended, its ports free",
                gone_release(*ran, failed.as_deref())
            ),
            Releasing::Failed { failed, tries, of } => {
                format!("could not release {name} (try {tries} of {of}): {failed}")
            }
            Releasing::Held { failed, tries } => format!(
                "{name} is held: its release failed {tries} times: {failed}. \
                 `sluice release {name}` runs it again; `sluice release {name} --accept-failure` frees it without"
            ),
        };
        say(out, json, &text)?;
    }
    Ok(())
}

/// What a round did with the checkouts whose release failed, was held or
/// found gone, as a JSON document lists it.
pub fn releases_json(releases: &[ReleaseOutcome]) -> Vec<serde_json::Value> {
    releases
        .iter()
        .filter_map(|r| {
            let (how, failed, tries) = match &r.how {
                Releasing::Released | Releasing::Accepted(_) => return None,
                Releasing::Gone { failed, .. } => ("gone", failed.as_ref(), None),
                Releasing::Failed { failed, tries, .. } => ("failed", Some(failed), Some(tries)),
                Releasing::Held { failed, tries } => ("held", Some(failed), Some(tries)),
            };
            let mut doc = serde_json::json!({
                "place": r.place,
                "branch": r.branch,
                "release": how,
                "failed": failed,
                "tries": tries,
            });
            if let Releasing::Gone { ran, .. } = &r.how {
                doc["ran"] = (*ran).into();
            }
            Some(doc)
        })
        .collect()
}

/// What `sluice release` did: the worktree, whether it held anything to
/// release, whether its failure was accepted, and what failed when its
/// release did.
pub struct Release {
    pub worktree: String,
    pub released: bool,
    pub accepted: bool,
    pub failed: Option<String>,
}

/// `argv` as one `sh -c` command line, each word quoted where it needs it.
pub fn shell_line(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            if !a.is_empty()
                && a.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,".contains(c))
            {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', r"'\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Exec<'_> {
    pub fn run(&self, s: &Sluice<'_>, out: &mut dyn Write) -> Result<i32> {
        let code = self.exec(s, out)?;
        if self.release && code != CONFLICT {
            release(s, self.host, &self.worktree, false, self.json, out)?;
        }
        Ok(code)
    }

    fn exec(&self, s: &Sluice<'_>, out: &mut dyn Write) -> Result<i32> {
        let seams = s.seams();
        let json = self.json;
        let worktree = match self.host.worktree(&self.worktree) {
            Ok(w) => w,
            Err(e) => {
                let text = format!("refused: {} is not a worktree: {e:#}", self.worktree);
                told::tell(out, json, Kind::Refused, &text, &[])?;
                return Ok(CONFLICT);
            }
        };
        let session = match seams.driver.session(&worktree) {
            Ok(x) => x,
            Err(e) => {
                let text = format!("refused: {} is not a worktree: {e:#}", self.worktree);
                told::tell(out, json, Kind::Refused, &text, &[])?;
                return Ok(CONFLICT);
            }
        };
        let plan: SessionPlan = match s.session_plan(&self.branch)? {
            Ok(p) => p,
            Err(SessionRefusal::NoBranch) => {
                let text = crate::no_branch(&self.branch, self.pushers)?;
                told::tell(out, json, Kind::Refused, &text, &overview())?;
                return Ok(CONFLICT);
            }
            Err(SessionRefusal::NotManaged) => {
                let text = format!(
                    "refused: no [branches] table in .config/sluice.toml on {} names it",
                    self.branch
                );
                told::tell(out, json, Kind::Refused, &text, &overview())?;
                return Ok(CONFLICT);
            }
        };
        // 84, record 0074 (design D6): the branch's tests on a host that
        // lacks what they ask of it are refused, nothing run or recorded;
        // a command after `--` is the session's own.
        if self.command.is_empty() {
            if let Some(why) = s.session_unmet(&plan)? {
                let text = format!(
                    "refused: {why}; {}'s tests \
                     ask for one ([executor] docker = true)",
                    self.branch
                );
                told::tell(out, json, Kind::Refused, &text, &[])?;
                return Ok(CONFLICT);
            }
        }
        // 97: a session's place whose worktree is gone is released by the
        // next run on the host, before this one counts.
        tell_abandoned(out, json, &s.release_abandoned()?)?;
        crate::preview::tell_dropped(out, json, &s.release_unusable_previews()?)?;
        // 64: told before it runs whether its head would form if submitted
        // now. It decides nothing, so one that cannot be formed is passed
        // over, and the run goes on either way.
        let forecast = s.session_forecast(&plan, &session).ok().flatten();
        if let Some(f) = &forecast {
            if matches!(f.forecast, Forecast::Conflict { .. }) {
                say(out, json, &crate::forecast_text(&f.forecast, &self.branch))?;
            }
        }
        let forecast = forecast.as_ref().map(crate::forecast_json);
        let command = (!self.command.is_empty()).then(|| shell_line(&self.command));
        match &command {
            None => say(
                out,
                json,
                &format!(
                    "running the checks {} affects against {} ({})",
                    session.head.short(),
                    self.branch,
                    plan.base.short()
                ),
            )?,
            Some(c) => say(out, json, &format!("running {c}"))?,
        }

        // 89, 91, 71, 150 (rule core.49; design D12): its room asked of
        // the host process, which holds the host's bound for every
        // repository, waiting in its order while it says why; a start
        // refused at the bound asks again. Ctrl-C gives the room or the
        // wait back.
        let here = crate::host_process::Here {
            s,
            host: self.host,
            pulls: &(),
            clock: self.clock,
        };
        let mut told = crate::host_process::room::Told::default();
        let (place, run) = 'start: loop {
            let mut holding =
                self.host
                    .host_process()
                    .hold(&here, sluice_seams::RunKind::Session, &worktree)?;
            let part = loop {
                match holding.next(std::time::Duration::from_millis(100))? {
                    Some(crate::host_process::Holds::Held(part)) => break part,
                    Some(crate::host_process::Holds::Waiting(why)) => {
                        if let Some(words) = told.first(why) {
                            say(out, json, words)?;
                        }
                    }
                    None => {}
                }
                if !self.pause.pause() {
                    say(out, json, "stopped: nothing ran")?;
                    return Ok(STOPPED);
                }
            };
            drop(holding);
            let _guard = seams.state.wait_lock(&self.branch)?;
            match s.start_session(&plan, &worktree, &session.head, command.as_deref(), part) {
                Ok(started) => {
                    let (place, run) = (started.place, started.run);
                    let held = self.host.lock_sessions()?;
                    self.host.begin_session(
                        held.as_ref(),
                        &SessionRun {
                            worktree: worktree.clone(),
                            place: place.name.clone(),
                            branch: self.branch.clone(),
                            pid: std::process::id(),
                            started: self
                                .clock
                                .now()
                                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                            run_dir: self.host.run_dir(&run).unwrap_or_default(),
                            command: command.clone(),
                            tasks: Vec::new(),
                        },
                    )?;
                    break 'start (place, run);
                }
                Err(e) if e.downcast_ref::<AtBound>().is_some() => {
                    if let Some(words) = told.first(crate::host_process::room::Why::AtBound) {
                        say(out, json, words)?;
                    }
                    if !self.pause.pause() {
                        say(out, json, "stopped: nothing ran")?;
                        return Ok(STOPPED);
                    }
                }
                Err(e) => return Err(e),
            }
        };
        let ports: Vec<(&String, &String)> = place
            .env
            .iter()
            .filter(|(k, _)| k.starts_with("SLUICE_PORT_"))
            .collect();
        if !ports.is_empty() {
            let p: Vec<String> = ports.iter().map(|(k, v)| format!("{k}={v}")).collect();
            say(out, json, &format!("ports: {}", p.join(" ")))?;
        }

        // The run's output, as it comes.
        let log = self.host.run_dir(&run).map(|d| d.join("out.log"));
        let mut shown = 0usize;
        let mut show = |out: &mut dyn Write| -> Result<()> {
            let Some(bytes) = log.as_ref().and_then(|l| std::fs::read(l).ok()) else {
                return Ok(());
            };
            if bytes.len() > shown {
                let text = String::from_utf8_lossy(&bytes[shown..]).into_owned();
                shown = bytes.len();
                if json {
                    std::io::stderr().write_all(text.as_bytes())?;
                } else {
                    out.write_all(text.as_bytes())?;
                }
            }
            Ok(())
        };
        let mut kept: Vec<TaskProgress> = Vec::new();
        let progress = loop {
            let p = seams.tests.progress(&run)?;
            show(out)?;
            if !matches!(p, Progress::Running) {
                break p;
            }
            // ADR 0035: where its tasks have got, kept on its record for a
            // person to read. They decide nothing, so one that cannot be read
            // or kept is passed over.
            if let Ok(tasks) = seams.tests.tasks(&run) {
                if tasks != kept && self.host.session_tasks(&worktree, &tasks).is_ok() {
                    kept = tasks;
                }
            }
            if !self.pause.pause() {
                self.host.end_run(&run)?;
                self.host.end_session(&worktree)?;
                unkept(s.session_ended(
                    &place.name,
                    &session.head,
                    command.is_some(),
                    RunOutcome::Stopped,
                ));
                say(out, json, "stopped: the run was ended; nothing recorded")?;
                return Ok(STOPPED);
            }
        };
        self.host.end_session(&worktree)?;
        let report = match progress {
            Progress::Finished(r) => r,
            _ => {
                unkept(s.session_ended(
                    &place.name,
                    &session.head,
                    command.is_some(),
                    RunOutcome::Lost,
                ));
                say(
                    out,
                    json,
                    "lost: the run's supervisor stopped before it reported",
                )?;
                self.finish(
                    out,
                    None,
                    &[],
                    false,
                    None,
                    &place.env,
                    forecast.as_ref(),
                    FAILED,
                )?;
                return Ok(FAILED);
            }
        };

        // 92: recorded only when the worktree holds exactly its head, and
        // only a run of the checks; never a run that hit its time limit.
        // Design D5: told as the core read it, a check that did not pass
        // after which the host lost what the run asked of it not run.
        let started = SessionStart {
            place: place.clone(),
            run: run.clone(),
        };
        let finished = s.finish_session(&plan, &session, &started, command.is_some(), &report)?;
        let report = finished.report;
        self.summary(out, &report, command.is_some())?;
        unkept(finished.unkept.map_or(Ok(()), |e| Err(anyhow::anyhow!(e))));
        let recorded = match finished.recorded {
            Recorded::Written(n) => {
                let head = session.head.short();
                let text = match n {
                    0 => format!("recorded already under their keys for {head}"),
                    1 => format!("recorded 1 result under its key for {head}"),
                    n => format!("recorded {n} results under their keys for {head}"),
                };
                say(out, json, &text)?;
                true
            }
            Recorded::NotClean => {
                say(out, json, "not recorded: worktree is not clean")?;
                false
            }
            Recorded::Changed { moved_to, paths } => {
                say(
                    out,
                    json,
                    &changed_text(&session.head, moved_to.as_ref(), &paths),
                )?;
                false
            }
            Recorded::Nothing => false,
        };
        let code = match &command {
            Some(_) => self
                .host
                .run_dir(&run)
                .and_then(|d| crate::runs::ended(&d).ok().flatten())
                .and_then(|e| (!e.timed_out).then_some(e.exit).flatten())
                .unwrap_or(if passed(&report) { LANDED } else { FAILED }),
            None if passed(&report) => LANDED,
            None => FAILED,
        };
        let missing = report.missing.clone();
        self.finish(
            out,
            Some(&report),
            &missing,
            recorded,
            Some(report.timed_out),
            &place.env,
            forecast.as_ref(),
            code,
        )?;
        Ok(code)
    }

    fn summary(&self, out: &mut dyn Write, report: &RunReport, command: bool) -> Result<()> {
        let json = self.json;
        if report.timed_out {
            return say(out, json, "timed out");
        }
        // Could-not-run-1 114: no outcome, so nothing of it is recorded.
        for n in &report.not_run {
            say(
                out,
                json,
                &format!("could not run: {}: {}", n.task, n.reason),
            )?;
        }
        if command {
            return Ok(());
        }
        // 149: a task a check was failed by failed as a check does, and
        // the check it failed is named after it, never as not run.
        let failed: Vec<&str> = report
            .tasks
            .iter()
            .chain(&report.dependencies)
            .filter(|t| t.outcome == Outcome::Fail)
            .map(|t| t.task.as_str())
            .collect();
        if !failed.is_empty() {
            say(out, json, &format!("failed: {}", failed.join(", ")))?;
        }
        for f in &report.failed_by {
            say(
                out,
                json,
                &format!("  {}", words::failed_by(&f.task, &f.by)),
            )?;
        }
        if !report.missing.is_empty() {
            say(
                out,
                json,
                &format!("not run: {}", report.missing.join(", ")),
            )?;
        }
        if passed(report) {
            let n = report.tasks.len();
            let tasks = if n == 1 { "task" } else { "tasks" };
            say(out, json, &format!("passed: {n} {tasks}"))?;
        }
        if !report.not_run.is_empty() {
            say(
                out,
                json,
                "nothing was recorded for a test that could not run: once the host can run it, run again",
            )?;
        }
        // 139, design D8: the run was read as refused nothing.
        if let Some(why) = &report.refusals_unread {
            say(out, json, &words::refusals_unread(why))?;
        }
        Ok(())
    }

    /// `--json`: the one document on stdout.
    #[allow(clippy::too_many_arguments)]
    fn finish(
        &self,
        out: &mut dyn Write,
        report: Option<&RunReport>,
        missing: &[String],
        recorded: bool,
        timed_out: Option<bool>,
        env: &std::collections::BTreeMap<String, String>,
        forecast: Option<&serde_json::Value>,
        code: i32,
    ) -> Result<()> {
        if !self.json {
            return Ok(());
        }
        let results = |of: &[sluice_seams::TaskResult]| -> Vec<serde_json::Value> {
            of.iter()
                .map(|t| {
                    serde_json::json!({
                        "task": t.task,
                        "hash": t.hash,
                        "outcome": t.outcome,
                        "recorded": recorded && matches!(t.source, sluice_seams::Source::Ran { .. }),
                    })
                })
                .collect()
        };
        let tasks = results(report.map(|r| r.tasks.as_slice()).unwrap_or_default());
        // 149: each task a check was failed by, as `tasks` has a check, and
        // each check so failed; neither is in `missing`.
        let dependencies = results(
            report
                .map(|r| r.dependencies.as_slice())
                .unwrap_or_default(),
        );
        let ports: serde_json::Map<String, serde_json::Value> = env
            .iter()
            .filter(|(k, _)| k.starts_with("SLUICE_PORT_"))
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        writeln!(
            out,
            "{}",
            serde_json::json!({
                "branch": self.branch,
                "tasks": tasks,
                "dependencies": dependencies,
                "failed_by": report.map(|r| r.failed_by.as_slice()).unwrap_or_default(),
                "missing": missing,
                "not_run": report.map(|r| r.not_run.as_slice()).unwrap_or_default(),
                "timed_out": timed_out.unwrap_or(false),
                "ports": ports,
                "forecast": forecast,
                "exit": code,
            })
        )?;
        Ok(())
    }
}

/// `sluice release`: release the session's place of the worktree `dir`,
/// at its session's or driver's asking (session-runs-1 clause 95; rule
/// core.16). Where sluice holds a place for `dir`, the release command is
/// the policy's, read from its branch's current revision (56); only when
/// the branch cannot be read is it the one the last run carried, and the
/// output says so. A release that fails leaves the place held with what
/// failed (123); with `accept`, the place is freed with nothing run and the
/// worktree left.
///
/// Where sluice holds nothing for it, the worktree released is the top of
/// the git worktree that holds `dir`, by the same path: the release
/// command is the one the repository's default branch's policy names at
/// its current revision, run with no ports, and nothing is kept for the
/// worktree once it answers, so `sluice release` runs it again when asked
/// again (95; rule core.10; record 0120). It says `nothing to release`
/// only where that policy names no release command, or where `dir` is in
/// no worktree of the repository; a default branch or a policy that
/// cannot be read fails it.
pub fn release(
    s: &Sluice<'_>,
    host: &dyn Host,
    dir: &str,
    accept: bool,
    json: bool,
    out: &mut dyn Write,
) -> Result<Release> {
    let seams = s.seams();
    let nothing = |out: &mut dyn Write, worktree: &str| -> Result<Release> {
        say(out, json, "nothing to release")?;
        Ok(Release {
            worktree: worktree.to_string(),
            released: false,
            accepted: false,
            failed: None,
        })
    };
    // A folder that has gone has nothing to release (record 0120).
    let Ok(named) = host.worktree(dir) else {
        return nothing(out, dir);
    };
    let (worktree, adopted) = match seams.checkout.adopted(&named)? {
        Some(a) => (named, Some(a)),
        None => match host.working_copy(dir)? {
            None => return nothing(out, dir),
            Some(top) => {
                let adopted = seams.checkout.adopted(&top)?;
                (top, adopted)
            }
        },
    };
    let failed = |out: &mut dyn Write, worktree: String, failed: String| -> Result<Release> {
        say(
            out,
            json,
            &format!("could not release {worktree}: {failed}"),
        )?;
        Ok(Release {
            worktree,
            released: false,
            accepted: false,
            failed: Some(failed),
        })
    };
    let (name, policy) = match &adopted {
        Some(adopted) => {
            if accept {
                let freed = s.accept_session(&worktree)?;
                host.end_session(&worktree)?;
                if freed {
                    say(
                        out,
                        json,
                        &format!(
                            "freed {worktree} without its release command: its runs ended, its ports free; the worktree stays"
                        ),
                    )?;
                }
                return Ok(Release {
                    worktree,
                    released: freed,
                    accepted: freed,
                    failed: None,
                });
            }
            let branch = &adopted.branch;
            let policy = match s.release_policy(branch) {
                Ok(p) => Some(p),
                Err(e) => {
                    say(
                        out,
                        json,
                        &format!(
                            "{branch} cannot be read ({e:#}): running the release command saved with the last run"
                        ),
                    )?;
                    None
                }
            };
            (adopted.name.clone(), policy)
        }
        // `--accept-failure` frees a hold, and sluice holds nothing here.
        None if accept => return nothing(out, &worktree),
        None => {
            // Record 0120: sluice holds nothing that names the branch the
            // worktree was taken off, so the default branch's policy
            // releases it, read at its current revision (56).
            let Some(default) = host.default_branch()? else {
                return failed(
                    out,
                    worktree,
                    "the repository's HEAD names no branch whose policy names its release command"
                        .into(),
                );
            };
            let branch = default.branch;
            match s.release_policy(&branch) {
                Ok(p) => (format!("{}{worktree}", crate::runs::SESSION), Some(p)),
                Err(e) => {
                    return failed(
                        out,
                        worktree,
                        format!(
                            "{branch} cannot be read ({e:#}), so its release command is not known"
                        ),
                    )
                }
            }
        }
    };
    let released = match seams.checkout.release_session(&name, policy.as_ref()) {
        Ok(r) => r,
        Err(e) => {
            let words = format!("{e:#}");
            let then = match adopted {
                Some(_) => "it keeps its ports until `sluice release` runs again, or `sluice release --accept-failure` frees it without",
                None => "sluice holds nothing for it: `sluice release` runs its release command again",
            };
            say(
                out,
                json,
                &format!("could not release {worktree}: {words}\n{then}"),
            )?;
            return Ok(Release {
                worktree,
                released: false,
                accepted: false,
                failed: Some(words),
            });
        }
    };
    let nothing_run = matches!(released, Released::Nothing);
    if adopted.is_some() || !nothing_run {
        host.end_session(&worktree)?;
    }
    if nothing_run {
        return nothing(out, &worktree);
    }
    say(
        out,
        json,
        &format!("released {worktree}: its runs ended, the release command run, its ports free"),
    )?;
    Ok(Release {
        worktree,
        released: true,
        accepted: false,
        failed: None,
    })
}

/// Why a run in a worktree that changed while it ran was not recorded
/// (92, rule core.18): the commit it moved to, and up to five of the paths
/// beyond its head, which a check may have written itself.
pub fn changed_text(
    head: &sluice_seams::Rev,
    moved_to: Option<&sluice_seams::Rev>,
    paths: &[String],
) -> String {
    let mut parts = Vec::new();
    if let Some(to) = moved_to {
        parts.push(format!(
            "the worktree moved from {} to {} while the run ran",
            head.short(),
            to.short()
        ));
    }
    if !paths.is_empty() {
        let mut named = paths
            .iter()
            .take(5)
            .map(|p| path_of(p))
            .collect::<Vec<_>>()
            .join(", ");
        if paths.len() > 5 {
            named.push_str(&format!(" and {} more", paths.len() - 5));
        }
        parts.push(format!(
            "after the run of {} the worktree holds {named} beyond its head; \
             these may be the checks' own output, and if so, list them in .gitignore",
            head.short()
        ));
    }
    if parts.is_empty() {
        parts.push(format!(
            "the worktree could not be read after the run of {}",
            head.short()
        ));
    }
    format!("not recorded: {}", parts.join("; "))
}

/// A path of what a worktree holds beyond its head, without the status
/// `git status --porcelain` puts before it.
fn path_of(line: &str) -> &str {
    let b = line.as_bytes();
    if b.len() > 3 && b[2] == b' ' && b[..2].iter().all(|c| b" MTADRCU?!".contains(c)) {
        &line[3..]
    } else {
        line
    }
}

/// 97: a result that could not be kept on the worktree is said on stderr,
/// and changes neither the run's exit nor what it printed.
fn unkept(kept: Result<()>) {
    if let Err(e) = kept {
        eprintln!("warning: the run's result could not be kept on the worktree: {e:#}");
    }
}

/// Every check the run named reported and passed, within its time limit;
/// a check failed by a task it depends on is no pass (15, 114, 149).
fn passed(report: &RunReport) -> bool {
    !report.timed_out && report.not_run.is_empty() && !report.failed()
}

#[cfg(test)]
mod tests {
    use super::changed_text;
    use sluice_seams::Rev;

    /// 92: a run not recorded because its worktree changed names what
    /// changed, up to five paths, and says a check may have written them.
    #[test]
    fn a_changed_worktree_is_named_without_blame() {
        let head = Rev("1a2b3c4d5e".into());
        let paths: Vec<String> = (1..=7).map(|n| format!("out/{n}.log")).collect();
        assert_eq!(
            changed_text(&head, None, &paths),
            "not recorded: after the run of 1a2b3c4 the worktree holds out/1.log, out/2.log, \
             out/3.log, out/4.log, out/5.log and 2 more beyond its head; these may be the \
             checks' own output, and if so, list them in .gitignore"
        );
        assert_eq!(
            changed_text(&head, None, &["?? out/1.log".to_string()]),
            changed_text(&head, None, &paths[..1]),
            "the status git puts before a path is left out"
        );
        assert_eq!(
            changed_text(&head, None, &paths[..1]),
            "not recorded: after the run of 1a2b3c4 the worktree holds out/1.log beyond its \
             head; these may be the checks' own output, and if so, list them in .gitignore"
        );
        assert_eq!(
            changed_text(&head, Some(&Rev("9f8e7d6c5b".into())), &[]),
            "not recorded: the worktree moved from 1a2b3c4 to 9f8e7d6 while the run ran"
        );
        assert_eq!(
            changed_text(&head, None, &[]),
            "not recorded: the worktree could not be read after the run of 1a2b3c4"
        );
    }
}
