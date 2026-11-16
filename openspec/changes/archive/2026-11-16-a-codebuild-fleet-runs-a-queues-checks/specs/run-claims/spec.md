## MODIFIED Requirements

### Requirement: The policy lists pools in order
The policy SHALL accept `[pools.<name>]` tables, each with one kind, a
`github` workflow path or a `codebuild` table naming the region and the
sign project, the `signer` its jobs sign as, and `runners`, for each
system it runs, the runner label of a `github` pool or the run project
of a `codebuild` pool. A pool whose runners name a system its signer
may not claim SHALL be refused. The order written SHALL be the order
pools are tried (107).

#### Scenario: A runner for a system the signer may not claim
- **WHEN** `[pools.forge] signer = "forge"` has a runner for `macos-arm64` and `[signers.forge] systems` lacks it
- **THEN** the policy is refused, naming `pools.forge` and `macos-arm64`

#### Scenario: A CodeBuild pool
- **WHEN** `[pools.codebuild]` has `codebuild = { region = "us-east-1", sign = "sluice-sign" }`, `signer = "codebuild"`, a `kms` signer, and `runners = { linux-x64 = "sluice-run-linux-x64" }`
- **THEN** the policy reads a pool of kind `codebuild` whose run for `linux-x64` goes to the project `sluice-run-linux-x64`
  (`a_codebuild_pool_names_its_region_sign_project_and_runners` in
  `crates/sluice-core/src/policy.rs`)

## ADDED Requirements

### Requirement: A pool signs as its kind's signer
A `github` pool's `signer` SHALL be a `github` signer and a
`codebuild` pool's a `kms` signer; a pool whose `signer` is of another
kind SHALL be refused, naming the pool, its kind and its signer's.

#### Scenario: A pool signing as another kind of signer
- **WHEN** `[pools.codebuild]` signs as a `github` signer, or `[pools.forge]`, a `github` pool, signs as a `kms` signer
- **THEN** the policy is refused, naming the pool, its kind and its signer's
  (`a_pool_signs_as_its_kinds_signer` in
  `crates/sluice-core/src/policy.rs`)
