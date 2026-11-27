# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. A wait's step is at the wait's time

- [x] 1.1 Add the required field `at` to `WaitAsk` in `crates/sluice/src/host_process/protocol.rs`, and set it from the wait's clock in `wait_ask` of `crates/sluice/src/lib.rs`, which sends each step.
- [x] 1.2 Run the step at `at` where `sluice host` passes only when asked, in `crates/sluice/src/host_process/reads.rs`. Check on Linux, where it failed before: `cargo nextest run -p sluice --features tools --test pull_requests`. Check on macOS: `sluice exec main -- cargo nextest run -p sluice --features tools --test pull_requests --test exec --test remote_branch --test peers -E 'not test(sandbox)'`.
- [x] 1.3 Move the times that `status_and_overview_show_a_hold_while_it_lasts` expects in `crates/sluice/tests/cli.rs`: its clock moves at each read, and each step reads it one more time. Check: `sluice exec main -- moon run cli:test`.

## 2. The change

- [x] 2.1 Run `openspec validate a-waits-step-is-at-the-waits-time --strict`. Check: `sluice exec main -- moon run root:specs root:changes`.
- [x] 2.2 Archive the change with `openspec archive a-waits-step-is-at-the-waits-time`.
