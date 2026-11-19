# Verify: a-sluice-command-shows-inside-its-callers-trace

Verified on `change/a-sluice-command-shows-inside-its-callers-trace` at
`e5aa1f22a`, which holds its base, `bolt/sluice-reports-its-work-plainly`
at `ba58c01d4`, by the merge `d912ef734`. Verified 2026-11-18 on deneb
with sluice 0.95.1.

**Verdict: the work holds, and the change is archived.** The branch
merges onto its base with no conflict. Every check of the bolt's gate
passed, along with each check a task names outside the gate. Every task
is ticked, and every requirement and scenario of the delta spec is met
on the merged tree. sluice could not record the gate's results, and the
command exited 3 for it, for a cause on the host that this change does
not touch (below).

## The whole checks

`sluice exec bolt/sluice-reports-its-work-plainly` ran once. It ran the
bolt's gate, `#check-fast`, as `.config/sluice.toml` names it, over the
checks `e5aa1f2` affects, against the base at `ba58c01`. It printed no
forecast of a conflict.

- 9 tasks completed: `nix:typecheck`, `root:changes`, `root:format`,
  `root:lint`, `seams:test`, `core:test`, `fakes:test`,
  `conformance:test` and `cli:test`. None failed.
- 1453 tests passed. None failed and none was ignored.
- `root:changes` held all 6 open changes valid under
  `openspec validate --strict`, and archiving this one onto the base's
  specs passes as well (core.61).

**The command then exited 3**, once every task had completed, with:

```
sluice: another fetch of this repository held /Users/chuck/.local/share/boomdrive/repositories/boomdrivedev/sluice/sluice.git/sluice-fetch.lock past 30s, the most a try waits for it
```

So sluice recorded none of the 9 results under their keys. This is
sluice's bounded wait on the repository's fetch lock, which core.29
asks for (`profiles/pushed_refs.rs`). Another fetch held the lock past
the 30 seconds a try may wait. As the run ended, deneb's one sluice
repository was being fetched by many processes at once:

- 13 `sluice run` processes, started an hour before and adopted by
  init.
- 7 `sluice overview --watch --json` processes under one
  `sluice-herdr-plugin-0.94.8` `watch.sh`, the oldest about an hour
  old.
- `sluice status <bolt> --all --json`, which `flume --json status` ran
  for several places, each starting its own `git fetch` of the branch's
  state ref (pid 33512's `git fetch … refs/sluice/state/bolt/sluice-reports-its-work-plainly`).
- Other places' `sluice exec` runs.

core.25, as amended 2026-11-13, rules out a one-shot status that
fetches the records itself while the host process has read within its
tick. Whether the host process had read within its tick when those
statuses fetched was not established here. This is reported for
whoever looks after sluice on deneb. Nothing in this change reads or
takes the fetch lock.

**Why the change is archived all the same.** Every check the gate
names ran and passed on the tree that would land. The step that failed
keeps their results for a later replay, and its failure decides nothing
about the work. A result left unrecorded is never counted as a pass:
the queue runs the gate again when the work is submitted. The other
path was to stop short and send the change back. The code stage could
fix nothing, since the cause is the host's load, and the stage's
instructions forbid running the gate a second time. So that path was
not taken.

## Checks the tasks name outside the gate

`#check-tools` and `#check-e2e` are main's, run when the bolt merges
onto main. Each check a task names outside the bolt's gate was run once
through `sluice exec bolt/sluice-reports-its-work-plainly -- …` on the
merged tree, and each passed:

- Task 6.2: `cargo test -p sluice --features tools --test moon
  a_finished_moon_run_s_checks_are_timed`, 1 passed.
- Task 7.1: `cargo test -p sluice --features tools --test exec
  an_exec_hands_its_checks_its_own_span_in_the_caller_s_trace`, 1
  passed.
- Task 7.3: `cargo check --workspace --all-targets --features
  sluice/tools` passed. `telemetry::tests` passed in the gate's
  `cli:test`, including `a_run_s_tools_send_no_metrics`.
- Tasks 1.1 and 7.2: `moon run site:build`, 147 pages built and every
  internal link valid.

`openspec validate a-sluice-command-shows-inside-its-callers-trace
--strict` passes.

## The merge with the base

`git merge-base --is-ancestor ba58c01d4 HEAD` holds, and the base has
not moved since. `git merge-tree` of the branch with
`origin/bolt/sluice-reports-its-work-plainly` writes a tree with no
conflict. The merge keeps both sides:

- `crates/sluice/src/telemetry.rs` holds #105's `PushMetricExporter`,
  `Instrument`, `Stream` and `meter_provider` beside this change's
  `Caller`, `command`, `checks`, `link`, `traceparent`, `extract` and
  `TRACER`. Its tests hold `a_trace_context_is_read_as_w3c_reads_it`
  and `a_caller_s_context_turns_nothing_on` beside
  `a_run_s_tools_send_no_metrics`.
- `docs/telemetry.md` and `site/src/content/docs/reference/telemetry.md`
  hold this change's four bullets just before the metrics bullet, which
  names `sluice.repository` as #105 rewrote it.
- The run an exec starts gets its environment from `telemetry::run_env`
  through `Checks::start`, as a round's run does. So #105's
  `OTEL_METRICS_EXPORTER=none` reaches an exec's checks too (151 as
  amended 2026-11-18; rule core.66), as the proposal counts on.

## Tasks

Every task, 1.1 to 7.3, is ticked. Every test the tasks and the delta
spec name is defined where they say, under the scenario's name, and
passed:

| Test | Where | Ran in |
|---|---|---|
| `a_trace_context_is_read_as_w3c_reads_it` | `crates/sluice/src/telemetry.rs` | gate |
| `a_caller_s_context_turns_nothing_on` | `crates/sluice/src/telemetry.rs` | gate |
| `the_host_process_keeps_no_caller_s_trace_context` | `crates/sluice/src/host_process/environment.rs` | gate |
| `a_command_nests_under_its_caller_s_trace` | `crates/sluice/tests/telemetry_logs.rs` | gate |
| `a_round_links_to_the_trace_its_work_was_submitted_under` | `crates/sluice/tests/telemetry_logs.rs` | gate |
| `an_exec_s_span_holds_each_check_it_ran` | `crates/sluice/tests/telemetry_logs.rs` | gate |
| `an_exec_s_command_is_one_check_named_scrubbed` | `crates/sluice/tests/telemetry_logs.rs` | gate |
| `a_submission_keeps_the_trace_it_was_submitted_under` | `crates/sluice/tests/cli.rs` | gate |
| `a_finished_command_run_is_timed_by_its_start_and_end` | `crates/sluice/tests/checks.rs` | gate |
| `a_finished_moon_run_s_checks_are_timed` | `crates/sluice/tests/profiles/moon.rs` | task 6.2 |
| `an_exec_hands_its_checks_its_own_span_in_the_caller_s_trace` | `crates/sluice/tests/exec.rs` | task 7.1 |

## Requirements and scenarios

Each requirement of the delta spec, `specs/telemetry/spec.md`, was read
against the merged code and the test it names. Each holds.

- **A command started under a caller's trace nests in it.**
  `Caller::read` reads `TRACEPARENT` and `TRACESTATE` with
  OpenTelemetry's `TraceContextPropagator`, and gives `None` for a
  value that does not read. `main` fills `cli.caller` from
  `Started.env`. `run` wraps every command it dispatches in
  `telemetry::command`, and `main` wraps a kept `sluice run` the same
  way. That makes a `sluice.command` span named by the command's words,
  as `sluice --help` lists them, with the caller's span as its parent,
  and records `sluice.exit`. With no caller it makes no span. The W3C
  test covers a good value with its trace state, then an all-zero trace
  id, upper-case hex, version `ff`, three fields and an empty value,
  each read as none. The nesting test reads one `sluice submit` span in
  the caller's trace, under the caller's span and holding the exit, and
  none for the submit started under no context.
- **A caller's trace turns nothing on.** `Settings::choose` reads no
  context. The test chooses settings beside a `TRACEPARENT`, once with
  no telemetry asked and once with `send = false`, and neither sends.
- **A round is a trace of its own under any command.** `sluice.round`
  is made with `parent: None`. The nesting test reads each round with
  no parent, in a trace of its own, never the caller's. The host
  process's list of environment variables keeps neither
  `TRACEPARENT` nor `TRACESTATE`.
- **A submission keeps the trace it was submitted under.**
  `SubmitRequest` and `GroupRequest` carry `trace`. `admit` writes it
  on every submission of the act, and a `None` is left out of the
  record. The CLI passes the caller's `TRACEPARENT` for a submit from a
  worktree, by name, and of several worktrees. Pull request intake
  writes `trace: None`. In the core, only the link hook reads it. The
  test, run with telemetry off, reads submissions 1 to 3 keeping the
  context and submission 4 keeping none.
- **A round that tests a submission links to its trace.**
  `sluice_core::links` does nothing until something sets it.
  `Telemetry::begin` sets it to `telemetry::link` while traces go, and
  each link holds `sluice.submission`. `step.rs` links each
  `sluice.start` and each `sluice.verdict` of a combination for each
  submission that kept a context. The test reads exactly one link on
  every start and verdict, to the caller's span with
  `sluice.submission=1`, and none for submission 2.
- **A finished run's checks keep their runner's times.**
  `Tests::timed` has no default. A finished moon run is answered from
  moon's records in the place. The `command` runner's one check is
  answered from the run's own start and end. The fakes, the host's room
  wrapper, the conformance suite's recording checks and the core's test
  hosts all answer `timed`. The command test and the moon test each
  pass.
- **An exec's span holds each check it ran.** After the session run
  finishes, and before the place is released, `exec` records
  `sluice.branch`, `sluice.ran` and `sluice.replayed`. It then sends a
  `sluice.check` span for each check that ran and has both times,
  built with the kept tracer over those times. The test reads
  `tests:t-a` passing at the given times, `tests:t-b` failing, no span
  for the replayed `tests:t-c`, `ran` 2 and `replayed` 1.
- **An exec's command is one check, named scrubbed.**
  `telemetry::checks` scrubs each check's name as a log's body is
  scrubbed. The test reads `[redacted]` in place of the token.
- **An exec's checks nest under its span.** The exec span is current
  while `Checks::start` builds the run's environment. The binary test
  reads the caller's trace and a span other than the caller's in the
  check's `TRACEPARENT`, and a `sluice exec` span whose parent is the
  caller's span at the collector. `support::isolated` removes
  `TRACEPARENT` and `TRACESTATE` from every process a test starts.
