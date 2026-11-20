## Purpose

The repository's own steps after a pool's checks: the room each run job and run build leaves for them between two `# sluice:` lines, the checkout they run in before the job releases it, how setting up keeps them, and how every reading of the setup accepts them while keeping them among the run job's steps (timber 135; record 0119).

## ADDED Requirements

### Requirement: Each run job and run build leaves room for the repository's own steps

The run jobs of `sluice.yml` and `sluice-pr.yml`, and each run project's
buildspec in the template `sluice init codebuild` writes, SHALL carry two
`# sluice:` lines after the checks and before the checkout's release,
between which the repository writes its own steps or commands. sluice
SHALL write nothing between them and name no tool there (timber 135).

#### Scenario: The workflows

- **WHEN** `sluice init actions` writes `sluice.yml` and `sluice-pr.yml`
- **THEN** each run job holds `# sluice: your own steps, after the
  checks, in $SLUICE_CHECKOUT` and `# sluice: end of your own steps`,
  with nothing between, after the step that runs `sluice job run` and
  the step that names `SLUICE_CHECKOUT`, and before the step that runs
  `sluice job release`
  (`each_run_job_leaves_room_for_the_repositorys_own_steps` in
  `crates/sluice/src/profiles/github/workflows.rs`)

#### Scenario: The buildspec

- **WHEN** `sluice init codebuild --systems linux-x64` writes its
  template
- **THEN** the run project's buildspec holds `# sluice: your own
  commands, after the checks, in $SLUICE_CHECKOUT` and `# sluice: end of
  your own commands`, with nothing between, among its `post_build`
  phase's commands after the one that exports `SLUICE_CHECKOUT`, and its
  `finally` block runs `sluice job release`
  (`each_run_build_leaves_room_for_the_repositorys_own_commands` in
  `crates/sluice/src/profiles/codebuild/template.rs`)

### Requirement: The repository's own steps run in the checkout the checks ran in

`sluice job run` SHALL name the checkout it kept in the file `--checkout`
gives, and the job SHALL name it to the repository's own steps as
`SLUICE_CHECKOUT`, holding what the checks left, until `sluice job
release` releases it after them (timber 135, 139; 88).

#### Scenario: A kept checkout

- **WHEN** `sluice job run --claim pool/forge/4 --branch main --out
  <dir> --checkout <file>` keeps its run's report
- **THEN** `<file>`'s first line is the path of the checkout the run's
  checks ran in, which still holds what they wrote, and its release has
  not run
  (`job_run_keeps_its_checkout_for_the_jobs_own_steps` in
  `crates/sluice/tests/cli.rs`)

### Requirement: Setting up keeps the repository's own steps

`sluice init actions` and `sluice init codebuild` SHALL read a file that
differs from what they write only between its `# sluice:` lines as
written, and leave it. `--force` SHALL write sluice's file with each
room's lines carried over, a workflow's by its job and a template's by
its system, saying a room of a system it no longer writes.

#### Scenario: A workflow with the repository's own steps

- **WHEN** `.github/workflows/sluice.yml` is as this sluice writes it
  but for two steps of the repository's between the run job's
  `# sluice:` lines
- **THEN** `sluice init actions` says it is as sluice writes it, with
  the repository's own steps, leaves it, and exits 0
  (`init_actions_keeps_the_repositorys_own_steps` in
  `crates/sluice/tests/cli.rs`)

#### Scenario: A workflow an earlier release wrote

- **WHEN** `sluice.yml` names an earlier release and holds steps of the
  repository's between its `# sluice:` lines, and `sluice init actions
  --force` runs
- **THEN** the workflow written names this release and holds those
  steps between its run job's `# sluice:` lines
  (`init_actions_keeps_the_repositorys_own_steps` in
  `crates/sluice/tests/cli.rs`)

#### Scenario: A template with the repository's own commands

- **WHEN** `.config/sluice-codebuild.yml` holds commands of the
  repository's between the `# sluice:` lines of `linux-x64`'s run
  project, and `sluice init codebuild --systems linux-x64,linux-arm64
  --force` runs
- **THEN** `linux-x64`'s run project holds those commands between its
  lines, and `linux-arm64`'s holds none
  (`init_codebuild_keeps_the_repositorys_own_commands` in
  `crates/sluice/tests/cli.rs`)

### Requirement: A reading of the setup accepts the repository's own steps

`sluice forge check` SHALL read `sluice.yml` on the signer's branch, and
`sluice pool check` each run project's buildspec, with the lines between
their `# sluice:` lines taken out, passing one otherwise as this sluice
writes it and saying it holds the repository's own steps (record 0119).

#### Scenario: A workflow on the signer's branch

- **WHEN** `sluice.yml` on `main` is as this sluice writes it but for a
  step of the repository's between the run job's `# sluice:` lines
- **THEN** `sluice forge check main` passes it, saying it is as sluice
  init actions writes it, with the repository's own steps
  (`forge_check_reads_a_workflow_with_the_repositorys_own_steps` in
  `crates/sluice/tests/profiles/forge.rs`)

#### Scenario: A run project's buildspec

- **WHEN** `sluice-run-linux-x64`'s buildspec is as this sluice writes
  it but for a command of the repository's between its `# sluice:` lines
- **THEN** `sluice pool check main` passes it, saying it is as sluice
  writes it, with the repository's own commands
  (`pool_check_reads_a_buildspec_with_the_repositorys_own_commands` in
  `crates/sluice/tests/cli.rs`)

### Requirement: The repository's own steps stay among the run job's steps

Where the lines between a pair of `# sluice:` lines, read as YAML, are
not entries of the run job's steps or the `post_build` phase's commands
alone, `sluice init actions` and `sluice init codebuild` SHALL leave the
file and exit 1, `--force` or not, and `sluice forge check` and `sluice
pool check` SHALL fail it, each naming the line (core.62).

#### Scenario: A line that gives the run job an environment

- **WHEN** the lines between the run job's `# sluice:` lines in
  `sluice.yml` end with `    environment: sluice`
- **THEN** `sluice init actions --force` leaves the workflow, names that
  line as leaving the run job's steps and exits 1, and `sluice forge
  check main` fails naming it
  (`init_actions_refuses_own_steps_that_leave_the_run_job` in
  `crates/sluice/tests/cli.rs`;
  `forge_check_names_own_steps_that_leave_the_run_job` in
  `crates/sluice/tests/profiles/forge.rs`)

#### Scenario: A string that swallows sluice's lines

- **WHEN** a step between the `# sluice:` lines opens a quoted string
  that closes in a line of sluice's after them
- **THEN** the reading names the line that opened it as leaving its
  place
  (`a_line_that_leaves_the_room_is_named` in
  `crates/sluice/src/own_steps.rs`)

#### Scenario: A command that leaves the buildspec's commands

- **WHEN** the lines between a run project's `# sluice:` lines end with
  `artifacts:` at the buildspec's top level
- **THEN** `sluice pool check main` fails, naming the run project and
  the line
  (`pool_check_names_own_commands_that_leave_the_buildspec` in
  `crates/sluice/tests/cli.rs`)
