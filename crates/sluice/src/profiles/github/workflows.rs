//! The workflows `sluice init actions` writes (record 0119; design D11 of
//! the change `a-runs-life-is-written-first-and-actions-runs-its-checks`):
//! `sluice.yml`, a pool's run in two jobs, a run job on the runner its
//! system names that holds no key, no identity token and no secret, and a
//! sign job that runs none of the combination's code, signs the keys its
//! tree bears out with the job's own identity and records the run's end;
//! and `sluice-pr.yml`, a pull request's head run on each system the pool's
//! runners name. Each is rendered with the release that writes it, which
//! its jobs install.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::own_steps::{self, Carry, Layout, OwnLine, Reading};

/// Where the workflows live in a repository.
pub const DIR: &str = ".github/workflows";

/// The pool's workflow, which a dispatch names.
pub const SLUICE: &str = "sluice.yml";

/// The pull requests' workflow.
pub const SLUICE_PR: &str = "sluice-pr.yml";

const SLUICE_TEMPLATE: &str = include_str!("workflows/sluice.yml");
const SLUICE_PR_TEMPLATE: &str = include_str!("workflows/sluice-pr.yml");

/// The line of `sluice-pr.yml`'s template that stands for the pool's
/// runners, one entry per system.
const RUNNERS_LINE: &str =
    "        include: [{system: linux-x64, runner: ubuntu-24.04}] # the pool's runners";

/// The line of `sluice.yml`'s template that stands for the pool's systems,
/// whose head runs its `sign-head` job signs one each.
const SYSTEMS_LINE: &str = "        system: [linux-x64] # the pool's systems";

/// The lines that open and close a run job's room for the repository's
/// own steps (record 0119 as amended 2026-11-18; design D1 of the change
/// `a-pools-run-job-runs-the-repositorys-own-steps`).
pub const ROOM_OPEN: &str = "# sluice: your own steps, after the checks, in $SLUICE_CHECKOUT";
pub const ROOM_CLOSE: &str = "# sluice: end of your own steps";

/// The job a workflow's line at `at` is in: the nearest job's line above
/// it, a key of `jobs:`.
fn job_of(lines: &[&str], at: usize) -> String {
    lines[..at]
        .iter()
        .rev()
        .find_map(|l| {
            let name = l.strip_prefix("  ")?.trim_end().strip_suffix(':')?;
            (!name.starts_with([' ', '#']) && !name.contains(' ')).then(|| name.to_string())
        })
        .unwrap_or_default()
}

/// The steps of the job `job`, which its room's entries join.
fn steps_of(job: &str) -> Vec<own_steps::Seg> {
    own_steps::keys(&["jobs", job, "steps"])
}

/// What a workflow's rooms are, and its run job's `packages: write`, the
/// line the repository may add directly after `contents: read` in the run
/// job's `permissions` for a store on GitHub's registry (design D4; timber
/// C.1; rule core.62).
fn layout() -> Layout<'static> {
    Layout {
        open: ROOM_OPEN,
        close: ROOM_CLOSE,
        key: &job_of,
        list: &steps_of,
        own_line: Some(OwnLine {
            line: "packages: write",
            after: "contents: read",
            key: "run",
            at: own_steps::keys(&["jobs", "run", "permissions"]),
        }),
    }
}

/// `had`, a workflow, read against `wants`, what this sluice writes it as
/// (design D3): as sluice writes it, with or without the repository's own
/// steps; differing; or the repository's own steps leaving the run job's.
pub fn read(had: &str, wants: &str) -> Result<Reading> {
    own_steps::read(had, wants, &layout())
}

/// What a workflow as sluice writes it holds of the repository's own, as
/// said after "as sluice writes it" or "written" (design D3).
pub fn holds(reading: &Reading) -> &'static str {
    match reading {
        Reading::AsWritten { rooms, own_line } => match (rooms.is_empty(), *own_line) {
            (true, false) => "",
            (false, false) => ", with the repository's own steps",
            (true, true) => ", with the run job's packages: write",
            (false, true) => ", with the repository's own steps and the run job's packages: write",
        },
        _ => "",
    }
}

/// `wants` with the repository's own steps in `had` carried into its run
/// job's room, by the job, and its run job's `packages: write` (design D3).
pub fn carry(had: &str, wants: &str) -> Result<Carry> {
    own_steps::carry(had, wants, &layout())
}

/// `sluice.yml` as sluice `version` writes it, its `sign-head` job signing
/// a pull request's head run on each system of the pool's `runners`.
pub fn sluice(version: &str, runners: &BTreeMap<String, String>) -> String {
    let systems: Vec<&str> = runners.keys().map(String::as_str).collect();
    let line = format!(
        "        system: [{}] # the pool's systems",
        systems.join(", ")
    );
    SLUICE_TEMPLATE
        .replace(SYSTEMS_LINE, &line)
        .replace("{{version}}", version)
}

/// `sluice-pr.yml` as sluice `version` writes it, one run job for each of
/// the pool's `runners`, by the system it names.
pub fn sluice_pr(version: &str, runners: &BTreeMap<String, String>) -> String {
    let entries: Vec<String> = runners
        .iter()
        .map(|(system, runner)| format!("{{system: {system}, runner: {runner}}}"))
        .collect();
    let line = format!(
        "        include: [{}] # the pool's runners",
        entries.join(", ")
    );
    SLUICE_PR_TEMPLATE
        .replace(RUNNERS_LINE, &line)
        .replace("{{version}}", version)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each top-level job of a workflow's `jobs:`, by its name: its lines.
    fn jobs(workflow: &str) -> BTreeMap<String, String> {
        let mut jobs = BTreeMap::new();
        let Some((_, body)) = workflow.split_once("\njobs:\n") else {
            return jobs;
        };
        let mut name = None;
        let mut lines = String::new();
        for line in body.lines() {
            match line.strip_prefix("  ").filter(|l| !l.starts_with(' ')) {
                Some(head) if head.ends_with(':') => {
                    if let Some(n) = name.take() {
                        jobs.insert(n, std::mem::take(&mut lines));
                    }
                    name = Some(head.trim_end_matches(':').to_string());
                }
                _ => {
                    lines.push_str(line);
                    lines.push('\n');
                }
            }
        }
        if let Some(n) = name {
            jobs.insert(n, lines);
        }
        jobs
    }

    /// Record 0119 (design D11; rule core.62): the run job runs the claimed
    /// run on the runner its system names, reading the repository alone:
    /// no environment, no identity token, no attestation and no secret; the
    /// sign job follows it whatever it came to, in the `sluice`
    /// environment, with the identity token and attestations, signing with
    /// a pinned `actions/attest` and running none of the combination's
    /// checks. The pull requests' workflow runs a job per runner with
    /// neither, and both are rendered with the release that writes them.
    #[test]
    fn the_run_job_holds_no_identity_and_the_sign_job_runs_no_check() {
        let runners = BTreeMap::from([
            ("linux-x64".to_string(), "ubuntu-24.04".to_string()),
            ("macos-arm64".to_string(), "macos-15".to_string()),
        ]);
        let w = sluice("1.2.3", &runners);
        assert!(w.contains("run-name: sluice ${{ inputs.claim"), "{w}");
        for input in ["claim:", "branch:", "combination:", "system:", "runner:"] {
            assert!(w.contains(&format!("      {input}")), "input {input}");
        }
        assert!(w.contains("workflows: [sluice-pr]"));
        assert!(!w.contains("{{version}}"));
        assert!(w.contains("SLUICE_VERSION: \"1.2.3\""));
        let j = jobs(&w);
        let run = &j["run"];
        assert!(run.contains("runs-on: ${{ inputs.runner }}"), "{run}");
        assert!(run.contains("contents: read"));
        for held in ["environment:", "id-token", "attestations", "secrets."] {
            assert!(!run.contains(held), "the run job holds {held}: {run}");
        }
        assert!(run.contains("sluice job run --claim"));
        let sign = &j["sign"];
        for needed in [
            "needs: run",
            "if: ${{ !cancelled() && github.event_name == 'workflow_dispatch' }}",
            "environment: sluice",
            "id-token: write",
            "attestations: write",
            "actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6",
            "sluice job sign",
            "sluice job record",
        ] {
            assert!(sign.contains(needed), "the sign job lacks {needed}: {sign}");
        }
        assert!(!sign.contains("sluice job run"), "{sign}");
        // A pull request's head run is signed on each of the pool's
        // systems, as the sign job signs a claimed run.
        let head = &j["sign-head"];
        for needed in [
            "if: github.event_name == 'workflow_run'",
            "system: [linux-x64, macos-arm64]",
            "environment: sluice",
            "id-token: write",
            "sluice job sign --head",
            "sluice job record --head",
        ] {
            assert!(head.contains(needed), "sign-head lacks {needed}: {head}");
        }
        assert!(!head.contains("sluice job run"), "{head}");
        for line in w.lines().filter(|l| l.contains("uses: ")) {
            let at = line.split('@').nth(1).unwrap_or_default();
            assert!(
                at.len() >= 40 && at[..40].bytes().all(|b| b.is_ascii_hexdigit()),
                "pinned by commit: {line}"
            );
        }

        let pr = sluice_pr("1.2.3", &runners);
        assert!(pr.contains(
            "include: [{system: linux-x64, runner: ubuntu-24.04}, {system: macos-arm64, runner: macos-15}]"
        ));
        assert!(pr.contains("on: pull_request"));
        let run = &jobs(&pr)["run"];
        for held in ["environment:", "id-token", "attestations", "secrets."] {
            assert!(!run.contains(held), "the head's run job holds {held}");
        }
        assert!(run.contains("sluice job run --head"));
    }

    /// Record 0119 as amended 2026-11-18 (design D1, D2 and D6 of the
    /// change `a-pools-run-job-runs-the-repositorys-own-steps`; timber
    /// 135; rule core.63): each run job, `sluice.yml`'s and
    /// `sluice-pr.yml`'s, runs `sluice job run` naming its checkout, then
    /// names it as `SLUICE_CHECKOUT` unless cancelled, then leaves its room,
    /// the two `# sluice:` lines with nothing between, at the steps'
    /// indentation, then releases the checkout and uploads the hand-over,
    /// each whatever came before.
    #[test]
    fn each_run_job_leaves_room_for_the_repositorys_own_steps() {
        let runners = BTreeMap::from([("linux-x64".to_string(), "ubuntu-24.04".to_string())]);
        for (name, w) in [
            ("sluice.yml", sluice("1.2.3", &runners)),
            ("sluice-pr.yml", sluice_pr("1.2.3", &runners)),
        ] {
            let run = &jobs(&w)["run"];
            let lines: Vec<&str> = run.lines().collect();
            let at = |what: &dyn Fn(&str) -> bool, said: &str| {
                lines
                    .iter()
                    .position(|l| what(l))
                    .unwrap_or_else(|| panic!("{name}'s run job has no {said}: {run}"))
            };
            let job_run = at(
                &|l| {
                    l.contains("sluice job run")
                        && l.contains("--checkout \"$RUNNER_TEMP/sluice-checkout\"")
                },
                "sluice job run --checkout",
            );
            let named = at(
                &|l| l.contains("echo \"SLUICE_CHECKOUT="),
                "step naming SLUICE_CHECKOUT",
            );
            let step = lines[..named]
                .iter()
                .rposition(|l| l.starts_with("      - "))
                .unwrap_or_else(|| panic!("{name}: no step names SLUICE_CHECKOUT: {run}"));
            assert!(
                lines[step..named]
                    .iter()
                    .any(|l| l.trim() == "if: ${{ !cancelled() }}"),
                "{name}: SLUICE_CHECKOUT is named unless cancelled: {run}"
            );
            let open = at(
                &|l| l == "      # sluice: your own steps, after the checks, in $SLUICE_CHECKOUT",
                "room's opening line",
            );
            assert_eq!(
                lines[open + 1],
                "      # sluice: end of your own steps",
                "{name}: nothing between the room's lines"
            );
            let release = at(
                &|l| {
                    l.contains("sluice job release --checkout \"$RUNNER_TEMP/sluice-checkout\"")
                        && l.contains("--out \"$RUNNER_TEMP/sluice-run\"")
                },
                "sluice job release",
            );
            assert_eq!(
                lines[release - 1].trim(),
                "if: always()",
                "{name}: the release runs whatever came before"
            );
            let upload = at(&|l| l.contains("actions/upload-artifact@"), "upload");
            assert_eq!(lines[upload + 1].trim(), "if: always()", "{name}");
            assert!(
                job_run < named && named < open && open < release && release < upload,
                "{name}: in order: {run}"
            );
            assert!(
                run.contains("    permissions:\n      contents: read\n    steps:"),
                "{name}"
            );
        }
    }
}
