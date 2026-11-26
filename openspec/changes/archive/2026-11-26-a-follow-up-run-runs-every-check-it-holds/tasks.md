# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. A follow-up run runs every check it holds

- [x] 1.1 In `start_run` in `crates/sluice-core/src/step.rs`, give `since` only to a run of the gate, a first run or a follow-up that holds every check (D1, D2). Check: `sluice exec main -- cargo test -p sluice-conformance --test run_life a_follow_up_run_runs_its_checks_after_the_work_ahead_lands`, which fails without the change, with `an_unproven_replay_runs_again_alone` still passing.
- [x] 1.2 Add `submit_from` to `crates/sluice-conformance/tests/run_life.rs`, so two sessions submit to one branch. Check: `sluice exec main -- moon run sluice-conformance:test`.

## 2. The change

- [x] 2.1 Run `openspec validate a-follow-up-run-runs-every-check-it-holds --strict`. Check: `sluice exec main -- moon run root:specs root:changes`.
- [x] 2.2 Archive the change with `openspec archive a-follow-up-run-runs-every-check-it-holds`, so `openspec/specs/run-life/spec.md` holds its requirement.
