## MODIFIED Requirements

### Requirement: A host process that stops loses nothing

The host process SHALL keep nothing that the records and the host's state do not hold. A run SHALL end with the host process that owns it, unless a hand-over passes it on. The next host process SHALL carry the work on from the records, and SHALL run no check again for a combination that has a result.

Before: a run continued after its host process ended. Now: a run ends with its owner. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

#### Scenario: Killed while a combination is under test

- **WHEN** the host process is killed while a combination's checks run, and `sluice status main` then runs
- **THEN** a host process is running again, and it ended the checks' process group before it started a worker
- **AND** the run has no outcome, and the combination is tested again

#### Scenario: A result read before the kill

- **WHEN** a combination's checks ended with a result, and the host process is killed before it lands the combination
- **THEN** the next host process lands the combination with no check run again

### Requirement: A newer sluice's command has an older host process hand over

A newer sluice's command SHALL have an older host process hand over. The older process SHALL finish each task's round and let its locks and socket go. It SHALL then start the newer sluice in its place, with the same pid and its workers. A command of an older sluice SHALL use a newer host process.

Before: the successor was detached, and the runs lost their owner. Now: the successor starts in place and owns the runs. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

#### Scenario: The host's sluice is upgraded

- **WHEN** a host process of sluice 0.71.0 runs, and a submit of sluice 0.72.0 runs on the host
- **THEN** a 0.72.0 host process answers with the pid of the 0.71.0 process, and the submission lands

#### Scenario: A round that outlasts a start's wait

- **WHEN** a 0.72.0 command asks a 0.71.0 host process to hand over, and one of its tasks takes twelve seconds to finish its round
- **THEN** the command waits until a 0.72.0 host process answers with the same pid, and then gets its answer
