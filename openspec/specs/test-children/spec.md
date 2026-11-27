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

### Requirement: A test waits on a state that another process changes

Another process can change a state that a test of sluice reads. The test SHALL read that state until it is the state that the test expects, up to a limit. It SHALL NOT read it one time and expect it. A test SHALL read such a state one time for each look, and SHALL keep the state that it read.

#### Scenario: A landing that another host made

- **WHEN** a second host lands a first host's submission on the remote
- **THEN** the test reads the submission's state on the first host until it is `landed`, for one minute at most (`a_host_whose_own_main_is_refused_counts_the_remotes_signers` in `crates/sluice/tests/peers.rs`)

#### Scenario: A source that the watcher starts again

- **WHEN** a test kills the watcher's source
- **AND** the killed source runs as a zombie until the watcher reaps it
- **THEN** each look of the test reads the running sources one time
- **AND** the new source is the one source of that read (`watch: source exits` in `plugins/herdr/tests/run.sh`)

### Requirement: The Rust test tasks report a leaked process

The test tasks of each Rust crate of sluice SHALL run each test in a process of its own. Their runner SHALL report a test that leaves a process that holds its output. The runner SHALL NOT fail a test for that alone: the guard fails a test that leaves a process. (`a-leaked-child-fails-its-test@9a82a5147ba6`)

#### Scenario: A leaked sleep

- **WHEN** a test starts `sleep 60` with its output inherited, and returns
- **THEN** the crate's test task names that test as one that leaked

### Requirement: The shell test tasks fail on a leaked process

The shell test tasks of the plugins and the check-e2e tier SHALL each
run under one script. The script SHALL give the test a scratch folder
and a state folder of its own. It SHALL fail the task where a process of
the test runs after the test ends, or a `sluice host` of the test runs
one second after. It SHALL end each such process. It SHALL send a stop
signal that it gets to the test. (`a-leaked-child-fails-its-test@9a82a5147ba6`)

#### Scenario: A watcher left by a shell test

- **WHEN** a shell test starts a watcher in the background and exits 0
- **THEN** the task fails, names the watcher's pid and command, and no process of the watcher runs after the task ends

#### Scenario: A host process left by a shell test

- **WHEN** a shell test starts a host process that holds work, and exits
  0
- **THEN** the task fails, names the host's pid and command, and the
  host does not run after the task ends

#### Scenario: A host process that ends by itself

- **WHEN** a shell test's host process ends a moment after the test
- **THEN** the task passes and says nothing

#### Scenario: A version file of a host that ended

- **WHEN** a shell test leaves the version file of a host that ended,
  and another process now holds its pid
- **THEN** the task passes, and the other process runs on

#### Scenario: A stopped shell test

- **WHEN** the script gets `SIGTERM` while the test and its host process
  run
- **THEN** the test gets `SIGTERM`, the task fails and names the host,
  and no process of the test runs after the task ends

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

### Requirement: A test that a signal stops stops its host processes

When a test process gets `SIGINT`, `SIGTERM` or `SIGHUP`, it SHALL send
`SIGTERM` to each `sluice host` that its live folders name. Then the
signal SHALL end the test process as before. A test runner sends these
signals when a person or a tool interrupts a run, or when a test is
past its time.
(`a-leaked-child-fails-its-test@9a82a5147ba6`)

#### Scenario: An interrupted run

- **WHEN** a test submits work, its `sluice host` holds the submission,
  and a person interrupts nextest while the test runs
- **THEN** nextest reports the test as ended by `SIGINT`, and the test's
  `sluice host` does not run one second after

### Requirement: The crate's test tasks run under the leak script

The test tasks of the `cli` project SHALL run their tests under the
script that the shell test tasks run under. So a test process that
`SIGKILL` ended fails the task, and its `sluice host` stops.
(`a-leaked-child-fails-its-test@9a82a5147ba6`)

#### Scenario: A test process that SIGKILL ended

- **WHEN** a test's `sluice host` holds a submission, and the test
  process gets `SIGKILL`
- **THEN** the task fails, names the host's pid and command, and the
  host does not run after the task ends

### Requirement: A test does not open files again and again in a folder that sluice host watches

A test of sluice SHALL NOT open a file again and again in a folder that `sluice host` watches. On Linux, each open is a file event. notify adds a watch only after it has read all the events that came. So events that do not stop hold back each new watch of `sluice host`. A test that asks for a lock again and again SHALL ask on one descriptor that it opened one time.

#### Scenario: A daemon that starts afresh while a test asks for the host's lock

- **WHEN** a daemon starts afresh in place on Linux, and a test asks for the host's lock all through the start
- **THEN** the image started afresh answers, and the test never gets the lock (`a_daemon_starting_afresh_in_place_keeps_the_hosts_lock` in `crates/sluice/tests/host_process.rs`)
