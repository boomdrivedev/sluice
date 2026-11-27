# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. The tests read processes alike on Linux and macOS

- [x] 1.1 Give the release of `a_worktrees_release_runs_in_sluice_host` as a list, in `crates/sluice/tests/exec.rs`. Check on Linux, where it failed before: `cargo test -p sluice --features tools --test exec a_worktrees_release_runs_in_sluice_host`. Check on macOS: `sluice exec main -- cargo nextest run -p sluice --features tools --test exec -E 'test(a_worktrees_release_runs_in_sluice_host)'`.
- [x] 1.2 Read the children of a process with `ps -ww` and `pgrep -alf`, in `children_of` of `plugins/herdr/tests/run.sh`. Check on Linux with `COLUMNS=80`, where the two watch tests failed before: `sh tests/dash.sh`. Check on macOS: `sluice exec main -- moon run plugin-herdr:test-sh plugin-herdr:test-dash`.
- [x] 1.3 Name each process that the guard found with `pgrep -alf`, in `tests/leaks.sh`. Check on Linux, where it failed before: `sh tests/leaks-cases.sh`. Check on macOS: `sluice exec main -- moon run root:leaks-cases`.

## 2. The change

- [x] 2.1 Run `openspec validate tests-read-processes-alike-on-linux-and-macos --strict`. Check: `sluice exec main -- moon run root:specs root:changes`.
- [x] 2.2 Archive the change with `openspec archive tests-read-processes-alike-on-linux-and-macos`.
