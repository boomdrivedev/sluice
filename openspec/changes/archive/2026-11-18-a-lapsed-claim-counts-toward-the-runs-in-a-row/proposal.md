## Why

A pool's job that ended with no record of its run was dispatched again
with no end. On 2026-11-18, the GitHub Actions pool's trial,
boomdrivedev/sluice-actions-trial on sluice 0.97.1, lost `pool/forge/1`:
its run job started, and the program it ran could not start on the
runner. Its claim lapsed (88, 77), the run had no outcome (60), and its
checks were dispatched again as `pool/forge/3`, 4 and 5 until the
submission was withdrawn. Each dispatch spends the pool's minutes.

Only 114's causes counted toward the runs in a row (115), and the time
limit (66) counts from each run's own running, so nothing bounded it. A
standing executor that stops on the same run each time loops the same
way.

Clause 115 as amended 2026-11-18 (boomdrivedev/blueprints a64bcef) and
record 0031 as amended 2026-11-18 decide it: a run whose claim lapsed
counts toward the runs in a row.

## What Changes

- **A lapsed claim counts toward the runs in a row.** A run whose claim
  lapsed, its standing executor stopping before it reported or its pool
  losing, cancelling or timing out its job before the run's record was
  written, counts as a run with no outcome for 114's causes does (115).
  Under the bound its follow-up holds its checks, as before. At the
  bound the combination reads not passed and the culprit's session is
  told each check and why. Nothing of a lapsed run is recorded under a
  key.
- **A lost job on GitHub Actions names its workflow run.** The pool's
  why names the workflow run whose log GitHub keeps, which the culprit's
  session is told (155).
- **Unchanged.** A run whose place could not be made counts toward
  nothing (core.58; record 0118). The time limit (66) counts as before.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `run-life`: runs in a row count a lapsed claim beside 114's causes.
- `actions-pool`: a lost job names its workflow run and counts toward
  the runs in a row.

## Out of scope

- **A CodeBuild pool's whys.** They already name the run build or the
  sign build, whose log CodeBuild keeps with it.
- **Clause 155's kept output.** Keeping a pool job's output where C.6
  keeps a run's is a change of its own; this one names where the pool
  keeps it.

## Impact

- `crates/sluice-core/src/step.rs`: the lapse and the lost run counted
  through `count_not_run` and `lapsed_end`.
- `crates/sluice/src/profiles/github/actions.rs`: `job` gives the
  workflow run's address as its why.
- Tests: `crates/sluice-conformance/tests/run_life.rs`
  (`a_pools_jobs_lost_in_a_row_read_the_combination_not_passed`),
  `crates/sluice-conformance/tests/verdicts.rs`
  (`a_run_whose_executor_stops_three_runs_in_a_row_is_blamed_with_why`),
  `crates/sluice/tests/profiles/forge.rs` (`the_pools_calls`).
- Records and rules: record 0031 as amended 2026-11-18, on its own
  branch; rule core.65.
- Sources: clauses 60, 66, 77, 88, 115 (as amended 2026-11-18), 155;
  record 0118.

## Touches

```
crates/sluice-core/src/step.rs
crates/sluice/src/profiles/github/actions.rs
crates/sluice-conformance/tests/run_life.rs
crates/sluice-conformance/tests/verdicts.rs
crates/sluice/tests/profiles/forge.rs
docs/architecture/core.md
openspec/changes/a-lapsed-claim-counts-toward-the-runs-in-a-row/**
```
