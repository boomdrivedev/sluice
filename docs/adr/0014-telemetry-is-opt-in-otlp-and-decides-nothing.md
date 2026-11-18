---
number: 14
title: Telemetry is opt-in OTLP and decides nothing
status: accepted
date: 2026-10-13
decision-makers:
- Chuck Swanberg
---

# Telemetry is opt-in OTLP and decides nothing

Deciders: the user, planning sluice's observability, 2026-10-13.
Sources: `agentplot/blueprints:design/sluice/requirements.md` clauses 26, 31, 51, 61 to 63; rules core.1, core.10, core.11, core.14; ADRs 0005, 0008, 0010; `docs/plans/phase-1-pass-2.md` piece 18; the signal `2026-11-09-agentplot-operator-737b5e55/01-sluice-ignores-otel-traces-exporter-none`; afterthought/swancloud record 0061 at a65cedab, as amended 2026-11-09; OpenTelemetry's SDK environment variables specification (`OTEL_SDK_DISABLED`, `OTEL_TRACES_EXPORTER`, `OTEL_METRICS_EXPORTER`, `OTEL_LOGS_EXPORTER`); moonrepo/starbase `crates/app/src/tracing/otel.rs`, `is_signal_enabled`.

Amended 2026-11-08: OpenTelemetry's own settings turn sluice's signals off, as moon reads them (moonrepo/starbase `crates/app/src/tracing/otel.rs`, `is_signal_enabled`). `OTEL_SDK_DISABLED=true` leaves telemetry off as if it were never asked for. `OTEL_TRACES_EXPORTER`, `OTEL_METRICS_EXPORTER` or `OTEL_LOGS_EXPORTER` at `none` turns off that signal alone, and the others go on. A signal turned off is never built, so with traces off a run's checks get no `TRACEPARENT` and a failure's record names no span. The user asked on 2026-11-08 that no tool send traces (afterthought/swancloud record 0061: "If we could just disable tracing everywhere, including on the Mac"). That record's amendment of 2026-11-09 has sluice honour the standard setting. The reading and the rest are the construct stage's own choices in the change `sluice-heeds-otels-off-switches`. Not taken: a tracer with no exporter, which makes spans for nothing, and a warning when every signal is off.

Amended 2026-11-09: where sluice's telemetry goes is the host's own setting, `[telemetry]` in `~/.config/sluice/config.toml`, by record 0106 (the statement's clause 151; rule core.53). The table says whether telemetry is sent, to which collector, as what service, with which signals and how much of a failure; a command's own `--otel` or `SLUICE_OTEL*` chooses for that command alone, over it. OpenTelemetry's variables only turn telemetry or a signal off, or name the collector where the table names none, and a run's checks are handed the collector and the signals sluice sends, none of the caller's where the table names a collector. A host with no `[telemetry]` reads the environment as this record says.

## Context and problem statement

An operator wants to see how a queue behaves over time: how deep it runs, how long a submission waits to land, how often candidates fail, how much a gate replays. A developer wants to see where a round spends its time, and what the task runner did inside a run. The records answer each question about one submission; nothing in them aggregates across rounds, branches or hosts. moon 2.6 exports its own traces and metrics over OTLP. How does sluice report, without anything it reports becoming something it relies on?

## Considered options

1. No telemetry: readers aggregate the records themselves.
2. A metrics endpoint served by sluice.
3. Opt-in OTLP export from each sluice process, as moon does it, with moon's export inside a run tied to the round that started it.

## Decision outcome

Option 3. A served endpoint binds a port and needs a process that outlives a step (core.10, ADR 0010); the records alone leave every reader to rebuild the same aggregates. OTLP is pushed by the process that has the data, to a collector the environment already runs, and is read by any backend.

- **Off unless asked.** `--otel` or `SLUICE_OTEL` turns it on; OpenTelemetry's `OTEL_SDK_DISABLED` and each signal's `OTEL_*_EXPORTER=none` turn it, or one signal, off again; `SLUICE_OTEL_SERVICE_NAME` names the service (default `sluice`); the standard `OTEL_EXPORTER_OTLP_*` variables say where to and with what headers. Export is OTLP over HTTP, protobuf, which every collector and the Datadog Agent take on port 4318.
- **The core names no backend.** A round is a `tracing` span, with recover, decide and perform beneath it, and forming, starting, the verdict, blame and the landing beneath those, each with the branch, submissions, candidate revision, executor and outcome as attributes. A metric is a `tracing` event: queue depth and window, verdicts by outcome, tasks run and replayed, culprits, conflicts, landings, refused landings, submit-to-landed time and a candidate's time under test. A place's cache restores are counted by the places profile. The binary sets up the exporter; with it off, no subscriber exists and the spans cost nothing.
- **Telemetry decides nothing.** Nothing reads it back; the records are the only truth (core.11, core.14). An exporter that cannot reach its collector loses what it held and says nothing. A short `sluice step` flushes before it exits, which can delay that exit by the exporter's timeout when the collector does not answer.
- **A run's checks join the round by attributes.** moon 2.6.1 reads no parent trace from its environment (no `TRACEPARENT`), so its spans cannot nest under sluice's. With telemetry on, a run's environment gets sluice's `OTEL_*` variables, moon gets `--otel`, and `OTEL_RESOURCE_ATTRIBUTES` carries `sluice.trace_id`, `sluice.span_id`, `sluice.place` and `sluice.revision`: every span and metric the checks export names the round that started them. The environment carries the start span as `TRACEPARENT` as well, so a tool that reads it nests its spans there.

### Consequences

- A run's checks see sluice's `OTEL_EXPORTER_OTLP_HEADERS` while telemetry is on. A collector on the host, as the Datadog Agent is, needs none; a key for a remote collector belongs in the collector's configuration, never in sluice's environment.
- A sandboxed run reaches a collector only on localhost, which `network = "localhost"` allows.
- The exporter speaks plain HTTP, not TLS: it sends to a collector on the host, which forwards to the backend over TLS itself.
- sluice's binary carries the OpenTelemetry SDK and an HTTP client; the core depends only on `tracing`.

## Amendment of 2026-11-18: what a metric carries

Decider: the design agent, on five findings from a week of sluice's telemetry in Datadog (2026-10-27, on the Willdan build host), held by the user on 2026-11-03 and sent as signals on their word of 2026-11-18 (`2026-11-18-boomdrive-ops-e12883fa/01` to `05`).

A run's ids rode on `OTEL_RESOURCE_ATTRIBUTES`, and a resource attribute goes on every signal a process sends, metrics among them. After one week moon's metrics carried about 700 values each of `sluice.trace_id`, `sluice.span_id`, `sluice.revision` and `sluice.place`, a count that grows with every run. A metrics backend bills each combination of a metric's tags, so the join this record chose for spans made the metrics' cost grow without limit.

**Decision.**

1. **No metric carries a value that grows with each run.** No metric sluice sends, or hands a run's tool the means to send, carries a run's trace id, span id, revision or place. A run's checks still join the round on their spans and logs, as this record decided: every span and log they send names the round that started them. The way a run's tool is handed those ids is the construct's to choose within that.
2. **Every metric names its repository.** Each metric sluice sends names the repository it is about, by the owner and name its remote gives, or the folder's name where it has none, beside `sluice.branch`. A host holds few repositories, so the count stays small.
3. **Every duration declares its unit.** A metric that measures time declares its unit, seconds, in OpenTelemetry's own way (`s`). `sluice.submission.landed_after` and `sluice.candidate.tested_for` are among them.
4. **What a replay saved is measured.** Tasks replayed are counted with the time each took when it last ran, so the time replays save sums to hours.
5. **sluice names no other tool's service.** A run's tool names itself, as record 0106 and core.53 have it. moon reporting as `unknown_service:moon` is moon's own setting, not sluice's to fill.

**Options not taken.**

- *Keep the ids on the resource and drop them at the collector.* That works only where someone configures the collector, and every other backend still bills the growth.
- *Turn a run's tools' telemetry off.* That loses the join of the checks' spans to the round, which this record exists for.
