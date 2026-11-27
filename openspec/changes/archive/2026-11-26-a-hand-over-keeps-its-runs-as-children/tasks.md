# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. A test names the check by its group

- [x] 1.1 In `crates/sluice/tests/host_process.rs`, find each process with the held check's command line in `matching`, and name the one that leads its process group in `check_of`. Check: `sluice exec main -- cargo test -p sluice --test host_process a_checks_forked_copy_is_not_the_check`.
- [x] 1.2 Run each test that calls `check_of`. Check: `sluice exec main -- cargo nextest run -p sluice --test host_process -E 'test(an_upgrade_keeps_its_runs_as_children)'`.

## 2. The change

- [x] 2.1 Run `openspec validate a-hand-over-keeps-its-runs-as-children --strict`. Check: `sluice exec main -- moon run root:specs root:changes`.
- [x] 2.2 Archive the change with `openspec archive a-hand-over-keeps-its-runs-as-children`.
