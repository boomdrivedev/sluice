## MODIFIED Requirements

### Requirement: One host process for each user on a host

A host SHALL run at most one sluice host process for each user, whatever
repositories that user's commands run in. A second `sluice host` started
while the first lives SHALL end at once and move nothing, and a `sluice
daemon` SHALL take the first's place or wait for it (host-daemon).
Several users on a host SHALL each have their own, each with its own
bound (135).

#### Scenario: Two repositories, one process

- **WHEN** a submission is queued in repository A and another in
  repository B, by the same user, each with `--no-wait`
- **THEN** one host process runs, and it lands both

#### Scenario: A second host process

- **WHEN** a host process runs and `sluice host` is started by hand
- **THEN** the second exits at once, and the first goes on

### Requirement: The host process ends once nothing is in play and nothing follows it

The host process SHALL end once no repository it holds has work in play
and nothing follows it, unless it is a daemon. Work in play is a
submission queued or under test, a checkout to release or a preview to
drop. A follower is a watch, a wait or a `sluice run`. A run waiting for
room or holding it from its count to its start also keeps it. It SHALL
also end at its next tick once its state folder is gone. A command that
meets it ending SHALL start another, so none is lost between the two
(135).

#### Scenario: The queue empties

- **WHEN** the one submission queued on a host lands, and nothing
  follows the host process
- **THEN** the host process has ended within two ticks of the landing

#### Scenario: A watch keeps it

- **WHEN** `sluice overview --json --watch` runs in a repository with
  nothing queued
- **THEN** the host process runs for as long as the watch does, and ends
  within two ticks of the watch's end

#### Scenario: The state folder is removed

- **WHEN** a host process runs with work in play and its state folder is
  deleted
- **THEN** it has ended within two ticks, and its socket is gone

#### Scenario: A daemon's queues empty

- **WHEN** the one submission queued on a host whose host process is
  `sluice daemon` lands, and nothing follows it
- **THEN** the daemon still runs three ticks after the landing
  (`an_idle_daemon_keeps_running` in `crates/sluice/tests/host_process.rs`)

### Requirement: A newer sluice's command has an older host process hand over

A command of a newer sluice that reaches an older host process SHALL
have it hand its repositories over and end, finishing the round each
task is in, starting no other and letting its locks go, then start its
own, having waited for it to end as long as it gives its tasks to finish
their rounds; an older daemon starts afresh in place instead
(host-daemon). A command of an older sluice that reaches a newer host
process SHALL use it. Versions SHALL be compared as release numbers.

#### Scenario: The host's sluice is upgraded

- **WHEN** a host process of sluice 0.71.0 runs, and a submit of sluice
  0.72.0 runs on the host
- **THEN** the 0.71.0 process ends, a 0.72.0 host process runs, and the
  submission lands

#### Scenario: A round that outlasts a start's wait

- **WHEN** a 0.72.0 command asks a 0.71.0 host process to hand over, and
  one of its tasks takes twelve seconds to finish its round
- **THEN** the command waits for the 0.71.0 process to end, and then
  reaches a 0.72.0 host process of its own start

### Requirement: The overview names the host process and the runs waiting

`sluice overview --json` SHALL give `host_process`, the host process's
`version`, `pid`, `since` and `daemon`, or null where none answers,
beside `host`, the host's name. Its `room` SHALL give `waiting`, the runs
waiting for room by kind, `turn`, whose the next room a combination and
a session both want is, `queue` or `sessions`, and `repositories`, each
repository's runs under test and runs waiting. `sluice status` with no
target SHALL print the same room on its line (150, 71).

#### Scenario: A session's check waits behind a combination

- **WHEN** the bound is one, a combination's run holds it, and `sluice
  exec main` waits for room
- **THEN** `sluice overview --json` gives `room.waiting.sessions` 1 and
  `host_process.pid` the host process's pid

#### Scenario: Two repositories' runs

- **WHEN** repository A has two runs under test and repository B has one
  combination waiting for room
- **THEN** `sluice overview --json` in either repository gives
  `room.repositories` with A's two runs under test and B's one waiting

#### Scenario: A daemon

- **WHEN** `sluice daemon` runs, and `sluice overview --json` runs in a
  repository
- **THEN** it gives `host_process.daemon` true and `host_process.pid` the
  daemon's pid; with a host process a wait started, `host_process.daemon`
  is false
  (`the_overview_names_a_daemon` in `crates/sluice/tests/host_process.rs`)
