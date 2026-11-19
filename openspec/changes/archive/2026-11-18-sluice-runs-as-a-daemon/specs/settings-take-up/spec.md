## MODIFIED Requirements

### Requirement: Work in play never waits for a command across the host process's hand-over

The host process SHALL start its successor itself, holding the host's
start lock from before it lets its own lock go until its successor
answers: a host process of another pid, or a daemon started afresh in
place with its own pid (host-daemon). So a command that meets neither
waits on the start lock and reaches the successor, and work in play
never waits for another command to start a host process (135; record
0107 as amended 2026-11-13).

#### Scenario: A submit during the hand-over

- **WHEN** the host process is handing over for a changed `[telemetry]`
  and a submit runs on the host meanwhile
- **THEN** the submit reaches the successor, exactly one host process
  runs once the hand-over ends, and the submission lands with no other
  command run
  (`a_submit_during_a_hand_over_reaches_the_successor` in
  `crates/sluice/tests/host_process.rs`)

#### Scenario: A submit during a daemon's start afresh

- **WHEN** `sluice daemon` is starting afresh in place for a changed
  `[telemetry]`, and a submit runs on the host meanwhile
- **THEN** the submit reaches the daemon started afresh, exactly one host
  process runs, and the submission lands
  (`a_submit_during_a_daemons_start_afresh_reaches_it` in
  `crates/sluice/tests/host_process.rs`)
