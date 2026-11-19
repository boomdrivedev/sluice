## MODIFIED Requirements

### Requirement: A branch's queue is measured

While telemetry is on and sends metrics, sluice SHALL send, each with
`sluice.repository` and `sluice.branch`, the gauges `sluice.queue.depth`
(submissions not yet out of the queue) and `sluice.window` (combinations
tested at once), and the counters `sluice.verdicts` by `sluice.outcome`,
`sluice.tasks.ran`, `sluice.tasks.replayed`, `sluice.culprits`
(submissions blamed), `sluice.conflicts`, `sluice.landed` and
`sluice.refused` (landings refused because the branch moved).

#### Scenario: A landing's metrics

- **WHEN** a submission is tested, passes and lands
- **THEN** sluice reports `sluice.queue.depth`, `sluice.verdicts`,
  `sluice.tasks.ran` and `sluice.landed`, each naming the repository and
  the branch (`a_landing_reports_its_round_verdict_and_landing` in
  `crates/sluice/tests/telemetry.rs`)

#### Scenario: A blamed submission is counted

- **WHEN** a submission's combination fails its gate
- **THEN** sluice reports `sluice.culprits` (`a_failure_reports_its_culprit`
  in `crates/sluice/tests/telemetry.rs`)

### Requirement: Waiting and testing are timed

While telemetry is on and sends metrics, sluice SHALL send, with
`sluice.repository` and `sluice.branch`, the histograms
`sluice.submission.landed_after` (seconds from submit to landed) and
`sluice.candidate.tested_for` (seconds a combination was under test, by
`sluice.outcome`), each declared in seconds, the unit `s`.

#### Scenario: A landing is timed

- **WHEN** a submission is tested, passes and lands
- **THEN** sluice reports `sluice.submission.landed_after` and
  `sluice.candidate.tested_for`
  (`a_landing_reports_its_round_verdict_and_landing` in
  `crates/sluice/tests/telemetry.rs`)

#### Scenario: A time is sent in seconds

- **WHEN** sluice's meter is sent `sluice.submission.landed_after`,
  `sluice.candidate.tested_for`, `sluice.tasks.time_saved` and
  `sluice.landed` as the core emits them
- **THEN** the first three are exported with the unit `s`, and
  `sluice.landed` with none (`a_time_is_sent_in_seconds` in
  `crates/sluice/src/telemetry.rs`)

### Requirement: A run's checks join the round

While telemetry is on, moon SHALL run with `--otel`, and a run's
environment SHALL carry:

- the collector sluice sends to and the traces and logs it sends;
- `OTEL_RESOURCE_ATTRIBUTES` extended with `sluice.place` and
  `sluice.revision`, each value percent-encoded where the variable's
  syntax would read it;
- while it sends traces, `sluice.trace_id` and `sluice.span_id` there,
  and `TRACEPARENT` naming the span that started the run.

Nothing of telemetry SHALL be added while telemetry is off.

#### Scenario: A resource attribute's value is encoded

- **WHEN** an attribute's value holds a comma, an equals sign or a space
- **THEN** each is percent-encoded and a `/` is left as it is
  (`values_are_encoded_where_the_syntax_would_read_them` in
  `crates/sluice/src/telemetry.rs`)

#### Scenario: A run joins the round

- **WHEN** telemetry is on with no table, `OTEL_RESOURCE_ATTRIBUTES`
  holds `deployment.environment=test`, and a run is started under a
  span at a place named `p 1`
- **THEN** its environment holds the caller's
  `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_METRICS_EXPORTER=none`, a
  `TRACEPARENT` naming the span, and `OTEL_RESOURCE_ATTRIBUTES` with
  `deployment.environment=test`, `host.name`, the trace and span ids,
  `sluice.place=p%201` and the revision
  (`a_run_s_checks_join_the_round` in
  `crates/sluice/tests/telemetry_logs.rs`)

#### Scenario: A run joins no trace while traces are off

- **WHEN** telemetry is on, `OTEL_TRACES_EXPORTER` is `none`, and a run
  is started under a span
- **THEN** its environment holds `OTEL_TRACES_EXPORTER=none`,
  `OTEL_METRICS_EXPORTER=none`, no `TRACEPARENT`, and
  `OTEL_RESOURCE_ATTRIBUTES` with the host's own attributes,
  `host.name`, `sluice.place` and `sluice.revision`, and no
  `sluice.trace_id` or `sluice.span_id`
  (`with_traces_off_a_run_s_checks_join_no_trace` in
  `crates/sluice/tests/telemetry_logs.rs`)

### Requirement: With no table, a run gets the caller's telemetry

Where the host has no `[telemetry]` table, a run SHALL get the caller's
`OTEL_*` as sluice read them when it started, but for
`OTEL_METRICS_EXPORTER`, which is `none`; its `OTEL_RESOURCE_ATTRIBUTES`
holding `host.name` where sluice named the host itself.

#### Scenario: A run gets the caller's collector and attributes

- **WHEN** telemetry is on with no table, the caller's
  `OTEL_EXPORTER_OTLP_ENDPOINT` names a collector and its
  `OTEL_RESOURCE_ATTRIBUTES` holds `deployment.environment=test`, and a
  run is started under a span
- **THEN** its environment holds the caller's
  `OTEL_EXPORTER_OTLP_ENDPOINT`, and `OTEL_RESOURCE_ATTRIBUTES` begins
  `deployment.environment=test,host.name=host-a`
  (`a_run_s_checks_join_the_round` in
  `crates/sluice/tests/telemetry_logs.rs`)

## ADDED Requirements

### Requirement: A run's tools send no metrics

While telemetry is on, a run's environment SHALL hold
`OTEL_METRICS_EXPORTER=none`, with a table or without, whether or not
sluice sends metrics and whatever the caller's environment holds, so the
run's ids on its resource ride on no metric (151 and record 0014, each
as amended 2026-11-18; rule core.66).

#### Scenario: A run's tools send no metrics

- **WHEN** a run's environment is built under a span with telemetry on,
  four ways: a table sending metrics; a table with `metrics = false`; no
  table; and no table with the caller's `OTEL_METRICS_EXPORTER` at
  `otlp`
- **THEN** each holds `OTEL_METRICS_EXPORTER=none`, and its
  `OTEL_RESOURCE_ATTRIBUTES` the trace and span ids, `sluice.place` and
  `sluice.revision` (`a_run_s_tools_send_no_metrics` in
  `crates/sluice/src/telemetry.rs`)

### Requirement: A metric carries only its repository, branch and outcome

Each metric sluice sends SHALL carry `sluice.repository` and
`sluice.branch`, `sluice.outcome` where it has one, and no other
attribute, so none names a run's trace, span, revision or place, a
submission, a submitter or a task (record 0014 as amended 2026-11-18;
rule core.66).

#### Scenario: A round's metrics carry nothing of a run

- **WHEN** a submission is tested, passes and lands; one is blamed; one
  is replayed; one conflicts; and a landing is refused
- **THEN** every metric each reports names `sluice.repository` and
  `sluice.branch`, and carries no field but those and `sluice.outcome`
  (`a_landing_reports_its_round_verdict_and_landing`,
  `a_failure_reports_its_culprit`, `a_replay_is_counted`,
  `a_conflict_is_counted` and `a_refused_landing_is_counted` in
  `crates/sluice/tests/telemetry.rs`)

### Requirement: A metric names its repository by its origin, else its folder

`sluice.repository` SHALL be the last two names of the path of the
repository's `origin`, where its URL names a host, a trailing `.git`
dropped; else the repository's folder's name, `.git` dropped. No other
part of the origin's URL SHALL be kept.

#### Scenario: A repository's name

- **WHEN** a repository is named from its origin and its common
  directory
- **THEN** each comes out as follows
  (`a_repository_is_named_by_its_origin_else_its_folder` in
  `crates/sluice/src/profiles/local_refs.rs`):
  - `https://user:token@github.com/acme/app.git`,
    `git@github.com:acme/app.git` and
    `ssh://git@git.example:2222/acme/app/` each give `acme/app`;
  - no origin with the common directory `/src/app/.git` gives `app`;
  - the origin `/srv/git/app.git` with the common directory
    `/srv/q/app.git` gives `app`;
  - no name holds `user`, `token` or a host.

### Requirement: What a replay saved is measured

While telemetry sends metrics, the round that reads a run's verdict
SHALL add to the counter `sluice.tasks.time_saved`, in seconds with the
unit `s`, the time each replayed task's key took when it last ran: the
`took_ms` of the newest pass the verdict counts for its key. A replay
that no counted pass proves, or whose pass recorded no time, SHALL add
nothing.

#### Scenario: A replay saves its key's last time

- **WHEN** a verdict is read of a run that replayed three keys: one whose
  counted passes took 4000 ms and later 6000 ms, one whose counted pass
  recorded no time, and one with no counted pass
- **THEN** it reads 6000 ms saved, and the third key unproven
  (`a_replay_saves_the_time_its_key_last_took` in
  `crates/sluice-core/src/verdict.rs`)

#### Scenario: A round counts what its replays saved

- **WHEN** `lib:test` takes 4000 ms when it runs, and a submission
  brings `lib/` back to a tree a landing before had, so the task is
  replayed
- **THEN** sluice reports `sluice.tasks.time_saved` of 4 seconds with
  the repository and the branch (`a_replay_counts_the_time_it_saved` in
  `crates/sluice/tests/telemetry.rs`)
