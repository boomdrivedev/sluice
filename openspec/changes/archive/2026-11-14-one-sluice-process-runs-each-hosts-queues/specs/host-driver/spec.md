## ADDED Requirements

### Requirement: sluice run follows the host process

`sluice run` SHALL run no round itself. It SHALL reach sluice's host
process, starting it where none answers, and follow it for its
repository until it is stopped (135; rule core.31). It SHALL print what
each pass of the repository prints, the lines a pass of `sluice run`
printed, and each branch the pass passed by with its words. `--rounds
<n>` SHALL end it after n passes, exiting 3 where its last pass passed a
branch by, and 0 otherwise.

#### Scenario: A unit that still runs sluice run

- **WHEN** a host's service manager runs `sluice run` in a repository
  with a submission queued
- **THEN** a host process runs, `sluice run` prints the landing the host
  process's pass made, and no second process forms a combination

#### Scenario: One pass

- **WHEN** `sluice run --rounds 1` runs in a repository whose `main` has
  one submission queued and nothing under test
- **THEN** it exits 0 after one pass of the host process, and `main`'s
  records hold the submission's combination

### Requirement: sluice run says nothing needs it

`sluice run` SHALL say once on stderr that sluice starts its own host
process and nothing needs `sluice run`, and SHALL be left out of `sluice
--help` (135; rule core.31).

#### Scenario: Started by hand

- **WHEN** a person runs `sluice run` in a repository
- **THEN** stderr's first line says sluice starts its own host process
  and nothing needs `sluice run`, and `sluice --help` names no `run`

## REMOVED Requirements

### Requirement: sluice run is its repository's driver on the host

**Reason**: No `sluice run` drives a repository. The host process moves
every repository's queues on the host (135).
**Migration**: "sluice run follows the host process", and `host-process`,
"A command that needs the queues moved reaches the host process or
starts it".

### Requirement: A second sluice run drives nothing while the first lives

**Reason**: No `sluice run` drives a repository (135). The host process
is the one that moves its queues, and a second `sluice run` is a second
follower.
**Migration**: `host-process`, "One host process for each user on a
host", and "sluice run follows the host process".

### Requirement: The driver is the host's reader of its repository

**Reason**: The host process is each repository's reader (129).
**Migration**: `overview-reader`, "The host process is each repository's
reader".

### Requirement: A reader gives its place to a driver that starts

**Reason**: No watch or wait is a reader to give the place up. One that
finds no reader starts the host process (129).
**Migration**: `overview-reader`, "The host process is each repository's
reader".

### Requirement: A wait that finds the driver live runs no round

**Reason**: No wait runs a round, whatever runs on the host (135; rule
core.31).
**Migration**: `submission-wait`, "A wait follows the host process to
the submission's outcome".

### Requirement: A submit beside a driver still makes its submission

**Reason**: A submit always makes its own submission and tells the host
process. There is no driver to stand beside (135).
**Migration**: `host-process`, "A submit writes its submission and tells
the host process".

### Requirement: A wait that finds no driver runs rounds as before

**Reason**: A plain install has the host process, which a wait starts
where none answers. No wait runs rounds (135, 129).
**Migration**: `submission-wait`, "A wait follows the host process to
the submission's outcome", and `overview-reader`, "A host process that
stops is started again within a tick".

### Requirement: The driver reads a branch while a combination is under test

**Reason**: The host process's pass runs the round of every branch in
play, a combination under test or a checkout waiting to be released
among them, as part of the pass `sluice run` ran (129, 135).
**Migration**: `host-process`, "A pass reads a branch while a
combination is under test".
