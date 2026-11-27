# host-daemon Specification

## Purpose
`sluice daemon`, sluice's host process kept running on a machine that
chooses it: how it takes the host's place, waits behind another daemon,
hands over in place and stops, so a machine that runs queues for others
needs no command run on it (135, 137, C.8; record 0093 as amended
2026-11-08 and 2026-11-18; rule core.31).

## Requirements

### Requirement: sluice daemon is the host process kept running

`sluice daemon` SHALL run as its user's sluice host process, in the
foreground, moving every repository its index names as the host process
does. It SHALL never end because no queue has work in play or nothing
follows it. It SHALL end only when it is stopped, when its state folder
is gone, or when it cannot take the host's place. sluice SHALL work as
before with no daemon running (135, 137; rule core.31).

#### Scenario: Idle, it keeps running

- **WHEN** `sluice daemon` runs on a host with nothing queued in any
  repository and nothing following it, for three ticks
- **THEN** it still runs, and the host process that answers at the
  socket has the daemon's pid
  (`an_idle_daemon_keeps_running` in `crates/sluice/tests/host_process.rs`)

#### Scenario: A submission to an idle daemon

- **WHEN** `sluice daemon` runs idle, and `sluice submit main --no-wait`
  runs in a repository whose `main` holds a policy
- **THEN** the daemon lands the submission, and no other host process
  starts
  (`a_daemon_lands_a_submission_with_no_other_host_process` in
  `crates/sluice/tests/host_process.rs`)

### Requirement: A daemon moves what other machines submit

A daemon SHALL move work in play that another machine writes to the
records of a repository its index names, a branch whose records are on
its remote among it, with no command run on the daemon's machine (129,
137).

#### Scenario: A gate box lands another machine's submission

- **WHEN** a branch's records are on its remote, its policy's signers
  name the daemon's host, the daemon's host runs `sluice daemon` and no
  other command, and another host submits to the branch
- **THEN** the daemon's host tests and lands the submission
  (`a_daemon_lands_what_another_host_submits` in
  `crates/sluice/tests/peers.rs`)

### Requirement: A daemon started in a repository adds it to the index

`sluice daemon` SHALL need no repository to start. Started in a
repository that has sluice, it SHALL add that repository to the host's
index, as every command of sluice's that finds one does (135).

#### Scenario: Started in a repository

- **WHEN** a repository with a policy on `main` is in no index, and
  `sluice daemon` is started in it
- **THEN** the host's index names the repository
  (`a_daemon_indexes_the_repository_it_starts_in` in
  `crates/sluice/tests/host_process.rs`)

#### Scenario: Started in no repository

- **WHEN** `sluice daemon` is started in `/`
- **THEN** it runs as the host process, and the index is as it was
  (`a_daemon_indexes_the_repository_it_starts_in` in
  `crates/sluice/tests/host_process.rs`)

### Requirement: A daemon takes the host's place from a host process that ends when idle

Where a host process that is no daemon holds the host when `sluice daemon` starts, the daemon SHALL ask it to end, wait as a newer command waits, and take its place. The runs of that process SHALL end with it. Where it has not ended by then, the daemon SHALL exit 3, naming its log.

Before: its runs went on. Now: they end, and the daemon tests them again. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

#### Scenario: A host process a wait started

- **WHEN** a host process started by `sluice wait` runs a combination's checks, and `sluice daemon` starts
- **THEN** the first host process ends, the host process that answers is the daemon, and the wait follows on to its submission's outcome

#### Scenario: A host process that does not hand over

- **WHEN** a process holds the host's lock and answers at its socket as a host process that is no daemon, and never ends when asked to
- **THEN** `sluice daemon` exits 3 once the hand-over's wait has passed, naming the log

### Requirement: A daemon waits behind another daemon

Where another daemon holds the host when `sluice daemon` starts, it SHALL
ask that daemon nothing, say once on stderr that it waits for it, naming
its pid and version, and take the host's place once that daemon ends. It
SHALL never end for the wait.

#### Scenario: A second daemon

- **WHEN** `sluice daemon` runs, and a second `sluice daemon` starts
- **THEN** the second's stderr names the first's pid once, the host
  process that answers is the first, and once the first is stopped the
  second answers
  (`a_second_daemon_waits_and_takes_the_place_of_the_first` in
  `crates/sluice/tests/host_process.rs`)

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

### Requirement: A daemon takes up a settings change by starting afresh in place

A daemon SHALL take up a change of the host's settings that no binding
takes up in place by starting afresh in its own process as the
executable it started as, with the environment it started with, keeping
its pid. The image started afresh SHALL read the settings file itself
(record 0107 as amended 2026-11-13; rule core.54).

#### Scenario: A changed collector

- **WHEN** the host's settings name one collector, `sluice daemon` runs
  with a submission queued, and the file is changed to name another
  collector
- **THEN** the second collector is sent traces, the submission lands,
  and the host process that answers has the daemon's pid and a later
  `since`
  (`a_daemon_takes_up_a_changed_telemetry_table_in_place` in
  `crates/sluice/tests/instance.rs`)

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

### Requirement: A daemon stops when told

`sluice daemon` signalled with `SIGTERM` or `SIGINT` SHALL let each task finish its round. It SHALL then end each worker's process group after the stop's grace, let its locks and its socket go, and exit 0. Each run that it ended SHALL have no outcome.

Before: a stopped daemon left its runs running. Now: a stop ends the runs. (`sluices-runs-end-with-its-host-process@e814b716f3b1`, `workers-end-with-their-owner@292e869158d6`)

#### Scenario: Stopped while a run goes on

- **WHEN** `sluice daemon` runs a combination's checks, and is sent `SIGTERM`
- **THEN** it exits 0, its socket is gone, and no process of the checks runs
- **AND** once `sluice wait` runs, the combination is tested again and the submission lands

### Requirement: sluice --help names sluice daemon

`sluice --help` SHALL name `daemon`, saying it keeps sluice's host
process running on a machine that chooses to, and that nothing needs it.

#### Scenario: The help

- **WHEN** `sluice --help` runs
- **THEN** it names `daemon`, and `sluice daemon --help` says nothing
  needs it
  (`the_help_names_sluice_daemon` in `crates/sluice/tests/cli.rs`)
