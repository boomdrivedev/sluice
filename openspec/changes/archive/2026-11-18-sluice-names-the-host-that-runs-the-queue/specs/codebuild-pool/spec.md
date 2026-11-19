# Spec Delta

## RENAMED Requirements

- FROM: `### Requirement: sluice pool check names a branch no host can lead`
- TO: `### Requirement: sluice pool check names who may run a branch's queue`

## ADDED Requirements

### Requirement: The start role changes nothing a run build runs

The start role `sluice init codebuild` writes, which the principal its
`StartPrincipal` names may assume, SHALL be allowed to start only the
run projects, and SHALL be denied a start that carries any override
CodeBuild gives a condition key for, or a variable but the five the
host that runs the queue passes. It SHALL hold no use of the signing
key, no read of the app's key and no start of the sign project, and the
template SHALL name no lead (record 0119, C.8; core.26).

#### Scenario: The start role in the template

- **WHEN** `sluice init codebuild` writes `.config/sluice-codebuild.yml`
- **THEN** its start role allows `codebuild:StartBuild` on the run
  projects alone, denies it by `Null` on each keyed override and on any
  other variable name, and names no `kms:` action, no secret and the
  sign project only to read its builds
  (`the_start_role_may_change_nothing_a_run_build_runs` in
  `crates/sluice/src/profiles/codebuild/template.rs`)

#### Scenario: The template's names

- **WHEN** `sluice init codebuild` writes `.config/sluice-codebuild.yml`
- **THEN** its parameter is `StartPrincipal`, its role and that role's
  output are `StartRole`, the role's policy is `start`, and no name,
  comment or description in it says `lead`, in any case
  (`the_template_names_the_start_role_and_no_lead` in
  `crates/sluice/src/profiles/codebuild/template.rs`)

## MODIFIED Requirements

### Requirement: The host names the AWS profile sluice reaches a pool as

The host's own settings SHALL accept `[aws] profile`, the AWS profile
every call of the CodeBuild pool uses on that host, read as the AWS
tools read it; where unset, the AWS SDK's own chain SHALL find the
credentials from the environment, or in a build its role. No credential
SHALL be kept between calls (record 0119; timber record 0017).

#### Scenario: A host naming a profile

- **WHEN** the host's settings say `[aws] profile = "sluice-start"` and
  the process's environment names `AWS_PROFILE=person`
- **THEN** the pool's calls are made as `sluice-start`
  (`an_aws_profile_in_the_settings_is_the_one_used` in
  `crates/sluice/src/instance.rs`)

### Requirement: sluice init codebuild writes the stack

`sluice init codebuild` SHALL write `.config/sluice-codebuild.yml`, a
CloudFormation template of a reserved-capacity fleet and a run project
for each Linux system it is given, the sign project, the hand-over
bucket, the KMS key, the secret for the app's key, the rule and the
roles, naming the release of sluice that wrote it, leaving a differing
file unless `--force`, and SHALL print how to deploy it (C.8; record
0119).

#### Scenario: A repository with no pool

- **WHEN** `sluice init codebuild --systems linux-x64` runs in a
  repository whose policy names no pool
- **THEN** it writes the template with one fleet and one run project
  for `linux-x64`, and prints the deploy command passing
  `StartPrincipal`, the secret to fill, the profile that assumes the
  stack's start role to name as `[aws] profile` on each host that may
  run the branch's queue, and `sluice init codebuild --stack`, with no
  word `lead` in what it prints
  (`init_codebuild_writes_the_stack` in `crates/sluice/tests/cli.rs`)

#### Scenario: A template already there

- **WHEN** `.config/sluice-codebuild.yml` differs from what sluice would
  write
- **THEN** `sluice init codebuild` leaves it, says how it differs, exits
  non-zero, and writes it with `--force`
  (`init_codebuild_leaves_a_differing_template_unless_forced` in
  `crates/sluice/tests/cli.rs`)

### Requirement: sluice pool check names who may run a branch's queue

`sluice pool check <branch>` SHALL say, for a branch whose policy names
signers, which signers' hosts may run its queue, those in its trust a
host can sign as; and SHALL fail, naming the branch, where its trust
names none, since no host can then run its queue (rule core.60; record
0119).

#### Scenario: A gate that trusts only the pool's signer

- **WHEN** `main` trusts only `codebuild`, the pool's `kms` signer
- **THEN** `sluice pool check main` says no host can run main's queue,
  says to trust the signer the hosts that run its queue sign as beside
  the pool's, with `systems = []`, and exits non-zero
  (`pool_check_names_a_branch_whose_queue_no_host_can_run` in
  `crates/sluice/tests/cli.rs`)

#### Scenario: A gate that trusts the hosts' signer beside the pool's

- **WHEN** `main` trusts `box`, an `ssh` signer whose `systems = []`,
  and `codebuild`
- **THEN** `sluice pool check main` says `main's queue is run by a host
  signing as box`
  (`pool_check_says_a_fleet_tests_side_by_side` in
  `crates/sluice/tests/cli.rs`)

## REMOVED Requirements

### Requirement: The lead's identity changes nothing a run build runs

**Reason**: The template's role is the start role, `StartRole`, which
"The start role changes nothing a run build runs" holds to the same
bounds; the template names no lead (core.26).

**Migration**: Write the template again with `sluice init codebuild
--force`, deploy it with `StartPrincipal` in place of `LeadPrincipal`,
and point each host's `[aws] profile` at the stack's `StartRole` output.
