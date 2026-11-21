# Tasks

## 1. Workers that end with their owner

- [ ] 1.1 Start each worker through process-wrap in a process group of its own, recorded under `host/workers/`, and remove the supervisor's `setsid`. Check: `cargo test -p sluice --test host_process a_runs_checks_are_a_child_group_of_sluice_host`.
- [ ] 1.2 End each worker's group when `sluice host` stops, give the run no outcome, and count it as a lapsed claim. Change the daemon's test that asserted its runs go on. Check: `cargo test -p sluice --test host_process a_stopped_host_process_ends_its_runs` and `a_stopped_daemon_ends_its_runs`.
- [ ] 1.3 Sweep `host/workers/` as `sluice host` starts, before it claims a run. Check: `cargo test -p sluice --test host_process a_killed_host_processs_runs_end_at_the_next_start`.
- [ ] 1.4 Hand over in place by `execve` for every host process, for a newer version and for `[telemetry]`, and take each worker from `host/workers/`. An older command still uses a newer host process. Check: `cargo test -p sluice --test host_process an_upgrade_keeps_its_runs_as_children` and `an_older_command_uses_a_newer_host_process`.

## 2. The guard

- [ ] 2.1 Add the guard to the test support. It stops the test's host processes, fails the test where a process of its folders is left, and ends each such process. Check: `cargo test -p sluice --test host_process a_test_that_leaves_a_run_fails` and `a_clean_test_passes`.
- [ ] 2.2 Find a detached process by the state folder and the scratch folders, and pass over another test's processes. Check: `cargo test -p sluice --test host_process the_guard_finds_a_detached_host_process` and `the_guard_passes_over_another_tests_process`.
- [ ] 2.3 Hold the guard in `OnDisk`, and build one in each test that starts sluice without `OnDisk`. Check: `sluice exec main -- moon run cli:test`.

## 3. The Rust test tasks

- [ ] 3.1 Pin cargo-nextest in the devenv shell, and add the root's nextest profile with `leak-timeout` set to fail. Check: `sluice exec main -- moon run root:toolchain`.
- [ ] 3.2 Run the `test`, `tools` and `sandbox` tasks of the five crates with `cargo nextest run`. Check: a fixture test that leaks `sleep 60` fails `sluice exec main -- moon run cli:test`, and the suite passes without it.
- [ ] 3.3 Change each test that the guard or nextest fails so that it ends its processes before it ends. Check: `sluice exec main -- moon run cli:tools`, then `cli:sandbox`, one at a time.

## 4. The shell test tasks

- [ ] 4.1 Add `tests/leaks.sh`, which runs a test command with its own folders and fails where a process of them is left. Check: `sluice exec main -- moon run root:leaks-cases`, a task of the root that runs the script on a test that leaves a watcher and on one that leaves nothing.
- [ ] 4.2 Run the plugins' shell test tasks and the check-e2e tier through `tests/leaks.sh`. Check: `sluice exec main -- moon run plugin-herdr:test-sh`, then `plugin-sluice:hooks`, one at a time.

## 5. The services

- [ ] 5.1 Set `AbandonProcessGroup` false and `KillMode` `control-group`, run sluice by the profile's `bin/sluice`, and have the activation ask a hand-over in place of a restart. Check: `sluice exec main -- moon run nix:test`.

## 6. Records

- [ ] 6.1 Amend the record of the host process with this change's decisions, each with Before and Now in plain words. Rewrite the rule on a check's process group in the same form. Check: `sluice exec main -- moon run root:changes`.
