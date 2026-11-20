## ADDED Requirements

### Requirement: forge check reads commit statuses where the branch takes pull requests
With a key, where the branch's policy gives it a pull requests table,
`sluice forge check [<branch>]` SHALL also report whether the app may
read the repository's commit statuses, which a request's conditions
read. A grant to read or to write SHALL pass. No grant SHALL fail,
naming Commit statuses. A branch whose policy gives it no pull requests
table SHALL have no line of commit statuses (records 0019 as amended
2026-11-18, 0071).

#### Scenario: No permission to read commit statuses
- **WHEN** `main`'s policy gives `[branches.main.pull_requests]`, and the app is installed with Contents, Workflows, Checks and Pull requests read and write and no Commit statuses
- **THEN** the check fails one thing, saying the app may not read `o/r`'s commit statuses and to give it Commit statuses read-only
  (`forge_check_names_the_commit_statuses_it_reads` in
  `crates/sluice/tests/profiles/forge.rs`)

#### Scenario: Reading commit statuses is enough
- **WHEN** `main` takes pull requests and the app is installed with Commit statuses read-only beside the other four
- **THEN** the check fails nothing and says the app may read `o/r`'s commit statuses
  (`forge_check_names_the_commit_statuses_it_reads` in
  `crates/sluice/tests/profiles/forge.rs`)

#### Scenario: A branch that takes no pull requests
- **WHEN** `main`'s policy gives it no pull requests table, and the app has no Commit statuses
- **THEN** the check fails nothing and says nothing of commit statuses
  (`forge_check_names_the_commit_statuses_it_reads` in
  `crates/sluice/tests/profiles/forge.rs`)

#### Scenario: Under a credential command
- **WHEN** the host's `[forge]` names a credential command and `main` takes pull requests
- **THEN** the check says nothing of commit statuses, as of every grant but the push (record 0108)
  (`forge_check_names_the_commit_statuses_it_reads` in
  `crates/sluice/tests/profiles/forge.rs`)

### Requirement: forge check reads whether the branch takes pull requests from the policy read whole
`sluice forge check [<branch>]` SHALL read whether the branch takes pull
requests from the policy it reads the pools from: read whole at the
branch's revision as the host holds it, or at main's for a branch not
yet made. It SHALL read the table that gives the branch its gate, even
where the policy refuses that branch's rules (record 0119 as amended
2026-11-18; core.27).

#### Scenario: From the command line
- **WHEN** `main`'s policy at its revision gives `[branches.main.pull_requests]` and names a signer, and the app has no Commit statuses
- **THEN** `sluice forge check main` exits 1, naming Commit statuses
  (`forge_check_reads_whether_the_branch_takes_pull_requests` in
  `crates/sluice/tests/profiles/forge.rs`)

#### Scenario: A branch not yet made
- **WHEN** `feature/x` is not made, and `main`'s policy gives `[branches."feature/*"]` a remote and a pull requests table, and the app has no Commit statuses
- **THEN** `sluice forge check feature/x` exits 1, naming Commit statuses
  (`forge_check_reads_whether_the_branch_takes_pull_requests` in
  `crates/sluice/tests/profiles/forge.rs`)

#### Scenario: A branch the policy refuses for want of signers
- **WHEN** `main`'s policy gives `[branches.main.pull_requests]` and names no `[signers]`, and the app has no Commit statuses
- **THEN** `sluice forge check main` exits 1, naming Commit statuses
  (`forge_check_reads_whether_the_branch_takes_pull_requests` in
  `crates/sluice/tests/profiles/forge.rs`)
