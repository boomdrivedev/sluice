## Why

An agent program that runs sluice's commands has a trace of its own:
Flume starts each agent with its start's trace context in
`TRACEPARENT` (flume 219), so a command the agent runs that reads a
trace context nests under the start. sluice reads none. Its rounds are
traces of their own, a command makes no span at all, and nothing in a
round says whose work it tests. A person who sees an agent wait twelve
minutes on `sluice submit` cannot go from that wait to the rounds that
tested the work, nor from a round back to the agent.

The statement's clause 153 says what sluice does instead: a command
started with a W3C trace context nests its own span under it; an
`exec`'s span holds each check it ran, with its time and outcome, and
hands its checks what a round's run hands them (151); a submission
keeps the context it was submitted under, and each round that tests a
combination holding it links to that context; and a context never
turns telemetry on. Rule core.53, as amended at 8e18722f, rules out a
command under a caller's context that starts a trace of its own, a
context that turns telemetry on, and a round that tests a submission
with no link to the context it kept.

## What Changes

- **A command reads its caller's trace context.** `TRACEPARENT`, with
  `TRACESTATE` where given, read once as the command starts, as W3C's
  trace context reads it; one that does not read is none.
- **A command started under one nests in it.** While telemetry sends
  traces, the command sends one span, `sluice <command>`, whose parent
  is the caller's span, holding its exit. A command started under none
  sends no span of its own, as today.
- **A round stays a trace of its own.** `sluice.round` has no parent
  wherever it runs, so a round a command runs in its own process nests
  under no command; the host process keeps no caller's context.
- **An exec's span holds each check it ran.** Beneath `sluice exec`,
  one `sluice.check` span per check its run ran, over the times its
  runner gave and with the outcome its report gives; the run's checks
  get `TRACEPARENT` naming the exec's span, as a round's run names its
  start. The check seam gains `timed`, a finished run's checks with the
  runner's times.
- **A submission keeps the context it was submitted under.** Its record
  holds it as `trace`, whether or not the submit sends.
- **A round links to it.** Each `sluice.start` and `sluice.verdict`
  span of a combination links to the context each of its submissions
  kept, the link naming the submission.
- **A context turns nothing on.** It decides only where sluice's spans
  nest.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `telemetry`: a command nests under its caller's trace, an exec's span
  holds its checks, a submission keeps its trace, and a round links to
  it; a round stays a trace of its own.

## Out of scope

- **Clause 154,** an entry's life as one span with its time waiting and
  under test, and the metrics by branch and how an entry ended. Rule
  core.53's amendment at 8e18722f covers it beside 153; it waits on its
  own change.
- **Clause 151 as amended 2026-11-18,** a run's checks handed traces
  and logs and never metrics (record 0014 as amended that day). This
  change hands an exec's checks whatever a round's run is handed, so
  that change's build reaches both.
- **moon nesting under a run's span.** moon reads no `TRACEPARENT`
  (record 0014); a run's checks still join by resource attributes.
- **Showing the kept context.** No status, wait or inbox output names
  it; it is for telemetry alone.
- **A preview's command, `sluice equip` and a landing beside the
  chain,** which run no check of a submission's and are tested by no
  round.

## Impact

- `crates/sluice/src/telemetry.rs`: the caller's context, the command's
  span, the check spans, and the link hook's binding.
- `crates/sluice/src/lib.rs` (`Cli`, `run`) and `main.rs`: the context
  carried from the environment to the command.
- `crates/sluice/src/exec.rs`: the check spans after a session run.
- `crates/sluice-seams`: `Tests::timed`; `crates/sluice/src/profiles/checks.rs`,
  `crates/sluice-fakes`, the host's room wrapper and the conformance
  suite's recording checks answer it.
- `crates/sluice-core`: the submission record's `trace`, the submit
  requests' context, the round's span with no parent, and the links on
  `sluice.start` and `sluice.verdict` through a hook that names no
  backend (record 0014: the core depends only on `tracing`).
- Tests: `crates/sluice/tests/telemetry_logs.rs`, `cli.rs`, `checks.rs`,
  `exec.rs` and `profiles/moon.rs`; the harness's `isolated` and
  `alone`.
- Docs: the telemetry reference page, `docs/telemetry.md`, and record
  0014's amendment.
- Rule core.53 as amended at 8e18722f, on
  `design/a-sluice-command-joins-its-callers-trace`, which lands on its
  own; the statement's clauses 151 and 153; flume 219 and flume record
  0067, the caller this change answers.
