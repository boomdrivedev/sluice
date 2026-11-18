## Why

A week of sluice's telemetry in Datadog (2026-10-27, on one build host)
showed five things, sent as signals on the user's word of 2026-11-18
(`2026-11-18-boomdrive-ops-e12883fa/01` to `05`) and decided in record
0014's amendment of 2026-11-18:

- A run's ids ride on `OTEL_RESOURCE_ATTRIBUTES`, and a resource
  attribute goes on every signal a process sends. moon's metrics carried
  about 700 values each of `sluice.trace_id`, `sluice.span_id`,
  `sluice.revision` and `sluice.place` after one week, a count that grows
  with every run, and a metrics backend bills each combination of a
  metric's tags.
- No metric names its repository. sluice's one host process holds every
  repository of its user (135), so `sluice.branch` alone cannot tell
  `main` of one repository from `main` of another.
- `sluice.submission.landed_after` and `sluice.candidate.tested_for`
  are seconds, but declare no unit, so a backend shows bare numbers
  (signal 05).
- Tasks replayed are counted, but nothing says what a replay saved.
- moon reports as `unknown_service:moon`, which is moon's own setting.

## What Changes

- **A run's tools send no metrics.** Every run's environment holds
  `OTEL_METRICS_EXPORTER=none` while telemetry is on, with a table or
  without. The run's ids stay on its resource, so every span and log its
  checks send still names the round that started them, and no metric
  does, since none is sent (151 as amended 2026-11-18; D1).
- **sluice's own metrics carry only what grows with branches.** A metric
  sluice sends carries `sluice.repository`, `sluice.branch` and, where it
  has one, `sluice.outcome`, and nothing else: the core lists them, and
  the round's tests hold every metric to the list (D2).
- **Every metric names its repository.** `sluice.repository` is the
  owner and name the repository's `origin` gives, else its folder's name,
  as Flume names a place's repository (flume 218). The binding reads it
  once and every sluice it makes carries it (D3).
- **Every time declares seconds.** The core lists the metrics it gives
  in seconds; sluice's meter declares each with the unit `s` (D4).
- **What a replay saved is counted.** A round adds to
  `sluice.tasks.time_saved`, in seconds, the time each replayed task's
  key took when it last ran, read from the pass the verdict counted for
  it (D5).
- **sluice names no other tool's service.** It already hands a run no
  `service.name`; nothing changes, and moon names itself (D6).
- **Rule core.66** says what a metric may carry, citing record 0014 as
  amended 2026-11-18; rule core.53 hands a run's checks the traces and
  logs sluice sends, as 151 now does.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `telemetry`: what a metric carries, its repository and unit, the time
  a replay saved, and a run's tools sending no metrics.

## Out of scope

- **The metrics clause 154 adds** (an entry's time waiting and under
  test, the entries ended, a run's wait for room), not built yet. When
  they are, rule core.66 holds them to the same keys and unit.
- **moon's own service name** (`unknown_service:moon`), moon's to set
  (record 0014 as amended, decision 5).
- **Flume's hand-over to its agents.** It gives an agent its place's
  facts, `flume.place` and `flume.start` among them, in
  `OTEL_RESOURCE_ATTRIBUTES`, so an agent program's metrics carry them
  as moon's carried sluice's ids. That is Flume's own design's (flume
  219; signal 2026-11-18-boomdrive-design-5ab9d968/01).

## Sources

- The statement's clause 151 as amended 2026-11-18 (boomdrivedev/blueprints
  c6bf590): a run's checks are handed the traces and logs sluice sends,
  never metrics.
- Record 0014 as amended 2026-11-18, on main at d4dde8f66 (#103), which
  this change's base takes up when main reaches it.

## Impact

- `crates/sluice-core`: `Peer` gains `repository`; `METRIC_KEYS` and
  `SECONDS`; every metric event in `step.rs` and `session.rs` names the
  repository; `verdict.rs` sums what replays saved.
- `crates/sluice`: `profiles/mod.rs` names the repository in
  `Bound::peer`; `telemetry.rs` declares seconds by a view and hands a
  run `OTEL_METRICS_EXPORTER=none`; the tests in `tests/telemetry.rs`
  and `tests/telemetry_logs.rs`.
- `docs/telemetry.md`, `site/src/content/docs/reference/telemetry.md`.
- `docs/architecture/core.md`: rule core.66, and core.53's hand-over to
  a run.

## Touches

```
crates/sluice-core/src/lib.rs
crates/sluice-core/src/step.rs
crates/sluice-core/src/session.rs
crates/sluice-core/src/verdict.rs
crates/sluice/src/profiles/mod.rs
crates/sluice/src/profiles/local_refs.rs
crates/sluice/src/telemetry.rs
crates/sluice/tests/telemetry.rs
crates/sluice/tests/telemetry_logs.rs
docs/telemetry.md
site/src/content/docs/reference/telemetry.md
docs/architecture/core.md
openspec/changes/sluices-measurements-name-their-repository-and-grow-with-no-run/**
```
