# Verify: each-submission-shows-its-wait-and-its-test

Verified on `change/each-submission-shows-its-wait-and-its-test` at
`cf99e821a`, against its base and gate,
`bolt/sluice-reports-its-work-plainly` (S66), on 2026-11-18.

**Verdict: falls short. The change is not archived.** The bolt's gate
fails in `cli:test`. The change causes the failure, so the change goes
back to its code stage.

## The whole checks

`sluice exec bolt/sluice-reports-its-work-plainly` (sluice 0.95.1) ran
`#check-fast` on `cf99e82` against the bolt at `f71e833`. It ended with
**exit 1**: 8 tasks completed and 1 failed.

| Task | Outcome |
|---|---|
| `root:lint`, `root:format`, `root:changes`, `nix:typecheck` | passed |
| `seams:test` (5), `core:test` (178), `fakes:test` (17), `conformance:test` | passed |
| `cli:test` | **failed**: `tests/cli.rs`, 241 passed and 2 failed |

These are the two failing tests in `crates/sluice/tests/cli.rs`:

- `status_lists_the_queue`, which pins times at lines 1558 and 1562;
- `status_and_overview_show_a_hold_while_it_lasts`, which pins a time
  at line 7285.

In both, the `OWED` lines show the outcome's `owed until` and
`handed over by notice at` times 1 to 3 seconds later than the test
pins. For example, `1:landed #1 owed until 2026-10-18T09:00:15Z ·
handed over by notice at 2026-10-11T09:00:19Z` is printed where the
test expects `…09:00:13Z · …09:00:16Z`.

**The cause is in this change.** `sluice_fakes::FakeClock::now` moves
one second each time it is read (`crates/sluice-fakes/src/lib.rs`,
`impl Clock for FakeClock`). Two of this change's additions to
`Sluice::carry` (`crates/sluice-core/src/lib.rs`) now read the clock:

- `keep_under_test(&now())`, on every write where a submission's time
  under test moved (design D2, task 2.3);
- `send_life(…, &at)` with `at = now()`, on every write that ends a
  submission (design D1 and D4, task 3.1).

Before this change, `carry` read no clock on those writes unless the
host runs the branch's queue (`kept`). Each extra read moves every
later timestamp, and the delivery's span and hand-over times are among
them. The code stage has two ways to fix it:

- move the two tests' pinned times to what the change now prints;
- or keep the write's time from a clock read the write already makes,
  so `carry` reads no extra time.

Design D2 says "asking the clock once, as it does to set `kept`". That
holds within one write, but not across the writes that read no clock
before.

**Two scenario tests were never run.** cargo stopped at the first test
binary that failed, `tests/cli.rs`. The integration tests that come
after it never ran in this gate, so the gate does not verify these two
scenario tests:

- `a_submission_s_end_is_measured` (`crates/sluice/tests/telemetry.rs`);
- `a_submission_s_life_is_a_span_in_the_trace_it_was_submitted_under`
  (`crates/sluice/tests/telemetry_logs.rs`).

**What sluice reported for the run.** sluice read `cli:test` as
`could not run`, because its sandbox refused:

- moon's name lookups (`network-outbound remote:*:53` and
  `/private/var/run/mDNSResponder`);
- cargo's write to `~/.cargo/.package-cache`;
- 14 more.

So it recorded nothing under `cli:test`'s key (core.35) and recorded 8
results for `cf99e82`. The refusals did not cause the two failures. The
failures are deterministic and come from the fake clock, as shown
above. When the bolt runs the gate on this work again, the failures
will come back.

## The change against its specs

`openspec validate each-submission-shows-its-wait-and-its-test --strict`
passes. Every task in `tasks.md`, 1.1 to 5.1, is ticked done. I read
each requirement against the code and the test its scenario names.

| Requirement | Scenario test | Where | In the gate |
|---|---|---|---|
| A submission keeps its time under test | `a_submission_s_time_under_test_counts_overlapping_combinations_once` | `sluice-core/src/line.rs` | ok |
| | `a_submission_s_time_under_test_stops_at_its_end` | `sluice-core/src/line.rs` | ok |
| | `every_write_keeps_a_submission_s_time_under_test` | `sluice-core/src/step.rs` | ok |
| A run with no outcome is never its submission's failure | `a_run_that_decided_nothing_is_counted_on_its_submissions` | `sluice-core/src/line.rs` | ok |
| | `a_run_no_host_took_counts_as_no_outcome_on_its_submissions` | `sluice-core/src/step.rs` | ok |
| A submission's life is a span | `a_submission_s_life_is_a_span_in_the_trace_it_was_submitted_under` | `sluice/tests/telemetry_logs.rs` | **not run** |
| | `a_life_names_how_it_ended_and_its_runs_with_no_outcome` | `sluice/src/telemetry.rs` | ok |
| A submission's end is measured | `a_write_names_the_submissions_it_ends` | `sluice-core/src/line.rs` | ok |
| | `a_submission_s_end_is_measured` | `sluice/tests/telemetry.rs` | **not run** |
| A run's room is held for its branch | `the_host_s_running_checks_are_counted_by_branch` | `sluice/src/host_process/room.rs` | ok |
| | `a_hold_names_its_branch_and_an_older_one_none` | `sluice/src/host_process/protocol.rs` | ok |
| The host's room is measured | `a_run_s_wait_for_room_is_measured_by_branch` | `sluice/src/host_process/room.rs` | ok |
| | `the_host_s_room_is_measured_by_branch` | `sluice/src/host_process/mod.rs` | ok |
| A submission's times and a run's wait are sent in seconds | `a_submission_s_times_are_sent_in_seconds` | `sluice/src/telemetry.rs` | ok |

Each test holds what its scenario says:

- `State::ends` covers the five end states.
- `Line::ended` names only the submissions this write moves into an end
  state, or writes ended.
- `keep_under_test` counts overlapping combinations once and stops at
  the end.
- `Ended::decided_nothing` counts runs no host took, runs whose checks
  are all `none` or `unproven`, and runs given no check. It leaves out
  a run its time limit ended. All three end sites in `step.rs` count it
  (`poll`, the run no live host took, and `end_run`).
- The life span's parent is the kept `TRACEPARENT`, or no parent. It
  links to the span that ended it, and only `failed` sets an error.
- The six metrics carry only keys in `METRIC_KEYS`. The three times are
  in `SECONDS`.
- `hold` names the branch through the seam, the room, the registry's
  `Entry` (an older entry reads as no branch) and the protocol's
  `Request::Hold`. `exec.rs` and `preview.rs` name theirs.
- The host process measures the room at each three-second scan and
  drains the waits whether or not it sends.

Record 0014's amendment, `docs/telemetry.md` and the site's telemetry
reference say all of this.

The code meets the specs as I read it. The change falls short on the
gate alone:

1. `status_lists_the_queue` and
   `status_and_overview_show_a_hold_while_it_lasts` fail because of the
   extra clock reads in `Sluice::carry`.
2. The two integration scenario tests above still need to pass in a
   gate run. They can only run once `tests/cli.rs` passes.
