# host-driver Specification

## Purpose
Where a host chooses to run `sluice run`, that one process reads and
drives a repository's queues on the host, and every wait and watch there
follows it; where none runs, sluice works as a plain command line, its
waits running the rounds (135, 129, 71; rule core.31; ADR 0068).

## Requirements

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
