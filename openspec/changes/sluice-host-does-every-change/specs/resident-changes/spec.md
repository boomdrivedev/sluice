## Purpose

`sluice host` makes each change to sluice's state. A change command sends its folder, its arguments and its caller's session to `sluice host` and prints the answer. It runs no git, reads no forge, writes no record and starts no other tool.

## ADDED Requirements

### Requirement: sluice host makes each change

`sluice host` SHALL make each change: a submit, an amend, a withdrawal, a set-aside, `sluice confirm`, `sluice push`, a release, `sluice branch` and `sluice protect`. The command SHALL send its folder and its arguments to `sluice host`. It SHALL print the output that `sluice host` answers, and SHALL exit with its code.

Before: each change command bound the repository and wrote the records in its own process. Now: `sluice host` does both. (`residents-do-the-network-and-git-work@a2d68cf95ffe`)

#### Scenario: A withdrawal

- **WHEN** `sluice host` runs, `#4` is queued on `main`, and `sluice withdraw 4 --json` runs with no git on its `PATH`
- **THEN** it prints the document that a withdrawal printed before this change, and exits 0
- **AND** `sluice status 4` says that `#4` is withdrawn, and the log of `sluice host` names the withdrawal

#### Scenario: An amend, a set-aside and a confirmation

- **WHEN** `sluice host` runs, and `sluice amend`, `sluice set-aside` and `sluice confirm` each run with no git on its `PATH`
- **THEN** each prints what it printed before this change, exits with the same code, and the records hold the change

#### Scenario: A branch made on its remote

- **WHEN** `sluice host` runs, and `sluice branch create feature/x --from main` runs with no git on its `PATH`, where the policy names `origin` for `feature/x`
- **THEN** it exits 0, and `origin` holds `feature/x` at `main`'s revision

### Requirement: A change command runs no git and reads no forge

A change command SHALL NOT run git. It SHALL NOT read or write a forge. It SHALL NOT write a record, a result or a file of the repository. Where no `sluice host` answers, the command SHALL say so and SHALL exit 3. (`residents-do-the-network-and-git-work@a2d68cf95ffe`, `clients-never-fetch-or-start-a-tool@8aa9c57f19b2`)

#### Scenario: sluice push with no network of its own

- **WHEN** a landing on `main` is held with the forge's words, `sluice host` runs with the test's remote, and `sluice push main` runs with no git on its `PATH`
- **THEN** `sluice host` pushes the landing, and the command prints what it printed before this change

#### Scenario: No host process answers a change

- **WHEN** a process holds the socket of `sluice host`, answers `hello`, and closes the connection when `sluice withdraw 4` asks `withdraw`
- **THEN** the withdrawal says that sluice's host process did not answer, and exits 3
- **AND** `#4` is still queued

### Requirement: sluice host makes a pull request's submit

For a branch that takes pull requests, `sluice host` SHALL push the submitted branch, open or update the request, mark it ready and take it in. The command SHALL NOT push and SHALL NOT call the forge. (`residents-do-the-network-and-git-work@a2d68cf95ffe`)

#### Scenario: A submit to a branch that takes pull requests

- **WHEN** `main` takes pull requests, and `sluice host` runs with the test's forge
- **AND** `sluice submit main --no-wait` runs in a worktree with no git on its `PATH`, and with `SLUICE_FORGE_API` naming an address that refuses each call
- **THEN** the test's forge holds an open request from the worktree's branch, marked ready, and the remote holds the worktree's head on that branch
- **AND** the submit prints what it printed before this change, and exits 0

### Requirement: A wait on a pull request follows sluice host

`sluice host` SHALL take each step of a wait on a pull request and of a wait on a branch that takes pull requests. It SHALL take each step of a group's wait. The wait SHALL read only what `sluice host` sends. (`residents-do-the-network-and-git-work@a2d68cf95ffe`, `reads-go-through-the-resident@69134e5825d7`)

#### Scenario: A wait on a request that lands

- **WHEN** `main` takes pull requests, request 7 is open, and `sluice wait main --pull-request 7` runs with no git on its `PATH` and with `SLUICE_FORGE_API` naming an address that refuses each call
- **THEN** once `sluice host` lands the request, the wait prints the landing as it printed it before this change, and exits 0

#### Scenario: A group's wait

- **WHEN** two worktrees run `sluice submit main --worktree a --worktree b` as one act, with no git on the command's `PATH`
- **THEN** the submit prints each outcome in their order, as it printed them before this change, and exits with the first that did not land

### Requirement: A change names its caller's session

A change command SHALL send the variables of its caller's session and its `TRACEPARENT` with its request. `sluice host` SHALL make the change with them in place of its own environment.

Before: a submit read the agent's name, the pane and the trace from its own environment. Now: it sends them, and `sluice host` uses them.

#### Scenario: A submit names its agent

- **WHEN** `sluice host` was started by a command with no `SLUICE_AGENT`, and `sluice submit main --no-wait` runs with `SLUICE_AGENT=feat`
- **THEN** the submission's record names the agent `feat`

### Requirement: A client starts no other tool

A command of sluice SHALL NOT start a process, but `sluice host` where none answers. It SHALL NOT start a toolchain, a task runner, a release command or a command of another tool. (`clients-never-fetch-or-start-a-tool@8aa9c57f19b2`)

#### Scenario: sluice equip with no tools of its own

- **WHEN** `sluice host` runs with the toolchain and the task runner on its `PATH`, and `sluice equip main` runs in a worktree with only `sluice` on its `PATH`
- **THEN** it prints each step of the equip, as it printed them before this change, and exits 0

#### Scenario: A worktree's release

- **WHEN** a worktree's place holds a release command that writes a file, and `sluice release` runs in that worktree with only `sluice` on its `PATH`
- **THEN** the file is written, the place is released, and the release command's parent is `sluice host`

### Requirement: sluice host keeps only the requests that a command of its release sends

`sluice host` SHALL NOT answer `serve`, `hold` or `results`. A `step` and a `pass` SHALL name the client's folder. `sluice host` SHALL answer a request that it does not know with the error that names both versions.

Before: a command told `sluice host` of a change by `serve`, asked a run's room by `hold`, asked results by `results`, and named the repository that it resolved with git. Now: `sluice host` makes the change and holds the room itself, and a command names its folder.

#### Scenario: An older serve

- **WHEN** a client says `hello` as the version of `sluice host`, and asks `serve` of a repository
- **THEN** it gets the error that names both versions and says to run sluice of that version

#### Scenario: A step from a folder

- **WHEN** `#4` is queued on `main`, and `sluice step main` runs in a worktree with no git on its `PATH`
- **THEN** it prints the round as it printed it before this change, and exits 0
