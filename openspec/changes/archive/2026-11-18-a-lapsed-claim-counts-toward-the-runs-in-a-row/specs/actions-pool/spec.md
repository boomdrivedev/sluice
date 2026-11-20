## MODIFIED Requirements

### Requirement: A pool's run is running from its run job's start

A pool's run SHALL be written `running` by the lead from the run job's
own start as the forge gives it, its limit counting from then, and
`ended` and `closed` by its sign job under its claim. A workflow run the
pool lost, cancelled or timed out before its sign job wrote the run's
end SHALL be a lapsed claim, whose run ends with no outcome naming the
workflow run, where GitHub keeps its log, and counts toward the runs in
a row (88, 77, 115; run-life).

#### Scenario: A run job starts

- **WHEN** the run job of `pool/forge/<n>`'s workflow run starts on
  `macos-15`
- **THEN** the run reads `running` from that start, and its end is
  written by the sign job
  (`a_pools_run_is_running_from_its_run_jobs_start` in
  `crates/sluice-conformance/tests/run_life.rs`)

#### Scenario: A cancelled workflow run

- **WHEN** someone cancels the workflow run while its run job runs, the
  first of its combination's runs to lapse
- **THEN** the run ends with no outcome, saying its job ended with no
  record, nothing of it is recorded, nobody is blamed, and a follow-up
  run holds its checks
  (`a_pools_job_ended_with_no_record_is_a_lapsed_claim` in
  `crates/sluice-conformance/tests/run_life.rs`)

#### Scenario: A lost job names its workflow run

- **WHEN** `pool/forge/11`'s workflow run completes with its sign job
  skipped
- **THEN** its job reads ended with no record, naming the workflow run's
  address, where GitHub keeps its log
  (`the_pools_calls` in `crates/sluice/tests/profiles/forge.rs`)
