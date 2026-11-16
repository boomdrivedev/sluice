# run-progress Specification

## Purpose
How far a combination under test has got: its tasks, recorded while
its run goes, and shown in the overview, `sluice status` and a waiting
command.

## Requirements

### Requirement: A run's tasks are recorded while it goes

While a combination is under test with the moon runner, each round that
finds its run still going SHALL record the run's tasks: every task the
run was to run, each `waiting`, `running`, `passed`, `failed` or
`replayed`, with when it ended and its exit code once it has ended, and
when it started where moon says. A task SHALL read `replayed` only when
moon says it answered from its cache (38). The command runner SHALL
record no tasks.

#### Scenario: Partway through

- **WHEN** a combination's run is to run `a:test`, `b:test` and
  `c:test`; `a:test` has passed, `c:test` has failed with exit code 3,
  and `b:test` has started and not ended
- **THEN** after the next round, `sluice overview --json` lists the
  combination's tasks as `a:test` passed, `b:test` running and
  `c:test` failed with exit 3, each ended task with its end time

#### Scenario: A replay

- **WHEN** moon answers `b:test` from its cache while the run goes
- **THEN** `b:test` reads `replayed`

#### Scenario: The command runner

- **WHEN** a branch's policy runs its checks with the command runner
- **THEN** its combination under test has no tasks in the overview

### Requirement: The tasks decide nothing

The recorded tasks SHALL be no run and no result: no verdict, gate,
blame or result SHALL read them (16; rules core.7, core.9). Recording
them SHALL change no branch's version (61, 62). They SHALL be removed
once their combination is no longer under test.

#### Scenario: A task that read failed

- **WHEN** a task's progress read `failed` while the run went, and the
  finished run reports it passed
- **THEN** the combination passes, and the results log holds only what
  the finished run reported

#### Scenario: The branch's version

- **WHEN** a round records a run's tasks and nothing else changes
- **THEN** the branch's version is what it was before the round

#### Scenario: The run finishes

- **WHEN** the combination's run finishes and its result is read
- **THEN** the overview shows the combination with no tasks

### Requirement: The overview shows the tasks

`sluice overview --json` SHALL give each combination under test whose
tasks are recorded a `tasks` list, each entry
`{target, state, started?, ended?, exit?}`, times as RFC 3339, and
SHALL leave `tasks` out where none are recorded. `sluice overview`
SHALL add how many tasks have ended of how many, as `3/5`, to a
combination under test with recorded tasks. The overview SHALL read
the tasks through the records alone and write nothing (rules core.11,
core.14).

#### Scenario: Text

- **WHEN** a combination under test has 3 of 5 tasks ended
- **THEN** its line in `sluice overview` includes `3/5`

### Requirement: The tasks stay on their host

A run's tasks SHALL be kept on the host that runs it, beside the
branch's records and never among them, and SHALL be readable there
with no sluice process running (26).

#### Scenario: Read with nothing running

- **WHEN** a round recorded a run's tasks and no sluice process is
  running
- **THEN** `sluice overview --json` lists them

### Requirement: A running task says when it started

A run's supervisor SHALL note, at most once a second while the run's
output grows and once as it ends, how long the output was and when, in
the run's own directory, removed with it; it SHALL note nothing of
what the output says. The moon runner SHALL give a running task the
time of the first note that reaches the end of its start checkpoint.
A run with no notes SHALL give its running tasks no start. Nothing a
round writes and no record SHALL change (61, 62; ADR 0028).

#### Scenario: A task started a minute ago

- **WHEN** moon started `b:test` in a run about a minute ago and it has
  not ended
- **THEN** after the next round, `sluice overview --json` lists
  `b:test` as `running` with a `started` within a second of when its
  checkpoint was printed

#### Scenario: A run with no notes

- **WHEN** a run's directory holds no notes, as a run started by an
  older sluice does
- **THEN** its running tasks have no `started`, and the rest of its
  tasks read as before

### Requirement: A combination's progress is one line

A combination under test's progress SHALL read as one line: how many
of its tasks have ended of how many, up to three running tasks,
longest first, each with how long it has run, and the combination's
time against its limit, as
`7/12 tasks · running b:test 4m02s · 6m10s of 30m`. Where no tasks are
kept, it SHALL give the combination's time alone.

#### Scenario: More than three running

- **WHEN** five of a combination's tasks are running
- **THEN** its line names the three that have run longest, then
  `+2 more`

#### Scenario: No tasks kept

- **WHEN** a branch runs its checks with the command runner and
  `main + #3` is under test
- **THEN** its line gives the combination's time against its limit
  and no count

### Requirement: The text views show a combination's progress

`sluice status <branch>`, `sluice status <n>`, a waiting
`sluice submit`, `sluice wait` and `sluice overview` SHALL show a
combination under test's progress line, read from the tasks the
overview reads. No JSON form SHALL change, and none of these SHALL
write anything (rules core.11, core.14).

#### Scenario: The branch's status

- **WHEN** `main + #3` is under test with 1 of its 3 tasks ended and
  `b:test` running
- **THEN** `sluice status main` prints a line
  `testing main + #3 · 1/3 tasks · running b:test …` under its header,
  and its row for #3 is as before

#### Scenario: A submission's status

- **WHEN** the same combination is under test
- **THEN** `sluice status 3` prints that line for #3's combination

#### Scenario: A waiting command

- **WHEN** `sluice wait 3` waits while #3's tasks end one by one
- **THEN** it prints the line each time the count ended or the tasks
  running change, and not at every round
