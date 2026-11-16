## ADDED Requirements

### Requirement: A pool's job releases its checkout before it ends

`sluice job run` SHALL release the checkout it made, by the release the
policy names, freeing its ports and scratch, before it exits, whatever
its run came to and when it is stopped, on every pool. A release that
fails SHALL be said in its output and its hand-over, and SHALL decide
nothing of the run's outcome (core.10; record 0016; 123).

#### Scenario: A run that passed

- **WHEN** `sluice job run --claim pool/codebuild/4 --branch main` keeps
  its run's report, and `main`'s policy names a release command
- **THEN** the release command has run in the checkout, the checkout is
  gone, and its ports and scratch are free before the command exits
  (`job_run_releases_its_checkout_before_it_exits` in
  `crates/sluice/tests/cli.rs`)

#### Scenario: A job stopped while its run goes

- **WHEN** `sluice job run` is stopped while its run's checks run
- **THEN** it ends the run's process group, runs the release, says it
  kept no report, and exits
  (`job_run_stopped_releases_its_checkout` in
  `crates/sluice/tests/cli.rs`)

### Requirement: A reused machine's next job starts with nothing of an earlier job's running

A pool whose machines run one job at a time and are reused SHALL have
each job begin by ending every container the machine's engine runs and
removing the sluice scratch and state an earlier job left, before it
makes its checkout, so a job its pool stopped before its release leaves
nothing running beside the next (record 0119; core.10).

#### Scenario: The run buildspec

- **WHEN** `sluice init codebuild` writes its template
- **THEN** each run project's buildspec, before `sluice job run`, ends
  every running container and removes the build user's sluice scratch
  directory and the state an earlier build kept, keeping the engine's
  images
  (`a_run_build_starts_with_nothing_of_an_earlier_build_running` in
  `crates/sluice/src/profiles/codebuild/template.rs`)
