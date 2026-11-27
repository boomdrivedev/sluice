## MODIFIED Requirements

### Requirement: A check the report holds no outcome for has none

A run's report SHALL give an outcome for each check of the run. A check
that it holds no outcome for SHALL have no outcome, saying that the
report holds none for it. A pass, a fail, a refusal, and the runner's
own word that it keeps the check out of its CI are each an outcome.
Under the bound, sluice SHALL NOT write the check failed or blame anyone
for it (60, 114, rule core.22). A run that decided nothing SHALL count
toward the runs in a row (115).

#### Scenario: A report holding one check of 39

- **WHEN** a run of 39 checks ends and its report holds a pass of one
- **THEN** sluice records the pass, and the other 38 have no outcome,
  each saying the report holds none for it. #103 is not blamed, and a
  follow-up run holds the 38
  (`a_check_the_report_holds_no_outcome_for_is_never_failed` in
  `crates/sluice-conformance/tests/run_life.rs`)

#### Scenario: Three runs in a row

- **WHEN** the report of `main + #1`'s run, and of each follow-up, holds
  no outcome for `b:test`, three runs in a row
- **THEN** no fourth run starts, and `main + #1` is not passed. sluice
  blames #1 and names `b:test` with why, the run's report holds no
  outcome for it. Nothing is recorded under its key
  (`a_check_no_report_holds_three_runs_in_a_row_is_blamed_with_why` in
  `crates/sluice-conformance/tests/run_life.rs`)
