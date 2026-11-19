# Verify: each-submission-shows-its-wait-and-its-test

Verified on `change/each-submission-shows-its-wait-and-its-test` at
`fec871eb5`, against its base and gate,
`bolt/sluice-reports-its-work-plainly` (S66), on 2026-11-18.

**Verdict: holds. The whole checks pass, every task is done and every
requirement and scenario is met, so the change is archived.**

## The whole checks

`sluice exec bolt/sluice-reports-its-work-plainly` (sluice 0.95.1) ran
`#check-fast` on `fec871e` against the bolt at `f71e833`, which the
branch holds. It ended with **exit 0**: 9 tasks completed, 9 passed, and
sluice recorded 9 results under their keys for `fec871e`.

| Task | Outcome |
|---|---|
| `root:lint`, `root:format`, `root:changes`, `nix:typecheck` | passed |
| `seams:test` (5), `core:test` (178), `fakes:test` (17) | passed |
| `conformance:test` | passed |
| `cli:test` | passed, every test binary run |

In `cli:test`, the binary's unit tests passed 348, `tests/cli.rs` 244,
`tests/telemetry.rs` 8 and `tests/telemetry_logs.rs` 24, and no binary
failed. `status_lists_the_queue` and
`status_and_overview_show_a_hold_while_it_lasts` pass with the times
task 6.1 moved to where the change's clock reads in `Sluice::carry` put
them (design D2, D4).

The run's only warnings are moon's: no internet connection for its
dependency install, and task outputs it could not archive into the
place's `.moon/cache/outputs`. Neither touched an outcome.

## The change against its specs

`openspec validate each-submission-shows-its-wait-and-its-test --strict`
passes. Every task in `tasks.md`, 1.1 to 6.1, is ticked done. Each
scenario's test exists where the spec names it, and each passed in the
gate run above.

| Requirement | Scenario test | Where | Gate |
|---|---|---|---|
| A submission keeps its time under test | `a_submission_s_time_under_test_counts_overlapping_combinations_once` | `sluice-core/src/line.rs` | ok |
| | `a_submission_s_time_under_test_stops_at_its_end` | `sluice-core/src/line.rs` | ok |
| | `every_write_keeps_a_submission_s_time_under_test` | `sluice-core/src/step.rs` | ok |
| A run with no outcome is never its submission's failure | `a_run_that_decided_nothing_is_counted_on_its_submissions` | `sluice-core/src/line.rs` | ok |
| | `a_run_no_host_took_counts_as_no_outcome_on_its_submissions` | `sluice-core/src/step.rs` | ok |
| A submission's life is a span | `a_submission_s_life_is_a_span_in_the_trace_it_was_submitted_under` | `sluice/tests/telemetry_logs.rs` | ok |
| | `a_life_names_how_it_ended_and_its_runs_with_no_outcome` | `sluice/src/telemetry.rs` | ok |
| A submission's end is measured | `a_write_names_the_submissions_it_ends` | `sluice-core/src/line.rs` | ok |
| | `a_submission_s_end_is_measured` | `sluice/tests/telemetry.rs` | ok |
| A run's room is held for its branch | `the_host_s_running_checks_are_counted_by_branch` | `sluice/src/host_process/room.rs` | ok |
| | `a_hold_names_its_branch_and_an_older_one_none` | `sluice/src/host_process/protocol.rs` | ok |
| The host's room is measured | `a_run_s_wait_for_room_is_measured_by_branch` | `sluice/src/host_process/room.rs` | ok |
| | `the_host_s_room_is_measured_by_branch` | `sluice/src/host_process/mod.rs` | ok |
| A submission's times and a run's wait are sent in seconds | `a_submission_s_times_are_sent_in_seconds` | `sluice/src/telemetry.rs` | ok |

What the code does, read against each requirement:

- **Time under test.** `Line::under_test` reads the submissions held by
  a combination on `doc.base` that is under test, leaving out those in
  an end state. `keep_under_test` starts `under_test_since`, keeps it,
  or adds the time since it to `under_test_ms`, so combinations that
  overlap count once and the time stops at the end. `Sluice::carry`
  keeps it on every write where `under_test_moved` says it moved, before
  the line's changes are read, and writes from that kept line.
- **No outcome.** `Ended::decided_nothing` counts a run no host took, a
  run whose checks are all `none` or `unproven`, and a run given no
  check, and leaves out a run its time limit ended.
  `Line::count_no_outcome` adds one on each submission the combination
  holds. `step.rs` calls it at all three sites that end a run.
  `runs_with_no_outcome` never sets an outcome.
- **The life span.** Once a write is written, `carry` sends each
  submission `Line::ended` gives, one in an end state that was read in
  none, through `send_life` and the `life` hook. `life_span` builds
  `sluice.submission` from the submission's time to the write's. Its
  parent is the kept `TRACEPARENT`, or none, never the current span. It
  links to the span that ended it, names the attributes D5 lists, and
  sets an error for `failed` alone. Its waiting time is the life less
  its time under test, never below zero, so the two sum to its length.
  The end-to-end test runs alone in a child process and asserts it ran
  and passed.
- **The end's metrics.** `sluice.submissions.ended` and
  `sluice.submission.waited` and `sluice.submission.tested` carry
  `sluice.repository`, `sluice.branch` and `sluice.outcome` alone. The
  integration test holds every line to `METRIC_KEYS`. The two
  histograms and `sluice.run.waited_for_room` are in `SECONDS`.
- **Room by branch.** `Tests::hold` takes the branch through the seam,
  the room, the registry's `Entry` (an older entry reads as no branch)
  and the protocol's `Request::Hold`. `exec.rs` and `preview.rs` name
  theirs. The host process measures the room at each three-second scan
  and drains the waits whether or not it sends.
- **Docs.** Record 0014's amendment, `docs/telemetry.md` and the site's
  telemetry reference name the span, the six metrics and the record's
  time under test.

Nothing falls short.
