## MODIFIED Requirements

### Requirement: A pool's job releases its checkout before it ends

`sluice job run` SHALL keep the checkout it made for the job's own
steps, naming it in `--checkout`'s file, or, stopped, release it at
once. `sluice job release` SHALL release it after the repository's own
steps, on every pool, whatever they came to, by the release the policy
names, freeing its ports and scratch. A release that fails SHALL be said
in its output and the hand-over's `why`, exiting 1, and decide nothing
of the run's outcome (core.10; record 0016; 123).

#### Scenario: A run that passed

- **WHEN** `sluice job run --claim pool/codebuild/4 --branch main
  --checkout <file>` keeps its run's report
- **THEN** the checkout stands with its ports and scratch held, its
  release has not run, and `<file>` names it
  (`job_run_keeps_its_checkout_for_the_jobs_own_steps` in
  `crates/sluice/tests/cli.rs`)

#### Scenario: The release after the repository's own steps

- **WHEN** `sluice job release --checkout <file> --out <dir>` runs after
  that run, and `main`'s policy names a release command
- **THEN** the release command has run in the checkout, the checkout is
  gone, its ports and scratch are free and `<file>` is gone; and where
  the release fails, it is said in the output and in `<dir>/why`, the
  report stands, and the command exits 1
  (`job_release_releases_the_kept_checkout` in
  `crates/sluice/tests/cli.rs`)

#### Scenario: Nothing kept

- **WHEN** `sluice job release --checkout <file>` runs where `<file>`
  does not exist
- **THEN** it releases nothing, says so and exits 0
  (`job_release_with_nothing_kept_releases_nothing` in
  `crates/sluice/tests/cli.rs`)

#### Scenario: A job stopped while its run goes

- **WHEN** `sluice job run` is stopped while its run's checks run
- **THEN** it ends the run's process group, runs the release, says it
  kept no report, writes no `--checkout` file, and exits
  (`job_run_stopped_releases_its_checkout` in
  `crates/sluice/tests/cli.rs`)

#### Scenario: The templates release last

- **WHEN** `sluice init actions` writes its workflows and `sluice init
  codebuild` its template
- **THEN** each run job runs `sluice job release` after the repository's
  own steps, `if: always()`, and before the hand-over's upload, and each
  run buildspec runs it in its `post_build` phase's `finally` block
  (`each_run_job_leaves_room_for_the_repositorys_own_steps` in
  `crates/sluice/src/profiles/github/workflows.rs`;
  `each_run_build_leaves_room_for_the_repositorys_own_commands` in
  `crates/sluice/src/profiles/codebuild/template.rs`)

### Requirement: A reused machine's next job starts with nothing of an earlier job's running

A pool whose machines run one job at a time and are reused SHALL have
each job begin by ending every container the machine's engine runs and
removing the sluice scratch and state, and the file naming a kept
checkout, an earlier job left, before it makes its checkout, so a job
its pool stopped before its release leaves nothing running beside the
next (record 0119; core.10).

#### Scenario: The run buildspec

- **WHEN** `sluice init codebuild` writes its template
- **THEN** each run project's buildspec, before `sluice job run`, ends
  every running container and removes the build user's sluice scratch
  directory, the state an earlier build kept and the file naming its
  kept checkout, keeping the engine's images
  (`a_run_build_starts_with_nothing_of_an_earlier_build_running` in
  `crates/sluice/src/profiles/codebuild/template.rs`)
