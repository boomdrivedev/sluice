## ADDED Requirements

### Requirement: forge check verifies a GitHub pool's setup
For a policy with a GitHub pool, `sluice forge check [<branch>]` SHALL
also report whether the pool's workflow is on the branch as
`sluice init actions` would write it, the app may dispatch it, the
environment holding the key admits only the signer's branch, and the
forge signer names that workflow. It SHALL exit non-zero when any of
these fails, naming which.

#### Scenario: The workflow and the policy disagree
- **WHEN** `[signers.forge]` names `.github/workflows/sluice.yml` and the branch has no such workflow
- **THEN** `sluice forge check main` exits non-zero and names the workflow and `sluice init actions`

#### Scenario: An environment any branch may use
- **WHEN** the environment holding the app's key has no branch rule
- **THEN** `sluice forge check main` exits non-zero and names the environment
