## MODIFIED Requirements

### Requirement: A combination's progress is one line

A combination under test's progress SHALL read as one line: how many
of its tasks have ended of how many, up to three running tasks,
longest first, each with how long it has run, and the time of its
running run against that run's own limit, counted from the run's own
`running`, as
`7/12 tasks · running b:test 4m02s · 6m10s of 30m`. Where no tasks are
kept, it SHALL give that time alone.

#### Scenario: More than three running

- **WHEN** five of a combination's tasks are running
- **THEN** its line names the three that have run longest, then
  `+2 more`

#### Scenario: No tasks kept

- **WHEN** a branch runs its checks with the command runner and
  `main + #3` is under test
- **THEN** its line gives its run's time against its limit and no
  count

## ADDED Requirements

### Requirement: A combination with no run running reads each run's state

Where no run of a combination under test is running, its line SHALL
give each run's state in place of its time: `waiting for room since
<time>` for an open run a live executor may take, `waiting for a host
of <system> since <time>` for one none may, and `starting on <host>`
for a claimed run (run-life).

#### Scenario: A run waiting for room

- **WHEN** `main + #3`'s one run is open and the host that may take it
  has no room
- **THEN** its line is `waiting for room since` with the time the run
  opened, and no time against a limit
  (`a_waiting_run_reads_waiting_since_it_opened` in
  `crates/sluice/src/progress.rs`)
