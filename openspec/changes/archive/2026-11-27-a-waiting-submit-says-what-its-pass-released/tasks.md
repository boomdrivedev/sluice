# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. A waiting submit says what its pass released

- [x] 1.1 Number each pass's report, and send it before the reading's end is marked, in `crates/sluice/src/host_process/tasks.rs`. Check: `sluice exec main -- cargo nextest run -p sluice --features tools --test landed`.
- [x] 1.2 A wait's step that ends the wait names the last pass reported once the passes in flight end, in `crates/sluice/src/host_process/reads.rs`. Check: the same.
- [x] 1.3 A waiting submit follows before it submits, and says the reports up to the step's pass before its end, in `main.rs`, `lib.rs` and `subscriber.rs`. Check: `sluice exec main -- cargo nextest run -p sluice --features tools --test landed -E 'test(a_waiting_submit_held_past_its_pass_says_what_the_pass_released)'`, which fails without the change.

## 2. A run ended at its limit

- [x] 2.1 `a_run_is_ended_at_its_limit_after_the_step_that_started_it` waits until the run reads finished. Check: `sluice exec main -- cargo nextest run -p sluice --features tools --test conformance -E 'test(a_run_is_ended_at_its_limit_after_the_step_that_started_it)'`.

## 3. The change

- [x] 3.1 Run `openspec validate a-waiting-submit-says-what-its-pass-released --strict`. Check: `sluice exec main -- moon run root:specs root:changes`.
- [x] 3.2 Archive the change with `openspec archive a-waiting-submit-says-what-its-pass-released`.
