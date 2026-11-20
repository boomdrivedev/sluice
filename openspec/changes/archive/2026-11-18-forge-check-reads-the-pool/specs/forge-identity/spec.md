## MODIFIED Requirements

### Requirement: forge check reads a pool on GitHub as sluice init actions sets it up
For a pool on GitHub that the policy names, read whole at the branch's
revision as the host holds it, or at main's for a branch not yet made,
`sluice forge check [<branch>]` SHALL also report whether the pool's
workflow is on the signer's branch as `sluice init actions` would write
it, the app may run workflows, the environment holding the app's key
admits only the signer's branch, and the pool's signer names that
workflow on that branch, exiting non-zero and naming each that fails.

#### Scenario: The workflow and the policy disagree
- **WHEN** `[signers.forge]` names `.github/workflows/sluice.yml` on `main` and `main` has no such workflow
- **THEN** `sluice forge check main` exits non-zero and names the workflow and `sluice init actions`, beside the pool's other lines
  (`forge_check_reads_the_pool_the_policy_names` in
  `crates/sluice/tests/profiles/forge.rs`)

#### Scenario: A branch not yet made
- **WHEN** `feature/x` is not made and `main`'s policy puts it on its remote and names a pool on GitHub
- **THEN** `sluice forge check feature/x` names the pool's lines as `main`'s policy names the pool
  (`forge_check_reads_the_pool_the_policy_names` in
  `crates/sluice/tests/profiles/forge.rs`)

#### Scenario: An environment any branch may use
- **WHEN** the environment holding the app's key has no branch rule
- **THEN** `sluice forge check main` exits non-zero and names the environment
  (`forge_check_names_an_environment_any_branch_may_use` in
  `crates/sluice/tests/profiles/forge.rs`)

## ADDED Requirements

### Requirement: forge check names a policy it cannot read whole
Where the branch's policy names its remote but cannot be read whole,
`sluice forge check [<branch>]` SHALL name the revision, the policy's
own words and that its pools went unread, and exit non-zero (record
0119 as amended 2026-11-18).

#### Scenario: A policy forge check cannot read whole
- **WHEN** `main`'s policy puts `main` on `origin` and its `[landing] subject` names `{nope}`
- **THEN** `sluice forge check main` exits non-zero, names `main`'s policy as refused with `[landing] subject names {nope}`, and names no line of the pool
  (`forge_check_reads_the_pool_the_policy_names` in
  `crates/sluice/tests/profiles/forge.rs`)
