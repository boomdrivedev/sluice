//! `sluice job`, a pool's job as `sluice.yml` runs it (record 0119; design
//! D11 of the change `a-runs-life-is-written-first-and-actions-runs-its-
//! checks`): `run`, in the run job, starts the run its claim holds, or a
//! pull request's head, and hands what it reported and what proves its
//! keys to the sign job in a directory, recording nothing, writing no
//! state and holding no key, and keeps its checkout for the job's own
//! steps, which `release` releases after them (record 0119 as amended
//! 2026-11-18; design D2 of the change
//! `a-pools-run-job-runs-the-repositorys-own-steps`); `sign`, in the sign
//! job, keeps of that report
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

use crate::{JobCommand, Pause, FAILED, INTERNAL, STOPPED};

pub mod codebuild;

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
            checkout: kept,
        } => {
            let unwritten = Unwritten::over(seams.state);
            let (tests, checkout) = (seams.tests, seams.checkout);
            let stopped = std::cell::Cell::new(false);
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
            let ended = (|| loop {
                match tests.progress(&run)? {
                    Progress::Running => {
                        if !pause.pause() {
                            stopped.set(true);
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
            })();
            // Record 0119 as amended 2026-11-18; rule core.63 (design D2 of
            // the change `a-pools-run-job-runs-the-repositorys-own-steps`):
            // the checkout the job made is kept, with its ports and scratch,
            // whatever its run came to, for the job's own steps, and named in
            // `kept` for `sluice job release`. Stopped, it is released at
            // once, everything started in it ended first, by the release the
            // policy names (record 0016, 123; rule core.10), and no file is
            // written. A release that fails is said, and decides nothing of
            // the run's outcome.
            let place = match (&claim, &head) {
                (Some(claim), _) => s
                    .seams()
                    .state
                    .read(&branch)
                    .and_then(|snap| sluice_core::line::Line::load(&branch, snap))?
                    .doc
                    .candidates
                    .iter()
                    .flat_map(|c| &c.runs)
                    .find(|r| r.holder() == Some(claim.as_str()))
                    .and_then(|r| r.place.clone()),
                (None, Some(head)) => Some(sluice_core::step::head_place(&Rev::new(head))),
                (None, None) => None,
            };
            if let Some(place) = place {
                match checkout.path(&place)? {
                    Some(path) if !stopped.get() => keep_checkout(&kept, &path, &place, out)?,
                    _ => {
                        release_checkout(checkout, &place, Some(&dir), "sluice job run", out)?;
                    }
                }
            }
            ended
        }
        JobCommand::Release {
            checkout: kept,
            out: dir,
        } => release(seams.checkout, &kept, dir.as_deref(), out),
        JobCommand::Sign {
            claim,
            head,
            build,
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
            if let Some(build) = &build {
                return codebuild::sign(&s, build, &dir, clock, out);
            }
            let (Some(branch), Some(from)) = (branch, from) else {
                bail!("sluice job sign: give --branch and --from, or --build");
            };
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
        JobCommand::Record {
            claim,
            head,
            build,
            branch,
            bundle,
            from,
        } => {
            let s = Sluice::new(seams, clock).as_peer(host.peer());
            if let Some(build) = &build {
                let from = from.ok_or_else(|| anyhow::anyhow!("--build needs --from"))?;
                return codebuild::record(&s, build, &from, out);
            }
            let Some(branch) = branch else {
                bail!("sluice job record: give --branch, or --build");
            };
            match (claim, head, bundle) {
                (Some(claim), None, bundle) => {
                    record(&s, &branch, &claim, bundle.as_deref(), from.as_deref(), out)
                }
                (None, Some(head), Some(bundle)) => {
                    record_head(&s, &branch, &Rev::new(head), &bundle, out)
                }
                _ => bail!("sluice job record: give --claim, or --head with --bundle"),
            }
        }
    }
}

/// The run a signed record's statement holds, read without verifying it:
/// the readers of the signer's runs verify it (78). A Sigstore bundle's,
/// or a `kms` signer's DSSE envelope's (record 0119; design D6 of the
/// change `a-codebuild-fleet-runs-a-queues-checks`).
fn bundled_run(bytes: &[u8]) -> Result<crate::signing::statement::Run> {
    use crate::signing::statement::Statement;
    let text = std::str::from_utf8(bytes).context("the bundle is no JSON")?;
    let v: serde_json::Value = serde_json::from_str(text).context("the bundle is no JSON")?;
    if v.get("dsseEnvelope").is_none() && v.get("payloadType").is_some() {
        let envelope: crate::signing::dsse::Envelope =
            serde_json::from_value(v).context("the record is no DSSE envelope")?;
        return Ok(Statement::from_envelope(&envelope)
            .context("the envelope's statement is no run's")?
            .predicate);
    }
    let statement = crate::profiles::github::keyless::statement_of(text)?;
    let st: Statement = serde_json::from_value(statement)
        .context("the bundle's statement is no run's")
        .and_then(Statement::checked)?;
    Ok(st.predicate)
}

/// The signer of `branch`'s pool `pool`, or of its one pool on GitHub where
/// `pool` is `None`, from its policy at `at`.
fn pool_signer(s: &Sluice<'_>, branch: &str, at: &Rev, pool: Option<&str>) -> Result<String> {
    let policy = s
        .policy_at(at)?
        .ok_or_else(|| anyhow::anyhow!("{branch}'s revision {at} has no policy"))?;
    let mut on_github = policy
        .pools
        .iter()
        .filter(|p| pool.map_or(p.kind.0 == "github", |n| p.name == n));
    match (on_github.next(), on_github.next()) {
        (Some(p), None) => Ok(p.signer.clone()),
        (None, _) => bail!(
            "{branch}'s policy names no pool {}",
            pool.unwrap_or("on GitHub")
        ),
        (Some(_), Some(_)) => bail!("{branch}'s policy names more than one pool on GitHub"),
    }
}

/// Where a signed record of `run` of `combination` is kept among its
/// signer's runs: by its combination, its system and its own digest.
fn record_path(combination: &Rev, system: Option<&str>, bytes: &[u8]) -> String {
    format!(
        "{combination}/{}/{}.json",
        system.filter(|s| !s.is_empty()).unwrap_or("any"),
        &crate::profiles::remote_refs::sha256_hex(bytes)[..16]
    )
}

/// 113, record 0119 (design D11): a pull request's head run's bundle,
/// added among the pool's signer's runs, where its results count by key
/// once read (15); no state is written, since a head's run is no
/// combination's.
fn record_head(
    s: &Sluice<'_>,
    branch: &str,
    head: &Rev,
    bundle: &Path,
    out: &mut dyn Write,
) -> Result<i32> {
    let bytes = std::fs::read(bundle)
        .with_context(|| format!("reading the bundle {}", bundle.display()))?;
    let run = bundled_run(&bytes)?;
    if run.claim.is_some() || run.combination != *head {
        bail!(
            "the bundle is of {} under {}, not of the head {head} under no claim",
            run.combination,
            run.claim.as_deref().unwrap_or("no claim")
        );
    }
    let at = base_of(s, branch)?;
    let signer = pool_signer(s, branch, &at, None)?;
    let system = Some(run.claims.system.as_str());
    let path = record_path(head, system, &bytes);
    s.seams().state.add_signed(branch, &signer, &path, &bytes)?;
    writeln!(
        out,
        "added {signer}'s {} signed results of the head {head}",
        run.results.len()
    )?;
    Ok(0)
}

/// Record 0119 (design D11; 77, 109): the sign job's end of the run
/// `claim` holds: its bundle, where it signed one, added to the pool's
/// signer's runs, then the run written ended, from the bundle's results,
/// and closed in one write. Where the claim no longer holds the run,
/// nothing is written and the record is refused; where it lapses between
/// the two, the bundle stays among the signer's runs, which a run's keys
/// count by alone (15).
fn record(
    s: &Sluice<'_>,
    branch: &str,
    claim: &str,
    bundle: Option<&Path>,
    from: Option<&Path>,
    out: &mut dyn Write,
) -> Result<i32> {
    let snap = s.seams().state.read(branch)?;
    let doc = sluice_core::line::Line::load(branch, snap)?.doc;
    let held = doc.candidates.iter().find_map(|c| {
        c.runs
            .iter()
            .find(|r| r.holder() == Some(claim) && r.state.held())
            .map(|r| (c, r))
    });
    let Some((c, r)) = held else {
        writeln!(
            out,
            "sluice job record: {claim} no longer holds a run of {branch}; nothing is recorded"
        )?;
        return Ok(FAILED);
    };
    // 78 (the actions-pool spec's "A key the tree does not bear out"): each
    // check of the run job's report that the sign job left out, judged
    // again here as it judged it, by the combination's tree alone, so that
    // check is left without a result rather than lost.
    let ran = from.map(handed_dir).filter(|d| d.join(REPORT).exists());
    let left = match &ran {
        Some(dir) => kept(s, branch, &claimed(s, branch, claim)?, dir)?.left,
        None => Vec::new(),
    };
    let (signed, why) = match bundle {
        Some(path) => {
            let bytes = std::fs::read(path)
                .with_context(|| format!("reading the bundle {}", path.display()))?;
            let run = bundled_run(&bytes)?;
            if run.claim.as_deref() != Some(claim) || run.combination != c.revision {
                bail!(
                    "the bundle is of {} under {}, not of {claim}'s run of {}",
                    run.combination,
                    run.claim.as_deref().unwrap_or("no claim"),
                    c.revision
                );
            }
            let base = doc
                .base
                .clone()
                .ok_or_else(|| anyhow::anyhow!("{branch}'s records hold no revision yet"))?;
            let pool = claim
                .strip_prefix("pool/")
                .and_then(|rest| rest.rsplit_once('/'))
                .map(|(pool, _)| pool)
                .ok_or_else(|| anyhow::anyhow!("{claim} is no pool's claim"))?;
            let signer = pool_signer(s, branch, &base, Some(pool))?;
            let path = record_path(&c.revision, r.system.as_deref(), &bytes);
            s.seams().state.add_signed(branch, &signer, &path, &bytes)?;
            let signed: Vec<sluice_core::SignedResult> = run
                .results
                .iter()
                .map(|r| sluice_core::SignedResult {
                    check: r.check.clone(),
                    outcome: r.outcome,
                    replayed: r.replayed,
                })
                .collect();
            (signed, "the sign job signed no outcome for it".to_string())
        }
        None if ran.is_some() => (Vec::new(), NOTHING_SIGNED.to_string()),
        None => {
            let why = from
                .map(handed_dir)
                .and_then(|d| std::fs::read_to_string(d.join(WHY)).ok())
                .unwrap_or_else(|| "the run job kept no report".into());
            (Vec::new(), why)
        }
    };
    if !s.end_claimed(branch, claim, &signed, &left, &why)? {
        writeln!(
            out,
            "sluice job record: {claim}'s claim lapsed before its run's end was written"
        )?;
        return Ok(FAILED);
    }
    let passed = signed
        .iter()
        .filter(|r| !r.replayed && r.outcome == sluice_seams::Outcome::Pass)
        .count();
    writeln!(
        out,
        "ended {claim}'s run of {}: {passed} of {} signed results passed",
        c.revision,
        signed.len()
    )?;
    for n in &left {
        writeln!(out, "left without a result: {}: {}", n.task, n.reason)?;
    }
    Ok(0)
}

/// Why a run whose report the sign job signed nothing of ended so.
const NOTHING_SIGNED: &str = "the sign job signed no key the combination's tree bears out";

/// Why a check of the run job's report that nothing it handed over proves
/// was left out, where the check profile gave no why of its own.
const UNPROVEN_KEY: &str = "nothing the run job handed over proves its key";

/// Record 0119 (design D11; 78, 109): what a sign job keeps of a run job's
/// report, read from `from`, the run job's directory: `results`, each key
/// the tree of `target.revision` bears out by the proof the job kept, and
/// `left`, each check of the report it keeps no result of, with why.
struct Kept {
    report: RunReport,
    results: Vec<crate::signing::statement::RunResult>,
    refused: Vec<sluice_seams::Refused>,
    unkept: Vec<sluice_seams::NotRun>,
    left: Vec<sluice_seams::NotRun>,
}

/// [`Kept`] of the report in `from`, judged by the policy at
/// `target.base`, running nothing of the combination.
fn kept(s: &Sluice<'_>, branch: &str, target: &Signed, from: &Path) -> Result<Kept> {
    use crate::signing::statement::RunResult;
    let report: RunReport = serde_json::from_slice(
        &std::fs::read(from.join(REPORT)).context("reading the run job's report")?,
    )
    .context("the run job's report")?;
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
    let left = report
        .tasks
        .iter()
        .filter(|t| !proof.proven.contains(&t.task))
        .map(|t| {
            let why = proof
                .refused
                .iter()
                .find(|r| r.task == t.task)
                .map(|r| r.why.clone())
                .or_else(|| {
                    proof
                        .unkept
                        .iter()
                        .find(|u| u.task == t.task)
                        .map(|u| u.reason.clone())
                })
                .unwrap_or_else(|| UNPROVEN_KEY.into());
            sluice_seams::NotRun {
                task: t.task.clone(),
                reason: why,
                key: Some(t.hash.clone()),
            }
        })
        .collect();
    Ok(Kept {
        report,
        results,
        refused: proof.refused,
        unkept: proof.unkept,
        left,
    })
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

/// `branch`'s current revision, whose gate a head's run is held to (rule
/// core.15).
fn base_of(s: &Sluice<'_>, branch: &str) -> Result<Rev> {
    s.seams()
        .branch
        .read(branch)?
        .ok_or_else(|| anyhow::anyhow!("no branch {branch}"))
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
    use crate::signing::statement::RUN_PREDICATE;
    let from = handed_dir(from);
    std::fs::create_dir_all(dir)
        .with_context(|| format!("making the job's directory {}", dir.display()))?;
    let _ = std::fs::remove_file(dir.join(SUBJECTS));
    if !from.join(REPORT).exists() {
        let why = std::fs::read_to_string(from.join(WHY))
            .unwrap_or_else(|_| "the run job handed over nothing".into());
        writeln!(out, "sluice job sign: nothing to sign: {why}")?;
        return Ok(0);
    }
    let Kept {
        report,
        results,
        refused,
        unkept,
        ..
    } = kept(s, branch, target, &from)?;
    for r in &refused {
        writeln!(out, "not signed: {} ({}): {}", r.task, r.key, r.why)?;
    }
    for u in &unkept {
        writeln!(out, "not signed: {}: {}", u.task, u.reason)?;
    }
    if results.is_empty() {
        writeln!(out, "sluice job sign: no key the tree bears out")?;
        return Ok(0);
    }
    let statement = statement_of(branch, target, &report, results, clock);
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

/// Record 0119 (design D11): the statement a pool's job signs of `report`'s
/// `results` for `target`, as the run's executor held it.
fn statement_of(
    branch: &str,
    target: &Signed,
    report: &RunReport,
    results: Vec<crate::signing::statement::RunResult>,
    clock: &dyn Clock,
) -> crate::signing::statement::Statement {
    use crate::signing::statement::{Claims, Held, Run, Statement};
    let e = &report.executor;
    let held = Held {
        memory: e.memory,
        network: e.network,
        docker: e.docker,
        grant: e.grant.clone(),
    };
    Statement::of(Run {
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
    })
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

/// Record 0119 as amended 2026-11-18 (design D2 of the change
/// `a-pools-run-job-runs-the-repositorys-own-steps`): the checkout `place`
/// at `path`, kept for the job's own steps, named in `file`, its path on
/// the first line and its place's name on the second.
fn keep_checkout(file: &Path, path: &Path, place: &str, out: &mut dyn Write) -> Result<()> {
    if let Some(parent) = file.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).with_context(|| format!("making {}", parent.display()))?;
    }
    std::fs::write(file, format!("{}\n{place}\n", path.display()))
        .with_context(|| format!("writing {}", file.display()))?;
    writeln!(
        out,
        "kept its checkout {place} at {} for the job's own steps; `sluice job release --checkout {}` releases it",
        path.display(),
        file.display()
    )?;
    Ok(())
}

/// Record 0016, 123 (design D8 of the change
/// `a-codebuild-fleet-runs-a-queues-checks`): the run's checkout `place`
/// released, or why it was not, said in `command`'s output and, beside the
/// report, in the hand-over's `why` where `dir` is given. Whether it was.
fn release_checkout(
    checkout: &dyn sluice_seams::Checkout,
    place: &str,
    dir: Option<&Path>,
    command: &str,
    out: &mut dyn Write,
) -> Result<bool> {
    match checkout.release(place) {
        Ok(()) => {
            writeln!(out, "released its checkout {place}")?;
            Ok(true)
        }
        Err(e) => {
            let why = format!("its checkout {place} was not released: {e:#}");
            writeln!(out, "{command}: {why}")?;
            if let Some(dir) = dir {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("making the job's directory {}", dir.display()))?;
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join(WHY))?;
                writeln!(file, "{why}")?;
            }
            Ok(false)
        }
    }
}

/// Record 0119 as amended 2026-11-18; rules core.10, core.16, core.63
/// (design D2 of the change `a-pools-run-job-runs-the-repositorys-own-
/// steps`): the checkout `sluice job run` kept, named on `file`'s second
/// line, released after the job's own steps by the release the policy
/// names, its ports and scratch freed, and `file` removed. A release that
/// fails is said, and in `dir`'s `why` where given, exiting 1 with `file`
/// kept, and decides nothing of the run's outcome; with no `file`, nothing
/// is released.
fn release(
    checkout: &dyn sluice_seams::Checkout,
    file: &Path,
    dir: Option<&Path>,
    out: &mut dyn Write,
) -> Result<i32> {
    let named = match std::fs::read_to_string(file) {
        Ok(named) => named,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            writeln!(
                out,
                "sluice job release: {} names no checkout kept; nothing is released",
                file.display()
            )?;
            return Ok(0);
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
    };
    let place = named
        .lines()
        .nth(1)
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .ok_or_else(|| anyhow::anyhow!("{} names no place on its second line", file.display()))?;
    if !release_checkout(checkout, place, dir, "sluice job release", out)? {
        return Ok(FAILED);
    }
    std::fs::remove_file(file).with_context(|| format!("removing {}", file.display()))?;
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

/// The variables a sign job's record names sluice's app by, from the
/// `sluice` environment: its id, and its private key's text.
pub const APP_ID_VAR: &str = "SLUICE_APP_ID";
pub const APP_KEY_VAR: &str = "SLUICE_APP_KEY";

/// Record 0119 (design D11): `instance`, where it names no app, given the
/// app the sign job's environment names, its key kept in a file only this
/// user reads for the command's life, so `sluice job record` writes the
/// branch's records as sluice; as it was where the environment names none.
pub fn with_app_from_env(
    mut instance: crate::instance::Instance,
) -> Result<crate::instance::Instance> {
    use std::os::unix::fs::OpenOptionsExt;
    let key = std::env::var(APP_KEY_VAR).unwrap_or_default();
    if instance.forge.is_some() || key.trim().is_empty() {
        return Ok(instance);
    }
    let app_id: u64 = std::env::var(APP_ID_VAR)
        .with_context(|| format!("{APP_KEY_VAR} is set, and {APP_ID_VAR} names no app"))?
        .trim()
        .parse()
        .with_context(|| format!("{APP_ID_VAR} is no app's id"))?;
    let path = std::env::temp_dir().join(format!("sluice-app-{}.pem", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("keeping the app's key in {}", path.display()))?;
    file.write_all(key.as_bytes())?;
    instance.forge = Some(crate::instance::Forge {
        app_id,
        source: crate::instance::Source::Key(path),
        discovery: crate::instance::DISCOVERY,
    });
    Ok(instance)
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

    fn pool_refusal(&self, branch: &str) -> Result<Option<Vec<u8>>> {
        self.inner.pool_refusal(branch)
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
