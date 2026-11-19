//! `sluice pool check <branch>` (record 0119; design D11 of the change
//! `a-codebuild-fleet-runs-a-queues-checks`): each CodeBuild pool the policy
//! at the branch's revision names, read back from AWS as the host reaches
//! it: who it reaches AWS as; each run project on a reserved-capacity fleet
//! of its system's environment, its buildspec as `sluice init codebuild`
//! writes it for this release, and its timeout longer than the branch's;
//! the sign project's buildspec; and the key's spec, usage and public half
//! against the policy's. It exits 1 naming each that fails, and says,
//! failing nothing, where the branch does not test side by side at the
//! fleets' size. A GitHub pool is `sluice forge check`'s.

use std::collections::BTreeMap;
use std::io::Write;
use std::rc::Rc;

use anyhow::{anyhow, Result};
use base64::Engine as _;
use sluice_core::policy::{Policy, Rules};
use sluice_core::Sluice;
use sluice_seams::Setting;

use crate::profiles::codebuild::aws::{Aws, Fleet, Project};
use crate::profiles::codebuild::sdk::{Chain, Sdk};
use crate::profiles::codebuild::template::{self, System};
use crate::profiles::codebuild::{self as codebuild};
use crate::FAILED;

/// One thing read back, and how it stands.
enum Line {
    Ok(String),
    Failed(String),
    /// Said, failing nothing.
    Note(String),
}

/// The variable a buildspec names the repository its builds clone by.
const REPOSITORY: &str = "SLUICE_REPOSITORY: \"";

/// The repository a run buildspec clones, as `sluice init codebuild`
/// writes it.
fn repository_of(buildspec: &str) -> Option<String> {
    let at = buildspec.find(REPOSITORY)? + REPOSITORY.len();
    let rest = &buildspec[at..];
    Some(rest[..rest.find('"')?].to_string())
}

/// How `had` differs from `wants`: its first line that differs.
fn differs(had: &str, wants: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (had.lines().collect(), wants.lines().collect());
    let n = (0..a.len().max(b.len()))
        .find(|i| a.get(*i) != b.get(*i))
        .unwrap_or(0);
    let line = |l: Option<&&str>| match l {
        Some(l) => format!("{:?}", l.trim()),
        None => "nothing".into(),
    };
    format!(
        "its line {} is {}, where sluice writes {}",
        n + 1,
        line(a.get(n)),
        line(b.get(n))
    )
}

/// `sluice pool check <branch>`: each pool of the policy at `branch`'s
/// revision read back, the CodeBuild ones from AWS as `aws` names the
/// profile, else the host's chain; 0 where each holds, 1 naming each that
/// does not.
pub fn check(
    s: &Sluice<'_>,
    branch: &str,
    aws: &crate::instance::Aws,
    out: &mut dyn Write,
) -> Result<i32> {
    let tip = s
        .seams()
        .branch
        .read(branch)?
        .ok_or_else(|| anyhow!("no branch {branch}"))?;
    let policy = s
        .policy_at(&tip)?
        .ok_or_else(|| anyhow!("{branch}'s revision {tip} has no policy"))?;
    let rules = policy
        .rules_for(branch)?
        .ok_or_else(|| anyhow!("{branch}'s policy names no rules for it"))?
        .clone();
    let mut lines = Vec::new();
    if policy.pools.is_empty() {
        lines.push(Line::Failed(format!("{branch}'s policy names no pool")));
    }
    // Rule core.60 (boomdrive-design's ruling of 2026-11-16): a host leads
    // a branch held on its remote only signing as a signer its gate trusts,
    // and a host signs as an `ssh` signer alone; a gate that trusts only
    // pools' signers has no lead, and its branch forms nothing.
    let hosts: Vec<&String> = rules
        .trust
        .iter()
        .filter(|t| {
            policy
                .signers
                .get(*t)
                .is_some_and(|s| s.identity.0 == "ssh")
        })
        .collect();
    match (rules.signed, hosts.as_slice()) {
        (true, []) => lines.push(Line::Failed(format!(
            "{branch}'s trust names no signer a host signs as, so no host can lead it and it \
             forms nothing: trust the signer its leading hosts sign as beside the pool's, \
             with systems = [] so it claims no run"
        ))),
        (true, signers) => {
            let names: Vec<&str> = signers.iter().map(|s| s.as_str()).collect();
            lines.push(Line::Ok(format!(
                "{branch}'s queue is run by a host signing as {}",
                names.join(" or ")
            )));
        }
        (false, _) => {}
    }
    for pool in &policy.pools {
        match pool.kind.0.as_str() {
            "codebuild" => lines.extend(fleet(&policy, &rules, pool, aws)?),
            kind => lines.push(Line::Note(format!(
                "[pools.{}] is a {kind} pool: `sluice forge check {branch}` reads it",
                pool.name
            ))),
        }
    }
    let mut failed = false;
    for line in &lines {
        let (tag, what) = match line {
            Line::Ok(w) => ("ok    ", w),
            Line::Failed(w) => {
                failed = true;
                ("failed", w)
            }
            Line::Note(w) => ("note  ", w),
        };
        writeln!(out, "{tag}  {what}")?;
    }
    Ok(if failed { FAILED } else { 0 })
}

/// The lines of one CodeBuild pool.
fn fleet(
    policy: &Policy,
    rules: &Rules,
    pool: &sluice_core::policy::Pool,
    aws: &crate::instance::Aws,
) -> Result<Vec<Line>> {
    let at = format!("[pools.{}]", pool.name);
    let (region, sign) = codebuild::table(&pool.kind)?;
    let calls: Rc<dyn Aws> = Rc::new(Sdk::new(
        Chain::of(aws),
        &region,
        Rc::new(crate::retry::Policy::command().recording_where_set()),
    ));
    let mut lines = Vec::new();
    match calls.caller_identity() {
        Ok(who) => lines.push(Line::Ok(format!("{at} is reached in {region} as {who}"))),
        Err(e) => {
            lines.push(Line::Failed(format!(
                "{at}: AWS in {region} answers nothing: {e:#}"
            )));
            return Ok(lines);
        }
    }
    let mut names: Vec<String> = pool.runners.values().cloned().collect();
    names.push(sign.clone());
    let projects: BTreeMap<String, Project> = match calls.batch_get_projects(&names) {
        Ok(p) => p.into_iter().map(|p| (p.name.clone(), p)).collect(),
        Err(e) => {
            lines.push(Line::Failed(format!(
                "{at}: its projects could not be read: {e:#}"
            )));
            return Ok(lines);
        }
    };
    let arns: Vec<String> = projects.values().filter_map(|p| p.fleet.clone()).collect();
    let fleets: BTreeMap<String, Fleet> = match arns.is_empty() {
        true => BTreeMap::new(),
        false => match calls.batch_get_fleets(&arns) {
            Ok(f) => f.into_iter().map(|f| (f.arn.clone(), f)).collect(),
            Err(e) => {
                lines.push(Line::Failed(format!(
                    "{at}: its fleets could not be read: {e:#}"
                )));
                BTreeMap::new()
            }
        },
    };
    let mut repository = None;
    let mut most = Vec::new();
    for (system, name) in &pool.runners {
        let Some(project) = projects.get(name) else {
            lines.push(Line::Failed(format!(
                "{name}, {at}'s run project for {system}, is no project of CodeBuild's in {region}"
            )));
            continue;
        };
        let Some(sys) = policy
            .systems
            .get(system)
            .and_then(|s| System::linux(system, &s.os, &s.arch))
        else {
            lines.push(Line::Failed(format!(
                "{at} runs {system} on {name}, and CodeBuild runs no Linux fleet for it"
            )));
            continue;
        };
        let environment = match sys.arch {
            template::Arch::X86_64 => "LINUX_EC2",
            template::Arch::Aarch64 => "ARM_EC2",
        };
        match project.fleet.as_ref().and_then(|f| fleets.get(f)) {
            Some(f) if f.environment == environment && project.environment == environment => {
                lines.push(Line::Ok(format!(
                    "{name} runs {system} on the reserved-capacity fleet {}, {environment}",
                    f.name
                )));
                most.push(f.max_capacity.or(f.base_capacity).unwrap_or(1));
            }
            Some(f) => lines.push(Line::Failed(format!(
                "{name} runs {system} on the fleet {}, {}, where {system} is {environment}",
                f.name, f.environment
            ))),
            None => lines.push(Line::Failed(format!(
                "{name} runs {system} on no reserved-capacity fleet: a run project names the \
                 fleet of its system"
            ))),
        }
        let had = project.buildspec.clone().unwrap_or_default();
        let repo = repository_of(&had);
        let wants = template::buildspecs(
            crate::VERSION,
            repo.as_deref().unwrap_or("OWNER/REPOSITORY"),
            &sys,
        )
        .0;
        match had.trim_end() == wants.trim_end() {
            true => lines.push(Line::Ok(format!(
                "{name}'s buildspec is as sluice {} writes it, cloning {}",
                crate::VERSION,
                repo.as_deref().unwrap_or_default()
            ))),
            false => lines.push(Line::Failed(format!(
                "{name}'s buildspec is not as sluice init codebuild writes it for sluice {}: {}",
                crate::VERSION,
                differs(&had, &wants)
            ))),
        }
        repository = repository.or(repo.map(|r| (r, sys.clone())));
        let limit = rules.timeout.map(|t| t.as_secs().div_ceil(60));
        match (project.timeout_minutes, limit) {
            (Some(m), Some(l)) if (m as u64) > l => lines.push(Line::Ok(format!(
                "{name} may take {m} minutes, longer than the branch's timeout of {l}"
            ))),
            (Some(m), Some(l)) => lines.push(Line::Failed(format!(
                "{name} may take {m} minutes, no longer than the branch's timeout of {l}: \
                 CodeBuild would stop a run sluice still waits on"
            ))),
            (m, None) => lines.push(Line::Ok(format!(
                "{name} may take {} minutes; the branch names no timeout",
                m.map_or("CodeBuild's".into(), |m| m.to_string())
            ))),
            (None, Some(l)) => lines.push(Line::Failed(format!(
                "{name} names no timeout, and the branch's is {l} minutes"
            ))),
        }
    }
    match (projects.get(&sign), &repository) {
        (None, _) => lines.push(Line::Failed(format!(
            "{sign}, {at}'s sign project, is no project of CodeBuild's in {region}"
        ))),
        (Some(project), Some((repo, sys))) => {
            let had = project.buildspec.clone().unwrap_or_default();
            let wants = template::buildspecs(crate::VERSION, repo, sys).1;
            match had.trim_end() == wants.trim_end() {
                true => lines.push(Line::Ok(format!(
                    "{sign}'s buildspec is as sluice {} writes it",
                    crate::VERSION
                ))),
                false => lines.push(Line::Failed(format!(
                    "{sign}'s buildspec is not as sluice init codebuild writes it for sluice {}: {}",
                    crate::VERSION,
                    differs(&had, &wants)
                ))),
            }
        }
        (Some(_), None) => lines.push(Line::Failed(format!(
            "{sign}'s buildspec could not be read against a run project's"
        ))),
    }
    lines.push(key(policy, pool, calls.as_ref()));
    let fleets_most = most.iter().min().copied();
    if rules.batch || fleets_most.is_some_and(|m| rules.parallel as i32 != m) {
        let m = fleets_most.map_or("the fleets' maximum".into(), |m| m.to_string());
        lines.push(Line::Note(format!(
            "a branch on a fleet tests side by side: batch = false, with parallel = {m}, the \
             fleets' maximum, where it tests with batch = {} and parallel = {}",
            rules.batch, rules.parallel
        )));
    }
    Ok(lines)
}

/// The key the pool's signer names, read back from KMS against the half
/// the policy lists.
fn key(policy: &Policy, pool: &sluice_core::policy::Pool, calls: &dyn Aws) -> Line {
    let at = format!("[signers.{}]", pool.signer);
    let Some(signer) = policy.signers.get(&pool.signer) else {
        return Line::Failed(format!("{at} is named by no signer"));
    };
    let Setting::Tables(t) = &signer.identity.1 else {
        return Line::Failed(format!("{at} names no kms key"));
    };
    let text = |f: &str| match t.first().and_then(|t| t.get(f)) {
        Some(Setting::Text(v)) => v.clone(),
        _ => String::new(),
    };
    let (arn, listed) = (text("key"), text("public"));
    match calls.get_public_key(&arn) {
        Err(e) => Line::Failed(format!("the key {arn} could not be read: {e:#}")),
        Ok(k) if k.spec != "ECC_NIST_P256" || k.usage != "SIGN_VERIFY" => Line::Failed(format!(
            "the key {arn} is {} with the usage {}, where a kms signer's is ECC_NIST_P256 with \
             SIGN_VERIFY",
            k.spec, k.usage
        )),
        Ok(k) if base64::engine::general_purpose::STANDARD.encode(&k.der) != listed.trim() => {
            Line::Failed(format!(
                "the key {arn}'s public half is not the one {at} lists: the policy lists \
                 another key's"
            ))
        }
        Ok(_) => Line::Ok(format!(
            "the key {arn} is ECC_NIST_P256, SIGN_VERIFY, and its public half is the one {at} \
             lists"
        )),
    }
}
