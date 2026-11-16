# actions-pool Specification

## Purpose
A pool on GitHub Actions: the workflows `sluice init actions` writes, a run's two jobs and `sluice job`, what each job may hold, and how a pool's runs are claimed, dispatched, followed and ended (record 0119).

## Requirements

### Requirement: sluice init actions writes the workflows

`sluice init actions` SHALL write `.github/workflows/sluice.yml` and
`.github/workflows/sluice-pr.yml` from the templates sluice carries,
naming the release of sluice that wrote them. It SHALL leave a workflow
file that differs from what it would write, saying how and exiting
non-zero, and write it only with `--force` (88, C.8).

#### Scenario: A workflow already there

- **WHEN** `.github/workflows/sluice.yml` exists and differs from what
  sluice would write
- **THEN** `sluice init actions` leaves it, prints how it differs, exits
  non-zero, and writes it with `--force`
  (`init_actions_leaves_a_differing_workflow_unless_forced` in
  `crates/sluice/tests/cli.rs`)

### Requirement: sluice init actions names the pool and its signer

`sluice init actions` SHALL add `[signers.forge]`, naming `sluice.yml` on
the branch it runs from, and `[pools.forge]`, with a runner for each
system the policy names, to `.config/sluice.toml` where the policy has
neither, and SHALL print what the operator must still do on GitHub: the
app's permission to run workflows, and the `sluice` environment holding
the app's key, admitting only the signer's branch.

#### Scenario: A repository with no pool

- **WHEN** `sluice init actions` runs on `main` in a repository whose
  policy names no pool and systems `linux-x64` and `macos-arm64`
- **THEN** it writes both workflows, adds `[signers.forge]` with
  `github = { workflow = ".github/workflows/sluice.yml", ref =
  "refs/heads/main" }` and `[pools.forge]` with a runner for each system,
  and prints the permission and the environment to set
  (`init_actions_writes_both_workflows_and_the_pool` in
  `crates/sluice/tests/cli.rs`)

#### Scenario: A pool already named

- **WHEN** the policy already has `[signers.forge]` and `[pools.forge]`
- **THEN** `sluice init actions` changes neither and writes only the
  workflows (`init_actions_keeps_a_pool_the_policy_names` in
  `crates/sluice/tests/cli.rs`)

### Requirement: A pool's run is claimed before its job is dispatched

The lead SHALL start a run no live standing host may take on the first
pool the policy lists for its system (107) by writing the run `claimed`
under `pool/<pool>/<n>`, and only then dispatching `sluice.yml` for that
claim on the runner the pool names for the system. The workflow run,
titled `sluice <claim>`, SHALL be the dispatch's proof: a round that
finds one SHALL dispatch nothing more (88; run-life).

#### Scenario: A run for macOS

- **WHEN** a run of `main + #3` needs `macos-arm64`, no live host of it
  has room, and `[pools.forge] runners` names `macos-15` for it
- **THEN** the run is written `claimed` by `pool/forge/<n>` before one
  dispatch of `sluice.yml` with that claim and `macos-15`
  (`a_pools_run_is_claimed_before_its_job_is_dispatched` in
  `crates/sluice-conformance/tests/run_life.rs`)

#### Scenario: The dispatch's answer was lost

- **WHEN** the lead wrote the claim and its dispatch went unanswered, and
  the next round finds a workflow run titled `sluice pool/forge/<n>`
- **THEN** nothing is dispatched again
  (`a_pools_dispatch_found_done_is_not_made_again` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: A pool's run is running from its run job's start

A pool's run SHALL be written `running` by the lead from the run job's
own start as the forge gives it, its limit counting from then, and
`ended` and `closed` by its sign job under its claim. A workflow run the
pool lost, cancelled or timed out before its sign job wrote the run's
end SHALL be a lapsed claim, whose run ends with no outcome (88, 77;
run-life).

#### Scenario: A run job starts

- **WHEN** the run job of `pool/forge/<n>`'s workflow run starts on
  `macos-15`
- **THEN** the run reads `running` from that start, and its end is
  written by the sign job
  (`a_pools_run_is_running_from_its_run_jobs_start` in
  `crates/sluice-conformance/tests/run_life.rs`)

#### Scenario: A cancelled workflow run

- **WHEN** someone cancels the workflow run while its run job runs
- **THEN** the run ends with no outcome, saying its job ended with no
  record, nothing of it is recorded, nobody is blamed, and a follow-up
  run holds its checks
  (`a_pools_job_ended_with_no_record_is_a_lapsed_claim` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: A run job holds no identity

A run job SHALL run the combination's checks with `contents: read`, no
environment, no `id-token` and no secret, so no check can obtain the
job's identity token or the app's key (79, 109; record 0119).

#### Scenario: The run job in the template

- **WHEN** `sluice init actions` writes `sluice.yml` and `sluice-pr.yml`
- **THEN** every run job names no environment, no `id-token` and no
  secret
  (`the_run_job_holds_no_identity_and_the_sign_job_runs_no_check` in
  `crates/sluice/src/profiles/github/workflows.rs`)

### Requirement: A sign job runs none of the combination's code

A sign job, which needs the run job, SHALL run none of the combination's
code: it SHALL read the run job's report and moon's manifests from the
run job's artifact, keep only the keys the combination's tree bears out,
sign the run's statement with its own keyless identity, add the record
to `refs/sluice/runs/<signer>` and write the run's end under its claim
(78, 109; record 0119).

#### Scenario: The sign job in the template

- **WHEN** `sluice init actions` writes `sluice.yml`
- **THEN** its sign job alone names the `sluice` environment and
  `id-token: write`, and runs no step of the combination's
  (`the_run_job_holds_no_identity_and_the_sign_job_runs_no_check` in
  `crates/sluice/src/profiles/github/workflows.rs`)

#### Scenario: A key the tree does not bear out

- **WHEN** a run job's report names a pass of `cli:build` under a key
  the combination's tree does not bear out
- **THEN** `sluice job sign` leaves that key out of the statement, and
  the check reads as left without a result
  (`job_sign_signs_only_what_the_tree_bears_out` in
  `crates/sluice/tests/cli.rs`)

### Requirement: sluice job run records nothing

`sluice job run --claim <c> --branch <b>` SHALL make the claimed run's
checkout at its combination, fetching the combination's own ref, run its
checks under the branch's policy at its revision, and write the run's
report and moon's manifests to a directory the job uploads. It SHALL
record nothing, write no state and hold no key (82, 88).

#### Scenario: A run job records nothing

- **WHEN** `sluice job run --claim pool/forge/4 --branch main` runs in a
  scratch clone of a repository whose `main` has that claim
- **THEN** it writes the report and the manifests to its directory, and
  neither `refs/sluice/state/main` nor any `refs/sluice/runs/` ref moved
  (`job_run_writes_its_report_and_records_nothing` in
  `crates/sluice/tests/cli.rs`)

### Requirement: sluice job record writes the run's end only under a claim that holds it

`sluice job record --claim <c> --branch <b> --bundle <file>` SHALL add
the bundle to the signer's ref and write the run `ended` and `closed`
under the claim, and SHALL refuse, writing nothing, where the claim no
longer holds the run (77; run-life).

#### Scenario: A record under a claim that lapsed

- **WHEN** `sluice job record` runs for `pool/forge/4` after the lead
  ended that run as lapsed
- **THEN** it exits non-zero, saying the claim no longer holds the run,
  and the run's written end is unchanged
  (`job_record_under_a_lapsed_claim_is_refused` in
  `crates/sluice/tests/cli.rs`)

### Requirement: The app's key reaches only the sign job of the signer's branch

The app's key SHALL be given only to the sign job of `sluice.yml`,
through the `sluice` environment, whose branch rule admits only the
branch `[signers.forge] github` names. A workflow run for a pull request
or any other branch SHALL get neither the key nor a certificate that
verifies as `forge` (113; record 0119).

#### Scenario: A pull request edits the workflow

- **WHEN** a pull request changes `.github/workflows/sluice.yml` to
  print the app's key
- **THEN** its workflow run gets no key, and anything it signs counts
  for nothing, its certificate naming the pull request's ref
  (`a_bundle_from_a_pull_requests_ref_counts_for_nothing` in
  `crates/sluice/tests/profiles/forge.rs`)
