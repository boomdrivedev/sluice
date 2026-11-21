## MODIFIED Requirements

### Requirement: A daemon stops when told

`sluice daemon` signalled with `SIGTERM` or `SIGINT` SHALL let each task finish its round. It SHALL then end each worker's process group after the stop's grace, let its locks and its socket go, and exit 0. Each run that it ended SHALL have no outcome.

Before: a stopped daemon left its runs running. Now: a stop ends the runs. (`sluices-runs-end-with-its-host-process@e814b716f3b1`, `workers-end-with-their-owner@292e869158d6`)

#### Scenario: Stopped while a run goes on

- **WHEN** `sluice daemon` runs a combination's checks, and is sent `SIGTERM`
- **THEN** it exits 0, its socket is gone, and no process of the checks runs
- **AND** once `sluice wait` runs, the combination is tested again and the submission lands
