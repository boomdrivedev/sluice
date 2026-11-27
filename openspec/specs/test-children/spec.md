# test-children Specification

## Purpose
A test of sluice fails when it leaves a process that continues to run. So no test leaves a host process, a run or a git command behind it.

## Requirements

### Requirement: A test fails when it leaves a process that runs

When a test of sluice ends, a guard SHALL stop each `sluice host` that the test started. The guard SHALL then fail the test where a process of the test still runs. It SHALL name each such process by its pid and its command. A test that passes in its body SHALL fail all the same. (`a-leaked-child-fails-its-test@9a82a5147ba6`)

#### Scenario: A test that leaves a run

- **WHEN** a test submits work, a combination's checks start, and the test ends while the checks run
- **THEN** the guard stops the test's `sluice host`, the test fails, and its words name the pid and the command of the checks

#### Scenario: A clean test

- **WHEN** a test lands a submission, and its checks and git commands ended before the test ends
- **THEN** the guard stops the test's `sluice host`, finds no other process, and the test passes

### Requirement: The guard ends each process that it finds

After the guard names the processes of a test, it SHALL end each of them. It SHALL send `SIGTERM` first, and `SIGKILL` after a grace of two seconds. No process of a test SHALL run after the guard is done. (`a-leaked-child-fails-its-test@9a82a5147ba6`)

#### Scenario: A check that ignores SIGTERM

- **WHEN** a test leaves a check that ignores `SIGTERM`
- **THEN** the test fails, and no process of that check runs three seconds after the test ends

### Requirement: The guard finds a detached process of a test

The guard SHALL find each process of a test, detached or not. A process of a test is one whose environment names the test's state folder. A process whose working folder is in one of the test's scratch folders is one too. Each test that starts `sluice host` SHALL give it a state folder of its own. (`a-leaked-child-fails-its-test@9a82a5147ba6`)

#### Scenario: A detached host process

- **WHEN** a test's command starts `sluice host` in a session of its own, and the test ends with that host process running
- **THEN** the guard finds it by the test's state folder and stops it

#### Scenario: Another test's process

- **WHEN** two tests run at one time, and the first ends while the second's `sluice host` runs
- **THEN** the first test's guard does not name or end the second test's process

### Requirement: The Rust test tasks report a leaked process

The test tasks of each Rust crate of sluice SHALL run each test in a process of its own. Their runner SHALL report a test that leaves a process that holds its output. The runner SHALL NOT fail a test for that alone: the guard fails a test that leaves a process. (`a-leaked-child-fails-its-test@9a82a5147ba6`)

#### Scenario: A leaked sleep

- **WHEN** a test starts `sleep 60` with its output inherited, and returns
- **THEN** the crate's test task names that test as one that leaked

### Requirement: The shell test tasks fail on a leaked process

The shell test tasks of the plugins and the check-e2e tier SHALL each run under one script. The script SHALL give the test a scratch folder and a state folder of its own. It SHALL fail the task where a process of the test runs after the test ends, and SHALL end that process. (`a-leaked-child-fails-its-test@9a82a5147ba6`)

#### Scenario: A watcher left by a shell test

- **WHEN** a shell test starts a watcher in the background and exits 0
- **THEN** the task fails, names the watcher's pid and command, and no process of the watcher runs after the task ends

### Requirement: A test names a run's check by the leader of its group

A test of sluice that finds a run's check by its command line SHALL
name the process that leads its process group. The run's supervisor
starts the check in a group of its own, so the check leads that group.
A copy that the check's shell forked SHALL NOT be named, because it is
in the group of the shell. So a test that reads the parent of the
check reads the run's supervisor, whatever the timing of the forks of
the shell.

#### Scenario: A check's shell between a fork and an exec

- **WHEN** a check's shell holds a copy of itself that it forked, and
  the copy has the command line of the shell
- **THEN** each read of the test names the shell, which leads the
  group, and does not name the copy

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
