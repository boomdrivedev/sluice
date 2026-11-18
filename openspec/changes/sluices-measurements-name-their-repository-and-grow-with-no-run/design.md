# Design

## Context

Record 0014 as amended 2026-11-18 decides five things of what a metric
carries; this change builds them. What the code does today, at
`5d7d1c3d0`:

- **sluice's own metrics** are `tracing` events in the core, read by
  `tracing_opentelemetry::MetricsLayer` in the binary
  (`crates/sluice/src/telemetry.rs`, `Telemetry::begin`): the gauges
  `sluice.queue.depth` and `sluice.window`, the counters
  `sluice.verdicts`, `sluice.tasks.ran`, `sluice.tasks.replayed`,
  `sluice.culprits`, `sluice.conflicts`, `sluice.landed`,
  `sluice.refused` (`crates/sluice-core/src/step.rs`) and
  `sluice.abandoned.released` (`session.rs`), and the histograms
  `sluice.submission.landed_after` and `sluice.candidate.tested_for`.
  Each carries `sluice.branch`, and the verdicts and the time under test
  `sluice.outcome`. None carries a run's ids. `MetricsLayer` makes each
  instrument by its name alone, with no unit.
- **A run's environment** (`variables` in `telemetry.rs`, called by
  `run_env` from `profiles/checks.rs` for every run, a candidate's or a
  session's) holds the collector, `OTEL_<SIGNAL>_EXPORTER=none` for each
  signal sluice does not send where the host has a table, and
  `OTEL_RESOURCE_ATTRIBUTES` with `host.name`, the table's attributes,
  `sluice.trace_id` and `sluice.span_id` while traces go, `sluice.place`
  and `sluice.revision`, and `TRACEPARENT`. moon runs with `--otel`
  (`profiles/moon.rs`), which exports its traces and, since 2.6.0, its
  metrics: `moon.task.runs`, `moon.task.duration`,
  `moon.action.executions`, `moon.action.duration` and
  `moon.operation.*`.
- **moon 2.6.1 reads no `TRACEPARENT`**, and no moon release since says
  it does (moon's changelog, read 2026-11-18). Its binary reads
  `OTEL_RESOURCE_ATTRIBUTES`, `OTEL_SERVICE_NAME`, the exporters'
  endpoints, headers, timeouts and compression, and each signal's
  `OTEL_*_EXPORTER`, which starbase's `is_signal_enabled` turns off at
  `none`. OpenTelemetry has no variable that sets the resource of one
  signal alone.
- **A replay's time.** A run's report gives each task's `took_ms`, a
  replay's being what restoring it took (119). `results.jsonl` keeps each
  run's `took_ms` on its line. `verdict::read` proves a replay by a pass
  of its key the branch counts, on its system and under the run's grant
  (108, 138; core.9, core.34).
- **The repository.** The core names none: the check seam's profile
  names its own repository to the host's room (`Tests::hold`). Every
  `Sluice` the binary makes for a bound repository is made
  `.as_peer(host.peer())`, from `Bound::peer`
  (`profiles/mod.rs`), or from the peer the pull request driver was
  given by it.

The family: Flume sends its own metrics through a meter, each with
`.with_unit("s")` for a time, and holds them to a list of the keys a
metric may carry, `METRIC_KEYS`, none naming a place, an intent, a mark,
a conversation or an account (flume 220; Flume's rule core.56). It names
a place's repository by its address, `<owner>/<name>` from the clone's
origin, else its folder's last name (flume 218; `repository_named`).
This design takes both forms: one reading of a repository's name across
the two tools, and one shape of a metric's keys. Flume's hand-over to
its agents gives `flume.place`, `flume.intent`, `flume.mark` and
`flume.start` in `OTEL_RESOURCE_ATTRIBUTES` (flume 219), which an agent
program's metrics carry as moon's carried sluice's ids; that is Flume's
own to settle, and out of scope here.

## Goals

- No metric sluice sends, or hands a run's tool the means to send,
  carries a value that grows with each run.
- A run's checks' spans and logs still name the round that started them.
- Every metric names its repository and branch; every time declares `s`.
- What replays saved sums to a figure a person can read.

## Non-goals

- The metrics of clause 154, not built yet.
- Anything a collector is configured to do.
- moon's own service name.

## Decisions

### D1. A run's tools send no metrics

`variables` puts `OTEL_METRICS_EXPORTER=none` in every run's
environment while telemetry is on: with a table, beside the others
sluice does not send; with none, over whatever the caller's environment
held. Everything else of a run's telemetry stays: the collector, the
traces and logs sluice sends, `TRACEPARENT`, and the run's ids on its
resource. moon's spans so keep naming the round, and no metric carries
the ids, since no metric is sent. moon runs with `--otel` as before.

The cost: moon's own metrics (`moon.task.duration` and the rest) no
longer arrive from sluice's runs. A run's spans hold the same times, by
task, under the round that started them.

This departs from the statement's clause 151 as written, "A run's checks
are handed the collector and the signals sluice sends": they are handed
the traces and logs sluice sends, and never metrics. The change waits on
151's amendment (rule core.12), asked of the design with it.

Not taken:

- *The ids by `TRACEPARENT` alone, off the resource.* moon reads no
  `TRACEPARENT`, so its spans would name no round, which record 0014
  keeps.
- *A run's metrics with the ids kept on the resource.* That is the
  growth the record rules out.
- *A collector that drops the ids.* The record's own option not taken:
  it works only where someone configures the collector.

### D2. A metric carries only what grows with branches

`sluice_core::METRIC_KEYS` lists the attributes a metric may carry:
`sluice.repository`, `sluice.branch` and `sluice.outcome`. Every metric
event the core emits carries `sluice.repository` and `sluice.branch`,
and `sluice.outcome` where it has one, and nothing else. Since metric
events are `tracing` events, written where they happen, the list is held
by the round's tests: `a_landing_reports_its_round_verdict_and_landing`
and its siblings in `crates/sluice/tests/telemetry.rs` assert every
metric event names the repository and the branch and carries no field
outside the list, as Flume's test holds its `METRIC_KEYS`.

Not taken: a filtering layer in the binary that drops keys outside the
list. It would hide an event written wrong rather than fail its test.

### D3. The repository by its origin, else its folder

`Peer` gains `repository: String`, the repository a sluice acts in as
its metrics name it; `Peer::default()` names `this-repository`, as it
names `this-host`. The binding names it once, in `Bound::bind`, by
`LocalRefs::repository_name`, which reads the URL of `origin` as
configured, with no process and no fetch (`remote_url`), and the common
directory; `Bound::peer` gives it. Every sluice
the binary makes for a repository so carries it, with no call site
changed.

The name is read as Flume reads a place's address (flume 218), by a pure
function `repository_named(origin: Option<&str>, common: &Path)` in
`profiles/local_refs.rs`:

- an `origin` whose URL names a host, in `scheme://[user@]host[:port]/…`
  or `[user@]host:…` form, gives the last two names of its path, a
  trailing `/` and `.git` dropped: `https://user:token@github.com/acme/app.git`
  and `git@github.com:acme/app.git` both give `acme/app`;
- an `origin` that is a path or `file://`, one whose path has fewer than
  two names, or none at all, gives the repository's folder's name: the
  folder holding the common directory where that is `.git`, else the
  common directory's own name with `.git` dropped (`app.git` gives
  `app`).

Nothing else of the URL is kept, a credential least of all.

Not taken:

- *A builder on `Sluice` called at each site.* Ten sites build a sluice
  for a repository, and one missed names none; every one already takes
  the binding's peer.
- *The remote a branch's policy names.* A repository's branches may route
  to different remotes (core.27); the name is the repository's, one per
  repository, as Flume's is.
- *GitHub's `Repo::from_url`.* It names a forge, which code outside a
  profile may not (core.1), and answers nothing for any other host.

### D4. Every time declares seconds

`sluice_core::SECONDS` lists the metrics the core gives in seconds:
`sluice.submission.landed_after`, `sluice.candidate.tested_for` and
`sluice.tasks.time_saved`. `Telemetry::begin` builds the meter provider
through one function, `meter_provider`, which adds a view giving each
instrument whose name `SECONDS` lists the unit `s`, OpenTelemetry's own
way to name seconds (`opentelemetry_sdk`'s `with_view` and
`Stream::builder().with_unit`). A unit test builds that provider on an
in-memory exporter, emits each metric as the core does, and reads each
exported metric's unit.

Not taken:

- *The core calling OpenTelemetry's meter, as Flume does.* Record 0014
  keeps the core on `tracing` alone.
- *The unit in the name* (`landed_after_seconds`): renames what a
  backend's dashboards already read, and a backend reads the unit, not
  the name.

### D5. A replay's saved time is its key's last run

`Verdict` gains `saved_ms: u64`: for each replay the verdict proves, the
`took_ms` of the newest pass it counted for the key, by `at`. A replay no
counted pass proves saved nothing, since it runs again (108); a pass that
recorded no time adds none. The round emits, beside
`sluice.tasks.replayed`, the counter `sluice.tasks.time_saved` with
`saved_ms` in seconds, an `f64`. Summed over a week, it says how much
running replays saved.

Not taken: the runner's estimate (`RunDetail::estimate_ms`, `saved_ms`),
moon's guess for the whole run, which a command runner never gives and
which is no task's own time.

### D6. sluice names no other tool's service

Nothing to build. With a table, a run's resource holds no
`service.name`, which the spec's "With a table, a run gets what sluice
sends" already holds; with none, the caller's `OTEL_*` reach the run as
the caller set them. moon reports as moon sets itself.

## Risks

- **A person who reads moon's metrics loses them** (D1). They read the
  run's spans instead. The statement's amendment is the place to weigh
  it.
- **A repository with no origin, or a path origin, is named by its
  folder,** which two repositories may share. The branch still tells
  their queues apart on one host, and the host's name across hosts.
- **Peer carries a fact of the repository.** It is a sluice's identity
  for the records it writes; the repository is part of that identity on
  every metric it sends.

## Rules

- core.66, new: what a metric carries (D1 to D4).
- core.53: a run's checks are handed the collector and the signals
  sluice sends; D1 departs from it until 151 is amended, as said.
- core.1: the repository's name names no forge (D3).
- core.9, core.34: a replay's time is read from the pass that proves it
  (D5).
