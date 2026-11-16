# run-outcome Specification

## Purpose
What a run records when it did not judge its test: a test that could
not start, one that says it could not run here, or one that did not pass
in a run after which the container engine its policy asks for no longer
answers (the `host-requirements` spec). Such a test has no result, so
nothing is recorded under its key, and a key reads only what tests
observed.

## Requirements

### Requirement: A test that could not run records no result

When a test's program could not be started, when it exits with a code
that the branch's `[tests] not_run` lists, or when it did not pass in a
run after which the host finds that the container engine the branch's
policy asks for no longer answers (114; the `host-requirements` spec),
sluice SHALL record no result for it under its key. It SHALL list the
test as not run, with the reason, and read every other test of the run
as before.

#### Scenario: An exit the policy names

- **WHEN** `[tests] not_run = [77]` and a test exits 77
- **THEN** `results.jsonl` gains no line for the test, and the run lists
  it as not run with "exited 77"

#### Scenario: Any other exit

- **WHEN** `[tests] not_run = [77]` and a test exits 1
- **THEN** the test's result is a fail under its key, as before

#### Scenario: A program that cannot start

- **WHEN** a run's program cannot be started on the host
- **THEN** every test of the run is listed as not run with the error,
  and none has a result

#### Scenario: An engine gone after the run

- **WHEN** `main`'s policy says `docker = true`, a test passes and
  another fails, and the container engine no longer answers when the
  host looks after the run
- **THEN** `results.jsonl` gains the pass and no line for the failed
  test, and the run lists it as not run, saying the container engine no
  longer answers; when the host looks, and where, is the
  `host-requirements` spec's

### Requirement: A combination whose tests could not run is tested again

When a combination's run leaves tests not run and fails no test on its
own result, sluice SHALL test the combination again and blame no
submission. When its tests could not run in three runs in a row, sluice
SHALL read the combination as failed and blame it as any failure. The
submitter SHALL be told each test that could not run, and why.

#### Scenario: Once

- **WHEN** a combination's run leaves `cli:tools` not run and nothing
  failed
- **THEN** the combination is tested again and no submission is blamed

#### Scenario: Three times in a row

- **WHEN** the same combination leaves `cli:tools` not run in three runs
  in a row
- **THEN** its newest submission is blamed with `cli:tools` could not
  run, and the reason

#### Scenario: A test that failed on its own

- **WHEN** a run leaves `cli:e2e` not run and `core:test` failed
- **THEN** the combination fails on `core:test` as before, and
  `cli:e2e` is named as not run

### Requirement: A session's run that could not run records nothing of it

When `sluice exec` runs the tests and some could not run, sluice SHALL
record the results of the tests that ran, record nothing for those that
could not run, exit 1, and leave `could not run` as the run's last
result when nothing failed.

#### Scenario: The next run passes

- **WHEN** a session's run leaves `cli:tools` not run, and a later run
  of the same head passes it
- **THEN** `cli:tools`'s key reads passed, not flaky

### Requirement: The operator may set aside a recorded result

`sluice set-aside <task> --at <time> --reason <text>` SHALL find exactly
one recorded result by its test and time, or say which ones match, and
SHALL record the set-aside, with the reason and who set it aside, beside
the results. The result's own line SHALL stay. From then on no branch
SHALL read the result, so a key holding a fail that was set aside and a
pass reads passed.

#### Scenario: A fail a stalled host produced

- **WHEN** `cli:tools`'s key holds a fail and a later pass, and the
  operator sets the fail aside with a reason
- **THEN** the key reads passed, and `results.jsonl` still holds the
  fail

#### Scenario: Two results match

- **WHEN** two results of `cli:tools` were recorded at the time given
- **THEN** nothing is set aside, and both are listed with their hashes

### Requirement: A test the time limit ended has no result

When a branch's time limit ends a run, sluice SHALL keep the result of
every test whose end the task runner recorded before the limit, and SHALL
record no result under the key of a test whose end it did not, whatever
the runner reported of that test (time-limit-1 66). It SHALL name each
such test as left without a result, keep its log, and read the
submission as timed out.

#### Scenario: A test running when the limit came

- **WHEN** a combination's run passes `a:test` and is ended by the time
  limit while `b:test` runs
- **THEN** `results.jsonl` gains a pass for `a:test` and no line for
  `b:test`, and the submission is blamed as timed out with `b:test`
  named and its log kept

#### Scenario: A test that caught the signal and exited

- **WHEN** moon reports a test failed with the exit it took on the
  signal, and its run record is from after the limit
- **THEN** the test has no result and is named as the limit ended it

#### Scenario: A test that failed before the limit

- **WHEN** a test fails and its run record is from before the limit, in
  a run the limit then ended
- **THEN** its fail is recorded under its key, as before

#### Scenario: The same head once the host keeps up

- **WHEN** the same head is submitted again after the limit ended its
  run, and `b:test` passes
- **THEN** the submission lands, and no test reads as flaky

### Requirement: A failure says what to do without changing inputs

When a submission fails on a test read as flaky, sluice SHALL say that
the same head lands once the operator sets the fail aside, or once a
real fix changes what the test reads, and SHALL NOT tell the submitter to
change what the test reads to get it tested again. When a submission
timed out, sluice SHALL name each test the limit ended, say nothing was
recorded for it, and say that the same head may be submitted again on a
slow host, and a test that never ends needs a fix.

#### Scenario: A flaky failure

- **WHEN** a submission fails with `a:test (flaky)` and the other result
  is a fail
- **THEN** the output gives the operator's `sluice set-aside` command,
  says the same head lands once the fail is set aside or a real fix
  changes what `a:test` reads, and has no line asking for a change to
  what `a:test` reads

#### Scenario: A timed-out submission

- **WHEN** a submission timed out while `b:test` ran
- **THEN** the output says `b:test` was running when the time limit ended
  the run, gives its log, and says nothing was recorded for it

### Requirement: sluice runs no readiness command

sluice SHALL run no command the repository writes to ask whether the
host can run the tests, before a run or after one (84, 114; record 0066
as amended 2026-10-30). A policy that names `[tests] ready` SHALL have
its tests run exactly as one that does not: the command is never run,
no run is refused for it, and no test is read as not run because of it.

#### Scenario: A failing ready command before the run

- **WHEN** a branch's policy says `ready = "false"` and its required test
  passes
- **THEN** `sluice exec main` runs the test, records its pass under its
  key, and runs no `false`

#### Scenario: A failing ready command after a fail

- **WHEN** a branch's policy says `ready = "test -e ready"`, the checkout
  holds `ready`, and a test removes it and exits 1
- **THEN** the test's fail is recorded under its key, and the run lists
  no test as not run

#### Scenario: Nothing of it is run at all

- **WHEN** a branch's policy says `ready = "touch asked"` and a test
  fails
- **THEN** no file `asked` exists in the checkout or its scratch after
  the run

#### Scenario: A combination on a host without what a test needs

- **WHEN** a branch's policy names `ready` that would fail on the host,
  and a combination's tests pass
- **THEN** the combination is tested once, passes and lands, and no
  checkout is opened and released for it without a run

### Requirement: A run whose tests could not be started is recorded with why

When a host has claimed a combination's run and cannot start its tests,
sluice SHALL list every test of the run as not run with the error's
words, record nothing under any test's key, release the checkout it
made, blame no submission, and write the run's record in the branch's
state saying why. The run SHALL be open again for any host (16, 114).

#### Scenario: The toolchain fails to enter

- **WHEN** `main`'s `[tests] toolchain` exits 1 in a combination's
  checkout, printing `error: flake.lock is not up to date`
- **THEN** the branch's log records the run with no result, each of its
  tests not run with the toolchain's exit and those words, the
  checkout is released, and `results.jsonl` gains no line

#### Scenario: The checkout cannot be made

- **WHEN** the checkout of a claimed run cannot be made
- **THEN** the run is recorded with no result and the error's words,
  it is open again with no host holding it, nobody is blamed, and it
  counts toward no runs in a row (60; record 0118)
  (`a_run_whose_checkout_cannot_be_made_is_recorded_with_why` and
  `a_claimed_run_whose_combination_the_remote_no_longer_holds_never_began`
  in `crates/sluice-conformance/tests/could_not_start.rs`)

#### Scenario: The executor turns the run down at its start

- **WHEN** a host claimed a run and its executor refuses, at the start,
  a limit the branch's policy states
- **THEN** the run is recorded with no result, each test not run with
  the executor's words

#### Scenario: Another host takes the run

- **WHEN** a host that signs `main`'s runs could not start a run's
  tests, and another host that signs them serves `main`
- **THEN** the run is open with no host holding it, and the other host
  may claim it at its next round

#### Scenario: The host's last run is taken

- **WHEN** a claimed run is not started because the host's bound on
  runs is full
- **THEN** the run is open again as today, with no record of a run with
  no result, and it counts toward nothing

### Requirement: A run that could not be started counts toward the runs in a row

A run whose checkout was made and whose tests could not be started SHALL
count as a run whose tests could not run, so that when a combination's
tests could not run in three runs in a row, whatever the causes, the
combination SHALL be read as not passed, its newest submission blamed,
and its session told each test that could not run and why (115).

#### Scenario: Three starts that fail

- **WHEN** `main`'s toolchain fails to enter in three runs in a row of
  the same combination
- **THEN** the third leaves the combination not passed, its newest
  submission blamed, and its session told each test could not run with
  the toolchain's words, and no fourth checkout is made for it

#### Scenario: Causes mixed

- **WHEN** a combination's first run could not start, its second left
  `cli:tools` exiting a `[tests] not_run` code, and its third could not
  start
- **THEN** the third leaves the combination not passed, each test named
  with the reason of the run that ended it

### Requirement: A run's time limit holds whatever became of what started it
A combination's run SHALL be ended with everything it started when its
branch's time limit passes, by the supervisor that holds the run, whether
or not the step, wait or driver that started it still runs, and the
host's next read of the branch SHALL record it timed out (66; rule
core.10).

#### Scenario: The step that started it has exited
- **WHEN** `sluice step main` starts a combination's run under a one-second limit and exits, and the run's test sleeps for five minutes
- **THEN** within the grace after the limit the run's processes are gone with no sluice command running, and the next `sluice step main` records the run timed out

### Requirement: A combination tested again says why its last run had no outcome

While a combination is tested again after a run that left tests not run
and judged nothing (114), `sluice status <branch>`, `sluice status <n>`
and `sluice wait` SHALL say on its testing line which host's run had no
outcome, and each test that could not run with why, whatever the
cause. It SHALL go once the combination's next run has a result
(record 0105; rules core.11, core.14).

#### Scenario: Status and wait

- **WHEN** #1's combination's run on `this-host` left `plugin:test` not
  run, saying `<why>`, and the combination is being tested again
- **THEN** `sluice status <branch>` prints #1's testing line ending
  `its last run, on this-host, had no outcome: plugin:test could not
  run: <why>`
  (`status_and_wait_say_why_a_combinations_last_run_had_no_outcome` in
  `crates/sluice/tests/cli.rs`)

#### Scenario: Its next run has a result

- **WHEN** the combination's next run passes `plugin:test`
- **THEN** no status or wait says anything of the run that had no
  outcome (`status_and_wait_say_why_a_combinations_last_run_had_no_outcome`)

### Requirement: The testing line names three tests for each reason

Under each reason, the testing line SHALL name up to three tests, in
the run's order, then how many more, and SHALL give each reason once.

#### Scenario: Many tests under one reason

- **WHEN** a run on `build-1` left 28 tests not run under one reason,
  and 1 under another
- **THEN** the testing line names the first three of the 28 and
  `and 25 more`, with the first reason, then the one test with its own
  (`a_combination_whose_last_run_had_no_outcome_says_why` in
  `crates/sluice/src/progress.rs`)

### Requirement: The JSON carries why under lost

`sluice status <n> --json` and `sluice wait --json` SHALL carry the
run that had no outcome under `lost`, with its host, its system where
the policy names one, and each test with why, for as long as the
testing line says it.

#### Scenario: A submission's status and its wait

- **WHEN** #1's combination is tested again after its run on
  `this-host` left `plugin:test` not run, saying `<why>`
- **THEN** `sluice status 1 --json` and `sluice wait 1 --json` carry
  `"lost": [{"host": "this-host", "not_run": [{"task": "plugin:test",
  "reason": "<why>"}]}]`
  (`status_and_wait_say_why_a_combinations_last_run_had_no_outcome`)

### Requirement: The run keeps why it had no outcome

The combination's run in `branch.json` SHALL keep, under `lost`, the
host whose run had no outcome and each test it left not run with why,
from that run until the run's next result.

#### Scenario: A pass with no proof

- **WHEN** a run of a combination on a branch whose state is on its
  remote keeps no proof of its passes
- **THEN** the combination's run in `branch.json` names the host and
  each test with why under `lost`, until its next run has a result
  (`a_pass_nothing_proves_has_no_outcome_and_runs_again` in
  `crates/sluice-conformance/tests/unkept_proof.rs`)

### Requirement: A test that did not pass in a run the sandbox refused something records no result

After a run in which a test did not pass and its time limit did not
end, sluice SHALL read what the sandbox refused the run. Where it
refused anything, each test that failed, was failed by a task it
depends on, or was left without a result SHALL be not run, naming what
was refused, with nothing recorded under its key and nobody blamed. A
pass SHALL stand (114, 139, 149).

#### Scenario: A refused run

- **WHEN** a run of `main`'s combination has `a:test`, which fails, and
  `b:test`, which passes, and the sandbox reports refusing the run
  `network-outbound /private/var/run/mDNSResponder`
- **THEN** `b:test`'s pass is recorded under its key, `a:test` is listed
  as not run, saying sluice's sandbox refused the run a name lookup,
  `results.jsonl` gains no line for it, no submission is blamed, and the
  combination is tested again
  (`a_check_failed_in_a_refused_run_has_no_outcome_and_a_pass_stands` in
  `crates/sluice-conformance/tests/could_not_run.rs`)

#### Scenario: No refusal

- **WHEN** a test fails in a run the sandbox reports refusing nothing
- **THEN** the fail is recorded under its key and blamed, as before, as
  the next run of the combination in the scenario above is

#### Scenario: A test failed by a task the run was refused

- **WHEN** a required test is skipped for a task it depends on that
  failed, in a run the sandbox reports refusing something
- **THEN** neither the test nor the task has a result, and no line is
  recorded under the task's key
  (`a_refusal_leaves_a_check_failed_by_a_dependency_unjudged` in
  `crates/sluice-conformance/tests/failed_dependency.rs`)

#### Scenario: A refusal in every run

- **WHEN** the sandbox refuses every run of a combination something,
  and a test fails in each
- **THEN** after as many runs in a row as the deployment states, the
  combination is not passed and its session is told the test could not
  run, naming what the sandbox refused
  (`a_refusal_in_every_run_reads_not_passed_at_the_bound_naming_it`)

### Requirement: The sandbox's refusals are read only from its own report

sluice SHALL read what the sandbox refused a run only from the kernel's
reports of the run's own mark, kept as the run goes, never from what a
test printed, its exit or how it failed. It SHALL read them only after
a run in which a test did not pass and its time limit did not end, and
never for the open executor (38, 139; rule core.35).

#### Scenario: What the sandbox refused

- **WHEN** a sandboxed test writes outside its checkout and fails
- **THEN** sluice reads that the sandbox refused the run
  `file-write-create` of that path, naming the program that tried
  (`the_sandbox_reports_what_it_refused_a_run` in
  `crates/sluice/tests/checks.rs`)

#### Scenario: A test that prints a refusal

- **WHEN** a sandboxed test writes nothing outside its checkout, prints
  "Operation not permitted" and fails
- **THEN** sluice reads that the sandbox refused the run nothing, and
  the fail is recorded under its key
  (`a_sandbox_run_that_writes_nothing_outside_reports_no_refusal`)

#### Scenario: Two runs at once

- **WHEN** two sandboxed runs go at once on a host and only one is
  refused something
- **THEN** only that run reads a refusal
  (`two_sandbox_runs_at_once_report_only_their_own_refusals`)

#### Scenario: A passing run, or one its time limit ended

- **WHEN** every test of a run passed, or its time limit ended it, and
  the sandbox refused it something
- **THEN** its results are read as before, and what it was refused is
  never read
  (`a_run_that_passed_or_timed_out_never_reads_its_refusals` in
  `crates/sluice-conformance/tests/could_not_run.rs`)

### Requirement: The sandbox's reports are kept from before the run's command starts until after it ends

Each refusal rule of a sandboxed run's profile SHALL carry the run's own
mark, but for a write to `/dev/dtracehelper`, refused with no report.
The kernel's reports of that mark SHALL be kept in the run's directory
from before its command starts until a second after it ends. Keeping
them SHALL never hold the run back (139, C.2 sandbox row; record 0072).

#### Scenario: The command starts once the reports are kept

- **WHEN** a run's watch takes a while to start
- **THEN** its command starts only once the watch has written its first
  line, and the watch is ended after the command
  (`a_watch_is_ready_before_the_command_starts_and_ends_after_it` in
  `crates/sluice/tests/runs.rs`)

#### Scenario: A watch that never starts

- **WHEN** a run's watch exits before writing a line
- **THEN** the run's command runs all the same, and its directory keeps
  what the watch said
  (`a_watch_that_never_gets_ready_is_said_and_the_run_goes_on`)

#### Scenario: A run ended from outside

- **WHEN** a run is ended through its place
- **THEN** its watch is ended too (`ending_a_run_ends_its_watch`)

#### Scenario: Every refusal rule carries the mark

- **WHEN** a sandboxed run's profile is written
- **THEN** every refusal rule in it carries the run's mark, but the one
  for `/dev/dtracehelper`, which reports nothing
  (`each_refusal_of_the_profile_carries_the_runs_mark` in
  `crates/sluice/src/profiles/checks.rs`)

#### Scenario: The reports read

- **WHEN** the kept reports hold the stream's first line, a report of
  another mark, a duplicate report and five refusals of the run's mark
- **THEN** the run's refusals are the five, each once, named as the
  first three and 2 more, a refusal of the resolver's socket named as a
  name lookup
  (`the_runs_marked_reports_are_its_refusals` in
  `crates/sluice/src/profiles/refusals.rs`)

### Requirement: A report that cannot be read is said with the run

Where sluice cannot read what the sandbox refused a run, the run SHALL
be read as one refused nothing, and SHALL say `sluice could not read
what its sandbox refused the run:` with the words of what failed:
`sluice exec` after the run's lines, and a submission's status from the
branch's log (139).

#### Scenario: The log cannot be streamed

- **WHEN** the program that streams the kernel's reports exits at once,
  saying why
- **THEN** sluice reads the run's refusals as unread, with its words
  (`a_sandbox_run_whose_log_cannot_be_read_reports_it_unread` in
  `crates/sluice/tests/checks.rs`)

#### Scenario: Through the queue

- **WHEN** a test fails in a run whose refusals could not be read
- **THEN** the fail is recorded under its key and blamed, and the run's
  `detail` line in the log carries `refusals_unread` with the words
  (`a_run_whose_refusals_could_not_be_read_is_read_as_before_and_says_so`
  in `crates/sluice-conformance/tests/could_not_run.rs`)

#### Scenario: The words

- **WHEN** the words are `log: exited 1`
- **THEN** sluice says `sluice could not read what its sandbox refused
  the run: log: exited 1` (`an_unread_report_is_said` in
  `crates/sluice/src/words.rs`)

### Requirement: A switch, a refusal and an engine gone give one reason

After a run in which a test did not pass, sluice SHALL ask first whether
the host's system was switched, then what the sandbox refused, then,
where the policy asks for one, whether the container engine has gone.
The first that holds SHALL be the reason, and none after it SHALL be
asked (84, 114, 139; records 0074, 0097).

#### Scenario: A switch and a refusal

- **WHEN** a test fails in a run during which the host's system was
  switched and the sandbox refused something
- **THEN** the test is not run, saying the host's system was switched,
  and what the sandbox refused is not read
  (`a_switch_and_a_refusal_name_the_switch` in
  `crates/sluice-conformance/tests/could_not_run.rs`)

#### Scenario: A refusal and an engine gone

- **WHEN** `main`'s policy says `docker = true`, a test fails, the
  sandbox refused the run something, and the container engine no longer
  answers after it
- **THEN** the test is not run, naming what the sandbox refused, and
  the engine is not looked at
  (`a_refusal_and_an_engine_gone_name_the_refusal`)

### Requirement: sluice exec reads a refusal as a combination's run does

A session's run of the branch's tests SHALL read what the sandbox
refused it as a combination's run does, recording nothing for a test
left not run. A command after `--` SHALL never be read so (92, 139;
record 0074).

#### Scenario: A session's run refused something

- **WHEN** a session's run of the branch's tests has a test that fails
  and the sandbox refused the run something
- **THEN** the test is listed as not run, naming what was refused, and
  nothing is recorded under its key
  (`a_session_run_refused_something_records_no_fail` in
  `crates/sluice-conformance/tests/could_not_run.rs`)

#### Scenario: S66

- **WHEN** `sluice exec` runs a test that writes a folder the host does
  not declare, and then runs it again once the host's `[[caches]]`
  declares that folder
- **THEN** the first run lists the test as not run, naming the write the
  sandbox refused, and records nothing; the second records its pass
  under the wider grant, the only result under the test's key
  (`exec_records_nothing_for_a_test_the_sandbox_refused_and_its_pass_under_a_wider_grant`
  in `crates/sluice/tests/exec.rs`)

### Requirement: A run whose checkout could not be made counts toward nothing

A run whose checkout could not be made never began. It SHALL count
toward no runs in a row and SHALL blame no submission, however many
times it is tried (60; record 0118).

#### Scenario: Checkouts that cannot be made, run after run

- **WHEN** a combination's checkout cannot be made in five runs in a
  row, and then can
- **THEN** each of the five is recorded with the error's words and no
  result, no submission is blamed, and once its checkout is made its
  tests run and it lands
  (`checkouts_that_cannot_be_made_run_after_run_blame_no_one` in
  `crates/sluice-conformance/tests/could_not_start.rs`)
