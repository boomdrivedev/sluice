# Tasks

## 1. The guard

- [ ] 1.1 Add the guard to the test support. It stops the test's host processes, fails the test where a process of its folders is left, and ends each such process. Check: `cargo test -p sluice --test host_process a_test_that_leaves_a_run_fails` and `a_clean_test_passes`.
- [ ] 1.2 Find a detached process by the state folder and the scratch folders, and pass over another test's processes. Check: `cargo test -p sluice --test host_process the_guard_finds_a_detached_host_process` and `the_guard_passes_over_another_tests_process`.
- [ ] 1.3 Hold the guard in `OnDisk`, and build one in each test that starts sluice without `OnDisk`. Check: `sluice exec main -- moon run cli:test`.

## 2. The Rust test tasks

- [ ] 2.1 Pin cargo-nextest in the devenv shell, and add the root's nextest profile with `leak-timeout` set to fail. Check: `sluice exec main -- moon run root:toolchain`.
- [ ] 2.2 Run the `test`, `tools` and `sandbox` tasks of the five crates with `cargo nextest run`. Check: a fixture test that leaks `sleep 60` fails `sluice exec main -- moon run cli:test`, and the suite passes without it.
- [ ] 2.3 Change each test that the guard or nextest fails so that it ends its processes before it ends. Check: `sluice exec main -- moon run cli:tools`, then `cli:sandbox`, one at a time.

## 3. The shell test tasks

- [ ] 3.1 Add `tests/leaks.sh`, which runs a test command with its own folders and fails where a process of them is left. Check: `sluice exec main -- moon run root:leaks-cases`, a task of the root that runs the script on a test that leaves a watcher and on one that leaves nothing.
- [ ] 3.2 Run the plugins' shell test tasks and the check-e2e tier through `tests/leaks.sh`. Check: `sluice exec main -- moon run plugin-herdr:test-sh`, then `plugin-sluice:hooks`, one at a time.
