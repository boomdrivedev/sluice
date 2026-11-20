# Tasks

Work in this change's place on its branch, cut from main. Run suites
through sluice (`sluice exec main -- cargo test ...`); a quick
`cargo check`, `cargo clippy` or one unit test may run directly. Commit
by pathspec, Conventional Commits, scoped to the part of the tree,
citing clauses by number and rules by id.

## 1. The rule

- [x] 1.1 Add a lapsed claim counted toward no runs in a row to rule core.65's details, citing 115 and record 0031 as amended 2026-11-18. True when `openspec validate --strict` passes on the specs and this change.

## 2. A lapsed claim counts toward the runs in a row

- [x] 2.1 In `crates/sluice-core/src/step.rs`, count a lapse and a lost run through `count_not_run` and end them through `lapsed_end`. True when `sluice exec main -- cargo test -p sluice-conformance --test run_life a_pools_jobs_lost_in_a_row_read_the_combination_not_passed` passes, and fails with the count taken out.
- [x] 2.2 Count a standing executor's stop the same way. True when `sluice exec main -- cargo test -p sluice-conformance --test verdicts a_run_whose_executor_stops_three_runs_in_a_row_is_blamed_with_why` passes.

## 3. A lost job on GitHub Actions names its workflow run

- [x] 3.1 In `crates/sluice/src/profiles/github/actions.rs`, give the workflow run's address as a lost job's why. True when `sluice exec main -- cargo test -p sluice --test forge the_pools_calls` passes.
