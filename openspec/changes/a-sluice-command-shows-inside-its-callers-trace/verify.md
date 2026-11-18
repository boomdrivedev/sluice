# Verify: a-sluice-command-shows-inside-its-callers-trace

Verified on `change/a-sluice-command-shows-inside-its-callers-trace` at
`afe581ea5`, cut from `bolt/sluice-reports-its-work-plainly` at
`5d7d1c3d0`. Its base now stands at `ba58c01d4`. Verified 2026-11-18.

**Verdict: the work holds, but it cannot land as it stands, so nothing
is archived.** The gate passes, every task is ticked, and every
requirement and scenario is met on the branch's own tree. The branch
conflicts with its base in three files. Archiving now would add a fourth
conflict, in a spec file. The code stage takes the base in, and the
change is verified again on the merged tree.

## The whole checks

`sluice exec bolt/sluice-reports-its-work-plainly` ran once and exited
0. It ran the bolt's gate, `#check-fast`, as `.config/sluice.toml` names
it, over the checks `afe581e` affects, against the base at `ba58c01`:

- 9 tasks passed: `nix:typecheck`, `root:changes`, `root:format`,
  `root:lint`, `seams:test`, `core:test`, `fakes:test`,
  `conformance:test` and `cli:test`.
- 1448 tests passed. None failed and none was ignored.
- sluice recorded the 9 results under their keys for `afe581e`.

Before the run, sluice printed this forecast:

```
would conflict: crates/sluice/src/telemetry.rs, docs/telemetry.md, site/src/content/docs/reference/telemetry.md with bolt/sluice-reports-its-work-plainly
```

`#check-tools` and `#check-e2e` are main's, run when the bolt merges
onto main. Four tasks name a check outside the bolt's gate. Each was run
once through `sluice exec bolt/sluice-reports-its-work-plainly -- ...`,
and each passed:

- Task 6.2: `cargo test -p sluice --features tools --test moon`,
  `a_finished_moon_run_s_checks_are_timed`.
- Task 7.1: the same command's `--test exec`,
  `an_exec_hands_its_checks_its_own_span_in_the_caller_s_trace`.
- Tasks 1.1 and 7.2: `moon run site:build`.

`openspec validate a-sluice-command-shows-inside-its-callers-trace
--strict` passes.

## What falls short: the branch conflicts with its base

Since this place was cut, the base has landed #105,
`change/sluices-measurements-name-their-repository-and-grow-with-no-run`
(`947b28702` to `ba58c01d4`). That change also touched telemetry, and
`git merge-tree` of the branch with `origin/bolt/sluice-reports-its-work-plainly`
leaves these conflicts:

- `crates/sluice/src/telemetry.rs`. The imports conflict: this change
  adds `propagation::TraceContextPropagator` and `trace::SdkTracer`,
  and #105 adds `metrics::exporter::PushMetricExporter`, `Instrument`
  and `Stream`. Beside them, `Caller`, `command`, `checks`, `link`,
  `traceparent` and `extract` are added next to #105's additions after
  `impl Settings`.
- `docs/telemetry.md`. This change's four new bullets sit just before
  the metrics bullet, which #105 rewrote to name `sluice.repository`.
- `site/src/content/docs/reference/telemetry.md`. The same clash as in
  `docs/telemetry.md`.

Record 0014 merges without a conflict, though it now holds two
amendments dated 2026-11-18.

Archiving here would also break `openspec/specs/telemetry/spec.md`.
#105 archived its own requirements into that spec on the base. Run on
this branch, `openspec archive` would build the spec from the old base,
and a three-way merge of that result with the base's spec conflicts.
The only way to settle that conflict is to edit the spec by hand, which
the repository forbids. Archiving after the base is taken in applies
this change's 9 added requirements to the base's spec as it stands.

The tree that passed is not the tree that would land. The merged
`telemetry.rs` brings #105's changes to what a run's checks are handed:
traces and logs, never metrics (151 as amended 2026-11-18). The proposal
counts on that change reaching an exec's checks too ("so that change's
build reaches both"), and nothing has tested that yet.

## Tasks

Every task, 1.1 to 7.2, is ticked. Every test the tasks and the delta
spec name is defined where they say, under the scenario's name:

| Test | Where |
|---|---|
| `a_trace_context_is_read_as_w3c_reads_it` | `crates/sluice/src/telemetry.rs` |
| `a_caller_s_context_turns_nothing_on` | `crates/sluice/src/telemetry.rs` |
| `the_host_process_keeps_no_caller_s_trace_context` | `crates/sluice/src/host_process/environment.rs` |
| `a_command_nests_under_its_caller_s_trace` | `crates/sluice/tests/telemetry_logs.rs` |
| `a_round_links_to_the_trace_its_work_was_submitted_under` | `crates/sluice/tests/telemetry_logs.rs` |
| `an_exec_s_span_holds_each_check_it_ran` | `crates/sluice/tests/telemetry_logs.rs` |
| `an_exec_s_command_is_one_check_named_scrubbed` | `crates/sluice/tests/telemetry_logs.rs` |
| `a_submission_keeps_the_trace_it_was_submitted_under` | `crates/sluice/tests/cli.rs` |
| `a_finished_command_run_is_timed_by_its_start_and_end` | `crates/sluice/tests/checks.rs` |
| `a_finished_moon_run_s_checks_are_timed` | `crates/sluice/tests/profiles/moon.rs` |
| `an_exec_hands_its_checks_its_own_span_in_the_caller_s_trace` | `crates/sluice/tests/exec.rs` |

## Requirements and scenarios

I read each requirement of the delta spec, `specs/telemetry/spec.md`,
against the code and the test it names. Each holds on the branch's tree.

- **A command started under a caller's trace nests in it.**
  `Caller::read` reads `TRACEPARENT` and `TRACESTATE` with
  OpenTelemetry's `TraceContextPropagator` and returns `None` for a
  value that does not read. `main` fills `cli.caller` from
  `Started.env`. `run` wraps every command it dispatches in
  `telemetry::command`, and `main` wraps a kept `sluice run` the same
  way. That makes a `sluice.command` span named by `Cli::words`, as
  `sluice --help` lists the commands, whose parent is the caller's
  span, and records `sluice.exit`. With no caller it makes no span. The
  test of the W3C reading covers a good value with its trace state, an
  all-zero trace id, hex in upper case, version `ff`, three fields and
  an empty value. The nesting test sends one `sluice submit` span in
  the caller's trace, with the caller's span as its parent and the exit
  `FAILED`, and none for the submit started under no context.
- **A caller's trace turns nothing on.** `Settings::choose` reads no
  context. The test chooses with no telemetry asked and with
  `send = false`, both beside a `TRACEPARENT`, and neither sends.
- **A round is a trace of its own under any command.** `sluice.round`
  is made with `parent: None`. The nesting test reads every round with
  no parent and each in a trace of its own, never the caller's. The
  host process's environment list keeps neither variable, and
  `the_host_process_keeps_no_caller_s_trace_context` holds it.
- **A submission keeps the trace it was submitted under.**
  `SubmitRequest` and `GroupRequest` carry `trace`. `admit` writes it
  on every submission of the act, and `Submission.trace` is left out of
  the record when it is `None`. The CLI passes `cli.caller`'s
  `TRACEPARENT` to a submit of several worktrees, a submit by name and
  a submit from a worktree. The pull request intake in `pulls.rs`
  writes `trace: None`. Nothing in the core reads the field except
  `link_kept`. The test, run with telemetry off, reads submissions 1 to
  3 keeping the context and submission 4 keeping none.
- **A round that tests a submission links to its trace.**
  `sluice_core::links` does nothing until something sets it.
  `Telemetry::begin` sets it to `telemetry::link` while traces go,
  which adds a link holding `sluice.submission`. `step.rs` links each
  `sluice.start`, from `StartFrom.traces`, and each `sluice.verdict`,
  from the live records, for each submission that kept a context. The
  test reads exactly one link, to the caller's span with
  `sluice.submission=1`, on every start and verdict, including a
  verdict on `[1, 2]`.
- **A finished run's checks keep their runner's times.**
  `Tests::timed` has no default. `Checks::timed` answers a finished
  run: moon's checks through `moon_tasks`, which `tasks` shares, and
  the `command` runner's one check from `result.json`'s start and end,
  or none when the command never started. The fakes, the host's room
  wrapper, the conformance suite's recording checks and the core's
  test hosts all answer it. The command test matches the span of the
  check's times to the report's `took_ms` for a pass and for a fail,
  and the moon test reads `alpha:test` and `beta:test`, each passed and
  each starting no later than it ends.
- **An exec's span holds each check it ran.** `exec::spans` runs after
  `finish_session` and before the place is released. It records
  `sluice.branch`, `sluice.ran` and `sluice.replayed`, and sends a
  `sluice.check` span for each check that ran and has both times, built
  with the kept `TRACER` under the exec span's context. The test reads
  `tests:t-a` passing at the given times, `tests:t-b` failing, no span
  for the replayed `tests:t-c`, `ran` 2 and `replayed` 1.
- **An exec's command is one check, named scrubbed.**
  `telemetry::checks` scrubs each check's name with `Scrub` over the
  environment the settings were chosen from. The test reads one check
  span whose `sluice.task` holds `[redacted]` and not the token.
- **An exec's checks nest under its span.** The exec span is current
  while `Checks::start` builds the run's environment, so the run gets
  its `TRACEPARENT`. The binary test reads trace `<t>` and a span other
  than `<s>` in the check's environment, and a `sluice exec` span whose
  parent is `<s>` in what reached the collector. `support::isolated`,
  and through it `telemetry_logs.rs`'s `alone`, remove `TRACEPARENT`
  and `TRACESTATE` from every process a test starts (design D9).

## What the code stage does next

1. Take `bolt/sluice-reports-its-work-plainly` into the branch. Keep
   both sides of each conflict in `telemetry.rs`, `docs/telemetry.md`
   and the site's telemetry page, and leave the change unarchived.
2. Commit the merge, then hand the change back for verify. Verify runs
   the gate on the merged tree, including the check that an exec's
   checks get #105's traces and logs with no metrics, and archives onto
   the base's `telemetry` spec.
