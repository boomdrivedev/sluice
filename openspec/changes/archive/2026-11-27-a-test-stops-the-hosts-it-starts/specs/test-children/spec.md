## ADDED Requirements

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

## MODIFIED Requirements

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
