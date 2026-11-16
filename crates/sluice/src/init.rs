//! `sluice init actions` (record 0119; design D14 of the change
//! `a-runs-life-is-written-first-and-actions-runs-its-checks`): the pool
//! on GitHub Actions set up in a working copy: both workflows written, the
//! signer and the pool appended to the policy where it names no pool on
//! GitHub, and what to set on GitHub said. It writes files and nothing
//! else; they land as any change does.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use anyhow::{bail, Context, Result};
use sluice_core::policy::POLICY_PATH;

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
    },
}

/// The pool `policy` names on GitHub, or the tables to append for the
/// `forge` signer and pool: the signer's workflow at `git_ref`, the pool's
/// runners for each system `[systems]` names that GitHub hosts, and, where
/// it names none, a `linux-x64` system appended with them (85).
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
    let systems = table.get("systems").and_then(|s| s.as_table());
    for (name, s) in systems.into_iter().flatten() {
        let field = |k: &str| s.get(k).and_then(|v| v.as_str()).unwrap_or_default();
        if let Some(runner) = hosted_runner(field("os"), field("arch")) {
            runners.insert(name.clone(), runner.to_string());
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
    Ok(Pool::Appended { text, runners })
}

/// The ref the signer's workflow runs from: the remote's default branch as
/// the working copy holds it, else `main`'s.
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
        (workflows::SLUICE, workflows::sluice(version)),
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
        match std::fs::read_to_string(&path) {
            Ok(had) if had == *wants => writeln!(out, "{shown}: as sluice writes it")?,
            Ok(had) if !force => {
                writeln!(
                    out,
                    "{shown}: left as it is; {}; --force writes it",
                    differs(&had, wants)
                )?;
                left = true;
            }
            _ => {
                std::fs::write(&path, wants).with_context(|| format!("writing {shown}"))?;
                writeln!(out, "{shown}: written")?;
            }
        }
    }
    match pool {
        Pool::Named(_) => writeln!(out, "{POLICY_PATH}: names its pool on GitHub already")?,
        Pool::Appended { text, .. } => {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&policy_path)
                .with_context(|| format!("appending to {POLICY_PATH}"))?;
            if !policy.ends_with('\n') {
                file.write_all(b"\n")?;
            }
            file.write_all(text.as_bytes())?;
            writeln!(
                out,
                "{POLICY_PATH}: [signers.forge] and [pools.forge] appended; a branch's \
                 `systems` names the systems its runs need, and `trust` names forge"
            )?;
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
