## MODIFIED Requirements

### Requirement: A newer sluice's command has an older host process hand over

A newer sluice's command SHALL have an older host process hand over. The older process SHALL finish each task's round. It SHALL then start the newer sluice in its place, with the same pid and its workers. It SHALL hand it the host's lock, the start lock and the socket. No other process SHALL get the lock between the two. Where the older process ends instead, the command SHALL start its own (`sluices-runs-end-with-its-host-process@e814b716f3b1`).

#### Scenario: The host's sluice is upgraded

- **WHEN** a host process of sluice 0.71.0 runs, and a submit of sluice 0.72.0 runs on the host
- **THEN** a 0.72.0 host process answers with the pid of the 0.71.0 process, and the submission lands

#### Scenario: A round that outlasts a start's wait

- **WHEN** a 0.72.0 command asks a 0.71.0 host process to hand over
- **AND** the process ends after one of its tasks takes twelve seconds to finish its round
- **THEN** the command waits for the 0.71.0 process to end, and then reaches a 0.72.0 host process of its own start

#### Scenario: Another process asks for the lock during the hand-over

- **WHEN** a newer command has a host process hand over, and another process asks for the host's lock all through the hand-over
- **THEN** that process never gets the lock
- **AND** the host process that answers has the first pid and a later `since`
  (`a_host_process_starting_afresh_in_place_keeps_the_hosts_lock` in
  `crates/sluice/tests/host_process.rs`)
