//! `sluice job sign --build` and `sluice job record --build`, a CodeBuild
//! pool's sign build (record 0119; design D6 and D7 of the change
//! `a-codebuild-fleet-runs-a-queues-checks`; 78, 109; rule core.62).
//!
//! The run builds share one role, so a check could write another build's
//! object in the hand-over bucket: the sign build keeps a run build's
//! hand-over only where its SHA-256 is the one CodeBuild recorded as it
//! uploaded it, and reads the claim and the branch from the build
//! CodeBuild answers, never from the hand-over. From there it signs as the
//! Actions sign job does, the run read from the branch's records and only
//! the keys the combination's tree bears out, with the pool's key in KMS,
//! and runs none of the combination's code.

use std::io::{Read as _, Write};
use std::path::Path;
use std::rc::Rc;

use anyhow::{anyhow, bail, Context, Result};
use sluice_core::{Clock, Sluice};
use sluice_seams::Setting;

use super::{claimed, handed_dir, kept, statement_of, Kept, REPORT, WHY};
use crate::profiles::codebuild::aws::{Aws, Build};
use crate::profiles::codebuild::sdk::{Chain, Sdk};
use crate::profiles::codebuild::{BRANCH, CLAIM};

/// The file of a sign build's directory holding the envelope the pool's key
/// signed.
pub const RECORD: &str = "record.json";

/// The directory of a sign build's directory holding the run build's
/// hand-over, as CodeBuild recorded it.
pub const RUN: &str = "run";

/// Most a hand-over may hold, unpacked.
const MOST: u64 = 512 * 1024 * 1024;

/// The calls in `region`, made as `chain` finds credentials: in a build,
/// the AWS SDK's chain, its role.
fn reach(chain: &Chain, region: &str) -> Rc<dyn Aws> {
    Rc::new(Sdk::new(
        chain.clone(),
        region,
        Rc::new(crate::retry::Policy::command().recording_where_set()),
    ))
}

/// The region an ARN names: `arn:<partition>:<service>:<region>:…`.
fn region_of(arn: &str) -> Result<String> {
    match arn.split(':').collect::<Vec<_>>()[..] {
        ["arn", _, _, region, ..] if !region.is_empty() => Ok(region.to_string()),
        _ => bail!("{arn} is no ARN naming its region"),
    }
}

/// The run build `arn`, as CodeBuild answers it: ended, and naming the
/// claim and the branch it was started for; and those.
fn run_build(aws: &dyn Aws, arn: &str) -> Result<(Build, String, String)> {
    let build = aws
        .batch_get_builds(&[arn.to_string()])?
        .pop()
        .ok_or_else(|| anyhow!("CodeBuild holds no build {arn}"))?;
    if !build.complete {
        bail!("the run build {arn} has not ended");
    }
    let named = |var: &str| {
        build
            .variables
            .get(var)
            .filter(|v| !v.is_empty())
            .cloned()
            .ok_or_else(|| anyhow!("the build {arn} names no {var}: it is no pool's run build"))
    };
    let (claim, branch) = (named(CLAIM)?, named(BRANCH)?);
    Ok((build, claim, branch))
}

/// The pool `claim` names, `pool/<pool>/<n>`.
fn pool_of(claim: &str) -> Result<&str> {
    claim
        .strip_prefix("pool/")
        .and_then(|rest| rest.rsplit_once('/'))
        .map(|(pool, _)| pool)
        .ok_or_else(|| anyhow!("{claim} is no pool's claim"))
}

/// A `kms = { key, public }` signer's key and listed public half.
fn kms_of(identity: &(String, Setting), signer: &str) -> Result<(String, String)> {
    let Setting::Tables(t) = &identity.1 else {
        bail!("[signers.{signer}] is no kms signer");
    };
    let text = |field: &str| match t.first().and_then(|t| t.get(field)) {
        Some(Setting::Text(v)) if identity.0 == "kms" => Ok(v.clone()),
        _ => Err(anyhow!("[signers.{signer}] names no kms {field}")),
    };
    Ok((text("key")?, text("public")?))
}

/// Design D6: the run build's hand-over, where CodeBuild recorded one and
/// its SHA-256 is the one CodeBuild recorded; otherwise why it is none. A
/// read AWS could not answer is an error, so the sign build fails and its
/// run is read again.
fn handed(aws: &dyn Aws, build: &Build) -> Result<Result<Vec<u8>, String>> {
    let Some(artifact) = &build.artifact else {
        return Ok(Err(format!(
            "the run build {} uploaded no hand-over",
            build.id
        )));
    };
    let Some((bucket, key)) = artifact.object() else {
        return Ok(Err(format!(
            "the run build {}'s hand-over is at {}, which names no object in S3",
            build.id, artifact.location
        )));
    };
    let Some(recorded) = &artifact.sha256 else {
        return Ok(Err(format!(
            "CodeBuild recorded no SHA-256 of the run build {}'s hand-over",
            build.id
        )));
    };
    let bytes = aws.get_object(&bucket, &key)?;
    let got = crate::profiles::remote_refs::sha256_hex(&bytes);
    if !got.eq_ignore_ascii_case(recorded.trim()) {
        return Ok(Err(format!(
            "the run build {}'s hand-over is not what CodeBuild recorded: s3://{bucket}/{key} \
             has the SHA-256 {got}, and CodeBuild recorded {recorded}",
            build.id
        )));
    }
    Ok(Ok(bytes))
}

/// `zip`'s files written under `dir`, each by a name enclosed in it, links
/// and anything past [`MOST`] left out: what a run build hands over is the
/// combination's to write.
fn unpack(zip: &[u8], dir: &Path) -> Result<()> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(zip)).context("the hand-over is no zip")?;
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).context("reading the hand-over")?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        if entry.is_symlink() {
            continue;
        }
        let path = dir.join(name);
        if entry.is_dir() {
            std::fs::create_dir_all(&path)?;
            continue;
        }
        total += entry.size();
        if total > MOST {
            bail!("the hand-over holds more than {MOST} bytes");
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(MOST)
            .read_to_end(&mut bytes)
            .context("reading the hand-over")?;
        std::fs::write(&path, bytes)?;
    }
    Ok(())
}

/// `sluice job sign --build <arn> --out <dir>` (design D6, D7): the run
/// build read from CodeBuild, refused where it is of a project the
/// branch's pool names for no system, has not ended or names no claim or
/// branch; its hand-over kept in `<dir>/run` only as CodeBuild recorded
/// it, and one that differs read as one with no report; the keys the
/// combination's tree bears out signed with the pool's key in KMS into
/// `<dir>/record.json`, and none written where none is borne out.
pub(super) fn sign(
    s: &Sluice<'_>,
    arn: &str,
    dir: &Path,
    clock: &dyn Clock,
    chain: &Chain,
    out: &mut dyn Write,
) -> Result<i32> {
    let aws = reach(chain, &region_of(arn)?);
    let (build, claim, branch) = run_build(aws.as_ref(), arn)?;
    let target = claimed(s, &branch, &claim)?;
    let policy = s
        .policy_at(&target.base)?
        .ok_or_else(|| anyhow!("{branch}'s revision {} has no policy", target.base))?;
    let name = pool_of(&claim)?;
    let pool = policy
        .pools
        .iter()
        .find(|p| p.name == name && p.kind.0 == "codebuild")
        .ok_or_else(|| anyhow!("{branch}'s policy names no CodeBuild pool {name}"))?;
    if !pool.runners.values().any(|p| *p == build.project) {
        bail!(
            "the build {arn} is of the project {}, which {branch}'s pool {name} names for no system",
            build.project
        );
    }
    let signer = policy.signers.get(&pool.signer).ok_or_else(|| {
        anyhow!(
            "[pools.{name}] signs as {}, which [signers] does not name",
            pool.signer
        )
    })?;
    let (key, public) = kms_of(&signer.identity, &pool.signer)?;

    std::fs::create_dir_all(dir)
        .with_context(|| format!("making the job's directory {}", dir.display()))?;
    let _ = std::fs::remove_file(dir.join(RECORD));
    let run = dir.join(RUN);
    let _ = std::fs::remove_dir_all(&run);
    std::fs::create_dir_all(&run)?;
    match handed(aws.as_ref(), &build)? {
        Ok(zip) => unpack(&zip, &run)?,
        Err(why) => std::fs::write(run.join(WHY), &why)?,
    }
    let from = handed_dir(&run);
    if !from.join(REPORT).exists() {
        let why = std::fs::read_to_string(from.join(WHY))
            .unwrap_or_else(|_| "the run build handed over nothing".into());
        writeln!(out, "sluice job sign: nothing to sign: {}", why.trim_end())?;
        return Ok(0);
    }
    let Kept {
        report,
        results,
        refused,
        unkept,
        ..
    } = kept(s, &branch, &target, &from)?;
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
    let statement = statement_of(&branch, &target, &report, results, clock);
    let envelope =
        crate::signing::kms::sign(reach(chain, &region_of(&key)?).as_ref(), &key, &statement)?;
    // What KMS signed verifies by the half the policy lists, as every
    // reader will verify it, before it is handed on.
    crate::signing::kms::verify(&envelope, &key, &crate::signing::kms::public_half(&public)?)
        .with_context(|| format!("the signature KMS answered for {key}"))?;
    std::fs::write(dir.join(RECORD), serde_json::to_vec(&envelope)?)?;
    writeln!(
        out,
        "signed {} keys the tree of {} bears out with {key}",
        statement.subject.len(),
        target.revision
    )?;
    Ok(0)
}

/// `sluice job record --build <arn> --from <dir>` (design D6): the claim
/// and the branch read from the run build again, `<dir>/record.json`,
/// where the sign build wrote one, added among the pool's signer's runs,
/// and the run written ended and closed under its claim in one write, as
/// `--claim` writes it; refused with nothing written where the claim no
/// longer holds the run.
pub(super) fn record(
    s: &Sluice<'_>,
    arn: &str,
    from: &Path,
    chain: &Chain,
    out: &mut dyn Write,
) -> Result<i32> {
    let aws = reach(chain, &region_of(arn)?);
    let (_, claim, branch) = run_build(aws.as_ref(), arn)?;
    let signed = from.join(RECORD);
    let signed = signed.exists().then_some(signed);
    super::record(
        s,
        &branch,
        &claim,
        signed.as_deref(),
        Some(&from.join(RUN)),
        out,
    )
}
