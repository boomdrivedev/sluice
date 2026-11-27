## ADDED Requirements

### Requirement: A test reads the whole command line of a process

A test of sluice that reads the command line of a process SHALL read all of it, on Linux and on macOS. The guard of the shell test tasks SHALL do this too. A test SHALL NOT read a command line cut to the width of a terminal or to `COLUMNS`. It SHALL NOT read the name of a process where it needs the command line.

#### Scenario: A narrow COLUMNS

- **WHEN** `COLUMNS` is 80, and the herdr plugin's watch tests read the processes beside the watcher
- **THEN** each test reads the source's whole command line, and finds the source (`watch: ten repositories, four processes` in `plugins/herdr/tests/run.sh`)

#### Scenario: The guard on Linux

- **WHEN** a shell test leaves `sleep 61` on Linux
- **THEN** the guard names the process as `sleep 61` (`A test that leaves a watcher` in `tests/leaks-cases.sh`)

### Requirement: A test reads the parents of a process that runs

A test of sluice that reads the parents of a process SHALL read them where that process still runs. A process that ended has no parent to read. So a test that finds which process started a command SHALL have the command record a parent that runs after the command ends.

#### Scenario: A release under dash

- **WHEN** `/bin/sh` is dash, and a test reads which process started a worktree's release
- **THEN** the release is a list, its parent is `sluice host`, and the test finds `sluice host` among the parents (`a_worktrees_release_runs_in_sluice_host` in `crates/sluice/tests/exec.rs`)
