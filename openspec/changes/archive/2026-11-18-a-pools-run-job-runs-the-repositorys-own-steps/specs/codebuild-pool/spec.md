## MODIFIED Requirements

### Requirement: The run build holds no key

The run build SHALL run the combination's checks under a role that may
write its own artifact and its logs, and whatever else the managed
policies the repository gives it for its own commands allow, and that
denies itself the signing key, the app's key and every CodeBuild and
IAM action. The only credential sluice gives it SHALL be a token of
sluice's app that reads the branch's repository alone, lapsing within
the hour (79, 109; timber 135; record 0119; core.62).

#### Scenario: The run role in the template

- **WHEN** `sluice init codebuild` writes its template
- **THEN** each run project's role allows no `kms:`, no
  `secretsmanager:` and no `codebuild:` action, and its buildspec runs
  `sluice job run` and nothing that signs
  (`the_run_build_holds_no_key_and_the_sign_build_runs_no_check` in
  `crates/sluice/src/profiles/codebuild/template.rs`)

#### Scenario: The repository's own policies

- **WHEN** the template is read with `RunPolicies` naming one managed
  policy, and with it empty
- **THEN** the run role is given that policy in the first and none in
  the second, and in both its own policy denies every `kms:` action on
  the signing key, every `secretsmanager:` action on the app's key, and
  every `codebuild:` and `iam:` action
  (`the_run_role_takes_the_repositorys_policies_and_its_denies_stand`
  in `crates/sluice/src/profiles/codebuild/template.rs`)

#### Scenario: The read token

- **WHEN** the lead starts a run build for `main` of `acme/widgets`
- **THEN** the token it passes was asked of GitHub with `contents: read`
  on `widgets` alone
  (`a_reading_token_asks_to_read_one_repository` in
  `crates/sluice/src/profiles/github/app.rs`)

### Requirement: sluice init codebuild writes the stack

`sluice init codebuild` SHALL write `.config/sluice-codebuild.yml`, a
CloudFormation template of a reserved-capacity fleet and a run project
for each Linux system it is given, the sign project, the hand-over
bucket, the KMS key, the secret, the rule and the roles, taking the
repository's own policies for the run role, naming the release that
wrote it, leaving a file differing outside the repository's own commands
unless `--force`, and SHALL print how to deploy it (C.8; record 0119).

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

### Requirement: sluice pool check reads a CodeBuild pool's setup back

`sluice pool check <branch>` SHALL report, for each CodeBuild pool the
policy at the branch's revision names, the identity the host reaches
AWS as; each run project on a reserved-capacity fleet of its system,
its buildspec as `sluice init codebuild` writes it, the repository's own
commands aside, and its timeout longer than the branch's `timeout`; the
sign project's buildspec; and the key against the policy's; and SHALL
exit non-zero naming each that fails (record 0119).

#### Scenario: A key the policy does not list

- **WHEN** the key's public half KMS answers differs from
  `[signers.codebuild] kms.public`
- **THEN** `sluice pool check main` names the key and exits non-zero
  (`pool_check_names_a_key_the_policy_does_not_list` in
  `crates/sluice/tests/cli.rs`)
