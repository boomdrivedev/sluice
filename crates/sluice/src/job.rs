//! `sluice job`, a pool's job as `sluice.yml` runs it (record 0119; design
//! D11 of the change `a-runs-life-is-written-first-and-actions-runs-its-
//! checks`): `run`, in the run job, starts the run its claim holds, or a
//! pull request's head, and hands what it reported and what proves its
//! keys to the sign job in a directory, recording nothing, writing no
//! state and holding no key; `sign`, in the sign job, keeps of that report
//! only the keys the combination's tree bears out, running nothing of it,
//! and writes the subjects and the predicate `actions/attest` signs.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use anyhow::{bail, Context, Result};
use sluice_core::{Clock, Seams, Sluice, Started};
use sluice_seams::{
    Carried, Changes, Guard, Progress, ReadRefused, ResultAsk, Rev, RunHandle, RunReport, Snapshot,
    State, TaskLog, TaskProgress, Written,
};

use crate::{JobCommand, Pause, INTERNAL, STOPPED};

/// The file of a run job's directory holding its run's report, as JSON.
pub const REPORT: &str = "report.json";

/// The directory of a run job's directory holding what proves its keys, in
/// the check profile's own form.
pub const PROOF: &str = "proof";

/// The file of a run job's directory saying why it holds no report.
pub const WHY: &str = "why";

/// The files of a sign job's directory `actions/attest` reads: the
/// subjects, as `sha256sum` writes them, the predicate, and its type.
pub const SUBJECTS: &str = "subjects";
pub const PREDICATE: &str = "predicate.json";
pub const PREDICATE_TYPE: &str = "predicate-type";

/// One `sluice job` command.
pub fn run(
    command: JobCommand,
    seams: Seams<'_>,
    host: &dyn crate::exec::Host,
    clock: &dyn Clock,
    pause: &dyn Pause,
    out: &mut dyn Write,
) -> Result<i32> {
    match command {
        JobCommand::Run {
            claim,
            head,
            branch,
            system,
            out: dir,
        } => {
            let unwritten = Unwritten::over(seams.state);
            let tests = seams.tests;
            let s = Sluice::new(
                Seams {
                    state: &unwritten,
                    ..seams
                },
                clock,
            )
            .as_peer(host.peer());
            let started = match (&claim, &head) {
                (Some(claim), None) => {
                    let mut releases = Vec::new();
                    s.start_claimed(&branch, claim, &mut releases)?
                        .ok_or_else(|| {
                            format!("no run of {branch} waits for its job under {claim}")
                        })
                }
                (None, Some(head)) => {
                    Ok(s.start_head(&branch, &Rev::new(head), system.as_deref())?)
                }
                _ => bail!("sluice job run: give --claim or --head, not both"),
            };
            let run = match started {
                Ok(Started::Running(run)) => run,
                Ok(Started::AtBound) => {
                    return no_report(&dir, "the runner had no room for the run", out)
                }
                Ok(Started::NotRun(why) | Started::Unmade(why)) => {
                    return no_report(&dir, &why, out)
                }
                Err(why) => return no_report(&dir, &why, out),
            };
            loop {
                match tests.progress(&run)? {
                    Progress::Running => {
                        if !pause.pause() {
                            no_report(&dir, "the job was stopped before its run ended", out)?;
                            return Ok(STOPPED);
                        }
                    }
                    Progress::Lost => {
                        return no_report(
                            &dir,
                            "the run's executor stopped before it reported",
                            out,
                        )
                    }
                    Progress::Finished(report) => {
                        return hand_over(&dir, tests, &run, &report, out);
                    }
                }
            }
        }
        JobCommand::Sign {
            claim,
            head,
            branch,
            system,
            from,
            out: dir,
        } => {
            let unwritten = Unwritten::over(seams.state);
            let s = Sluice::new(
                Seams {
                    state: &unwritten,
                    ..seams
                },
                clock,
            )
            .as_peer(host.peer());
            let target = match (&claim, &head) {
                (Some(claim), None) => claimed(&s, &branch, claim)?,
                (None, Some(head)) => Signed {
                    base: base_of(&s, &branch)?,
                    revision: Rev::new(head),
                    system: system.clone(),
                    claim: None,
                },
                _ => bail!("sluice job sign: give --claim or --head, not both"),
            };
            sign(&s, &branch, &target, &from, &dir, clock, out)
        }
    }
}

/// What a sign job signs for: the combination's revision, or a head's,
/// the branch's revision whose policy it reads, the run's system and its
/// claim.
struct Signed {
    base: Rev,
    revision: Rev,
    system: Option<String>,
    claim: Option<String>,
}

/// The revision `branch`'s records stand on, whose gate a run is held to
/// (rule core.15).
fn base_of(s: &Sluice<'_>, branch: &str) -> Result<Rev> {
    let snap = s.seams().state.read(branch)?;
    let doc = sluice_core::line::Line::load(branch, snap)?.doc;
    doc.base
        .ok_or_else(|| anyhow::anyhow!("{branch}'s records hold no revision yet"))
}

/// The run `claim` holds on `branch`, read from its records, the
/// combination fetched by its own ref (82): never what the run job says
/// of it.
fn claimed(s: &Sluice<'_>, branch: &str, claim: &str) -> Result<Signed> {
    let snap = s.seams().state.read(branch)?;
    let doc = sluice_core::line::Line::load(branch, snap)?.doc;
    let base = doc
        .base
        .clone()
        .ok_or_else(|| anyhow::anyhow!("{branch}'s records hold no revision yet"))?;
    let (c, r) = doc
        .candidates
        .iter()
        .find_map(|c| {
            c.runs
                .iter()
                .find(|r| r.holder() == Some(claim) && r.state.held())
                .map(|r| (c, r))
        })
        .ok_or_else(|| anyhow::anyhow!("no run of {branch} is held under {claim}"))?;
    s.seams().state.fetch_candidate(branch, &c.id)?;
    Ok(Signed {
        base,
        revision: c.revision.clone(),
        system: r.system.clone(),
        claim: Some(claim.to_string()),
    })
}

/// The run job's directory under `from`: `from` itself where it holds the
/// report or says why it holds none, else its one directory that does, as
/// a download of one artifact by pattern leaves it.
fn handed_dir(from: &Path) -> std::path::PathBuf {
    let holds = |d: &Path| d.join(REPORT).exists() || d.join(WHY).exists();
    if holds(from) {
        return from.to_path_buf();
    }
    let inner: Vec<_> = std::fs::read_dir(from)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && holds(p))
        .collect();
    match &inner[..] {
        [one] => one.clone(),
        _ => from.to_path_buf(),
    }
}

/// Record 0119 (design D11; 78, 79, 109): the keys of the run job's report
/// the tree of `target.revision` bears out, by the proof the job kept,
/// with nothing of the combination run, written as the subjects and the
/// run's predicate for `actions/attest`; a report that is missing, or
/// whose every key is refused, writes no subjects, and the record that
/// follows ends the run with no outcome or with what was signed.
fn sign(
    s: &Sluice<'_>,
    branch: &str,
    target: &Signed,
    from: &Path,
    dir: &Path,
    clock: &dyn Clock,
    out: &mut dyn Write,
) -> Result<i32> {
    use crate::signing::statement::{Claims, Held, Run, RunResult, Statement, RUN_PREDICATE};
    let from = handed_dir(from);
    std::fs::create_dir_all(dir)
        .with_context(|| format!("making the job's directory {}", dir.display()))?;
    let _ = std::fs::remove_file(dir.join(SUBJECTS));
    let report: RunReport = match std::fs::read(from.join(REPORT)) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("the run job's report")?,
        Err(_) => {
            let why = std::fs::read_to_string(from.join(WHY))
                .unwrap_or_else(|_| "the run job handed over nothing".into());
            writeln!(out, "sluice job sign: nothing to sign: {why}")?;
            return Ok(0);
        }
    };
    let policy = s
        .policy_at(&target.base)?
        .ok_or_else(|| anyhow::anyhow!("{branch}'s revision {} has no policy", target.base))?;
    let proof =
        s.seams()
            .tests
            .proven_from(&from.join(PROOF), &target.revision, &policy.tests, &report)?;
    let proven = |t: &&sluice_seams::TaskResult| proof.proven.contains(&t.task);
    // 149: a task a check depends on is signed only where it failed it.
    let failed_by = |t: &&sluice_seams::TaskResult| t.outcome == sluice_seams::Outcome::Fail;
    let results: Vec<RunResult> = report
        .tasks
        .iter()
        .filter(proven)
        .chain(report.dependencies.iter().filter(proven).filter(failed_by))
        .map(|t| RunResult {
            check: t.task.clone(),
            key: t.hash.clone(),
            outcome: t.outcome,
            replayed: matches!(t.source, sluice_seams::Source::Replayed { .. }),
            took_ms: t.took_ms,
            attempts: t.attempts,
        })
        .collect();
    for r in &proof.refused {
        writeln!(out, "not signed: {} ({}): {}", r.task, r.key, r.why)?;
    }
    for u in &proof.unkept {
        writeln!(out, "not signed: {}: {}", u.task, u.reason)?;
    }
    if results.is_empty() {
        writeln!(out, "sluice job sign: no key the tree bears out")?;
        return Ok(0);
    }
    let e = &report.executor;
    let held = Held {
        memory: e.memory,
        network: e.network,
        docker: e.docker,
        grant: e.grant.clone(),
    };
    let statement = Statement::of(Run {
        results,
        claims: Claims {
            executor: e.kind.clone(),
            confined: e.confined,
            system: target.system.clone().unwrap_or_default(),
        },
        branch: branch.to_string(),
        combination: target.revision.clone(),
        claim: target.claim.clone(),
        at: clock
            .now()
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        held: (held != Held::default()).then_some(held),
    });
    let subjects: String = statement
        .subject
        .iter()
        .map(|s| format!("{}  {}\n", s.digest["sha256"], s.name))
        .collect();
    std::fs::write(
        dir.join(PREDICATE),
        serde_json::to_vec(&statement.predicate)?,
    )?;
    std::fs::write(dir.join(PREDICATE_TYPE), RUN_PREDICATE)?;
    std::fs::write(dir.join(SUBJECTS), subjects)?;
    writeln!(
        out,
        "signs {} keys the tree of {} bears out",
        statement.subject.len(),
        target.revision
    )?;
    Ok(0)
}

/// Record 0119 (design D11): the run's report, and what proves its keys,
/// kept in `dir` for the sign job.
fn hand_over(
    dir: &Path,
    tests: &dyn sluice_seams::Tests,
    run: &RunHandle,
    report: &RunReport,
    out: &mut dyn Write,
) -> Result<i32> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("making the job's directory {}", dir.display()))?;
    tests.keep_proof(run, report, &dir.join(PROOF))?;
    std::fs::write(dir.join(REPORT), serde_json::to_vec_pretty(report)?)?;
    let (passed, failed): (Vec<_>, Vec<_>) = report
        .tasks
        .iter()
        .partition(|t| t.outcome == sluice_seams::Outcome::Pass);
    writeln!(
        out,
        "{} passed, {} failed, {} without a result; its report is in {}",
        passed.len(),
        failed.len(),
        report.missing.len() + report.not_run.len(),
        dir.join(REPORT).display()
    )?;
    Ok(0)
}

/// Record 0119 (design D11): a run job whose run reported nothing says
/// why in `dir`, which the sign job records as a run that ended with no
/// outcome (77).
fn no_report(dir: &Path, why: &str, out: &mut dyn Write) -> Result<i32> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("making the job's directory {}", dir.display()))?;
    std::fs::write(dir.join(WHY), why)?;
    writeln!(out, "sluice job run: no report: {why}")?;
    Ok(INTERNAL)
}

/// Record 0119 (design D11): a branch's records as a pool's run job reads
/// them: read through, and written nowhere. What a start writes is held
/// here for the rest of the command and goes no further, so the job holds
/// the records only as they were read; whatever is recorded or kept is
/// dropped.
pub struct Unwritten<'a> {
    inner: &'a dyn State,
    written: RefCell<BTreeMap<String, Snapshot>>,
}

impl<'a> Unwritten<'a> {
    pub fn over(inner: &'a dyn State) -> Self {
        Unwritten {
            inner,
            written: RefCell::new(BTreeMap::new()),
        }
    }

    fn overlaid(&self, branch: &str, read: impl FnOnce() -> Result<Snapshot>) -> Result<Snapshot> {
        match self.written.borrow().get(branch) {
            Some(s) => Ok(s.clone()),
            None => read(),
        }
    }
}

impl State for Unwritten<'_> {
    fn lock<'b>(&'b self, branch: &str) -> Result<Option<Box<dyn Guard + 'b>>> {
        self.inner.lock(branch)
    }

    fn wait_lock<'b>(&'b self, branch: &str) -> Result<Box<dyn Guard + 'b>> {
        self.inner.wait_lock(branch)
    }

    fn read(&self, branch: &str) -> Result<Snapshot> {
        self.overlaid(branch, || self.inner.read(branch))
    }

    fn refresh(&self) -> Result<()> {
        self.inner.refresh()
    }

    fn read_held(&self, branch: &str) -> Result<Snapshot> {
        self.overlaid(branch, || self.inner.read_held(branch))
    }

    fn hold(&self) {
        self.inner.hold()
    }

    fn stale(&self, branch: &str) -> Option<ReadRefused> {
        self.inner.stale(branch)
    }

    fn refusal(&self, branch: &str) -> Result<Option<Vec<u8>>> {
        self.inner.refusal(branch)
    }

    fn intake_refusal(&self, branch: &str) -> Result<Option<Vec<u8>>> {
        self.inner.intake_refusal(branch)
    }

    fn beats(&self) -> Result<BTreeMap<String, (String, Vec<u8>)>> {
        self.inner.beats()
    }

    fn observed_beat(&self, host: &str) -> Result<Option<(String, String)>> {
        self.inner.observed_beat(host)
    }

    fn observed(&self, branch: &str) -> Result<Option<(u64, String)>> {
        self.inner.observed(branch)
    }

    fn fetch_heads(&self, branch: &str) -> Result<()> {
        self.inner.fetch_heads(branch)
    }

    fn fetch_candidates(&self, branch: &str) -> Result<()> {
        self.inner.fetch_candidates(branch)
    }

    fn fetch_candidate(&self, branch: &str, id: &str) -> Result<()> {
        self.inner.fetch_candidate(branch, id)
    }

    fn carry(&self, _branch: &str) -> Result<Carried> {
        Ok(Carried::default())
    }

    fn closed_homes(&self, branch: &str) -> Result<Vec<String>> {
        self.inner.closed_homes(branch)
    }

    fn read_in(&self, branch: &str, home: &str) -> Result<Snapshot> {
        self.inner.read_in(branch, home)
    }

    fn read_finished_in(&self, branch: &str, home: &str, name: &str) -> Result<Option<Vec<u8>>> {
        self.inner.read_finished_in(branch, home, name)
    }

    fn read_finished(&self, branch: &str, name: &str) -> Result<Option<Vec<u8>>> {
        self.inner.read_finished(branch, name)
    }

    fn read_all_finished(&self, branch: &str) -> Result<BTreeMap<String, Vec<u8>>> {
        self.inner.read_all_finished(branch)
    }

    /// Held here, onto the records as read, and written nowhere.
    fn write(&self, branch: &str, read_at: u64, changes: Changes) -> Result<Written> {
        let mut snap = self.read(branch)?;
        if snap.version != read_at {
            return Ok(Written::Refused);
        }
        for (path, bytes) in changes.put {
            snap.files.insert(path, bytes);
        }
        for (path, bytes) in changes.append {
            snap.files.entry(path).or_default().extend(bytes);
        }
        for path in changes.remove {
            snap.files.remove(&path);
        }
        snap.version += 1;
        let version = snap.version;
        self.written.borrow_mut().insert(branch.to_string(), snap);
        Ok(Written::Written(version))
    }

    fn branches(&self) -> Result<Vec<String>> {
        self.inner.branches()
    }

    fn allocate_id(&self, branch: &str) -> Result<u64> {
        bail!("a pool's run job numbers nothing of {branch}")
    }

    fn results_for(&self, branch: &str, ask: &ResultAsk) -> Result<Vec<u8>> {
        self.inner.results_for(branch, ask)
    }

    fn verifies_own(&self, branch: &str, at: &Rev) -> Result<bool> {
        self.inner.verifies_own(branch, at)
    }

    fn record_results(&self, _lines: &[u8]) -> Result<()> {
        Ok(())
    }

    fn set_asides(&self) -> Result<Vec<u8>> {
        self.inner.set_asides()
    }

    fn set_aside(&self, _line: &[u8]) -> Result<()> {
        Ok(())
    }

    fn keep_logs(&self, _branch: &str, _candidate: &str, logs: &[TaskLog]) -> Result<Vec<TaskLog>> {
        Ok(logs.to_vec())
    }

    fn progress(&self, branch: &str) -> Result<BTreeMap<String, Vec<TaskProgress>>> {
        self.inner.progress(branch)
    }

    fn put_progress(
        &self,
        _branch: &str,
        _candidate: &str,
        _tasks: Option<&[TaskProgress]>,
    ) -> Result<()> {
        Ok(())
    }
}
