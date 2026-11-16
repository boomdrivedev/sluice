## MODIFIED Requirements

### Requirement: A combination whose tests could not run is tested again

When a combination's run leaves tests not run and fails no test on its
own result, sluice SHALL test those tests again in a follow-up run of
the same system that holds them alone, keep every other test's outcome,
and blame no submission (run-life). When its tests could not run in
three runs in a row, a run and its follow-ups, sluice SHALL read the
combination as failed and blame it as any failure. The submitter SHALL
be told each test that could not run, and why.

#### Scenario: Once

- **WHEN** a combination's run passes `core:test`, leaves `cli:tools`
  not run and nothing failed
- **THEN** a follow-up run holds `cli:tools` alone, `core:test` keeps
  its pass, and no submission is blamed
  (`a_check_that_could_not_run_records_nothing_and_runs_again` in
  `crates/sluice-conformance/tests/could_not_run.rs`)

#### Scenario: Three times in a row

- **WHEN** the same combination leaves `cli:tools` not run in three runs
  in a row
- **THEN** its newest submission is blamed with `cli:tools` could not
  run, and the reason

#### Scenario: A test that failed on its own

- **WHEN** a run leaves `cli:e2e` not run and `core:test` failed
- **THEN** the combination fails on `core:test` as before, and
  `cli:e2e` is named as not run

### Requirement: A test the time limit ended has no result

When a branch's time limit ends a running run, its limit counted from the
run's own `running` (run-life), sluice SHALL keep the result of every
test whose end the task runner recorded before the limit, and SHALL
record no result under the key of a test whose end it did not, whatever
the runner reported of that test (time-limit-1 66). It SHALL name each
such test as left without a result, keep its log, and read the
submission as timed out. A run that never ran is never timed out (87;
run-life).

#### Scenario: A test running when the limit came

- **WHEN** a combination's run passes `a:test` and is ended by the time
  limit while `b:test` runs
- **THEN** `results.jsonl` gains a pass for `a:test` and no line for
  `b:test`, and the submission is blamed as timed out with `b:test`
  named and its log kept

#### Scenario: A test that caught the signal and exited

- **WHEN** moon reports a test failed with the exit it took on the
  signal, and its run record is from after the limit
- **THEN** the test has no result and is named as the limit ended it

#### Scenario: A test that failed before the limit

- **WHEN** a test fails and its run record is from before the limit, in
  a run the limit then ended
- **THEN** its fail is recorded under its key, as before

#### Scenario: The same head once the host keeps up

- **WHEN** the same head is submitted again after the limit ended its
  run, and `b:test` passes
- **THEN** the submission lands, and no test reads as flaky

#### Scenario: A run that waited before it ran

- **WHEN** a run waited for room longer than the branch's time limit and
  then ran within it
- **THEN** it is not timed out
  (`a_run_that_waited_for_room_has_its_whole_limit` in
  `crates/sluice-conformance/tests/run_life.rs`)
