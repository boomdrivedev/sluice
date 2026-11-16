## MODIFIED Requirements

### Requirement: forge check verifies the setup
`sluice forge check [<branch>]` SHALL report, for the branch's remote,
whether the key reads, a token mints, the app is installed on the
repository with permission to write contents, workflows, checks and
pull requests, and the branch's rules on the forge let only the app
bypass them. For a policy with a GitHub pool, it SHALL also report
whether the pool's workflow is on the branch as `sluice init actions`
would write it, the app may dispatch it, the environment holding the
key admits only the signer's branch, and the forge signer names that
workflow. It SHALL exit non-zero when any of these fails, naming which.

#### Scenario: A ruleset that lets the operator through
- **WHEN** the ruleset on `main` lists the operator's account as a bypass beside the app
- **THEN** `sluice forge check main` exits non-zero and names the extra bypass

#### Scenario: All set
- **WHEN** the app is installed with the permissions it needs and is the ruleset's only bypass
- **THEN** `sluice forge check main` exits 0

#### Scenario: No permission to post checks
- **WHEN** the app is installed with Contents and Workflows write but only Checks read
- **THEN** `sluice forge check main` exits non-zero and names the Checks permission

#### Scenario: The workflow and the policy disagree
- **WHEN** `[signers.forge]` names `.github/workflows/sluice.yml` and the branch has no such workflow
- **THEN** `sluice forge check main` exits non-zero and names the workflow and `sluice init actions`

#### Scenario: An environment any branch may use
- **WHEN** the environment holding the app's key has no branch rule
- **THEN** `sluice forge check main` exits non-zero and names the environment
