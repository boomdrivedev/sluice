## Purpose

A test of sluice fails when it leaves a process that continues to run. So no test leaves a host process, a run or a git command behind it.

## ADDED Requirements

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

### Requirement: The Rust test tasks fail on a leaked process

The test tasks of each Rust crate of sluice SHALL run each test in a process group of its own. Their runner SHALL fail a test that leaves a process that holds its output. (`a-leaked-child-fails-its-test@9a82a5147ba6`)

#### Scenario: A leaked sleep

- **WHEN** a test starts `sleep 60` with its output inherited, and returns
- **THEN** the crate's test task fails and names that test as one that leaked

### Requirement: The shell test tasks fail on a leaked process

The shell test tasks of the plugins and the check-e2e tier SHALL each run under one script. The script SHALL give the test a scratch folder and a state folder of its own. It SHALL fail the task where a process of the test runs after the test ends, and SHALL end that process. (`a-leaked-child-fails-its-test@9a82a5147ba6`)

#### Scenario: A watcher left by a shell test

- **WHEN** a shell test starts a watcher in the background and exits 0
- **THEN** the task fails, names the watcher's pid and command, and no process of the watcher runs after the task ends
