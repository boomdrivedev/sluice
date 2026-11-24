## ADDED Requirements

### Requirement: The herdr plugin's tests wait on events

The herdr plugin's tests SHALL wait for what they check by checking it
at least ten times a second, within a bound in seconds. A test SHALL
NOT sleep a fixed time to wait for a watcher or a process to end.

#### Scenario: A host at rest

- **WHEN** `moon run plugin-herdr:test-sh` runs on a host with no other
  work
- **THEN** it passes within 90 seconds

### Requirement: The herdr plugin's tests run in groups at once

`plugin-herdr:test-sh` and `plugin-herdr:test-dash` SHALL each run the
plugin's tests in four groups at once, each group in a scratch folder
of its own, and SHALL fail where any group fails or leaves a process.

#### Scenario: One group fails

- **WHEN** one test of the third group fails
- **THEN** `moon run plugin-herdr:test-sh` fails naming that test, and
  the other groups' tests run to their end

### Requirement: No tools test is a chain of scenarios

Each test of `cli:tools` SHALL hold one scenario. A test that steps
the queue until a state holds SHALL wait at most 50 milliseconds
between its steps.

#### Scenario: A host at rest

- **WHEN** `moon run cli:tools` runs on a host with no other work
- **THEN** no test takes more than 60 seconds
