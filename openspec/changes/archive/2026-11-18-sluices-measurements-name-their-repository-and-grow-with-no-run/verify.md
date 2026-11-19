# Verify

The change `sluices-measurements-name-their-repository-and-grow-with-no-run`
on `change/sluices-measurements-name-their-repository-and-grow-with-no-run`
at `dd27c1d80`, against its base and gate,
`bolt/sluice-reports-its-work-plainly` at `5d7d1c3d0` (S66).

Verdict: the whole checks pass, every task is done, and every requirement
and scenario of the delta spec is met by code and a test that passes. The
change is ready to archive.

## The whole checks

`sluice exec bolt/sluice-reports-its-work-plainly`, run once on
2026-11-18: the bolt's gate, `#check-fast`, for the checks `dd27c1d`
affects. Exit 0.

| Task | Outcome |
|---|---|
| `nix:typecheck` | passed |
| `fakes:test` | passed |
| `conformance:test` | passed |
| `core:test` | passed |
| `cli:test` | passed |
| `root:format` | passed |
| `root:changes` | passed |
| `root:lint` | passed |

sluice said `passed: 8 tasks` and `recorded 8 results under their keys for
dd27c1d`. `root:specs` was not among the affected checks, since the
change writes no spec under `openspec/specs/`. moon warned that it could
not archive `cli:test`'s outputs into the place's `.moon/cache`, which
decides nothing.

`a_repository_is_named_by_the_origin_it_configures` is behind the
`tools` feature, which `#check-fast` does not build. It was run as task
2.2 names it: `sluice exec bolt/sluice-reports-its-work-plainly -- cargo
test -p sluice --features tools --test local_refs
a_repository_is_named_by_the_origin_it_configures`, 1 passed.

`openspec validate sluices-measurements-name-their-repository-and-grow-with-no-run --strict`:
valid.

## Tasks

All nine are ticked, and each one's named check passes:

| Task | Check | Outcome |
|---|---|---|
| 1.1 core.66 and core.53 | `openspec validate --strict`; `docs/architecture/core.md` holds core.66 after core.53 with its details, and core.53's rule and details hand a run's checks the traces and logs, never metrics | met |
| 2.1 `repository_named` | `a_repository_is_named_by_its_origin_else_its_folder` | passed (`cli:test`) |
| 2.2 `Peer::repository`, `Bound` | `a_repository_is_named_by_the_origin_it_configures` | passed (above) |
| 2.3 `METRIC_KEYS`, every metric names its repository | `a_landing_reports_its_round_verdict_and_landing` | passed (`cli:test`) |
| 3.1 `SECONDS`, `meter_provider` | `a_time_is_sent_in_seconds` | passed (`cli:test`) |
| 4.1 `Verdict::saved_ms` | `a_replay_saves_the_time_its_key_last_took` | passed (`core:test`) |
| 4.2 `sluice.tasks.time_saved` | `a_replay_counts_the_time_it_saved` | passed (`cli:test`) |
| 5.1 `OTEL_METRICS_EXPORTER=none` | `a_run_s_tools_send_no_metrics` | passed (`cli:test`) |
| 5.2 the run's tests, moon's comment, the pages | `a_run_s_checks_join_the_round` | passed (`cli:test`) |

## Requirements and scenarios

**A branch's queue is measured** (modified). Every metric event in
`crates/sluice-core/src/step.rs` and `session.rs`, the queue's gauges,
the counters and the histograms, carries `sluice.repository` from the
sluice's peer beside `sluice.branch`; a search of the source finds no
metric event without it. "A landing's metrics" and "A blamed submission
is counted": `a_landing_reports_its_round_verdict_and_landing` and
`a_failure_reports_its_culprit`, passed.

**Waiting and testing are timed** (modified). The two histograms carry
the repository and are declared in `s` by the view `meter_provider`
adds for each name in `sluice_core::SECONDS`. "A landing is timed":
`a_landing_reports_its_round_verdict_and_landing`, passed. "A time is
sent in seconds": `a_time_is_sent_in_seconds` exports the three times
with the unit `s` and `sluice.landed` with none, passed.

**A run's checks join the round** (modified). `variables` keeps the
collector, the traces and logs, `TRACEPARENT` and the run's ids on its
resource, and adds `OTEL_METRICS_EXPORTER=none`; `run_env` still adds
nothing while telemetry is off. "A resource attribute's value is
encoded": `values_are_encoded_where_the_syntax_would_read_them`, passed.
"A run joins the round": `a_run_s_checks_join_the_round` asserts the
endpoint, `OTEL_METRICS_EXPORTER=none`, `TRACEPARENT` and the
attributes, passed. "A run joins no trace while traces are off":
`with_traces_off_a_run_s_checks_join_no_trace` asserts
`OTEL_METRICS_EXPORTER=none` beside `OTEL_TRACES_EXPORTER=none`, passed.

**With no table, a run gets the caller's telemetry** (modified). With no
table the caller's `OTEL_*` reach the run, `OTEL_METRICS_EXPORTER`
written over to `none`. "A run gets the caller's collector and
attributes": `a_run_s_checks_join_the_round`, passed.

**A run's tools send no metrics** (added). `variables` inserts
`OTEL_METRICS_EXPORTER=none` on both arms, with a table and without.
"A run's tools send no metrics": `a_run_s_tools_send_no_metrics` builds
the four environments, a table sending metrics, a table with
`metrics = false`, no table, and no table with the caller's
`OTEL_METRICS_EXPORTER=otlp`, and finds `none` and the run's ids in each,
passed.

**A metric carries only its repository, branch and outcome** (added).
`sluice_core::METRIC_KEYS` lists the three keys; the helper
`metrics_carry_only_their_keys` in `crates/sluice/tests/telemetry.rs`
holds every metric line to naming the repository and the branch and to
no key outside the list. "A round's metrics carry nothing of a run":
the helper is called by `a_landing_reports_its_round_verdict_and_landing`,
`a_failure_reports_its_culprit`, `a_replay_is_counted`,
`a_conflict_is_counted` and `a_refused_landing_is_counted`, each passed.

**A metric names its repository by its origin, else its folder** (added).
`repository_named` in `crates/sluice/src/profiles/local_refs.rs` takes
the last two names of a hosted URL's path, `.git` and a trailing `/`
dropped, and otherwise the folder; `LocalRefs::repository_name` reads
`origin` as configured, with no process and no fetch, and `Bound::bind`
reads it once and gives it on every peer. "A repository's name":
`a_repository_is_named_by_its_origin_else_its_folder` holds the five
cases and that no name holds `user`, `token` or a host, passed.

**What a replay saved is measured** (added). `verdict::read` sums, for
each proven replay, the `took_ms` of the newest counted pass by `at`,
adding nothing for an unproven replay or a pass with no time; the round
emits `sluice.tasks.time_saved` in seconds with the repository and the
branch. "A replay saves its key's last time":
`a_replay_saves_the_time_its_key_last_took` reads 6000 ms saved and
`c:test` unproven, passed. "A round counts what its replays saved":
`a_replay_counts_the_time_it_saved` sees `4.0` seconds with the
repository and the branch, passed.

## Rules

- core.66 is in `docs/architecture/core.md` after core.53, with its
  details citing 151 as amended 2026-11-18, record 0014 as amended
  2026-11-18 and design D1 to D4.
- core.53's rule and details hand a run's checks the traces and logs
  sluice sends, never metrics.
- core.1: `repository_named` names no forge.

## Observations

Nothing falls short. The proposal's "Touches" leaves out files the work
had to change: `crates/sluice/src/profiles/moon.rs`, which task 5.2 names,
`crates/sluice/tests/local_refs.rs`, which task 2.2 names, and the
`Peer { .. }` literals in `crates/sluice-conformance/tests/` and
`crates/sluice/tests/cli.rs`, which take the new field. Each is a direct
consequence of the tasks.
