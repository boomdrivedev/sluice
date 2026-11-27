# resident-reads Specification

## Purpose
`sluice host` answers each read of sluice's state from the state that it keeps. A read command sends its folder to `sluice host` and prints the answer. It reads nothing itself. `sluice host` answers only the requests of its own release.

## Requirements

### Requirement: sluice host answers each read from the state that it keeps

`sluice host` SHALL answer a status, an overview without `--watch`, the home view, a forecast, `sluice inbox` and a wait's record. Each answer SHALL be the command's output on standard output and standard error, and its exit code. The client SHALL print the answer and SHALL exit with that code. (`reads-go-through-the-resident@69134e5825d7`)

#### Scenario: A status in text and in JSON

- **WHEN** `#4` is queued on `main`, and `sluice status main` and `sluice status main --json` run
- **THEN** each prints `main`'s queue with `#4`, as a status printed it before this change, and exits 0

#### Scenario: The home view

- **WHEN** a worktree submitted `#4` to `main`, and `sluice` runs in that worktree with no command
- **THEN** it prints the worktree's home view with `#4`, and exits 0

### Requirement: A read command sends its folder and reads nothing

A read command SHALL send its folder to `sluice host`. `sluice host` SHALL find the repository, the worktree, the branch and the head from the folder. The read command SHALL NOT run git. It SHALL NOT read a file of the repository, of the records or of the results.

Before: a read command found its repository with git, and read the records itself. Now: `sluice host` does both. (`reads-go-through-the-resident@69134e5825d7`)

#### Scenario: A status with no git

- **WHEN** `sluice host` runs, `#4` is queued on `main`, and `sluice status main --json` runs with no git on its `PATH`
- **THEN** it prints `main`'s queue with `#4`, and exits 0

#### Scenario: A folder in no repository

- **WHEN** `sluice status main` runs in a folder that is in no git repository
- **THEN** it prints the error that a status printed there before this change, and exits with the same code

### Requirement: A read past its bound is answered from the last state

Where `sluice host` has no current reading within the read's bound, it SHALL answer from the state that it last kept. It SHALL say on standard error when it last read the repository. Where no `sluice host` answers, the read command SHALL exit 3 and SHALL say so.

Before: the command read the records itself. Now: `sluice host` answers, or the command exits 3. (`reads-go-through-the-resident@69134e5825d7`)

#### Scenario: A host process held in a slow reading

- **WHEN** the remote of `main` does not answer, so the reading of the repository does not end, and `sluice status main` runs
- **THEN** after the read's bound it prints `main`'s queue as `sluice host` last kept it, says when that was, and exits 0

#### Scenario: No host process answers

- **WHEN** a process holds the socket of `sluice host`, answers `hello`, and closes the connection when `sluice status main` asks `status`
- **THEN** the status says that sluice's host process did not answer, reads no record, and exits 3

### Requirement: sluice host answers another tool on its socket

Any process that speaks the protocol on the socket of `sluice host` SHALL get the same answers as a sluice command. It SHALL NOT need to start a sluice command to read sluice's state. (`flume-has-a-resident@49876bc5c73c`)

#### Scenario: A client of another tool

- **WHEN** a test client, which is no sluice command, says `hello` on the socket and asks `status` of `main` for a worktree's folder
- **THEN** it gets the output that `sluice status main` prints, and no sluice command was started

### Requirement: sluice host names its version to a request that it does not know

`sluice host` SHALL answer a request that it does not know with an error. The error SHALL name the client's version and the version of `sluice host`, and SHALL say to run sluice of that version. `sluice host` SHALL NOT answer `read`, `follow` or `room`. A newer command SHALL have an older `sluice host` hand over before it asks.

Before: `sluice host` answered each request of each earlier release. Now: it answers only the requests that a command of its own release sends.

#### Scenario: An older one-shot read

- **WHEN** a client says `hello` as sluice 0.97.0 to a newer `sluice host`, and asks `read` of a repository
- **THEN** it gets an error that names 0.97.0 and the version of `sluice host`, and says to run sluice of that version
- **AND** `sluice host` read nothing for it

#### Scenario: An older watch

- **WHEN** a client says `hello` as sluice 0.97.0 to a newer `sluice host`, and asks `follow` of a repository as a watch
- **THEN** it gets the same error, and `sluice host` writes no file under `readers/` for that repository

#### Scenario: A newer command at an older host process

- **WHEN** an older `sluice host` that does not know `status` answers a newer `sluice status main` with an error
- **THEN** the status names the version of that `sluice host`, reads no record, and exits 3

### Requirement: A read gives one version of a branch's records whole

A read of a branch's records in the host's own files SHALL give one
version of the records whole, also while a round writes them. It SHALL
NOT fail because a write moved a file during the read. It SHALL NOT give
files of two versions (131, core.41).

#### Scenario: A status while a round lands

- **WHEN** a round lands a submission on `main` and moves it to the
  finished records while `sluice status main --json --all` reads them
- **THEN** the status gives `main`'s submissions at one version, the
  landed one once, and exits 0
  (`a_read_beside_writes_is_one_version_whole` in
  `crates/sluice/tests/local_files.rs`)
