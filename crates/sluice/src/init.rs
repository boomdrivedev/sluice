//! `sluice init actions` (record 0119; design D14 of the change
//! `a-runs-life-is-written-first-and-actions-runs-its-checks`): the pool
//! on GitHub Actions set up in a working copy: both workflows written, the
//! signer and the pool appended to the policy where it names no pool on
//! GitHub, and what to set on GitHub said. It writes files and nothing
//! else; they land as any change does.
//!
//! `sluice init codebuild` (record 0119; design D10 of the change
//! `a-codebuild-fleet-runs-a-queues-checks`): the pool on a CodeBuild fleet,
//! its CloudFormation template written and how to deploy it said; and, with
//! `--stack`, the deployed stack's key read and its signer and pool
//! appended to the policy where it names neither.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use anyhow::{bail, Context, Result};
use sluice_core::policy::POLICY_PATH;

use crate::own_steps::{Carry, Reading};
use crate::profiles::codebuild::template;
use crate::profiles::github::workflows;
use crate::FAILED;

/// The runner GitHub hosts for a system, by its `os` and `arch` as
/// `[systems]` names them.
fn hosted_runner(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("linux", "x86_64") => Some("ubuntu-24.04"),
        ("linux", "aarch64") => Some("ubuntu-24.04-arm"),
        ("macos", "aarch64") => Some("macos-15"),
        ("macos", "x86_64") => Some("macos-15-intel"),
        ("windows", "x86_64") => Some("windows-2025"),
        _ => None,
    }
}

/// What a policy read with `toml` says of the pool: the runners of the
/// pool on GitHub it names, or what to append for one.
enum Pool {
    Named(BTreeMap<String, String>),
    Appended {
        text: String,
        runners: BTreeMap<String, String>,
        /// Whether a `[systems]` table is appended, the policy naming none.
        systems: bool,
        /// Each system `[systems]` names that GitHub hosts no runner for,
        /// as `<name> (<os> <arch>)`.
        unhosted: Vec<String>,
    },
}

/// The pool `policy` names on GitHub, or the tables to append for the
/// `forge` signer and pool: the signer's workflow at `git_ref`, the pool's
/// runners for each system `[systems]` names that GitHub hosts, each it
/// hosts none for named, and, where it names none, a `linux-x64` system
/// appended with them (85).
fn pool_of(policy: &str, git_ref: &str) -> Result<Pool> {
    let table: toml::Table = policy
        .parse()
        .with_context(|| format!("reading {POLICY_PATH}"))?;
    let pools = table.get("pools").and_then(|p| p.as_table());
    let github = pools.and_then(|p| {
        p.values()
            .filter_map(|v| v.as_table())
            .find(|t| t.contains_key("github"))
    });
    if let Some(pool) = github {
        let runners = pool
            .get("runners")
            .and_then(|r| r.as_table())
            .map(|r| {
                r.iter()
                    .filter_map(|(s, v)| Some((s.clone(), v.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        return Ok(Pool::Named(runners));
    }
    let workflow = format!("{}/{}", workflows::DIR, workflows::SLUICE);
    let mut text = String::new();
    let signers = table.get("signers").and_then(|s| s.as_table());
    if signers.is_some_and(|s| s.contains_key("forge")) {
        bail!(
            "{POLICY_PATH} names [signers.forge] and no pool on GitHub; name the pool, or rename the signer"
        );
    }
    if pools.is_some_and(|p| p.contains_key("forge")) {
        bail!("{POLICY_PATH} names [pools.forge], which is no pool on GitHub");
    }
    let mut runners = BTreeMap::new();
    let mut unhosted = Vec::new();
    let systems = table.get("systems").and_then(|s| s.as_table());
    for (name, s) in systems.into_iter().flatten() {
        let field = |k: &str| s.get(k).and_then(|v| v.as_str()).unwrap_or_default();
        match hosted_runner(field("os"), field("arch")) {
            Some(runner) => {
                runners.insert(name.clone(), runner.to_string());
            }
            None => unhosted.push(format!("{name} ({} {})", field("os"), field("arch"))),
        }
    }
    if systems.is_none() {
        text.push_str("\n[systems]\nlinux-x64 = { os = \"linux\", arch = \"x86_64\" }\n");
        runners.insert("linux-x64".into(), "ubuntu-24.04".into());
    }
    if runners.is_empty() {
        bail!("{POLICY_PATH}'s [systems] names no system GitHub hosts a runner for");
    }
    let listed: Vec<String> = runners
        .iter()
        .map(|(s, r)| format!("{s} = \"{r}\""))
        .collect();
    text.push_str(&format!(
        "\n[signers.forge]\ngithub = {{ workflow = \"{workflow}\", ref = \"{git_ref}\" }}\n\
         \n[pools.forge]\ngithub = {{ workflow = \"{workflow}\" }}\nsigner = \"forge\"\n\
         runners = {{ {} }}\n",
        listed.join(", ")
    ));
    Ok(Pool::Appended {
        text,
        runners,
        systems: systems.is_none(),
        unhosted,
    })
}

/// The ref the signer's workflow runs from: the remote's default branch as
/// the working copy holds it, else `main`'s, never the branch the working
/// copy has checked out, which may be a feature branch's (design D14).
fn signer_ref(dir: &Path) -> String {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .args(["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let name = String::from_utf8_lossy(&o.stdout).trim().to_string();
            match name.strip_prefix("refs/remotes/origin/") {
                Some(b) => format!("refs/heads/{b}"),
                None => "refs/heads/main".into(),
            }
        }
        _ => "refs/heads/main".into(),
    }
}

/// How `had` differs from `wants`: its first line that differs, by number.
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
        "line {} holds {}, where sluice writes {}",
        n + 1,
        line(a.get(n)),
        line(b.get(n))
    )
}

/// `sluice init actions` in the working copy at `dir`.
pub fn actions(dir: &Path, force: bool, out: &mut dyn Write) -> Result<i32> {
    let policy_path = dir.join(POLICY_PATH);
    let policy = std::fs::read_to_string(&policy_path).with_context(|| {
        format!(
            "{} holds no policy; sluice init actions adds the pool to one",
            policy_path.display()
        )
    })?;
    let git_ref = signer_ref(dir);
    let pool = pool_of(&policy, &git_ref)?;
    let runners = match &pool {
        Pool::Named(r) | Pool::Appended { runners: r, .. } => r.clone(),
    };
    let version = crate::VERSION;
    let files = [
        (workflows::SLUICE, workflows::sluice(version, &runners)),
        (
            workflows::SLUICE_PR,
            workflows::sluice_pr(version, &runners),
        ),
    ];
    let mut left = false;
    let at = dir.join(workflows::DIR);
    std::fs::create_dir_all(&at).with_context(|| format!("making {}", at.display()))?;
    for (name, wants) in &files {
        let path = at.join(name);
        let shown = format!("{}/{name}", workflows::DIR);
        // Record 0119 as amended 2026-11-18 (design D3, D4): the
        // repository's own steps, and its run job's `packages: write`, are
        // its own; a line of them that leaves the run job's steps is left,
        // `--force` or not, since carrying them would carry it.
        let had = match std::fs::read_to_string(&path) {
            Ok(had) => had,
            Err(_) => {
                std::fs::write(&path, wants).with_context(|| format!("writing {shown}"))?;
                writeln!(out, "{shown}: written")?;
                continue;
            }
        };
        let written = match workflows::read(&had, wants)? {
            reading @ Reading::AsWritten { .. } => {
                writeln!(
                    out,
                    "{shown}: as sluice writes it{}",
                    workflows::holds(&reading)
                )?;
                continue;
            }
            Reading::Leaves(l) => Carry::Leaves(l),
            Reading::Differs(d) if !force => {
                writeln!(out, "{shown}: left as it is; {d}; --force writes it")?;
                left = true;
                continue;
            }
            Reading::Differs(_) => workflows::carry(&had, wants)?,
        };
        match written {
            Carry::Leaves(l) => {
                writeln!(
                    out,
                    "{shown}: left as it is; {l} leaves the run job's steps, where the \
                     repository's own steps go; move it between the run job's `# sluice:` lines"
                )?;
                left = true;
            }
            Carry::Carried { text, dropped } => {
                std::fs::write(&path, &text).with_context(|| format!("writing {shown}"))?;
                writeln!(
                    out,
                    "{shown}: written{}",
                    workflows::holds(&workflows::read(&text, wants)?)
                )?;
                for job in dropped {
                    writeln!(
                        out,
                        "{shown}: the repository's own steps of the job {job}, which sluice \
                         no longer writes, are not carried"
                    )?;
                }
            }
        }
    }
    match pool {
        Pool::Named(_) => writeln!(out, "{POLICY_PATH}: names its pool on GitHub already")?,
        Pool::Appended {
            text,
            systems,
            unhosted,
            ..
        } => {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&policy_path)
                .with_context(|| format!("appending to {POLICY_PATH}"))?;
            if !policy.ends_with('\n') {
                file.write_all(b"\n")?;
            }
            file.write_all(text.as_bytes())?;
            let appended = match systems {
                true => "[systems], naming linux-x64, [signers.forge] and [pools.forge]",
                false => "[signers.forge] and [pools.forge]",
            };
            writeln!(
                out,
                "{POLICY_PATH}: {appended} appended; a branch's `systems` names the systems \
                 its runs need, and `trust` names forge"
            )?;
            for system in unhosted {
                writeln!(
                    out,
                    "{POLICY_PATH}: GitHub hosts no runner for {system}; name one for it in \
                     [pools.forge] runners to run it on the pool"
                )?;
            }
        }
    }
    let branch = git_ref.strip_prefix("refs/heads/").unwrap_or(&git_ref);
    writeln!(out, "\nThen, on GitHub:")?;
    writeln!(
        out,
        "  - give sluice's app the permission to run workflows: Actions, read and write"
    )?;
    writeln!(
        out,
        "  - make an environment named `sluice` that only {branch} may deploy from, \
         holding the secret SLUICE_APP_KEY, the app's private key, and the variable \
         SLUICE_APP_ID, its id"
    )?;
    writeln!(
        out,
        "  - land these files on {branch}, where the workflow runs from"
    )?;
    Ok(if left { FAILED } else { 0 })
}

/// The working copy's repository on GitHub, by its `origin`: what the
/// template's builds clone.
fn repository(dir: &Path) -> Result<String> {
    let url = crate::profiles::pushed_refs::remote_url(dir, "origin")
        .with_context(|| format!("reading {}'s origin", dir.display()))?;
    let repo = crate::profiles::github::Repo::from_url(&url).ok_or_else(|| {
        anyhow::anyhow!("origin ({url}) is no repository on GitHub, which the run builds clone")
    })?;
    Ok(repo.to_string())
}

/// The systems a CodeBuild pool runs: `asked`, by the policy's names, each
/// a system CodeBuild runs on Linux; else each `[systems]` names that
/// CodeBuild runs on Linux, the others said in `left`; else, where the
/// policy names none, `linux-x64`.
fn codebuild_systems(
    policy: &toml::Table,
    asked: &[String],
    left: &mut Vec<String>,
) -> Result<Vec<template::System>> {
    let named = policy.get("systems").and_then(|s| s.as_table());
    let of = |name: &str| -> Option<(String, String)> {
        let s = named?.get(name)?.as_table()?;
        let field = |k: &str| {
            s.get(k)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        };
        Some((field("os"), field("arch")))
    };
    let known = |name: &str| match name {
        "linux-x64" => Some(("linux".to_string(), "x86_64".to_string())),
        "linux-arm64" => Some(("linux".to_string(), "aarch64".to_string())),
        _ => None,
    };
    if !asked.is_empty() {
        return asked
            .iter()
            .map(|name| {
                let (os, arch) = of(name).or_else(|| known(name)).ok_or_else(|| {
                    anyhow::anyhow!("{POLICY_PATH}'s [systems] names no system {name}")
                })?;
                template::System::linux(name, &os, &arch).ok_or_else(|| {
                    anyhow::anyhow!("CodeBuild runs no Linux fleet for {name} ({os} {arch})")
                })
            })
            .collect();
    }
    let Some(named) = named else {
        return Ok(vec![template::System::linux(
            "linux-x64",
            "linux",
            "x86_64",
        )
        .expect("linux")]);
    };
    let mut systems = Vec::new();
    for name in named.keys() {
        let (os, arch) = of(name).unwrap_or_default();
        match template::System::linux(name, &os, &arch) {
            Some(s) => systems.push(s),
            None => left.push(format!("{name} ({os} {arch})")),
        }
    }
    if systems.is_empty() {
        bail!("{POLICY_PATH}'s [systems] names no system CodeBuild runs on Linux");
    }
    Ok(systems)
}

/// The policy at `dir`, its text and its table.
fn read_policy(dir: &Path, command: &str) -> Result<(std::path::PathBuf, String, toml::Table)> {
    let path = dir.join(POLICY_PATH);
    let text = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "{} holds no policy; sluice init {command} adds the pool to one",
            path.display()
        )
    })?;
    let table = text
        .parse()
        .with_context(|| format!("reading {POLICY_PATH}"))?;
    Ok((path, text, table))
}

/// `sluice init codebuild` in the working copy at `dir` (design D10): the
/// template written for `systems`, else each its policy's `[systems]`
/// names that CodeBuild runs on Linux, each it leaves out said; one that
/// differs left unless `force`, saying how and exiting 1; and how to
/// deploy it, the secret to fill and the next step said.
pub fn codebuild(dir: &Path, systems: &[String], force: bool, out: &mut dyn Write) -> Result<i32> {
    let (_, _, policy) = read_policy(dir, "codebuild")?;
    let mut left = Vec::new();
    let systems = codebuild_systems(&policy, systems, &mut left)?;
    let repository = repository(dir)?;
    let wants = template::render(crate::VERSION, &repository, &systems);
    let path = dir.join(template::PATH);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("making {}", parent.display()))?;
    }
    let shown = template::PATH;
    let mut differing = false;
    match std::fs::read_to_string(&path) {
        Ok(had) if had == wants => writeln!(out, "{shown}: as sluice writes it")?,
        Ok(had) if !force => {
            writeln!(
                out,
                "{shown}: left as it is; {}; --force writes it",
                differs(&had, &wants)
            )?;
            differing = true;
        }
        _ => {
            std::fs::write(&path, &wants).with_context(|| format!("writing {shown}"))?;
            let names: Vec<&str> = systems.iter().map(|s| s.name.as_str()).collect();
            writeln!(
                out,
                "{shown}: written, a fleet and a run project for {}",
                names.join(", ")
            )?;
        }
    }
    for system in left {
        writeln!(
            out,
            "{shown}: CodeBuild runs no Linux fleet for {system}; it is left out"
        )?;
    }
    writeln!(out, "\nThen, in AWS:")?;
    writeln!(
        out,
        "  - deploy it, as `sluice` or a name of your own:\n    aws cloudformation deploy \
         --template-file {shown} --stack-name sluice --capabilities CAPABILITY_IAM \
         --parameter-overrides LeadPrincipal=<the ARN the hosts that lead the branch reach AWS \
         as> AppId=<sluice's app id> MaxCapacity=<the branch's parallel>"
    )?;
    writeln!(
        out,
        "  - fill the secret sluice-app-key with the app's private key:\n    aws \
         secretsmanager put-secret-value --secret-id sluice-app-key --secret-string \
         file://<the app's key>"
    )?;
    writeln!(
        out,
        "  - name the profile that assumes the stack's lead role as [aws] profile on each \
         host that leads the branch"
    )?;
    writeln!(
        out,
        "  - then `sluice init codebuild --stack sluice`, which names the pool in {POLICY_PATH}"
    )?;
    Ok(if differing { FAILED } else { 0 })
}

/// `sluice init codebuild --stack <stack>` in the working copy at `dir`
/// (design D10): the stack's key read by its alias with `GetPublicKey`,
/// through the operator's own AWS chain, `profile` where named, in
/// `region`, else the chain's; a key of another spec or usage refused;
/// and `[signers.codebuild]`, naming the ARN KMS answers and the public
/// half, and `[pools.codebuild]`, naming the region, the stack's sign
/// project and its run project for each system, appended as text where the
/// policy names neither, its own lines untouched.
pub fn codebuild_stack(
    dir: &Path,
    systems: &[String],
    stack: &str,
    region: Option<&str>,
    profile: Option<&str>,
    out: &mut dyn Write,
) -> Result<i32> {
    use crate::profiles::codebuild::aws::Aws as _;
    use crate::profiles::codebuild::sdk::{Chain, Sdk};
    let (path, text, policy) = read_policy(dir, "codebuild --stack")?;
    let named = |table: &str| {
        policy
            .get(table)
            .and_then(|t| t.as_table())
            .is_some_and(|t| t.contains_key("codebuild"))
    };
    match (named("signers"), named("pools")) {
        (true, true) => {
            writeln!(
                out,
                "{POLICY_PATH}: names [signers.codebuild] and [pools.codebuild] already"
            )?;
            return Ok(0);
        }
        (false, false) => {}
        _ => bail!(
            "{POLICY_PATH} names one of [signers.codebuild] and [pools.codebuild]; name both, or \
             neither for sluice init codebuild --stack to append them"
        ),
    }
    let mut left = Vec::new();
    let systems = codebuild_systems(&policy, systems, &mut left)?;
    let chain = Chain::of(&crate::instance::Aws {
        profile: profile.map(String::from),
    });
    let region = match region {
        Some(r) => r.to_string(),
        None => chain.region().ok_or_else(|| {
            anyhow::anyhow!("the AWS chain names no region: name the stack's with --region")
        })?,
    };
    let aws = Sdk::new(
        chain,
        &region,
        std::rc::Rc::new(crate::retry::Policy::command().recording_where_set()),
    );
    let alias = template::key_alias(stack);
    let key = aws
        .get_public_key(&alias)
        .with_context(|| format!("reading the stack {stack}'s key, {alias}, in {region}"))?;
    if key.spec != "ECC_NIST_P256" || key.usage != "SIGN_VERIFY" {
        bail!(
            "{alias} is a key of the spec {} and the usage {}; the stack's key signs as \
             ECC_NIST_P256 with the usage SIGN_VERIFY",
            key.spec,
            key.usage
        );
    }
    use base64::Engine as _;
    let public = base64::engine::general_purpose::STANDARD.encode(&key.der);
    crate::signing::kms::public_half(&public)
        .with_context(|| format!("the public half KMS answered for {alias}"))?;
    let mut appended = String::new();
    if !policy.contains_key("systems") {
        appended.push_str("\n[systems]\n");
        for s in &systems {
            let arch = match s.arch {
                template::Arch::X86_64 => "x86_64",
                template::Arch::Aarch64 => "aarch64",
            };
            appended.push_str(&format!(
                "{} = {{ os = \"linux\", arch = \"{arch}\" }}\n",
                s.name
            ));
        }
    }
    let names: Vec<String> = systems.iter().map(|s| format!("\"{}\"", s.name)).collect();
    let runners: Vec<String> = systems
        .iter()
        .map(|s| format!("{} = \"{}\"", s.name, template::run_project(stack, &s.name)))
        .collect();
    appended.push_str(&format!(
        "\n[signers.codebuild]\nkms = {{ key = \"{}\", public = \"{public}\" }}\n\
         systems = [{}]\n\
         \n[pools.codebuild]\ncodebuild = {{ region = \"{region}\", sign = \"{}\" }}\n\
         signer = \"codebuild\"\nrunners = {{ {} }}\n",
        key.arn,
        names.join(", "),
        template::sign_project(stack),
        runners.join(", ")
    ));
    let separator = if text.ends_with('\n') { "" } else { "\n" };
    let whole = format!("{text}{separator}{appended}");
    sluice_core::policy::Policy::parse(&whole)
        .with_context(|| format!("{POLICY_PATH} with the pool appended"))?;
    std::fs::write(&path, whole).with_context(|| format!("appending to {POLICY_PATH}"))?;
    writeln!(
        out,
        "{POLICY_PATH}: [signers.codebuild], naming the key {}, and [pools.codebuild] appended; \
         a branch's `trust` names codebuild and the signer its leading hosts sign as, that \
         signer's `systems = []` so it claims no run, and the branch's `systems` the systems \
         its runs need",
        key.arn
    )?;
    for system in left {
        writeln!(
            out,
            "{POLICY_PATH}: CodeBuild runs no Linux fleet for {system}; the pool runs none of it"
        )?;
    }
    writeln!(
        out,
        "\nThen `sluice pool check <branch>` reads the pool back. A branch on the fleet tests \
         side by side: batch = false, and parallel the fleets' maximum."
    )?;
    Ok(0)
}
