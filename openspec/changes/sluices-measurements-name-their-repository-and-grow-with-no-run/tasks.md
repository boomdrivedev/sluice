# Tasks

Work in this change's place on
`change/sluices-measurements-name-their-repository-and-grow-with-no-run`,
cut from `bolt/sluice-reports-its-work-plainly`, which is its base and
its gate (S66). Each task leaves the tree building and its named check
passing. A unit test runs directly with `cargo test`; a test of
`crates/sluice/tests/` runs through sluice against the base, as
CLAUDE.md's "Running checks" has it: `sluice exec
bolt/sluice-reports-its-work-plainly -- cargo test …`. A test named as
rewritten keeps its name. Commit by pathspec, Conventional Commits
scoped to the part of the tree (`core`, `cli`, `docs`, `site`), citing
record 0014 as amended 2026-11-18, rules by id and the design's
decisions by D-number.

## 1. The rule

- [ ] 1.1 In `docs/architecture/core.md`, add rule core.66 after core.53, "No metric sluice sends, or hands a run's tools the means to send, carries a value that grows with each run: a metric carries its repository, its branch and, where it has one, its outcome, and nothing else; a run's tools send their spans and logs, naming the round that started them, and no metrics; and a metric that measures time declares it in seconds, `s`. (Record 0014 as amended 2026-11-18.)", and its details: what it rules out (moon's metrics carrying `sluice.trace_id`, `sluice.span_id`, `sluice.revision` and `sluice.place`, about 700 values each in a week, 2026-10-27; a metric with no repository; a time with no unit; a key outside `METRIC_KEYS`), what it does not (a span or a log naming a run; a collector's own processing), and its sources (statement 151 as amended 2026-11-18, blueprints c6bf590, 153, 154; record 0014 as amended 2026-11-18; Flume's rule core.56; this change's D1 to D4). In core.53 and its details, say a run's checks are handed the collector and the traces and logs sluice sends, never metrics, citing 151 as amended 2026-11-18. True when `openspec validate sluices-measurements-name-their-repository-and-grow-with-no-run --strict` passes.

## 2. Every metric names its repository and nothing of a run

- [ ] 2.1 In `crates/sluice/src/profiles/local_refs.rs`, add `repository_named(origin: Option<&str>, common: &Path) -> String` as D3 reads it, the last two names of a host URL's path, else the folder's name, `.git` dropped, nothing else of the URL kept; and a unit test `a_repository_is_named_by_its_origin_else_its_folder` holding the cases of the spec's "A repository's name". True when `cargo test -p sluice --lib a_repository_is_named_by_its_origin_else_its_folder` passes.
- [ ] 2.2 In `crates/sluice/src/profiles/local_refs.rs`, add `LocalRefs::repository_name`, `repository_named` of the URL `remote_url("origin")` reads, none where `origin` has no URL, and the common directory; in `crates/sluice-core/src/lib.rs`, give `Peer` `repository: String`, `this-repository` by default (D3); in `crates/sluice/src/profiles/mod.rs`, read it once in `Bound::bind`, keep it on `Bound` and give it in `Bound::peer`. Add `a_repository_is_named_by_the_origin_it_configures` to `crates/sluice/tests/local_refs.rs`: a scratch repository whose `origin` is `https://user:token@git.example/acme/app.git` is named `acme/app`, and one with no `origin` by its folder, with no fetch. True when `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --features tools --test local_refs a_repository_is_named_by_the_origin_it_configures` passes.
- [ ] 2.3 In `crates/sluice-core/src/lib.rs`, add `METRIC_KEYS`, `sluice.repository`, `sluice.branch` and `sluice.outcome`, its doc citing rule core.66 (D2); in `crates/sluice-core/src/step.rs` and `session.rs`, give every metric event `sluice.repository` from the sluice's peer beside `sluice.branch`. In `crates/sluice/tests/telemetry.rs`, hold every metric line of `a_landing_reports_its_round_verdict_and_landing` to naming `sluice.repository=this-repository` and the branch and to no field outside `METRIC_KEYS`, through one helper the other round tests also call, and write the expected lines of `a_failure_reports_its_culprit`, `a_replay_is_counted`, `a_conflict_is_counted` and `a_refused_landing_is_counted` with the repository. Say in `docs/telemetry.md` and `site/src/content/docs/reference/telemetry.md` that each metric carries `sluice.repository`, how it is named, and `sluice.branch`. True when `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --test telemetry a_landing_reports_its_round_verdict_and_landing` passes.

## 3. Every time declares seconds

- [ ] 3.1 In `crates/sluice-core/src/lib.rs`, add `SECONDS`, `sluice.submission.landed_after`, `sluice.candidate.tested_for` and `sluice.tasks.time_saved` (D4); in `crates/sluice/src/telemetry.rs`, build the meter provider through `meter_provider(exporter, resource)`, which adds a view giving each instrument `SECONDS` names the unit `s`, and call it from `Telemetry::begin`; and a unit test `a_time_is_sent_in_seconds` that builds it on an `InMemoryMetricExporter`, emits the four metrics of the spec's scenario through `MetricsLayer` under a scoped subscriber, flushes, and reads each exported metric's unit. Mark the two histograms' rows as declared in `s` in both telemetry pages. True when `cargo test -p sluice --lib a_time_is_sent_in_seconds` passes.

## 4. What a replay saved is counted

- [ ] 4.1 In `crates/sluice-core/src/verdict.rs`, give `Verdict` `saved_ms: u64`, summing for each proven replay the `took_ms` of the newest pass by `at` among those it counted for the key, nothing for an unproven replay or a pass with no time (D5); and a unit test `a_replay_saves_the_time_its_key_last_took` with the spec's three keys. True when `cargo test -p sluice-core a_replay_saves_the_time_its_key_last_took` passes.
- [ ] 4.2 In `crates/sluice-core/src/step.rs`, beside `sluice.tasks.replayed`, emit `monotonic_counter.sluice.tasks.time_saved` with the verdict's `saved_ms` in seconds as an `f64`, with the repository and the branch; add it to both telemetry pages' table, counter, `s`, the time each replayed task's key last took. Add `a_replay_counts_the_time_it_saved` to `crates/sluice/tests/telemetry.rs`, as `a_replay_is_counted` with `f.world.takes("lib:test", 4000, 1)`. True when `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --test telemetry a_replay_counts_the_time_it_saved` passes.

## 5. A run's tools send no metrics

- [ ] 5.1 In `crates/sluice/src/telemetry.rs`, have `variables` put `OTEL_METRICS_EXPORTER=none` in every run's environment, with a table or without, over the caller's (D1), its doc and the module's saying so; and a unit test `a_run_s_tools_send_no_metrics` building the four environments of the spec's scenario with `variables` under a valid span context. True when `cargo test -p sluice --lib a_run_s_tools_send_no_metrics` passes.
- [ ] 5.2 In `crates/sluice/tests/telemetry_logs.rs`, assert `OTEL_METRICS_EXPORTER=none` in `a_run_s_checks_join_the_round` and `with_traces_off_a_run_s_checks_join_no_trace`; in `crates/sluice/src/profiles/moon.rs`, say in the comment on `--otel` that moon sends its spans and no metrics. In both telemetry pages' "The checks inside a run", say a run's tools get the traces and logs sluice sends and no metrics, so their spans name the round and no metric names a run, and that moon names its own service. True when `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --test telemetry_logs a_run_s_checks_join_the_round` passes.
