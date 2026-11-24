## Purpose

`sluice host` makes each change to sluice's state. A change command sends its folder, its arguments and its caller's session to `sluice host` and prints the answer. It runs no git for the change, reads no forge and writes no record.

## ADDED Requirements

### Requirement: sluice host makes each change

`sluice host` SHALL make each change: a submit, an amend, a withdrawal, `sluice confirm`, a release of a checkout or a preview, `sluice branch` and `sluice protect`. The command SHALL send its folder and its arguments to `sluice host`. It SHALL print the output that `sluice host` answers, and SHALL exit with its code.

Before: each change command bound the repository and wrote the records in its own process. Now: `sluice host` does both. (`residents-do-the-network-and-git-work@a2d68cf95ffe`)

#### Scenario: A withdrawal

- **WHEN** `sluice host` runs, `#4` is queued on `main`, and `sluice withdraw 4 --json` runs
- **THEN** it prints the document that a withdrawal printed before this change, and exits 0
- **AND** `sluice status 4` says that `#4` is withdrawn, and the log of `sluice host` names the withdrawal
- **AND** the command ran no git but the upkeep of the branch protection's hook

#### Scenario: An amend and a confirmation

- **WHEN** `sluice host` runs, and `sluice amend` and `sluice confirm` each run
- **THEN** each prints what it printed before this change, exits with the same code, and the records hold the change
- **AND** neither command ran git but the upkeep of the branch protection's hook

#### Scenario: A branch made on its remote

- **WHEN** `sluice host` runs, and `sluice branch create feature/x --from main` runs, where the policy names `origin` for `feature/x`
- **THEN** it exits 0, and `origin` holds `feature/x` at `main`'s revision
- **AND** the command ran no git but the upkeep of the branch protection's hook

#### Scenario: sluice protect

- **WHEN** `sluice host` runs, and `sluice protect` runs in the repository
- **THEN** the repository's hook names the path of `sluice host`, and the command prints what it printed before this change

### Requirement: A change command runs no git and reads no forge

A change command SHALL NOT run git for its change. It SHALL NOT read or write a forge. It SHALL NOT write a record, a result or a file of the repository. Where no `sluice host` answers, the command SHALL say so and SHALL exit 3. (`residents-do-the-network-and-git-work@a2d68cf95ffe`, `clients-never-fetch-or-start-a-tool@8aa9c57f19b2`)

#### Scenario: No host process answers a change

- **WHEN** a process holds the socket of `sluice host`, answers `hello`, and closes the connection when `sluice withdraw 4` asks `withdraw`
- **THEN** the withdrawal says that sluice's host process did not answer, and exits 3
- **AND** `#4` is still queued

### Requirement: sluice host makes a pull request's submit

For a branch that takes pull requests, `sluice host` SHALL push the submitted branch, open or update the request, mark it ready and take it in. The command SHALL NOT push and SHALL NOT call the forge. (`residents-do-the-network-and-git-work@a2d68cf95ffe`)

#### Scenario: A submit to a branch that takes pull requests

- **WHEN** `main` takes pull requests, and `sluice host` runs with the test's forge
- **AND** `sluice submit main --no-wait` runs in a worktree with `SLUICE_FORGE_API` naming an address that refuses each call
- **THEN** the test's forge holds an open request from the worktree's branch, marked ready, and the remote holds the worktree's head on that branch
- **AND** the submit prints what it printed before this change, and exits 0

### Requirement: A wait on a pull request follows sluice host

`sluice host` SHALL take each step of a wait on a pull request and of a wait on a branch that takes pull requests. It SHALL take each step of a group's wait. The wait SHALL read only what `sluice host` sends. (`residents-do-the-network-and-git-work@a2d68cf95ffe`, `reads-go-through-the-resident@69134e5825d7`)

#### Scenario: A wait on a request that lands

- **WHEN** `main` takes pull requests, request 7 is open, and `sluice wait main --pull-request 7` runs with `SLUICE_FORGE_API` naming an address that refuses each call
- **THEN** once `sluice host` lands the request, the wait prints the landing as it printed it before this change, and exits 0

#### Scenario: A group's wait

- **WHEN** two worktrees run `sluice submit main --worktree a --worktree b` as one act
- **THEN** the submit prints each outcome in their order, as it printed them before this change, and exits with the first that did not land
- **AND** the command ran no git but the upkeep of the branch protection's hook

### Requirement: A change names its caller's session

A change command SHALL send the variables of its caller's session and its `TRACEPARENT` with its request. `sluice host` SHALL make the change with them in place of its own environment.

Before: a submit read the agent's name, the pane and the trace from its own environment. Now: it sends them, and `sluice host` uses them.

#### Scenario: A submit names its agent

- **WHEN** `sluice host` was started by a command with no `SLUICE_AGENT`, and `sluice submit main --no-wait` runs with `SLUICE_AGENT=feat`
- **THEN** the submission's record names the agent `feat`

### Requirement: sluice host keeps only the requests that a command of its release sends

`sluice host` SHALL answer a request that no command of its release sends with the error that names both versions. A `step` and a `pass` SHALL name the client's folder.

Before: `sluice step` and a watch named the repository that they resolved with git. Now: they name their folder, and `sluice host` resolves it.

#### Scenario: An older step

- **WHEN** a client says `hello` as the version of `sluice host`, and asks `step` naming a repository and no folder
- **THEN** it gets the error that names both versions and says to run sluice of that version

#### Scenario: A step from a folder

- **WHEN** `#4` is queued on `main`, and `sluice step main` runs in a worktree
- **THEN** it prints the round as it printed it before this change, and exits 0
- **AND** the command ran no git but the upkeep of the branch protection's hook
