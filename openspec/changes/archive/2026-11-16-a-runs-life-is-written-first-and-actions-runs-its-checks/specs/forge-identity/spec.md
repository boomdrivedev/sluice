## MODIFIED Requirements

### Requirement: forge check verifies the setup
`sluice forge check [<branch>]` SHALL report, for the branch's remote,
whether every ruleset on the branch lets only the app through, and
SHALL exit non-zero when any check fails, naming which. With a key, it
SHALL also report whether the key reads, a token mints, and the app is
installed on the repository with permission to write contents,
workflows, checks and pull requests.

#### Scenario: A ruleset that lets the operator through
- **WHEN** the ruleset on `main` lists the operator's account as a bypass beside the app
- **THEN** `sluice forge check main` exits non-zero and names the extra bypass

#### Scenario: All set
- **WHEN** the app is installed with the permissions it needs and is the ruleset's only bypass
- **THEN** `sluice forge check main` exits 0

#### Scenario: No permission to post checks
- **WHEN** the app is installed with Contents and Workflows write but only Checks read
- **THEN** `sluice forge check main` exits non-zero and names the Checks permission

## ADDED Requirements

### Requirement: forge check reads a pool on GitHub as sluice init actions sets it up
For a policy at the branch's revision that names a pool on GitHub,
`sluice forge check [<branch>]` SHALL also report whether the pool's
workflow is on the signer's branch as `sluice init actions` would write
it, the app may run workflows, the environment holding the app's key
admits only the signer's branch, and the pool's signer names that
workflow on that branch, exiting non-zero and naming each that fails.

#### Scenario: The workflow and the policy disagree
- **WHEN** `[signers.forge]` names `.github/workflows/sluice.yml` on `main` and `main` has no such workflow
- **THEN** `sluice forge check main` exits non-zero and names the workflow and `sluice init actions`
  (`forge_check_names_a_pools_missing_workflow` in
  `crates/sluice/tests/profiles/forge.rs`)

#### Scenario: An environment any branch may use
- **WHEN** the environment holding the app's key has no branch rule
- **THEN** `sluice forge check main` exits non-zero and names the environment
  (`forge_check_names_an_environment_any_branch_may_use` in
  `crates/sluice/tests/profiles/forge.rs`)
