## MODIFIED Requirements

### Requirement: sluice init actions writes the workflows

`sluice init actions` SHALL write `.github/workflows/sluice.yml` and
`.github/workflows/sluice-pr.yml` from the templates sluice carries,
naming the release of sluice that wrote them. It SHALL leave a workflow
file that differs from what it would write, the repository's own steps
and its run job's `packages: write` aside (repository-steps), saying how
and exiting non-zero, and write it only with `--force` (88, C.8).

#### Scenario: A workflow already there

- **WHEN** `.github/workflows/sluice.yml` exists and differs from what
  sluice would write
- **THEN** `sluice init actions` leaves it, prints how it differs, exits
  non-zero, and writes it with `--force`
  (`init_actions_leaves_a_differing_workflow_unless_forced` in
  `crates/sluice/tests/cli.rs`)

### Requirement: A run job holds no identity

A run job SHALL run the combination's checks with no environment, no
`id-token`, no attestations and no secret of sluice's, its permissions
`contents: read` and, where the repository adds it for its own steps,
`packages: write`, and nothing else, so no check can obtain the job's
identity token or the app's key. The repository's own steps MAY name
the repository's own secrets (79, 109; timber 135; record 0119).

#### Scenario: The run job in the template

- **WHEN** `sluice init actions` writes `sluice.yml` and `sluice-pr.yml`
- **THEN** every run job names no environment, no `id-token` and no
  secret
  (`the_run_job_holds_no_identity_and_the_sign_job_runs_no_check` in
  `crates/sluice/src/profiles/github/workflows.rs`)

#### Scenario: A run job whose token writes the repository's packages

- **WHEN** `sluice.yml`'s run job's `permissions` hold `packages:
  write` after `contents: read`, and the file is otherwise as this
  sluice writes it
- **THEN** `sluice init actions` reads it as sluice writes it and
  `--force` keeps the line, and the same file with `id-token: write`
  there instead differs
  (`init_actions_takes_the_run_jobs_packages_write_alone` in
  `crates/sluice/tests/cli.rs`)
