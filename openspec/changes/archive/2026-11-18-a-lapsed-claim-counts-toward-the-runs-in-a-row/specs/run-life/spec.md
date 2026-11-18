## MODIFIED Requirements

### Requirement: A combination's verdict is read over a run and its follow-ups

A combination's verdict SHALL be read over each run and its follow-ups,
each check by its last outcome, and a fail SHALL be read at once,
whatever is left undecided beside it.

#### Scenario: A fail beside an unproven replay

- **WHEN** a run fails `a:test` and replays `b:test` unproven
- **THEN** the combination fails on `a:test` at once, and no follow-up
  run is opened (`a_fail_beside_an_unproven_replay_is_read_at_once` in
  `crates/sluice-conformance/tests/run_life.rs`)

## ADDED Requirements

### Requirement: Runs in a row count a lapsed claim beside 114's causes

A run that left checks with no outcome for 114's causes, or whose claim
lapsed, its executor stopping before it reported or its pool losing its
job before the run's record was written, SHALL count toward the runs in
a row whose checks could not run. At the deployment's count the
combination SHALL read not passed, each check named with why, and
nothing of a lapsed run SHALL be recorded under a key (60, 77, 88, 115;
record 0031 as amended 2026-11-18).

#### Scenario: A pool's job lost in runs in a row

- **WHEN** the pool's job of `main + #1`'s run, and of each follow-up,
  ends with no record of its run, three runs in a row
- **THEN** no job is dispatched for it again, `main + #1` fails, and #1
  is told each check with the job's words, nothing recorded under a key
  (`a_pools_jobs_lost_in_a_row_read_the_combination_not_passed` in
  `crates/sluice-conformance/tests/run_life.rs`)

#### Scenario: A standing executor that stops in runs in a row

- **WHEN** the executor of `main + #1`'s run, and of each follow-up,
  stops before it reports, three runs in a row
- **THEN** no run of it starts again, and #1 is blamed, each check named
  as its executor stopping before it reported
  (`a_run_whose_executor_stops_three_runs_in_a_row_is_blamed_with_why`
  in `crates/sluice-conformance/tests/verdicts.rs`)
