## ADDED Requirements

### Requirement: A run is in one of five states, written in its branch's home

A combination's run SHALL be in one of five states, written in its
branch's home: `open`, waiting for an executor or for room; `claimed`,
held by one claim, its checkout named; `running`, its checks started,
since when; `ended`, holding its report; and `closed`, nothing of it
left to do. A run SHALL move only forward, and a run whose checks must
run again SHALL be a new run (record 0122; 75, 88).

#### Scenario: A run's life on one host

- **WHEN** `main + #1` is opened on a branch in the host's own files and
  its run passes
- **THEN** the branch's log holds, for its run, `opened`, `claimed`,
  `running`, `ended` and `closed` in that order, and `branch.json` ends
  with the run `closed` and its report under `ended`
  (`a_runs_states_are_written_in_order` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: A run's state is written by its claim holder or the lead that opened it

A run's state SHALL be written only by the instance whose claim holds it
or by the lead that opened it. The lead writes `open`, a pool's claim
and its `running`, and the end of a run whose claim lapsed or that no
live executor may take; the claim holder writes the rest, a pool's job
its run's end (record 0122; 75, 77, 88).

#### Scenario: Another host's claim

- **WHEN** host B claims a run of `main`, whose records are on its
  remote, and runs it to its end while host A leads
- **THEN** host B wrote `claimed`, `running`, `ended` and `closed`, and
  host A wrote only `open`
  (`a_claim_holder_writes_its_own_runs_states` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: Nothing acts on a run before the state naming the act is written

A run's checkout SHALL be made, and its checks started, only once the run
is written `claimed`; a pool's job dispatched only once the run is
written `claimed` by the pool's claim; and its checkout released only
once the run is written `ended`. A submitter SHALL be told only once the
outcome is written, and a branch landed only once the landing is written
(record 0122; rule core.14).

#### Scenario: A host that stops after starting the checks

- **WHEN** host A writes `main + #1`'s run `claimed`, makes its checkout,
  starts its checks, and stops before it writes `running`
- **THEN** A's next round finds the supervisor's run for the checkout,
  writes the run `running` from the run's own start, starts nothing
  again and releases nothing
  (`a_run_started_before_its_running_was_written_is_not_started_again` in
  `crates/sluice-conformance/tests/run_life.rs`)

#### Scenario: A host that stops after making the checkout

- **WHEN** host A writes the run `claimed`, makes its checkout, and stops
  before it starts the checks
- **THEN** A's next round finds the checkout made at the combination,
  starts the checks in it once, and makes no second checkout
  (`a_checkout_made_before_the_start_is_used_once` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: Each act leaves a proof the next reader checks

Each act on a run SHALL leave a proof that the next reader checks before
it acts, so an act found done is not done again: the checkout the place
profile lists at its combination, the run the supervisor holds for the
checkout, the workflow run titled with the claim, the results recorded
under the run's keys, the checkout no longer listed, the hand-over
recorded, and the branch holding the landing (record 0122; rule
core.16).

#### Scenario: A host that stops after recording the results

- **WHEN** host A records a run's results and stops before it writes the
  run `ended`
- **THEN** A's next round writes `ended` from the same report, and no
  result is recorded twice
  (`results_recorded_before_the_end_was_written_are_not_recorded_again`
  in `crates/sluice-conformance/tests/run_life.rs`)

#### Scenario: A host that stops after releasing

- **WHEN** host A releases an ended run's checkout and stops before it
  writes the run `closed`
- **THEN** A's next round finds the checkout gone, writes the run
  `closed`, and runs no release command again
  (`a_release_found_done_is_closed_and_not_run_again` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: A run's time limit counts from its own running

A run's time limit SHALL count from the time its state was written
`running`, from the run's own start. An `open` or `claimed` run SHALL be
under no limit while a live executor that may take it is there (66, 87).

#### Scenario: A run that waited for room

- **WHEN** `main`'s time limit is 25 minutes, a run of `main + #1` waits
  30 minutes for room on the one host that may take it, then runs for
  10 minutes and passes
- **THEN** it is not timed out, `main + #1` passes, and nobody is blamed
  (`a_run_that_waited_for_room_has_its_whole_limit` in
  `crates/sluice-conformance/tests/run_life.rs`)

#### Scenario: A follow-up run's limit

- **WHEN** a run of `main + #1` ran 20 minutes of its 25 and a follow-up
  run holds one of its checks
- **THEN** the follow-up run's limit counts 25 minutes from its own
  `running` (`a_follow_up_run_has_a_limit_of_its_own` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: A run no live executor may take ends with no outcome

An `open` run that no live executor may take, by its system, by the
branch's signers or because each live host of it turned it down, SHALL
be ended by the lead once the combination's time limit has passed since
it opened, with no outcome for each check, each saying who turned it
down and why, or that no host that may take it was live (66, 87; rule
core.22).

#### Scenario: A system no live host is

- **WHEN** a run of `main + #3` needs `macos-arm64`, no host of it is
  live and no pool runs it, and `main`'s time limit of 25 minutes passes
  since the run opened
- **THEN** the run ends with no outcome, `cli:build` saying no host that
  may take it was live, and `results.jsonl` gains nothing
  (`a_run_no_live_executor_may_take_blames_no_one` in
  `crates/sluice-conformance/tests/run_life.rs`)

#### Scenario: Turned down for its engine

- **WHEN** the only live host turns a run down because no container
  engine answers there, and the time limit passes
- **THEN** the run ends with no outcome, each check naming the host and
  its reason (`a_run_no_host_takes_for_its_engine_is_not_passed_at_its_time_limit`
  in `crates/sluice-conformance/tests/engine.rs`)

### Requirement: A combination whose run no executor took is not passed and blames no one

A combination with a run ended because no live executor may take it
SHALL read not passed. No check of it SHALL be written failed, nobody
SHALL be blamed, it SHALL count toward no runs in a row whose checks
could not run, and its submissions SHALL go back to the queue and be
formed again (66, 87, 115; rule core.22).

#### Scenario: Formed again

- **WHEN** `main + #3`'s run ended because no host of `macos-arm64` was
  live
- **THEN** `main + #3` is not passed, #3 is not blamed and stays queued,
  and the next round forms it again
  (`a_run_no_live_executor_may_take_blames_no_one` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: A run's report is read only once the run has ended

A run's report SHALL be read only once its executor says the run has
ended: the supervisor's own word that the run is over, or a pool's
record written under its claim. Nothing of a run SHALL be recorded or
read while it goes (60, 114).

#### Scenario: A report read while the run goes

- **WHEN** a run of `main + #103`'s 39 checks is running, and a report of
  it holding one pass is there to read
- **THEN** nothing of the run is recorded or read until it has ended
  (`a_report_is_not_read_while_its_run_goes` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: A check the report holds no outcome for has none

A run's report SHALL give an outcome for each check the run was given. A
check it holds none for, neither passed, failed, refused nor kept out of
the runner's CI by the runner's own word, SHALL have no outcome, saying
the report holds none for it. It SHALL never be written failed, and
nobody SHALL be blamed for it (60, 114; rule core.22).

#### Scenario: A report holding one check of 39

- **WHEN** a run of 39 checks has ended and its report holds a pass of
  one
- **THEN** the pass is recorded, the other 38 have no outcome, each
  saying the report holds none for it, #103 is not blamed, and a
  follow-up run holds the 38
  (`a_check_the_report_holds_no_outcome_for_is_never_failed` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: A follow-up run holds only what a run left undecided

A run that ends with replays no counted pass proves (108) or checks with
no outcome (60, 77, 114) SHALL keep every other check's outcome. The lead
SHALL open one follow-up run of the same system holding only those
checks, naming the run it follows, with no replay for the checks whose
replays were unproven (record 0122; S42).

#### Scenario: An unproven replay

- **WHEN** a run of `main + #1` passes `a:test` by running it and replays
  `b:test`, whose key no counted pass proves
- **THEN** the run ends with `a:test` passed and `b:test` unproven, a
  follow-up run holds `b:test` alone with no replay, and once it passes
  `main + #1` passes (`an_unproven_replay_runs_again_alone` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: A combination's verdict is read over a run and its follow-ups

A combination's verdict SHALL be read over each run and its follow-ups,
each check by its last outcome, and a fail SHALL be read at once,
whatever is left undecided beside it. Runs in a row that left checks
with no outcome for 114's causes SHALL count toward the runs in a row
whose checks could not run as before (115).

#### Scenario: A fail beside an unproven replay

- **WHEN** a run fails `a:test` and replays `b:test` unproven
- **THEN** the combination fails on `a:test` at once, and no follow-up
  run is opened (`a_fail_beside_an_unproven_replay_is_read_at_once` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: A change of lead changes no run

A claim SHALL lapse only when its own holder has shown no sign of life
for the deployment's bound, by the reader's own observation, or when the
pool's job holding it has ended with no record of its run; never because
the lead changed or the reader took the lead. The claim holder's report
SHALL be written whichever instance leads (73, 77, 88).

#### Scenario: The lead moves under a running claim

- **WHEN** host A leads `main` and runs one of its runs under its own
  claim, and host B takes the lead after the lease while A keeps showing
  its sign of life
- **THEN** A's claim holds, A writes its run `ended` with its report, and
  the run passes (`a_change_of_lead_keeps_a_live_claim` in
  `crates/sluice-conformance/tests/run_life.rs`)

#### Scenario: A lead that stops

- **WHEN** host A leads `main`, runs one of its runs under its own claim,
  and stops
- **THEN** once B has seen A's sign of life stand for the bound, the run
  ends with no outcome, saying A's claim lapsed, and a follow-up run
  holds its checks (`a_stopped_leads_claim_lapses_by_its_own_sign_of_life`
  in `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: A run's written end stands

A run written `ended` or `closed` SHALL never be written otherwise by any
reader, and a report that finds its run ended SHALL write nothing. Where
a landing finds no signed result behind a passed run, the run SHALL
stand, and the checks with none SHALL read as missing for the
combination, which a follow-up run settles (78; rule core.22).

#### Scenario: A late report

- **WHEN** host A's claim on a run lapsed, the lead wrote the run
  `ended` with no outcome, and A then reads its run's report
- **THEN** nothing of A's report is written, and the run's end is the
  lead's
  (`s20_a_lapsed_claim_runs_again_elsewhere_and_its_late_report_is_not_written`
  in `crates/sluice-conformance/tests/peers.rs`)

#### Scenario: A passed run with no signed result

- **WHEN** a run of `main + #1` ended passed, and the landing reads no
  signed result behind `cli:build`
- **THEN** the run stays `ended` passed, a follow-up run holds
  `cli:build`, and nothing lands until it passes
  (`a_passed_run_with_no_signed_result_stands_and_is_followed_up` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: A status reads each run's state

`sluice status <branch>`, `sluice status <n>`, a wait and `sluice
overview` SHALL give each run of a combination under test by its state:
an open run as waiting since it opened, a claimed run as starting on its
host, and a running run as testing so long of its own limit; the
overview's document SHALL carry each run's `state` and `since` (26;
rules core.11, core.14).

#### Scenario: A run waiting for room

- **WHEN** a run of `main + #1` has waited 12 minutes for room on the one
  host that may take it
- **THEN** `sluice status main` says `waiting for room since` with the
  time it opened, and no time against its limit
  (`a_run_waiting_for_room_says_since_when` in
  `crates/sluice-conformance/tests/reading.rs`)

#### Scenario: A running run

- **WHEN** a run of `main + #1` waited 30 minutes for room and has run
  for 4 minutes of a 25-minute limit
- **THEN** its line gives `4m00s of 25m` (`a_running_runs_time_is_its_own`
  in `crates/sluice-conformance/tests/reading.rs`)
