# Tasks

## 1. Workers that end with their owner

- [x] 1.1 Start each worker in a process group of its own, which records itself under `host/workers/` before its exec, and remove the supervisor's `setsid`. Check: `cargo test -p sluice --test host_process a_runs_checks_are_a_child_group_of_sluice_host`.
- [x] 1.2 End each worker's group when `sluice host` stops, give the run no outcome, and count it as a lapsed claim. Test a lost run that names no check again. Change the daemon's test that asserted its runs go on. Check: `cargo test -p sluice --test host_process a_stopped_host_process_ends_its_runs` and `a_stopped_daemon_ends_its_runs`, and `cargo test -p sluice-core a_lost_run_with_no_named_check_is_tested_again`.
- [x] 1.3 Sweep `host/workers/` as `sluice host` starts, before it claims a run. Check: `cargo test -p sluice --test host_process a_killed_host_processs_runs_end_at_the_next_start`.
- [x] 1.4 Hand over in place by `execve` for every host process, for a newer version and for `[telemetry]`, and take each worker from `host/workers/`. A daemon that takes the host's place asks the host process to end. An older command still uses a newer host process. Check: `cargo test -p sluice --test host_process an_upgrade_keeps_its_runs_as_children`, `a_stop_after_a_hand_over_ends_the_run`, `a_daemon_takes_the_place_of_a_host_process_that_ends_when_idle` and `an_older_command_uses_a_newer_host_process`.

## 2. The guard

- [x] 2.1 Add the guard to the test support. It stops the test's host processes, fails the test where a process of its folders is left, and ends each such process. Check: `cargo test -p sluice --test host_process a_test_that_leaves_a_run_fails`, `a_clean_test_passes` and `the_guard_ends_a_process_that_ignores_sigterm`.
- [x] 2.2 Find a detached process by the state folder and the scratch folders, and pass over another test's processes. Check: `cargo test -p sluice --test host_process the_guard_finds_a_detached_host_process` and `the_guard_passes_over_another_tests_process`.
- [ ] 2.3 Hold the guard in `OnDisk` and in each `Scratch`. Check: `sluice exec main -- moon run cli:test`.

## 3. The Rust test tasks

- [x] 3.1 Pin cargo-nextest in the devenv shell, and add the root's nextest profile with `leak-timeout` set to fail. Check: `sluice exec main -- moon run root:toolchain`.
- [x] 3.2 Run the `test`, `tools` and `sandbox` tasks of the five crates with `cargo nextest run`. Check: a fixture test that leaks `sleep 60` fails `cargo nextest run`, and the suite passes without it.
- [ ] 3.3 Change each test that the guard or nextest fails so that it ends its processes before it ends. Check: `sluice exec main -- moon run cli:tools`, then `cli:sandbox`, one at a time.

## 4. The shell test tasks

- [x] 4.1 Add `tests/leaks.sh`, which runs a test command in a process group of its own with its own folders, and fails where a process of the test is left. Check: `sluice exec main -- moon run root:leaks-cases`, a task of the root that runs the script on a test that leaves a watcher and on one that leaves nothing.
- [ ] 4.2 Run the plugins' shell test tasks and the check-e2e tier through `tests/leaks.sh`. Check: `sluice exec main -- moon run plugin-herdr:test-sh`, then `plugin-herdr:test-dash`, `plugin-sluice:hooks` and `plugin-sluice:lay-out`, one at a time.

## 5. The services

- [x] 5.1 Set `AbandonProcessGroup` false and `KillMode` `control-group`, run sluice by the system profile's `bin/sluice`, add `sluice hand-over`, and have the activation run it in place of a restart. Check: `sluice exec main -- moon run nix:test`, and `cargo test -p sluice --test host_process a_hand_over_starts_the_host_process_afresh_as_this_sluice`.

## 6. Records

- [x] 6.1 Amend the record of the host process with this change's decisions, each with Before and Now in plain words. Rewrite the rule on a check's process group in the same form. Check: `sluice exec main -- moon run root:changes`.
