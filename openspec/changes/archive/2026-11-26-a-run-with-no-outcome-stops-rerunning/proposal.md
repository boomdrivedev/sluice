## Why

On 2026-11-26, boomdrive's `main` held #40 under test for more than 30
minutes. `sluice status main` showed "starting on deneb" for all that
time, and #40 had `runs_with_no_outcome` 24. A run started about each
20 seconds.

v0.110.3 fixed the cause of each empty run: a follow-up run now runs
each check that it holds. But the loop had a second fault. A check that
the run's report holds no outcome for has no outcome. sluice opens a
follow-up run for it, and that run counted toward nothing (record 0122,
design D8). Thus, when the report holds no outcome for the check again
and again, the follow-up runs have no end.

This case broke the claim
`a-candidate-with-no-outcome-in-runs-in-a-row-reads-not-passed`. By that
claim, a candidate reads as not passed when its checks have no outcome
in the deployment's count of runs in a row.

## What Changes

- A check that the run's report holds no outcome for counts toward the
  runs in a row whose checks could not run (115). A check that the run
  left not run counts the same way.
- Under the bound, the check has no outcome, and sluice blames nobody. A
  follow-up run holds the check, as before.
- At the bound, the third such run in a row, the combination reads not
  passed. sluice blames its newest submission and names the check with
  why: "the run's report holds no outcome for it". No fourth run starts.
- Beside a check that failed on its own result, such a check keeps no
  outcome, and sluice blames nobody for it, as before.
- A run of every check can end with a report that holds no task. Its
  group of checks then has no outcome with the same reason, and the run
  counts the same way.

## Capabilities

### Modified Capabilities

- `run-life`: a check that the report holds no outcome for counts toward
  the runs in a row.

## Impact

- `crates/sluice-core/src/step.rs`: `finish_run` reads the checks that
  the report holds nothing for with the checks that the run left not
  run.
- `crates/sluice-conformance/tests/run_life.rs`: the test of three runs
  in a row whose report holds no outcome for a check.
