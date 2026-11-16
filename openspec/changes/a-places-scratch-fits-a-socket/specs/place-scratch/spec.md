# Spec Delta

## RENAMED Requirements

- FROM: `### Requirement: A checkout from an older sluice is released where it was made`
- TO: `### Requirement: A worktree's checkout from an older sluice takes the short scratch at its next run`

## MODIFIED Requirements

### Requirement: A worktree's checkout from an older sluice takes the short scratch at its next run

A worktree's checkout whose record names a scratch under the
repository's `sluice/.tmp/`, or names none, SHALL be given a scratch
under the user's directory at its first run with none of its runs going.
Its record SHALL name the new path before the scratch is moved. Its
former scratch SHALL be moved to the new path, what is in it kept (35
as amended at agentplot/blueprints f15896b; record 0063 as amended
2026-11-05; rule core.30).

#### Scenario: A worktree's checkout with no scratch on its record

- **WHEN** a worktree's checkout record names no scratch, its scratch
  directory is at the former name and holds a file, and the worktree
  runs `sluice exec`
- **THEN** the run is given a scratch under the user's directory, the
  record names it, the file is in it, and the former directory is gone
  (`a_place_from_before_takes_the_short_scratch_at_its_next_run` in
  `crates/sluice/tests/profiles/places.rs`)

#### Scenario: A worktree's checkout whose record names a scratch in the repository

- **WHEN** a worktree's checkout record names a scratch under the
  repository's `sluice/.tmp/` that holds a file, and the worktree runs
  `sluice exec` twice
- **THEN** both runs are given the same scratch under the user's
  directory, the record names it, the file is in it, and the former
  directory is gone
  (`a_place_in_the_repository_takes_the_short_scratch_at_its_next_run`
  in `crates/sluice/tests/profiles/places.rs`)

### Requirement: A check under the sandbox writes its scratch and binds sockets in it

Under the sandbox, a check SHALL be able to write anywhere under its
scratch and to bind and connect to a Unix socket anywhere under it, at
the path `SLUICE_TMP` names, which SHALL be the path the checkout's
record names and the sandbox's rules name, resolved, at a worktree's run
and at a landing's alike (35 as amended, C.4 sandbox row).

#### Scenario: A tool's session socket in a folder under the scratch

- **WHEN** on macOS a worktree at a path longer than 255 bytes, of a
  branch whose policy names `[executor] kind = "sandbox"`, runs
  `sluice exec` with a check that makes `$SLUICE_TMP/x/sessions/work/`
  and binds a Unix socket at `x-client.sock` in it
- **THEN** the check passes, and the socket's path is under the resolved
  path of `/tmp`, then `/sluice-` and the user's id, and shorter than 104
  bytes
  (`exec_under_the_sandbox_binds_a_socket_in_a_folder_under_its_scratch`
  in `crates/sluice/tests/exec.rs`, its binary given no
  `SLUICE_USER_SCRATCH`)

#### Scenario: A tool's session socket in a folder under a landing's scratch

- **WHEN** on macOS a worktree at a path longer than 255 bytes submits
  to a branch whose policy names `[executor] kind = "sandbox"` and a
  check that makes `$SLUICE_TMP/x/sessions/work/` and binds a Unix socket
  at `x-client.sock` in it, failing where `$SLUICE_TMP` is not under the
  resolved path of `/tmp`, then `/sluice-` and the user's id, or the
  socket's path is not shorter than 104 bytes
- **THEN** the candidate's check passes and the submission lands
  (`a_landing_under_the_sandbox_binds_a_socket_in_a_folder_under_its_scratch`
  in `crates/sluice/tests/exec.rs`, its binary given no
  `SLUICE_USER_SCRATCH`)

## ADDED Requirements

### Requirement: A run beside a worktree's going run keeps its former scratch

While a run of a worktree whose checkout record names its former scratch
is going, a run beside it SHALL be given the former scratch, and the
record SHALL still name it (35 as amended at agentplot/blueprints
f15896b; record 0063 as amended 2026-11-05; rule core.30).

#### Scenario: A run of the worktree is going

- **WHEN** a worktree's checkout record names a scratch under the
  repository's `sluice/.tmp/`, a run of the worktree is going, and the
  worktree runs `sluice exec` beside it
- **THEN** the run beside it is given the former scratch and the record
  still names it; once the going run has ended, the next run is given a
  scratch under the user's directory with the former scratch's file in
  it (`a_place_with_a_run_going_keeps_its_former_scratch_for_it` in
  `crates/sluice/tests/profiles/places.rs`)

### Requirement: A former scratch the system cannot move is removed

Where the system cannot move a worktree's former scratch to its new
path, the new path SHALL be made empty and the former scratch removed
(35 as amended at agentplot/blueprints f15896b; record 0063 as amended
2026-11-05; rule core.30).

#### Scenario: A former scratch that cannot be moved

- **WHEN** a worktree's checkout record names a scratch under the
  repository's `sluice/.tmp/`, a directory that is not empty already
  stands at its new path, and the worktree runs `sluice exec`
- **THEN** the run is given the new path, the record names it, and the
  former directory is gone
  (`a_former_scratch_that_cannot_be_moved_is_removed` in
  `crates/sluice/tests/profiles/places.rs`)

### Requirement: A candidate's checkout from an older sluice is released where its record names

A candidate's checkout made by an older sluice SHALL keep its scratch at
the path its record names until it is released, and its release SHALL
remove that directory (35 as amended; record 0063 as amended; rule
core.30).

#### Scenario: A candidate's checkout whose record names a scratch in the repository

- **WHEN** a candidate's checkout record names a scratch under the
  repository's `sluice/.tmp/` and the checkout is released
- **THEN** that directory and the record are gone
  (`a_place_in_the_repository_takes_the_short_scratch_at_its_next_run`
  in `crates/sluice/tests/profiles/places.rs`)

## REMOVED Requirements

### Requirement: A checkout whose record names a scratch in the repository is released there

**Reason**: A worktree's checkout whose record names a scratch under the
repository's `sluice/.tmp/` takes the short scratch at its next run (35
as amended; record 0063 as amended 2026-11-05), as the requirement "A
worktree's checkout from an older sluice takes the short scratch at its
next run" says; a candidate's is released where its record names, as
"A candidate's checkout from an older sluice is released where its
record names" says.

**Migration**: None: a host's sluice moves each worktree's scratch at its
next run.
