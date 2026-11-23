## MODIFIED Requirements

### Requirement: A daemon takes the host's place from a host process that ends when idle

Where a host process that is no daemon holds the host when `sluice daemon` starts, the daemon SHALL ask it to end, wait as a newer command waits, and take its place. The runs of that process SHALL end with it. Where it has not ended by then, the daemon SHALL exit 3, naming its log.

Before: its runs went on. Now: they end, and the daemon tests them again. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

#### Scenario: A host process a wait started

- **WHEN** a host process started by `sluice wait` runs a combination's checks, and `sluice daemon` starts
- **THEN** the first host process ends, the host process that answers is the daemon, and the wait follows on to its submission's outcome

#### Scenario: A host process that does not hand over

- **WHEN** a process holds the host's lock and answers at its socket as a host process that is no daemon, and never ends when asked to
- **THEN** `sluice daemon` exits 3 once the hand-over's wait has passed, naming the log

### Requirement: A daemon stops when told

`sluice daemon` signalled with `SIGTERM` or `SIGINT` SHALL let each task finish its round. It SHALL then end each worker's process group after the stop's grace, let its locks and its socket go, and exit 0. Each run that it ended SHALL have no outcome.

Before: a stopped daemon left its runs running. Now: a stop ends the runs. (`sluices-runs-end-with-its-host-process@e814b716f3b1`, `workers-end-with-their-owner@292e869158d6`)

#### Scenario: Stopped while a run goes on

- **WHEN** `sluice daemon` runs a combination's checks, and is sent `SIGTERM`
- **THEN** it exits 0, its socket is gone, and no process of the checks runs
- **AND** once `sluice wait` runs, the combination is tested again and the submission lands
