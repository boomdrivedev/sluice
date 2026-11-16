## ADDED Requirements

### Requirement: A gate that trusts a pool's signer and a lead that may claim nothing runs every run on the pool

A branch whose gate trusts a pool's signer and its leading hosts'
signer, one naming `systems = []`, SHALL be led by such a host and SHALL
have every run started on the pool, one job per run, whatever room
those hosts have, since their signer may claim no run (79, 87, 107).
Each such run SHALL hold no part of any host's bound (71; record 0119).

#### Scenario: A lead with room beside a pool

- **WHEN** `main` trusts `lead`, whose `systems = []`, and `codebuild`,
  the pool `codebuild` signs as `codebuild`, two hosts signing as `lead`
  are live with room on `linux-x64`, and two combinations of `main` open
- **THEN** one of them leads `main`, each combination's run is claimed by
  `pool/codebuild/<n>`, none is claimed by either host, and no part of
  either host's bound is held for them
  (`a_gate_trusting_only_a_pools_signer_runs_every_run_on_its_pool` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: A run build is started after its claim, with its claim and nothing else

The lead SHALL start a run on a CodeBuild pool only after writing it
`claimed`, by one `StartBuild` of the run project the pool's `runners`
names for the run's system, carrying the variables `SLUICE_CLAIM`,
`SLUICE_BRANCH`, `SLUICE_COMBINATION`, `SLUICE_SYSTEM` and
`SLUICE_READ_TOKEN` and the claim as its idempotency token, and no
other override. A build that bears the claim SHALL be the start's proof:
a round that finds one SHALL start nothing more (88; record 0122).

#### Scenario: A run for linux-x64

- **WHEN** a claimed run of `main + #3` needs `linux-x64` and `runners`
  names `sluice-run-linux-x64` for it
- **THEN** one `StartBuild` of `sluice-run-linux-x64` carries the five
  variables and the claim as its token, and no buildspec, image,
  environment, fleet, role, source or artifacts override
  (`a_run_build_is_started_with_its_claim_and_nothing_else` in
  `crates/sluice/src/profiles/codebuild/mod.rs`)

#### Scenario: The start's answer was lost

- **WHEN** a start went unanswered and the next round finds a build of
  `sluice-run-linux-x64` whose `SLUICE_CLAIM` is the claim
- **THEN** nothing is started again
  (`a_run_build_found_is_not_started_again` in
  `crates/sluice/src/profiles/codebuild/mod.rs`)

### Requirement: The lead's identity changes nothing a run build runs

The lead role `sluice init codebuild` writes SHALL be allowed to start
only the run projects, and SHALL be denied a start that carries any
override CodeBuild gives a condition key for, or a variable but the five
the lead passes. It SHALL hold no use of the signing key, no read of the
app's key and no start of the sign project (record 0119, C.8).

#### Scenario: The lead role in the template

- **WHEN** `sluice init codebuild` writes `.config/sluice-codebuild.yml`
- **THEN** its lead role allows `codebuild:StartBuild` on the run
  projects alone, denies it by `Null` on each keyed override and on any
  other variable name, and names no `kms:` action, no secret and the
  sign project only to read its builds
  (`the_lead_may_change_nothing_a_run_build_runs` in
  `crates/sluice/src/profiles/codebuild/template.rs`)

### Requirement: The run build holds no key

The run build SHALL run the combination's checks under a role that may
write only its own artifact and its logs, with no use of the signing
key, no read of the app's key and no start of any build. The only
credential it is given SHALL be a token of sluice's app that reads the
branch's repository and nothing else, which the lead mints for its start
and which lapses within the hour (79, 109; record 0119; core.62).

#### Scenario: The run role in the template

- **WHEN** `sluice init codebuild` writes its template
- **THEN** each run project's role names no `kms:`, no
  `secretsmanager:` and no `codebuild:` action, and its buildspec runs
  `sluice job run` and nothing that signs
  (`the_run_build_holds_no_key_and_the_sign_build_runs_no_check` in
  `crates/sluice/src/profiles/codebuild/template.rs`)

#### Scenario: The read token

- **WHEN** the lead starts a run build for `main` of `acme/widgets`
- **THEN** the token it passes was asked of GitHub with `contents: read`
  on `widgets` alone
  (`a_reading_token_asks_to_read_one_repository` in
  `crates/sluice/src/profiles/github/app.rs`)

### Requirement: A run build's end starts its sign build

A run build that ends `SUCCEEDED`, `FAILED`, `FAULT` or `TIMED_OUT` SHALL
start one build of the sign project, through the rule the template
writes, passing the run build's ARN as `SLUICE_RUN_BUILD` and nothing
the lead chose. A run build that was stopped SHALL start none (88;
record 0119).

#### Scenario: The rule in the template

- **WHEN** `sluice init codebuild` writes its template
- **THEN** its rule matches the run projects' builds in those four
  states and no other, and starts the sign project with
  `SLUICE_RUN_BUILD` from the event's build ARN
  (`the_rule_starts_the_sign_build_for_every_end_but_a_stop` in
  `crates/sluice/src/profiles/codebuild/template.rs`)

### Requirement: The sign build runs none of the combination's code

The sign build SHALL read the run build from CodeBuild, its claim and
branch from its variables, keep its hand-over only where the artifact's
SHA-256 is the one CodeBuild recorded, read the run under the claim from
the branch's records, keep only the keys the combination's tree bears
out by the proof the run build kept, sign the run's statement with the
pool's `kms` key, and write the run's end under its claim. It SHALL run
none of the combination's code (78, 109; record 0119; core.62).

#### Scenario: A hand-over another build wrote

- **WHEN** the run build's artifact in the bucket holds a report whose
  SHA-256 differs from the one `BatchGetBuilds` answers for the build
- **THEN** `sluice job sign --build` signs nothing, `sluice job record
  --build` writes the run ended with no outcome, saying the run build's
  hand-over is not what CodeBuild recorded, and nobody is blamed
  (`job_sign_keeps_a_hand_over_only_as_codebuild_recorded_it` in
  `crates/sluice/tests/cli.rs`)

#### Scenario: A run build's passes

- **WHEN** the run build of `pool/codebuild/4` handed over passes of
  `cli:build` and `#check-fast` whose keys the combination's tree bears
  out
- **THEN** `refs/sluice/runs/codebuild` gains one envelope signed by the
  pool's key over both, and the run reads `ended` and `closed` under
  the claim (`job_record_build_ends_the_run_with_what_the_key_signed`
  in `crates/sluice/tests/cli.rs`)

#### Scenario: The sign role in the template

- **WHEN** `sluice init codebuild` writes its template
- **THEN** only the sign role may `kms:Sign`, with `ECDSA_SHA_256` and
  `DIGEST`, and read the app's key, and the sign buildspec runs only
  sluice's `job sign` and `job record`
  (`the_run_build_holds_no_key_and_the_sign_build_runs_no_check` in
  `crates/sluice/src/profiles/codebuild/template.rs`)

### Requirement: A CodeBuild pool's job stands as its run build does

`Tests::job` for a CodeBuild pool SHALL read `None` where no run build
bears the claim, `Queued` while the run build is submitted or queued,
and `Running` from the end of its queued phase until a sign build of it
completes, `Ended` with the run recorded where that sign build
succeeded (77, 88).

#### Scenario: A run build through its life

- **WHEN** the claim's run build is queued, then in its build phase,
  then ended with its sign build succeeded
- **THEN** the job reads `Queued`, then `Running` since its queued
  phase's end, then `Ended` recorded
  (`a_claims_builds_read_as_its_job` in
  `crates/sluice/src/profiles/codebuild/mod.rs`)

### Requirement: A CodeBuild pool's job that no sign build recorded ends with why

`Tests::job` SHALL read `Ended` with nothing recorded, and why, where
the claim's run build was stopped, or ended with no sign build bearing
it within `[peers] alive` of its end; the lead's lapse of the run SHALL
give that why (77, 88; record 0122).

#### Scenario: A run build stopped

- **WHEN** someone stops the claim's run build
- **THEN** the job reads `Ended` with nothing recorded, and the lead
  ends the run with no outcome, saying its build was stopped
  (`a_stopped_run_build_is_a_job_ended_with_no_record` in
  `crates/sluice/src/profiles/codebuild/mod.rs`)

#### Scenario: No sign build takes a run build

- **WHEN** the claim's run build ended `SUCCEEDED` and no sign build
  bears it once `[peers] alive` has passed since
- **THEN** the job reads `Ended` with nothing recorded, the lapse
  naming the rule that starts the sign build
  (`a_run_build_no_sign_build_took_is_a_job_ended_with_no_record` in
  `crates/sluice/src/profiles/codebuild/mod.rs`)

#### Scenario: The lead's lapse gives the pool's why

- **WHEN** a pool's job reads `Ended` with nothing recorded and why
- **THEN** the run ends with no outcome, its `why` the pool's, nobody
  blamed, and a follow-up run holds its checks
  (`a_pools_job_ended_with_its_why_lapses_with_it` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: A pool's read that fails lapses nothing

A read of a pool's job that fails, as one AWS refuses, SHALL be logged
on the branch once per cause and host with the service's words and its
end, and the run SHALL be read as it stood, lapsing nothing (77, 133;
core.28).

#### Scenario: The pool refuses every read for a while

- **WHEN** a lead's reads of a running pool run's job fail for three
  rounds with one cause, then answer
- **THEN** the branch's log holds one line of the cause and one of its
  end, and the run is still `running` under its claim
  (`a_pools_read_refused_is_logged_once_and_lapses_nothing` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: The host names the AWS profile sluice reaches a pool as

The host's own settings SHALL accept `[aws] profile`, the AWS profile
every call of the CodeBuild pool uses on that host, read as the AWS
tools read it; where unset, the AWS SDK's own chain SHALL find the
credentials from the environment, or in a build its role. No credential
SHALL be kept between calls (record 0119; timber record 0017).

#### Scenario: A host naming a profile

- **WHEN** the host's settings say `[aws] profile = "sluice-lead"` and
  the process's environment names `AWS_PROFILE=person`
- **THEN** the pool's calls are made as `sluice-lead`
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
  for `linux-x64`, and prints the deploy command, the secret to fill and
  `sluice init codebuild --stack`
  (`init_codebuild_writes_the_stack` in `crates/sluice/tests/cli.rs`)

#### Scenario: A template already there

- **WHEN** `.config/sluice-codebuild.yml` differs from what sluice would
  write
- **THEN** `sluice init codebuild` leaves it, says how it differs, exits
  non-zero, and writes it with `--force`
  (`init_codebuild_leaves_a_differing_template_unless_forced` in
  `crates/sluice/tests/cli.rs`)

### Requirement: sluice init codebuild names the pool from its stack

`sluice init codebuild --stack <name>` SHALL read the stack's key by its
alias and append `[signers.codebuild]`, naming the key's ARN and its
public half, and `[pools.codebuild]`, naming the region, the sign
project and a run project for each system, where the policy names
neither, leaving the file's own lines as they were (record 0119).

#### Scenario: A deployed stack

- **WHEN** `sluice init codebuild --stack sluice` runs, and KMS answers
  the alias `alias/sluice-sign` with the key's ARN and a P-256 public
  half
- **THEN** the policy gains `[signers.codebuild]` with that ARN and
  half, and `[pools.codebuild]` with the region, `sluice-sign` and
  `sluice-run-linux-x64`, its own lines untouched
  (`init_codebuild_from_a_stack_appends_the_pool` in
  `crates/sluice/tests/cli.rs`)

### Requirement: sluice pool check reads a CodeBuild pool's setup back

`sluice pool check <branch>` SHALL report, for each CodeBuild pool the
policy at the branch's revision names, the identity the host reaches
AWS as; each run project on a reserved-capacity fleet of its system,
its buildspec as `sluice init codebuild` writes it and its timeout
longer than the branch's `timeout`; the sign project's buildspec; and
the key's spec, usage and public half against the policy's; and SHALL
exit non-zero naming each that fails (record 0119).

#### Scenario: A key the policy does not list

- **WHEN** the key's public half KMS answers differs from
  `[signers.codebuild] kms.public`
- **THEN** `sluice pool check main` names the key and exits non-zero
  (`pool_check_names_a_key_the_policy_does_not_list` in
  `crates/sluice/tests/cli.rs`)

### Requirement: sluice pool check names a branch no host can lead

`sluice pool check <branch>` SHALL fail, naming it, where the branch's
trust names no signer a host can sign as, which no host can lead (rule
core.60; record 0119).

#### Scenario: A gate that trusts only the pool's signer

- **WHEN** `main` trusts only `codebuild`, the pool's `kms` signer
- **THEN** `sluice pool check main` says no host can lead it and exits
  non-zero
  (`pool_check_names_a_branch_no_host_can_lead` in
  `crates/sluice/tests/cli.rs`)

### Requirement: sluice pool check says a fleet tests side by side

`sluice pool check <branch>` SHALL say, without failing, where a branch
on a CodeBuild pool tests with `batch = true` or a `parallel` other than
its fleets' maximum, naming `batch = false` and the maximum (record
0119).

#### Scenario: A branch that tests one combination at a time

- **WHEN** `main` names the pool, `batch = true` and `parallel = 3`, and
  the fleet's maximum is ten
- **THEN** `sluice pool check main` says a branch on a fleet tests side
  by side, `batch = false` with `parallel = 10`, and exits zero where
  every other line passes
  (`pool_check_says_a_fleet_tests_side_by_side` in
  `crates/sluice/tests/cli.rs`)
