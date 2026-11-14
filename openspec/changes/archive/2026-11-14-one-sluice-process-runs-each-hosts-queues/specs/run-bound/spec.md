## RENAMED Requirements

- FROM: `### Requirement: A running check is recorded where every sluice on the host reads it`
- TO: `### Requirement: The host's count is kept in the host process`
- FROM: `### Requirement: Two repositories never both take the last free run`
- TO: `### Requirement: A run holds its room from the count that chose it to its start`

## MODIFIED Requirements

### Requirement: The bound is the host's, across every repository

The host's bound, `[runs] max` in the host's config, SHALL count every
check running on the host: every repository's submissions' runs, agents'
runs (`sluice exec`) and previews' runs. A submission's run SHALL start
only while that count is under the bound, whatever repository, branch or
window it serves (71).

#### Scenario: Three repositories share a bound of three (S60)

- **WHEN** three repositories on one host each have submissions queued,
  the host's config sets `max = 3`, and four combinations, at least one
  in each repository, are ready to test at once
- **THEN** three runs start, and the fourth starts only once one of the
  three has ended, whichever repository it belongs to

#### Scenario: Another repository's agent run holds the bound

- **WHEN** the bound is 2, an agent in repository B is running `sluice
  exec`, repository A's own run is under test, and A has another
  combination ready
- **THEN** A starts no further run until one of the two ends

### Requirement: The host's count is kept in the host process

sluice's host process SHALL keep the host's count, each run started, each
room held and each run waiting, and no other process of this sluice SHALL
decide whether a run may start (71, 135). A running check SHALL also be
recorded in the host's state, never in a branch's records, so the next
host process counts runs that outlived the last, and an older sluice
counts them and is counted. A record whose run ended, or whose room was
given back unstarted, SHALL count for nothing (core.11, core.18).

#### Scenario: A killed run frees its part of the bound

- **WHEN** a submission's run's supervisor is killed with its record left
  in the host's state
- **THEN** the host process counts it no longer and may start a run in
  its stead

#### Scenario: A finished run waiting for its result to be read

- **WHEN** a submission's run has ended and its round has not yet read its
  result
- **THEN** it does not count against the bound

#### Scenario: A run that outlived the host process

- **WHEN** the bound is one, a combination's run is under test, and the
  host process is killed and then started again by a command
- **THEN** the new host process counts the run, and starts no other
  until it ends

### Requirement: A run holds its room from the count that chose it to its start

A round SHALL hold the room for each run it decides to start before it
writes that decision, and keep it through its landings, releases and
outcomes until the run starts, so no other run takes it between (71;
rule core.49). A start that fails, a refused write of the decision and a
round that stops SHALL give the room back at once. No run SHALL be
marked starting with no room held, save under a pool's job's claim, and
one so marked, a stopped round's, SHALL be opened again by the next
round.

#### Scenario: An agent's check asks between the count and the start (S75)

- **WHEN** the bound is one, a round decides to start a combination's run
  and then lands an earlier submission before starting it, and `sluice
  exec main` asks for room during that landing
- **THEN** the combination's run starts, its checkout is made, and the
  agent's check waits until that run ends

#### Scenario: A start that fails

- **WHEN** a round holds room for a combination's run and making its
  checkout fails
- **THEN** the room is free again at once, and a waiting agent's check
  starts at its next ask

#### Scenario: Two rounds race for one free run

- **WHEN** the bound is 3, two runs are running, and rounds in two
  repositories each decide at once to start one run
- **THEN** exactly one of the two runs starts, and the other combination
  waits and is started by a later round once the host has room

## ADDED Requirements

### Requirement: A start refused at the bound starts no further run that round

A start the check seam refuses at the bound SHALL start no further run
that round: each run decided behind it SHALL be opened again, its room
given back, with no place made (71; rule core.49).

#### Scenario: A start refused at the bound

- **WHEN** the bound is two, a round decides two combinations' runs, and
  the check seam refuses the first start at the bound
- **THEN** neither run starts, the second is open again with no place
  made, and the next round starts both

### Requirement: A pool's job holds the room for the run its claim holds

A run a round marks starting under the claim it gives a pool's job SHALL
hold no room of that round's host. The job's own round SHALL hold the
run's room as it starts it, and SHALL open the run again where its host
has none (88, S19; rule core.49).

#### Scenario: A pool's job starts the run its claim holds (S19)

- **WHEN** a lead marks a run starting under the claim it gives a pool's
  job, and the job's round runs
- **THEN** the job's round holds the run's room and starts it, and the
  run is not opened again as a stopped round's

### Requirement: A combination's run starts first, taking turns with sessions' runs

Where a combination's run and a run a session or a preview asks for wait
for room at once, the combination's SHALL start first. Once a
combination's run has started in room a waiting session's or preview's
run could have taken, the next such room SHALL go to that waiting run,
and the turn SHALL then be the combinations' again (150, S75). The part
of the bound the host's config keeps for combinations, `[runs]
reserved`, SHALL never be a session's or a preview's.

#### Scenario: Turns on a bound of two (S75)

- **WHEN** the bound is two with none reserved, two agents' checks hold
  both runs, a third agent's check waits, and a combination falls due on
  `main`
- **THEN** when the first agent's run ends the combination's run starts
  and the third agent's check still waits, and when the second agent's
  run ends the third agent's check starts, though `main`'s next
  combination waits

#### Scenario: The reserve

- **WHEN** the bound is two with one reserved, one agent's check runs,
  and no combination waits
- **THEN** a second agent's check waits, and the free run is left for a
  combination

### Requirement: Sessions' and previews' runs start in the order they came to wait

Among runs a session or a preview asks for, the one that has waited
longest SHALL start first, whatever repository or branch it is for, and
no such run SHALL start while one that came to wait before it still
waits (150).

#### Scenario: Two agents wait

- **WHEN** the bound is one and held, and an agent's `sluice exec` in
  repository A starts waiting, and then one in repository B
- **THEN** when the run ends, A's check starts and B's goes on waiting

### Requirement: A run waits only while it is asked for

A combination's run SHALL count as waiting only within thirty seconds of
its branch's round last asking room for it. A session's or a preview's
SHALL count only while the command that asked for it waits, and SHALL
stop counting once that command ends, however it ends (150; rule
core.49).

#### Scenario: An agent's check stopped while it waits

- **WHEN** an agent's `sluice exec` waiting for room is stopped with
  Ctrl-C, and the bound then has a free run
- **THEN** the next agent's check that waits starts in it, and the stopped
  one is counted nowhere

#### Scenario: A queue whose rounds stopped

- **WHEN** a combination's run waited for room and no round has asked for
  it for thirty seconds
- **THEN** an agent's check that asks for room starts in the next free
  run

### Requirement: A check waiting for room says why

A `sluice exec` or `sluice preview` waiting for room SHALL say once why
it waits: `waiting: the host is at its bound`, or `waiting: a
combination's run starts first` while a free run is owed to a waiting
combination.

#### Scenario: A free run owed to a combination

- **WHEN** the bound is two, one run is free, a combination waits for it
  and the turn is the combinations', and an agent runs `sluice exec main`
- **THEN** it prints `waiting: a combination's run starts first`

### Requirement: A freed part goes to the repository with the fewest runs under test

A part of the bound that frees SHALL go to a repository with a
combination's run ready that has the fewest runs under test on the host,
its runs holding a part of any kind, and among equals to the one whose
ready run has waited longest; within a repository its own order of runs
SHALL stand (71, 13; record 0123). No run under test SHALL be stopped to
make room.

#### Scenario: One waiting beside a busy repository

- **WHEN** the bound is three, repository A has three runs under test
  and four combinations ready, and repository B has one combination ready
  and none under test
- **THEN** when one of A's runs ends, B's combination's run starts in its
  part, and A's next waits
  (`a_freed_part_goes_to_the_repository_with_the_fewest_under_test` in
  `crates/sluice/src/host_process/room.rs`)

#### Scenario: Equals, the longest waiting first

- **WHEN** the bound is two and held, repositories A and B each have one
  run under test, and B's combination came to wait before A's
- **THEN** the next part that frees starts B's combination's run
  (`among_equals_the_longest_waiting_goes_first` in
  `crates/sluice/src/host_process/room.rs`)

#### Scenario: Nothing is stopped

- **WHEN** repository A holds every run of the bound and repository B's
  combination comes to wait
- **THEN** each of A's runs goes on to its end, and B's starts in the
  first part that frees
  (`nothing_under_test_is_stopped_for_the_share` in
  `crates/sluice/src/host_process/room.rs`)

### Requirement: The repository a freed part is owed to takes it at once

When a part of the bound frees, the host process SHALL have the
repository it is owed to pass at once, so the part goes to that
repository rather than to whichever asks first (71; record 0123).

#### Scenario: A busy repository asks first

- **WHEN** repository A's run ends while A's task is mid-pass and asks
  for a part at once, and repository B, with none under test, has a
  combination waiting
- **THEN** A is refused, B's task passes, and B's combination's run takes
  the part
  (`a_freed_part_wakes_the_repository_it_is_owed_to` in
  `crates/sluice/src/host_process/tasks.rs`)

### Requirement: A pool's runs are outside the bound

A run launched on a pool SHALL hold no part of the host's bound and
SHALL count in no repository's runs under test, so a repository on a
pool neither takes a part nor waits for one (record 0119; record 0123).

#### Scenario: A repository on a pool

- **WHEN** the bound is one and held by repository A's run, and
  repository B's policy launches its combination's run on a pool
- **THEN** B's run is launched with no part held, and A's next
  combination still waits only for A's run to end
  (`a_pools_launch_holds_no_part` in
  `crates/sluice-conformance/tests/peers.rs`)
