## Purpose

`sluice host` answers each read of sluice's state from the state that it keeps. A read command sends its folder to `sluice host` and prints the answer. It reads nothing itself.

## ADDED Requirements

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

### Requirement: An older command keeps working against a newer sluice host

`sluice host` SHALL keep answering each request of the protocol of the released sluice: `read`, `results`, `follow`, `serve`, `step`, `pass`, `hold`, `room` and `hand-over`. While anything follows a repository, `sluice host` SHALL keep writing that repository's reader's document in the host's state. The new requests SHALL go beside the old ones. (`never-break-an-existing-client@ed293965fc5c`)

#### Scenario: An older one-shot status

- **WHEN** the new `sluice host` runs, and a client sends `read` for a repository as an older status does
- **THEN** `sluice host` answers with a current reading in the form that it answered before

#### Scenario: An older watch

- **WHEN** the new `sluice host` runs, a client sends `follow` of kind `watch` as an older watch does, and a submission is queued
- **THEN** the reader's document in the host's state holds the submission, as an older watch reads it

#### Scenario: A newer command at an older host process

- **WHEN** an older `sluice host` that does not know `status` answers a newer `sluice status main` with an error
- **THEN** the status names the version of that `sluice host`, reads no record, and exits 3
