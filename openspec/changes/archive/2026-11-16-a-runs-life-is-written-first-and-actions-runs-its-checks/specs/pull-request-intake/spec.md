## MODIFIED Requirements

### Requirement: A branch turns pull requests on in its policy
A branch's policy SHALL accept a `[branches."<pattern>".pull_requests]`
table with `reviews`, a count of approving reviews, default 0, and
`checks`, a list of check names other systems post, default empty;
sluice's own checks are read from its results, never listed there. The
table SHALL be refused on a branch with no `remote`. The conditions
SHALL be read from the branch's revision.

#### Scenario: Pull requests need a remote
- **WHEN** a branch's policy has a `pull_requests` table and no `remote`
- **THEN** the policy is refused with a message naming the branch and `remote`

#### Scenario: A pull request cannot loosen its own conditions
- **WHEN** a pull request changes `.config/sluice.toml` to lower `reviews`
- **THEN** its conditions are still the ones on the branch's revision

#### Scenario: sluice's own check in checks
- **WHEN** `checks` names a check sluice itself posts on a request's head
- **THEN** the policy is refused, naming the check and saying sluice reads its own checks from its results
  (`a_pull_request_table_naming_sluices_own_check_is_refused` in
  `crates/sluice-core/src/policy.rs`)

### Requirement: A pull request table on records each host keeps refuses its branch alone
A branch's `pull_requests` table SHALL refuse that branch alone, naming
`[signers]` and `sluice init actions`, in a policy that names no
`[signers]`, whose branches' records each host keeps; the policy's other
branches SHALL be read as before (56, 137, S64).

#### Scenario: Pull requests need records on the remote
- **WHEN** main's policy has a `pull_requests` table and `remote = "origin"`, names no `[signers]`, and gives `bolt/*` a table too
- **THEN** a `sluice submit main` exits 2 naming main, `[signers]` and `sluice init actions`, pushes and opens nothing, and a submission to `bolt/x` is tested and lands

#### Scenario: Status shows the refusal
- **WHEN** main is so refused
- **THEN** `sluice status main` prints the refusal, and `sluice overview` names main with it

## ADDED Requirements

### Requirement: A run on a pull request's head counts toward its combination

A run of `sluice-pr.yml` on a pull request's head, signed by
`sluice.yml`'s sign job on the branch the forge signer names, SHALL be
recorded under its keys and systems as any run is, and a combination
that shares a key on a system SHALL read that result without running the
check again (15, 113). A head run signed by a signer the branch does not
trust SHALL count for nothing.

#### Scenario: Passed before it was ready (S40)

- **WHEN** a pull request's head passed `cli:build` on three systems on
  the pool, and it is submitted behind one submission that changes no
  input of `cli:build`
- **THEN** its combination reads the three passes by key and runs only
  the checks the submission ahead changes
  (`s40_a_pull_requests_head_runs_count_by_key` in
  `crates/sluice-conformance/tests/peers.rs`)

#### Scenario: A head run by a signer the branch does not trust

- **WHEN** a pull request's head was run and signed by a signer `main`
  does not trust
- **THEN** its combination runs every required check as if the head had
  never been run
  (`a_head_run_by_an_untrusted_signer_counts_for_nothing` in
  `crates/sluice-conformance/tests/peers.rs`)
