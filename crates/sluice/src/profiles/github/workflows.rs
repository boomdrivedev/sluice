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

/// `sluice.yml` as sluice `version` writes it.
pub fn sluice(version: &str) -> String {
    SLUICE_TEMPLATE.replace("{{version}}", version)
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
        let w = sluice("1.2.3");
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
            "if: always()",
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
        for line in w.lines().filter(|l| l.contains("uses: ")) {
            let at = line.split('@').nth(1).unwrap_or_default();
            assert!(
                at.len() >= 40 && at[..40].bytes().all(|b| b.is_ascii_hexdigit()),
                "pinned by commit: {line}"
            );
        }

        let runners = BTreeMap::from([
            ("linux-x64".to_string(), "ubuntu-24.04".to_string()),
            ("macos-arm64".to_string(), "macos-15".to_string()),
        ]);
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
}
