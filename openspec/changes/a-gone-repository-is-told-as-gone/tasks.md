# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. A gone repository is told as gone

- [x] 1.1 Add `repository_gone` in `crates/sluice/src/profiles/mod.rs`: a git folder with no `HEAD` file or no `objects` folder is gone. Check: `sluice exec main -- cargo test -p sluice --lib profiles::tests`.
- [x] 1.2 The look, the scheduler and the index ask `repository_gone`. A look that finds the repository gone ends it, in `crates/sluice/src/host_process/tasks.rs`. Check: `sluice exec main -- cargo test -p sluice --lib host_process::tasks`.
- [x] 1.3 Add `a_repository_gone_as_a_round_writes_in_it_is_gone` in `crates/sluice/tests/host_process.rs`, which fails without the change. Check: `sluice exec main -- cargo test -p sluice --test host_process gone`.
- [x] 1.4 Run cli:test under load nine times. `a_status_beside_a_slow_look_reads_as_held_and_says_so` failed one time, and its failure now prints the log of `sluice host`. Check: `sluice exec main -- cargo nextest run -p sluice --test-threads 54 -E 'not test(sandbox) and not test(speed)'`.

## 2. The change

- [ ] 2.1 Run `openspec validate a-gone-repository-is-told-as-gone --strict`. Check: `sluice exec main -- moon run root:specs root:changes`.
- [ ] 2.2 Archive the change with `openspec archive a-gone-repository-is-told-as-gone`.
