# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. A check that the report holds nothing for counts toward the runs in a row

- [x] 1.1 In `finish_run` in `crates/sluice-core/src/step.rs`, add `v.none` to the checks that `read_not_run` reads where the run decided nothing (D1, D2). Keep the checks that the tree does not bear out out of the count (D4). Check: `sluice exec main -- cargo test -p sluice-conformance --test run_life a_check_no_report_holds_three_runs_in_a_row_is_blamed_with_why`, which fails without the change.
- [x] 1.2 Give a group of checks that a run of every check holds no task for no outcome (D3). Check: `sluice exec main -- moon run core:test conformance:test`.

## 2. The change

- [x] 2.1 Run `openspec validate a-run-with-no-outcome-stops-rerunning --strict`. Check: `sluice exec main -- moon run root:specs root:changes`.
- [x] 2.2 Archive the change with `openspec archive a-run-with-no-outcome-stops-rerunning`, so `openspec/specs/run-life/spec.md` holds its requirement.
