## Why

A person who reads sluice's telemetry cannot see one submission's life.
The rounds that test it are traces of their own, and nothing sends the
submission itself: when it was submitted, how it ended, how long it sat
in the queue and how long a combination holding it was under test. The
metrics measure a landed submission alone (`sluice.landed`,
`sluice.submission.landed_after`), so a submission blamed, dropped for a
conflict or withdrawn is never measured, and nothing tells its waiting
from its testing. `sluice.candidate.tested_for` times a combination, not
a submission. Nothing measures the host's bound: how many checks run
against it, or how long a run waited for room under it, though the
overview shows the runs that wait.

The statement's clause 154 says what sluice sends instead. Each entry's
life is a span of its own, from its submission to its end, landed,
refused, dropped for a conflict, blamed or withdrawn, naming how it
ended and holding its time waiting with no candidate of it under test
and its time under test; a run that ended with no outcome is named so
and is never the entry's failure (114). sluice's metrics add, by branch
and how an entry ended, each entry's time waiting and under test, the
entries ended, the host's running checks against its bound, and how
long a run sluice chose waited for room (71). No metric names an
entry's head, its submitter or the context it was submitted under.
Rule core.53, as amended at 8e18722f, rules out a run with no outcome
named as its entry's failure and a metric with an entry's head, its
submitter or a trace context as an attribute.

## What Changes

- **A submission keeps its time under test on its record.** Every write
  of a branch's records keeps, on each submission, how long a
  combination holding it was under test, `under_test_ms`, and since
  when one is, `under_test_since`, combinations that overlap counted
  once, stopping at its end. It also counts the runs of its
  combinations that ended with no outcome, `runs_with_no_outcome`.
  Nothing sluice decides reads them.
- **A submission's end is the write that ends it.** The write that
  moves a submission into an end state, landed, refused, failed,
  conflict or withdrawn, sends its life once, whichever process and
  host makes it.
- **Its life is one span.** `sluice.submission`, from its submission to
  that write, in the trace it was submitted under where it kept one
  (153), and a trace of its own otherwise, naming its number, head,
  work and agent, its outcome, its seconds waiting and under test, and
  its runs with no outcome. Only a submission blamed is an error.
- **Its end is measured.** `sluice.submissions.ended`, and the
  histograms `sluice.submission.waited` and `sluice.submission.tested`
  in seconds, each with the repository, the branch and the outcome.
- **A run's room is held for its branch.** The check seam's `hold`
  names the branch; a session's or a preview's ask names the branch its
  run is held to; the host's registry of running checks names it.
- **The host's room is measured.** sluice's host process sends, by
  repository and branch, `sluice.host.running` and `sluice.host.bound`,
  and `sluice.run.waited_for_room` in seconds for each run whose part
  of the bound it held.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `telemetry`: a submission's life as a span, its end measured by
  outcome, its time under test kept on its record, a run with no
  outcome never its failure, and the host's room measured by branch.

## Depends on

Two changes on `bolt/sluice-reports-its-work-plainly` this one builds
on, which land on the bolt before its code is written (tasks 1.1):

- `a-sluice-command-shows-inside-its-callers-trace` (153): the
  submission's kept `trace`, the tracer the binary keeps for spans with
  times of their own (`TRACER`), `telemetry::traceparent`, and the
  `sluice_core::links` hook this change's `life` hook follows.
- `sluices-measurements-name-their-repository-and-grow-with-no-run`
  (record 0014 as amended 2026-11-18, rule core.66): `Peer.repository`,
  `sluice_core::METRIC_KEYS`, `sluice_core::SECONDS` and the meter's
  view that declares them in `s`, and the round tests' helper that holds
  every metric to the list.

## Out of scope

- **The metrics a landing already sends,** `sluice.landed`,
  `sluice.submission.landed_after` and `sluice.candidate.tested_for`,
  which stay as they are: dashboards read them.
- **Showing a submission's time under test** in a status, a wait or the
  overview.
- **A pool's runs,** which take no part of a host's bound.
- **A round linking to a submission's life.** The round links to the
  trace the submission kept (153), which holds its life.

## Impact

- `crates/sluice-core`: `State::ends`, the submission record's
  `under_test_ms`, `under_test_since` and `runs_with_no_outcome`;
  `Line`'s states as read, `ended`, `under_test_moved`,
  `keep_under_test` and `count_no_outcome`; `Ended::decided_nothing`;
  `Sluice::carry` keeping the time and sending each end; the `life`
  hook; `SECONDS`; the round's calls to `hold` naming the branch.
- `crates/sluice-seams`: `Tests::hold` takes the branch.
- `crates/sluice`: `telemetry.rs` sends a life as a span;
  `host_process/room.rs` holds, counts and times runs by branch;
  `host_count.rs`'s entry names its branch; `host_process/protocol.rs`,
  `client.rs`, `ask.rs` and `mod.rs` carry a hold's branch and send the
  room's metrics; `exec.rs` and `preview.rs` name theirs;
  `profiles/checks.rs` passes it on.
- `crates/sluice-fakes`, the conformance suite's recording checks and
  `sluice-core`'s test host: `hold` with a branch.
- Tests: `crates/sluice-core/src/line.rs` and `step.rs`;
  `crates/sluice/src/telemetry.rs`, `host_process/room.rs`,
  `host_process/protocol.rs` and `host_process/mod.rs`;
  `crates/sluice/tests/telemetry.rs` and `telemetry_logs.rs`.
- Docs: record 0014's amendment for this build, `docs/telemetry.md` and
  `site/src/content/docs/reference/telemetry.md`.
- Rules core.53 as amended at 8e18722f, and core.66; the statement's
  clauses 71, 114, 150, 153 and 154.

## Touches

```
crates/sluice-core/src/lib.rs
crates/sluice-core/src/life.rs
crates/sluice-core/src/line.rs
crates/sluice-core/src/records.rs
crates/sluice-core/src/step.rs
crates/sluice-seams/src/lib.rs
crates/sluice-fakes/src/lib.rs
crates/sluice-conformance/**
crates/sluice/src/telemetry.rs
crates/sluice/src/host_count.rs
crates/sluice/src/host_process/room.rs
crates/sluice/src/host_process/protocol.rs
crates/sluice/src/host_process/client.rs
crates/sluice/src/host_process/ask.rs
crates/sluice/src/host_process/mod.rs
crates/sluice/src/profiles/checks.rs
crates/sluice/src/exec.rs
crates/sluice/src/preview.rs
crates/sluice/tests/telemetry.rs
crates/sluice/tests/telemetry_logs.rs
crates/sluice/tests/profiles/conformance.rs
docs/adr/0014-telemetry-is-opt-in-otlp-and-decides-nothing.md
docs/telemetry.md
site/src/content/docs/reference/telemetry.md
openspec/changes/each-submission-shows-its-wait-and-its-test/**
```
