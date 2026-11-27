## MODIFIED Requirements

### Requirement: A newer sluice's command has a daemon start afresh in place

A command of a newer sluice that reaches an older daemon SHALL ask it to
hand over, naming its own executable. The daemon SHALL let each task
finish its round. It SHALL then start afresh in its own process as that
executable, running `daemon` with the environment it started with. It
SHALL keep its pid, so whatever keeps it running keeps the new one. It
SHALL hand its locks and socket to the new image, so no other process
gets the host's lock meanwhile. The command SHALL then use it (135).

#### Scenario: The daemon is asked to hand over

- **WHEN** `sluice daemon` runs with a submission queued, and a command
  asks it to hand over, naming the sluice executable
- **THEN** a host process with the daemon's pid and a later `since`
  answers as a daemon
- **AND** its log says it starts afresh as that executable, and the
  submission lands
  (`a_daemon_asked_to_hand_over_starts_afresh_in_place` in
  `crates/sluice/tests/host_process.rs`)

#### Scenario: A newer command reaches an older daemon

- **WHEN** a process holds the host's lock and answers at its socket as
  a daemon of sluice 0.71.0
- **AND** a command of sluice 0.72.0 reaches the host process
- **THEN** the command asks it to hand over naming the command's own
  executable, and reaches the host process started in its place
  (`a_newer_client_has_an_older_daemon_start_afresh` in
  `crates/sluice/tests/host_process.rs`)

#### Scenario: Another process asks for the lock during the start afresh

- **WHEN** `sluice daemon` runs, a newer command has it start afresh,
  and another process asks for the host's lock all through
- **THEN** that process never gets the lock
- **AND** the host process that answers has the daemon's pid and a later
  `since`
  (`a_daemon_starting_afresh_in_place_keeps_the_hosts_lock` in
  `crates/sluice/tests/host_process.rs`)

### Requirement: A command during a daemon's start afresh reaches the daemon

A daemon SHALL hold the host's start lock and its own lock across its
start afresh. It SHALL hand both to the image started afresh, which
holds the start lock until it answers. So a command that meets neither
waits on the start lock and reaches the daemon started afresh. No other
host process starts (135).

#### Scenario: A submit during the start afresh

- **WHEN** a daemon is starting afresh for a changed `[telemetry]`, and
  a submit runs on the host meanwhile
- **THEN** the submit reaches the daemon started afresh, exactly one
  host process runs, and the submission lands
  (`a_submit_during_a_daemons_start_afresh_reaches_it` in
  `crates/sluice/tests/host_process.rs`)

### Requirement: A daemon whose start afresh fails goes on as it was

Where a daemon's start afresh fails, it SHALL keep its lock and take its
socket back. It SHALL go on passing every repository it holds, and say
once in its log the executable and why. A newer command that asked it
SHALL fail, naming the log, once the hand-over's wait ends.

#### Scenario: An executable that is not there

- **WHEN** `sluice daemon` runs, and a command asks it to hand over
  naming an executable that is not there
- **THEN** the host process that answers has the daemon's pid and its
  first `since`, and its log names the executable once
  (`a_daemon_whose_start_afresh_fails_goes_on` in
  `crates/sluice/tests/host_process.rs`)
