## ADDED Requirements

### Requirement: A command started under a caller's trace nests in it

A command SHALL read its caller's W3C trace context from `TRACEPARENT`
and, where given, `TRACESTATE` in its environment as it starts; one
that does not read as W3C's trace context SHALL be none (153). While
telemetry sends traces, a command started under one SHALL send one span
named `sluice <command>`, such as `sluice exec` or `sluice job run`,
whose parent is the caller's span, holding `sluice.exit`, its exit. A
command started under none SHALL send no span of its own.

#### Scenario: A command nests under its caller

- **WHEN** telemetry sends traces, `sluice submit` is started under
  `TRACEPARENT` `00-<t>-<s>-01`, and another submit under none
- **THEN** one `sluice submit` span is sent in trace `<t>` with parent
  `<s>` and the first submit's exit as `sluice.exit`, and no
  `sluice submit` span is sent for the second
  (`a_command_nests_under_its_caller_s_trace` in
  `crates/sluice/tests/telemetry_logs.rs`)

#### Scenario: A context is read as W3C reads it

- **WHEN** `TRACEPARENT` is `00-` then 32 hex digits, 16 hex digits and
  `01`, beside a `TRACESTATE`; or holds an all-zero trace id, hex in
  upper case, version `ff`, or three fields
- **THEN** the first reads as a context with its trace state and its
  `TRACEPARENT` as given, and each of the others reads as none
  (`a_trace_context_is_read_as_w3c_reads_it` in
  `crates/sluice/src/telemetry.rs`)

### Requirement: A caller's trace turns nothing on

A caller's trace context SHALL name only where sluice's spans nest. It
SHALL never turn telemetry or a signal on: whether sluice sends is the
command's own choice, else the host's `[telemetry]`, as before (151;
rule core.53).

#### Scenario: A context with nothing asked sends nothing

- **WHEN** the command gives no telemetry choice, the host has no
  `[telemetry]`, and the environment holds a `TRACEPARENT`
- **THEN** the settings chosen send nothing
  (`a_caller_s_context_turns_nothing_on` in
  `crates/sluice/src/telemetry.rs`)

### Requirement: A round is a trace of its own under any command

Each round SHALL be one trace of its own, its `sluice.round` with no
parent, whatever process runs it and under whatever command's span.
sluice's host process SHALL keep no `TRACEPARENT` or `TRACESTATE` of
the command that starts it.

#### Scenario: A round run under a command's span

- **WHEN** telemetry sends traces and a submit started under a caller's
  context waits while its combination is tested and lands
- **THEN** each `sluice.round` span has no parent and a trace that is
  neither the caller's nor another round's
  (`a_command_nests_under_its_caller_s_trace` in
  `crates/sluice/tests/telemetry_logs.rs`)

#### Scenario: The host process keeps no caller's context

- **WHEN** the host process's environment is chosen from a command's
  that holds `TRACEPARENT` and `TRACESTATE`
- **THEN** it keeps neither
  (`the_host_process_keeps_no_caller_s_trace_context` in
  `crates/sluice/src/host_process/environment.rs`)

### Requirement: A submission keeps the trace it was submitted under

A submission SHALL keep on its record, as `trace`, the `TRACEPARENT` of
the context its submit was started under, whether or not the submit
sends telemetry; each submission of one submit of several worktrees
SHALL keep the same. A submission submitted under none, and a pull
request's taken in from the forge, SHALL keep none. Nothing sluice
decides SHALL read it (153).

#### Scenario: Each submission keeps its submit's context

- **WHEN** with telemetry off, `sluice submit` is started under
  `00-<t>-<s>-01`, a submit of two worktrees as one act under it, and a
  submit under none
- **THEN** the first submission's record holds `trace`
  `00-<t>-<s>-01`, each of the act's two holds it, and the last's holds
  no `trace`
  (`a_submission_keeps_the_trace_it_was_submitted_under` in
  `crates/sluice/tests/cli.rs`)

### Requirement: A round that tests a submission links to its trace

While telemetry sends traces, each `sluice.start` span and each
`sluice.verdict` span of a combination SHALL hold one link to the
trace each submission it holds kept, the link holding
`sluice.submission`, that submission's number (153; rule core.53).

#### Scenario: A combination's start and verdict link to its work's caller

- **WHEN** a combination holds submission 1, submitted under
  `00-<t>-<s>-01`, and submission 2, submitted under none, and is
  tested and lands
- **THEN** each of its `sluice.start` and `sluice.verdict` spans holds a
  link to span `<s>` of trace `<t>` with `sluice.submission` 1, and no
  link for submission 2
  (`a_round_links_to_the_trace_its_work_was_submitted_under` in
  `crates/sluice/tests/telemetry_logs.rs`)


### Requirement: A finished run's checks keep their runner's times

The check seam SHALL answer, for a run that has ended and while its
directory stands, each of its checks as its runner timed it: moon's
from moon's records in the place, as while the run went, and the
`command` runner's one check from the run's own start and end. It is
never a result (37, 38).

#### Scenario: A finished command run is timed by its start and end

- **WHEN** a run of the `command` runner has ended
- **THEN** the check seam's times for it hold its one check, started
  when the run started and ended when it ended
  (`a_finished_command_run_is_timed_by_its_start_and_end` in
  `crates/sluice/tests/checks.rs`)

#### Scenario: A finished moon run's checks keep their times

- **WHEN** a moon run of `alpha:test` and `beta:test` has ended
- **THEN** the check seam's times for it hold each with its start and
  its end, its start no later than its end
  (`a_finished_moon_run_s_checks_are_timed` in
  `crates/sluice/tests/profiles/moon.rs`)

### Requirement: An exec's span holds each check it ran

While telemetry sends traces, `sluice exec` started under a caller's
context SHALL send beneath its `sluice exec` span one `sluice.check`
span for each check its run ran whose runner gave its start and end,
over those times, with `sluice.task` and `sluice.outcome`, `pass` or
`fail` as the run's report has it. A check answered from an earlier
run, or with no time from its runner, SHALL have none, and the
`sluice exec` span SHALL hold `sluice.ran` and `sluice.replayed`.

#### Scenario: Each check run is a span over its runner's times

- **WHEN** `sluice exec` started under a caller's context runs
  `tests:t-a`, which passes from 12:00:01 to 12:00:04, `tests:t-b`,
  which fails, and `tests:t-c`, answered from an earlier run
- **THEN** its `sluice exec` span holds a `sluice.check` span for
  `tests:t-a` from 12:00:01 to 12:00:04 with `sluice.outcome` `pass`,
  one for `tests:t-b` with `fail`, and none for `tests:t-c`, and holds
  `sluice.ran` 2 and `sluice.replayed` 1
  (`an_exec_s_span_holds_each_check_it_ran` in
  `crates/sluice/tests/telemetry_logs.rs`)

### Requirement: An exec's command is one check, named scrubbed

A command `sluice exec` runs after `--` SHALL be its run's one check,
its `sluice.check` span's `sluice.task` the command's words with
whatever reads as a secret replaced by `[redacted]`, as a log's body
is.

#### Scenario: A command's secret never names its span

- **WHEN** `sluice exec` started under a caller's context runs
  `-- env API_TOKEN=ghp_0123456789abcdefghijABCDEFGHIJ012345 true`
- **THEN** its one `sluice.check` span's `sluice.task` holds
  `[redacted]` and not the token
  (`an_exec_s_command_is_one_check_named_scrubbed` in
  `crates/sluice/tests/telemetry_logs.rs`)

### Requirement: An exec's checks nest under its span

While telemetry sends traces, the run `sluice exec` starts under a
caller's context SHALL be handed what a combination's run is handed
(151), its `TRACEPARENT` naming the `sluice exec` span in the caller's
trace.

#### Scenario: An exec's checks get its span through the binary

- **WHEN** the `sluice` binary runs `sluice --otel exec <branch> -- sh
  -c 'env > seen'` with `TRACEPARENT` `00-<t>-<s>-01` and a collector
- **THEN** `seen`'s `TRACEPARENT` names trace `<t>` and a span other
  than `<s>`, and the traces sent hold a `sluice exec` span whose
  parent is `<s>`
  (`an_exec_hands_its_checks_its_own_span_in_the_caller_s_trace` in
  `crates/sluice/tests/exec.rs`)
