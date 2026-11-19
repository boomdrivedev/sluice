## Context

The statement's clause 153 has a sluice command started with a W3C
trace context nest its span under it, an `exec`'s span hold each check
it ran, and a submission keep its context for each round that tests a
combination holding it to link to; a context never turns telemetry on.
Rule core.53, as amended at 8e18722f (on
`design/a-sluice-command-joins-its-callers-trace`), rules out a command
under a caller's context that starts a trace of its own, a context read
from the environment that turns telemetry on, and a round that tests a
submission with no link to the context it kept.

The caller is an agent program. Flume gives each agent it starts its
start's trace context in `TRACEPARENT`, and no `TRACESTATE` (flume 219;
flume record 0067), and the agent's commands inherit it.

What sluice has today (`crates/sluice/src/telemetry.rs`; record 0014):

- Telemetry starts in `main` from the host's `[telemetry]` and the
  command's own choices, and is off unless asked. The core emits
  `tracing` spans and metric events; the binary's subscriber layers
  `tracing-opentelemetry` 0.34 over them. The core depends only on
  `tracing`.
- No command makes a span. Each round is a trace rooted at
  `sluice.round` (`sluice-core/src/step.rs`), made under whatever span
  is current, which today is none.
- A run's environment is built by `Checks::start` under the current
  span: `run_env` hands it the collector, the signals sluice sends,
  `OTEL_RESOURCE_ATTRIBUTES` with the span's ids, and `TRACEPARENT`
  naming the span. A round's run is started under `sluice.start`; a
  session's run under no span, so it gets no `TRACEPARENT`.
- A run's environment keeps nothing of sluice's own but `KEEP` and the
  locale, so no caller's `TRACEPARENT` reaches a check. sluice's host
  process keeps its starter's variables by a list (`OTEL_*`, `SLUICE_*`,
  `XDG_*`, `DOCKER_*` and a few names), which holds no `TRACEPARENT`.
- `Tests::tasks` says where each check of a run still going has got,
  with the runner's times; it answers nothing once the run has ended.

## Goals / Non-Goals

**Goals:**

- A command started under a caller's context shows inside the caller's
  trace, and a round that tests its work links back to it (153).
- An `exec` shows each check it ran, timed as its runner timed it.
- A context never turns telemetry or a signal on.

**Non-Goals:**

- An entry's life as a span and the metrics by how an entry ended
  (154): their own change.
- A run's checks handed no metrics (151 as amended 2026-11-18): the
  change of record 0014's amendment of that day.
- Showing the kept context in any output.

## Decisions

### D1. The caller's context is read once, as W3C reads it

As a command starts, `main` reads `TRACEPARENT` and, where given,
`TRACESTATE` from the environment it started with (`Started.env`), by
OpenTelemetry's W3C trace context propagator over those two names, the
names OpenTelemetry's environment carrier gives them. A value the
propagator reads as no valid context, a wrong version, an all-zero
trace or span id, hex in upper case, a field of the wrong length, is no
context, said nowhere. The read context is `telemetry::Caller`: the
remote span context, and its `TRACEPARENT` as W3C writes it, which is
what a submission keeps (D4).

It rides on `Cli` as a field clap never parses (`#[arg(skip)] caller:
Option<Caller>`), which `main` fills; tests over the fakes set it on
the parsed `Cli`, touching no process's environment.

Not taken: a flag, `--trace-parent`, which the agent program would have
to add to each command it runs, when it already sets the variable for
every process it starts; and reading the context into
`telemetry::Settings`, whose choice of whether telemetry sends must
never read it (D7).

### D2. A command under a caller's context sends one span in its trace

While telemetry sends traces, a command whose `cli.caller` is some
makes one span before anything beneath it starts, its parent set to the
caller's context (`OpenTelemetrySpanExt::set_parent`), and enters it
for the command's whole run. Its name is the command's words as
`sluice --help` lists them: `sluice exec`, `sluice submit`, `sluice
job run`, and `sluice` for the home view (`otel.name` on a span named
`sluice.command`). It holds `sluice.exit`, the command's exit, recorded
as it returns; an `exec`'s also holds `sluice.branch`, `sluice.ran` and
`sluice.replayed` (D6). OpenTelemetry's parent-based sampling holds: a
caller's context marked not sampled leaves the command's spans
unsampled.

One function of `telemetry` makes it, which `run` calls for every
command it dispatches and `main` for a kept `sluice run`, which does not
go through `run`. `sluice host`, `sluice supervise`, the credential
helper, `sluice forge check` and the hook make none: the host process
keeps no caller's context (D3), and the others end before telemetry
starts.

A command with no caller's context makes no span of its own, as today:
its rounds are traces of their own and a session's run gets no
`TRACEPARENT`.

Not taken: every command a trace of its own. A host whose plugins ask
`sluice status` every few seconds would send a trace per ask, and 153
asks only for nesting. Not taken either: a span named `sluice.<command>`
in the round's form, which would collide with the round's own
`sluice.previews` and `sluice.start`.

### D3. A round is a trace of its own wherever it runs

`sluice.round` is made with no parent (`info_span!(parent: None, ...)`),
so a round a command runs in its own process, `sluice step`, or a
waiting command over the fakes, where the host process is in-process,
nests under no command's span. The host process could never nest one:
it keeps no `TRACEPARENT` or `TRACESTATE` of the command that starts it,
since its environment keeps names by list (`host_process/environment.rs`),
and a test now holds that. A round reaches the work's caller by links
(D5), never by nesting, so one round that tests the work of several
agents is no single agent's.

### D4. A submission keeps its caller's context, as its traceparent

`SubmitRequest` and `GroupRequest` carry `trace: Option<String>`, the
caller's `TRACEPARENT` as W3C writes it, and every submission the act
writes keeps it on its record, `Submission.trace`
(`#[serde(default, skip_serializing_if = "Option::is_none")]`). A submit
of several worktrees keeps one context on each submission. A submission
by name keeps it as one from a worktree does. A pull request's entry,
taken in from the forge, keeps none: the forge carries no context. A
carry to another home keeps it with the record (core.64).

It is kept whether or not the submit sends: the host process that tests
the work may send where the command did not (`--otel=false`,
`SLUICE_OTEL=0`). Nothing sluice decides reads it, and no output shows
it; only the round's links read it (D5).

The context kept is the caller's, not the submit's own span. The
caller's exists whatever the submit sends, and from the caller's span a
backend shows the submit beneath it. Not taken: the submit's own span,
which exists only where the submit sends traces, so a round could link
to a span no one sent.

### D5. The spans that test a combination link to its submissions' contexts

Each `sluice.start` span, a run of a combination starting, and each
`sluice.verdict` span, its run's verdict read, holds one link for each
submission the combination holds that kept a context, the link holding
`sluice.submission`, its number. A round started on a pool's job links
the same way, since the records it reads hold the context.

The core names no backend (record 0014). It hands each kept context, as
its text, to a link hook in `sluice-core`, a function the binary sets
once and that does nothing until it is set:
`sluice_core::links::set(fn(&tracing::Span, &str, u64))`, called by the
core as `links::link(&span, trace, submission)`. The binary's
telemetry sets it as traces start: it reads the text as D1 reads a
`TRACEPARENT`, and adds the link with
`OpenTelemetrySpanExt::add_link_with_attributes`. A text that does not
read adds nothing.

Not taken: `sluice-core` depending on `tracing-opentelemetry`, which
record 0014 keeps out of the core; a link on `sluice.round`, which is
made before the round knows which combinations it tests; and links on
`sluice.land` and `sluice.blame`, which test nothing. The entry's own
span, from submission to end, is clause 154's.

### D6. An exec's span holds a span per check its run ran, timed by its runner

Once its run has finished, `sluice exec` asks the check seam for the
run's checks with their times, `Tests::timed(&run)`, a new method of
the seam: each check of a run that has ended, as its runner timed it,
while the run's directory stands. The check profile answers moon's
from moon's records in the place, as `tasks` reads them while the run
goes (ADR 0028), and the `command` runner's one check from the run's
own start and end (`runs::ended`). The fakes answer what the run said
of its tasks. The host's room wrapper and the conformance suite's
recording checks pass it on. It has no default, so no implementation
answers nothing by omission. `tasks` keeps answering only a run that
goes, since a round keeps what it says for a combination under test.

Beneath its span, the exec then sends one `sluice.check` span for each
check of its report that ran (`Source::Ran`) whose runner gave its
start and its end, from that start to that end, with `sluice.task` and
`sluice.outcome` (`pass` or `fail`, the report's). A check answered
from an earlier run ran nothing and has no span; a check its runner
gave no start or end for has none either. The exec's span holds
`sluice.ran` and `sluice.replayed`, the report's counts, as a
verdict's does.

A command after `--` is the run's one check, named by its words, and an
agent's command line may carry a secret: each check's name is scrubbed
as a log's body is (`crate::scrub::Scrub`, over the environment the
settings were chosen from) before it is sent, which leaves a task's
name as it is.

The check spans carry times sluice did not see, so `tracing`'s spans,
which take their times from when they are entered and closed, cannot
make them. The binary keeps the tracer it starts (beside `SENT`), and
makes each with OpenTelemetry's span builder, its start and end times
given, under the exec span's context
(`OpenTelemetrySpanExt::context`). The exec makes them before it
releases its place, since moon's records are in the place.

Not taken: `tracing` spans entered and closed as the exec's loop sees
each check move, timed to its tenth of a second polling and missing a
check that starts and ends between two looks; and `tasks` answering a
finished run, which a round's `keep_tasks` would then keep for a run
that has ended.

### D7. A context turns nothing on

`Settings::choose` reads no `TRACEPARENT`: whether telemetry sends is
the command's choice, else the host's table, as before, and a context
read while telemetry is off makes no span and hands a run nothing (151;
rule core.53). Only D4 keeps it, as a record's field.

### D8. An exec's checks nest under its span

`Checks::start` builds a run's environment under the current span, and
the exec's span is current while it starts its run, so `TRACEPARENT`
names the exec's span, and `OTEL_RESOURCE_ATTRIBUTES` its trace and span
ids, as a round's run names its `sluice.start`. Nothing in the profile
changes for it.

### D9. Tests start every sluice under no context

The tests' `support::isolated`, which every process a test starts goes
through, `telemetry_logs.rs`'s `alone` among them, takes `TRACEPARENT`
and `TRACESTATE` out. A suite run under `sluice exec` while it sends
traces hands its checks a `TRACEPARENT`; without this a sluice binary a
test starts would nest under it and keep it on its records. A test over
the fakes gives a context only by setting `cli.caller`.

## Risks / Trade-offs

- A command's span is sent only when it ends, so a waiting submit's
  shows once its outcome comes, and a command killed outright sends
  none. Accepted: 153 asks the span be the command's whole life.
- An older sluice that rewrites a submission's live record drops
  `trace`, which it does not know, and later rounds on newer hosts then
  link nothing for it. Accepted: telemetry decides nothing, and the
  hosts of one branch move to one version.
- A trace with many submissions gains links on every start and verdict
  of every combination that holds them. Each link is a context and one
  number, and a combination holds at most its window's submissions.
- The link hook is process-global. It is set once, before any round
  runs, by the one telemetry the process starts, and does nothing where
  telemetry is off.
