## ADDED Requirements

### Requirement: A submission keeps its time under test

Every write of a branch's records SHALL keep on each submission's
record `under_test_ms`, the milliseconds a combination on the branch's
revision holding it was under test, combinations that overlap counted
once, and `under_test_since`, since when one is; a submission's time
SHALL stop at its end. They SHALL be kept whether or not telemetry is
on, and nothing sluice decides SHALL read them (154).

#### Scenario: Combinations that overlap count once

- **WHEN** a submission's own combination and a deeper one holding it
  go under test at 12:00, the first is judged at 12:04 and the deeper
  one at 12:10
- **THEN** its record holds 600000 ms under test and no
  `under_test_since`
  (`a_submission_s_time_under_test_counts_overlapping_combinations_once`
  in `crates/sluice-core/src/line.rs`)

#### Scenario: A submission's time stops at its end

- **WHEN** a submission under test since 12:00 is withdrawn at 12:03
- **THEN** its record holds 180000 ms under test and no
  `under_test_since` (`a_submission_s_time_under_test_stops_at_its_end`
  in `crates/sluice-core/src/line.rs`)

#### Scenario: Every write keeps it

- **WHEN** a submission's combination goes under test, the clock
  advances four minutes, and the round that reads its verdict writes
- **THEN** its record holds 240000 ms under test
  (`every_write_keeps_a_submission_s_time_under_test` in
  `crates/sluice-core/src/step.rs`)

### Requirement: A run with no outcome is never its submission's failure

A run that its time limit did not end and in which no check passed or
failed SHALL count one on `runs_with_no_outcome` of each submission its
combination holds, and the submission's life SHALL name the count. It
SHALL never make that life's outcome `failed` or its status an error
(114; rule core.53).

#### Scenario: A run that decided nothing is counted

- **WHEN** runs end that no host took, whose every check is `none`,
  whose checks are `none` and `unproven`, that were given no check, that
  passed a check, that failed one, and that their time limit ended
- **THEN** the first four each count one on every submission their
  combination holds, and the last three none
  (`a_run_that_decided_nothing_is_counted_on_its_submissions` in
  `crates/sluice-core/src/line.rs`)

#### Scenario: A run no host took counts on its submissions

- **WHEN** a combination's run is ended at its time limit with no live
  host that may take it
- **THEN** each of the combination's submissions holds one run with no
  outcome, and none is blamed
  (`a_run_no_host_took_counts_as_no_outcome_on_its_submissions` in
  `crates/sluice-core/src/step.rs`)

### Requirement: A submission's life is a span

While telemetry sends traces, the write that ends a submission SHALL
send one span, `sluice.submission`, from its submission to that write,
whose parent is the trace it kept, else none, naming its repository,
branch, number, head, work, agent, outcome, seconds waiting and under
test, and runs with no outcome, and linking to the span that ended it.
Only `failed` SHALL be an error (154; rule core.53).

#### Scenario: A submission's life in the trace it was submitted under

- **WHEN** telemetry sends traces, a submission is made under
  `TRACEPARENT` `00-<t>-<s>-01` and lands, and another is made under
  none and lands
- **THEN** a `sluice.submission` span is sent for each, the first in
  trace `<t>` with parent `<s>`, the second the root of a trace of its
  own, each starting at its submission, with `sluice.outcome` `landed`
  and `sluice.waited_seconds` and `sluice.tested_seconds` that sum to
  its length
  (`a_submission_s_life_is_a_span_in_the_trace_it_was_submitted_under`
  in `crates/sluice/tests/telemetry_logs.rs`)

#### Scenario: A life names how it ended

- **WHEN** a life is sent for a submission blamed, and one for a
  submission withdrawn after two runs with no outcome
- **THEN** the first span's status is an error, and the second's is
  not, with `sluice.outcome` `withdrawn` and `sluice.no_outcome` 2
  (`a_life_names_how_it_ended_and_its_runs_with_no_outcome` in
  `crates/sluice/src/telemetry.rs`)

### Requirement: A submission's end is measured

While telemetry sends metrics, the write that ends a submission SHALL
add one to `sluice.submissions.ended` and send its seconds waiting with
no combination of it under test, `sluice.submission.waited`, and under
test, `sluice.submission.tested`, each with `sluice.repository`,
`sluice.branch` and `sluice.outcome`, its end's word, and nothing that
names its head, its submitter or its trace (154; rules core.53,
core.66).

#### Scenario: Each end is found once

- **WHEN** a write moves submissions from `queued` to `landed`, from
  `testing` to `withdrawn`, from `failed` to `withdrawn` and from
  `refused` to `queued`, and writes one made `landed`
- **THEN** it names the first, the second and the last as ended, and
  no other (`a_write_names_the_submissions_it_ends` in
  `crates/sluice-core/src/line.rs`)

#### Scenario: Ends are measured by how they ended

- **WHEN** one submission lands, one is blamed and one is withdrawn
  from the queue
- **THEN** sluice reports `sluice.submissions.ended`,
  `sluice.submission.waited` and `sluice.submission.tested` for each,
  with `sluice.outcome` `landed`, `failed` and `withdrawn`, the
  withdrawn one tested 0 seconds, each naming the repository and the
  branch and no field outside `METRIC_KEYS`
  (`a_submission_s_end_is_measured` in
  `crates/sluice/tests/telemetry.rs`)

### Requirement: A run's room is held for its branch

A run's part of the host's bound SHALL be held for the branch it is
for: a combination's for its branch, a session's or a preview's for the
branch its run is held to. The host's registry of running checks SHALL
name it beside the repository; an ask or an entry that names none SHALL
count toward the bound under no branch.

#### Scenario: Runs are counted by branch

- **WHEN** a room holds two parts for `main` and one for `bolt/x` of
  one repository, and its registry holds an entry naming no branch
- **THEN** it counts two runs for `main`, one for `bolt/x`, and all
  four against its bound
  (`the_host_s_running_checks_are_counted_by_branch` in
  `crates/sluice/src/host_process/room.rs`)

#### Scenario: A hold names its branch

- **WHEN** a hold naming `main` is written and read, and one an older
  sluice wrote with no branch is read
- **THEN** the first reads `main` and the second none
  (`a_hold_names_its_branch_and_an_older_one_none` in
  `crates/sluice/src/host_process/protocol.rs`)

### Requirement: The host's room is measured

While its telemetry sends metrics, sluice's host process SHALL send
every three seconds, for each repository and branch with a run counted
against the bound then or at its last sending, `sluice.host.running`,
its runs, and `sluice.host.bound`, the bound, reading 0 once after its
last run; and for each run whose part it held, `sluice.run.waited_for_room`,
the seconds from the ask first refused to the hold, 0 where none was
(71, 150, 154).

#### Scenario: A run's wait is timed by branch

- **WHEN** a combination of `main` is refused at 0 s and held at 20 s,
  one of `bolt/x` of the same repository is held at its first ask, and
  a session's run waits from 5 s and is held at 12 s
- **THEN** the room gives 20 s for `main`, 0 s for `bolt/x` and 7 s for
  the session's branch, once each
  (`a_run_s_wait_for_room_is_measured_by_branch` in
  `crates/sluice/src/host_process/room.rs`)

#### Scenario: The room is sent by branch

- **WHEN** the room counts two runs for `main` with a bound of 4, then
  none, and a wait of 20 s was drained
- **THEN** the first sending reports `sluice.host.running` 2 and
  `sluice.host.bound` 4 for `main` and `sluice.run.waited_for_room` 20,
  the second `sluice.host.running` 0 for `main`, and the third nothing
  for it, each with the repository and the branch
  (`the_host_s_room_is_measured_by_branch` in
  `crates/sluice/src/host_process/mod.rs`)

### Requirement: A submission's times and a run's wait are sent in seconds

`sluice.submission.waited`, `sluice.submission.tested` and
`sluice.run.waited_for_room` SHALL be declared in seconds, the unit
`s`; `sluice.submissions.ended`, `sluice.host.running` and
`sluice.host.bound` SHALL declare no unit (rule core.66).

#### Scenario: Each is declared as it measures

- **WHEN** sluice's meter is sent each of the six as the core and the
  host process emit them
- **THEN** the first three are exported with the unit `s` and the
  others with none (`a_submission_s_times_are_sent_in_seconds` in
  `crates/sluice/src/telemetry.rs`)
