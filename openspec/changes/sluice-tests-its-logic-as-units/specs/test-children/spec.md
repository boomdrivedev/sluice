## ADDED Requirements

### Requirement: A unit test starts no process

A test of sluice's logic SHALL run in the crate that holds the logic, through the crate's seams and the fakes of `sluice-fakes`. It SHALL start no process, no git and no network. Where a wait or a pace matters, it SHALL run on the fakes' clock or on tokio's paused clock. (`logic-is-tested-as-units-and-few-tests-run-the-binary@3131d0f94b83`)

#### Scenario: A round's intake of a pull request

- **WHEN** a test proves that a round takes a reviewed pull request into its branch's queue
- **THEN** the test runs `sluice::run` in its own process, over the fakes and a fake forge
- **AND** no process starts while it runs

#### Scenario: Ten statuses at one time

- **WHEN** a test proves that ten statuses during one reading share that reading
- **THEN** the test runs the host process's handler in its own process, on tokio's paused clock

### Requirement: A small end-to-end set runs the real binary

The tests that start the `sluice` binary SHALL be the end-to-end set alone. The set SHALL have one test for each command's main path, and one for each part of the host process's life. It SHALL have one contract test for each seam with a real tool. Each test of the set SHALL end each process that it starts. The set SHALL start at most 4 tests at one time. (`logic-is-tested-as-units-and-few-tests-run-the-binary@3131d0f94b83`, `a-leaked-child-fails-its-test@9a82a5147ba6`)

#### Scenario: A command's main path

- **WHEN** the `tools` task runs
- **THEN** `wt_submit_lands_or_fails_by_its_exit` starts the binary, lands a submission through a real host process and real git, and ends each process that it started

#### Scenario: The set's bound

- **WHEN** the `tools` task runs on a host with 16 cores
- **THEN** at most 4 tests of the set run at one time

### Requirement: Each behaviour is proven by one test

Each behaviour of sluice that a test proves SHALL be proven by exactly one test. A test that proves what another test proves SHALL go. (`logic-is-tested-as-units-and-few-tests-run-the-binary@3131d0f94b83`)

#### Scenario: Two tests of one behaviour

- **WHEN** a test of the binary and a scenario of the conformance crate both prove that a lapsed claim runs again on another host
- **THEN** the conformance scenario stays, and the test of the binary goes
