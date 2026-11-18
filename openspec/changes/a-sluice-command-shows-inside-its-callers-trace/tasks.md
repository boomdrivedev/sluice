# Tasks

Work in this change's place on its branch, cut from
`bolt/sluice-reports-its-work-plainly`, which is `<base>` below. Run a
unit test directly and an integration test through sluice
(`sluice exec bolt/sluice-reports-its-work-plainly -- ...`). Commit by
pathspec, Conventional Commits, scoped to the part of the tree, citing
clause 153, rule core.53 and the design's decisions by id. Each test a
task names is one the task writes, named as the spec's scenario names
it.

## 1. The decision recorded

- [x] 1.1 Amend record 0014 with 153's build, dated the day it is written: a command reads its caller's W3C trace context and nests one span in it, making none of its own without one; a round stays a trace of its own; a submission keeps its caller's `TRACEPARENT` as `trace`; a combination's `sluice.start` and `sluice.verdict` link to it through a hook the binary sets, the core still depending only on `tracing`; an exec's span holds a span per check it ran, timed by its runner; a context turns nothing on. Name the change and design D1 to D9 as the construct stage's own choices, with the paths not taken. True when `sluice exec bolt/sluice-reports-its-work-plainly -- moon run site:build` passes, which makes the decision pages from `docs/adr`.

## 2. A command reads its caller's context

- [x] 2.1 In `crates/sluice/src/telemetry.rs`, add `Caller`, read from an environment's `TRACEPARENT` and `TRACESTATE` by OpenTelemetry's W3C trace context propagator: the remote span context and its `TRACEPARENT` as W3C writes it, none where it does not read (design D1); and the reading of a kept `TRACEPARENT` text into a span context that D5's link uses. True when `cargo test -p sluice --lib a_trace_context_is_read_as_w3c_reads_it` passes.
- [x] 2.2 Hold that `Settings::choose` reads no context: telemetry asked of nothing, with no table and a `TRACEPARENT` in the environment, sends nothing (design D7; rule core.53). True when `cargo test -p sluice --lib a_caller_s_context_turns_nothing_on` passes.

## 3. A command nests under its caller

- [x] 3.1 Give `Cli` the caller's context, `#[arg(skip)] caller: Option<Caller>`; add the one function of `telemetry` that makes a command's span, `sluice.command` with `otel.name` the command's words as `sluice --help` lists them, its parent set to the caller's context before anything beneath it starts, and records `sluice.exit` as the command returns, making none where `caller` is none; call it from `run` for every command it dispatches; and make `sluice.round` with no parent in `crates/sluice-core/src/step.rs` (design D2, D3). True when `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --test telemetry_logs a_command_nests_under_its_caller_s_trace` passes.
- [x] 3.2 Hold that sluice's host process keeps no `TRACEPARENT` or `TRACESTATE` of the command that starts it (design D3). True when `cargo test -p sluice --lib the_host_process_keeps_no_caller_s_trace_context` passes.

## 4. A submission keeps the trace it was submitted under

- [x] 4.1 In `crates/sluice-core`, give `SubmitRequest` and `GroupRequest` `trace: Option<String>` and `Submission` `trace` (`#[serde(default, skip_serializing_if = "Option::is_none")]`), written on every submission the act writes and read by nothing the core decides; and have the CLI's submit, from a worktree, by name or of several worktrees, pass `cli.caller`'s `TRACEPARENT` (design D4). True when `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --test cli a_submission_keeps_the_trace_it_was_submitted_under` passes.

## 5. A round links to the trace its work kept

- [x] 5.1 Add `sluice_core::links`, a hook set once that does nothing until set, `set(fn(&tracing::Span, &str, u64))` and `link(&span, trace, submission)`; in `step.rs`, call it on each `sluice.start` and `sluice.verdict` span for each submission the combination holds that kept a `trace`; and in `Telemetry::begin`, while traces go, set it to read the text as 2.1 reads a `TRACEPARENT` and add the link with `sluice.submission` (design D5; rule core.53). True when `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --test telemetry_logs a_round_links_to_the_trace_its_work_was_submitted_under` passes.

## 6. An exec's span holds each check it ran

- [x] 6.1 Add `Tests::timed(&run)` to `crates/sluice-seams`, with no default: each check of a run that has ended, as its runner timed it, while its directory stands, never a result. Answer it in `crates/sluice/src/profiles/checks.rs`, the `command` runner's one check from the run's own start and end (`runs::ended`); in `crates/sluice-fakes`, with what the run said of its tasks; and pass it on in the host's room wrapper (`host_process/room.rs`), the conformance suite's `RecordedChecks` and `sluice-core`'s test `Host` (design D6). True when `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --test checks a_finished_command_run_is_timed_by_its_start_and_end` passes.
- [x] 6.2 Answer `timed` for a moon run from moon's records in the place, as `tasks` reads them while the run goes, without the guard that leaves `tasks` empty once the run has ended (design D6; ADR 0028). True when `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --features tools --test moon a_finished_moon_run_s_checks_are_timed` passes.
- [x] 6.3 In `crates/sluice/src/exec.rs`, once the run has finished and before the place is released, ask `timed` and send beneath the exec's span a `sluice.check` span for each check of the report that ran with a start and an end from its runner, over those times, with `sluice.task` and `sluice.outcome`; give the exec's span `sluice.branch`, `sluice.ran` and `sluice.replayed`; keep the tracer `Telemetry::begin` starts, beside `SENT`, to build the check spans with their times under the exec span's context (design D6, D8). True when `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --test telemetry_logs an_exec_s_span_holds_each_check_it_ran` passes.
- [x] 6.4 Scrub each check's name as a log's body is before its span is sent, so a command after `--` that carries a secret names its span with `[redacted]` in its place (design D6). True when `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --test telemetry_logs an_exec_s_command_is_one_check_named_scrubbed` passes.

## 7. Through the binary

- [ ] 7.1 In `main.rs`, fill `cli.caller` from the environment the command started with (`Started.env`), and make a kept `sluice run`'s span by the function 3.1 adds; have `support::isolated` take `TRACEPARENT` and `TRACESTATE` out of every process a test starts (design D1, D2, D9). True when `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --features tools --test exec an_exec_hands_its_checks_its_own_span_in_the_caller_s_trace` passes.
- [ ] 7.2 Say on the telemetry reference page, `site/src/content/docs/reference/telemetry.md`, and in `docs/telemetry.md` what a command started under a caller's trace context sends, that a submission keeps the context and the rounds that test it link to it, what an exec's span holds, and that a context turns nothing on (153; record 0014 as 1.1 amends it). True when `sluice exec bolt/sluice-reports-its-work-plainly -- moon run site:build` passes.
