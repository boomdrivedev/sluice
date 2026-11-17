## MODIFIED Requirements

### Requirement: A branch turns pull requests on in its policy
A branch's policy SHALL accept a `[branches."<pattern>".pull_requests]`
table with `reviews`, a count of approving reviews, default 0, and
`checks`, a list of check names other systems post, default empty;
sluice's own required checks are read from its results, never listed
there. The table SHALL be refused on a branch with no `remote`. The
conditions SHALL be read from the branch's revision.

#### Scenario: Pull requests need a remote
- **WHEN** a branch's policy has a `pull_requests` table and no `remote`
- **THEN** the policy is refused with a message naming the branch and `remote`

#### Scenario: A pull request cannot loosen its own conditions
- **WHEN** a pull request changes `.config/sluice.toml` to lower `reviews`
- **THEN** its conditions are still the ones on the branch's revision

#### Scenario: sluice's own check in checks
- **WHEN** `checks` lists a check run sluice itself posts for a required check
- **THEN** the policy is refused, and the message says sluice reads its own checks from its results

## ADDED Requirements

### Requirement: A run on a pull request's head counts toward its combination
A run on a pull request's head by a signer the branch trusts SHALL be
recorded under its keys and systems as any run is, and a combination
that shares a key on a system SHALL read that result without running
the check again (15, 113).

#### Scenario: Passed before it was ready
- **WHEN** a pull request's head passed `cli:build` on three systems on the pool, and it is submitted behind one submission that changes no input of `cli:build`
- **THEN** its combination reads the three passes by key and runs only the checks the submission ahead changes (S40)

#### Scenario: A head run by an untrusted signer
- **WHEN** a pull request's head was run by a signer `main` does not trust
- **THEN** its combination runs every required check as if the head had never been run
