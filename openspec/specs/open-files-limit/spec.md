# open-files-limit Specification

## Purpose
Every sluice process raises its soft limit on open files to its hard
limit as it starts, so a supervisor's low default holds neither sluice
nor the runs it starts, and no host need raise it for sluice (135;
record 0126).

## Requirements

### Requirement: sluice raises its soft limit on open files as it starts

Every sluice process SHALL raise its soft limit on open files to its
hard limit, as far as the system lets one process open, before it
reads a file or starts a thread or a process. A soft limit already at
its hard limit SHALL be left as it is. The runs it starts SHALL have
the raised limit: nothing of sluice's sets a run's limit (135; record
0126; rule core.59).

#### Scenario: Started under launchd's default

- **GIVEN** a soft limit on open files of 256 and a higher hard limit
- **WHEN** `sluice exec <branch> -- <command>` runs
- **THEN** the command runs under a soft limit equal to its hard limit,
  or on macOS to `kern.maxfilesperproc` where that is lower

### Requirement: A refused raise is said and sluice goes on

Where the system refuses the raise, sluice SHALL say so on its
standard error, `sluice: warning: the limit on open files stays as
sluice was started with:` and the system's words, and SHALL go on
under the limit it was given (record 0126).

#### Scenario: The system refuses

- **GIVEN** a system that refuses a raise of the soft limit
- **WHEN** any sluice command runs
- **THEN** it warns once on standard error and does what it would do
  under the limit it was given
