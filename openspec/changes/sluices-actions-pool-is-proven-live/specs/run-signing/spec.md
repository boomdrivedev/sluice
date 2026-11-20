## MODIFIED Requirements

### Requirement: Run records are signed statements
Every result recorded on a branch whose state is on the remote SHALL be
an in-toto statement in a DSSE envelope: one subject per check, its key and
system, its contents the outcome, the claims, the combination, the
claim it ran under and the time. An `ssh` signer SHALL sign with its
SSH key, a `github` signer keylessly, as a sigstore bundle holding the
envelope, and a `kms` signer with its key in KMS. A record whose
signature sluice cannot verify SHALL count for nothing.

#### Scenario: A box's result
- **WHEN** the `box` host records a pass of `cli:build` on `linux-x64`
- **THEN** `refs/sluice/runs/box` holds a DSSE envelope over an in-toto statement with a subject for that key on `linux-x64`, signed by the box's SSH key

#### Scenario: A forge job's result
- **WHEN** the pool's sign job records the trial's pass on `macos-arm64`
- **THEN** `refs/sluice/runs/forge` holds a sigstore bundle whose envelope is the run's statement, signed with the sign job's keyless identity
  (`a_keyless_bundle_holds_the_runs_statement` in
  `crates/sluice/src/profiles/github/keyless.rs`;
  `the_trials_runs_count_for_its_gate` in
  `crates/sluice/src/profiles/remote_refs.rs`)

#### Scenario: A CodeBuild sign build's result
- **WHEN** the CodeBuild pool's sign build records a pass of `cli:build` on `linux-x64`
- **THEN** `refs/sluice/runs/codebuild` holds a DSSE envelope over the same statement, its signature the KMS key's and its `keyid` the key's ARN
  (`a_kms_envelope_holds_the_runs_statement` in
  `crates/sluice/src/signing/kms.rs`)

### Requirement: A forge signer signs keylessly

A `github` signer's run record SHALL be a sigstore bundle whose envelope
is the run's statement, signed with the sign job's workload identity
through the sigstore instance GitHub uses for the repository: the
public-good instance for a public repository, GitHub's own for a
private one (78, 141).

#### Scenario: A sign job on main

- **WHEN** `sluice.yml`'s sign job on `main` signed the trial's pass on
  `linux-x64`, and `[signers.forge] github` names that workflow on
  `main`
- **THEN** every host reads the pass as signed by `forge`
  (`the_trials_runs_count_for_its_gate` in
  `crates/sluice/src/profiles/remote_refs.rs`)

#### Scenario: A private repository's bundle

- **WHEN** the bundle comes from GitHub's own instance, with a timestamp
  and no transparency log entry
- **THEN** it verifies against GitHub's trusted root and counts as
  `forge` (`a_bundle_of_githubs_own_instance_counts` in
  `crates/sluice/tests/profiles/forge.rs`)
