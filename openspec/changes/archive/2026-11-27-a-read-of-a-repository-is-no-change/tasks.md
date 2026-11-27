# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. A read is no change

- [x] 1.1 Add `reads_only`, and drop its events in the handler of `Stream::new`, in `crates/sluice/src/host_process/watch.rs`, with the unit test `a_read_is_no_change`. Check: `sluice exec main -- cargo nextest run -p sluice --lib -E 'test(a_read_is_no_change)'`.
- [x] 1.2 Check on Linux, where it failed in four runs of five: `cargo nextest run -p sluice --test host_process -E 'test(=the_host_process_keeps_its_repositories_across_a_new_binding)'`, five runs, and the whole `host_process` binary.

## 2. The checks

- [x] 2.1 Run the projects' tasks one at a time: `sluice exec main -- moon run cli:test`, then `root:lint`, `root:format`, `root:specs` and `root:changes`.

## 3. The change

- [x] 3.1 Run `openspec validate a-read-of-a-repository-is-no-change --strict`.
- [x] 3.2 Archive the change with `openspec archive a-read-of-a-repository-is-no-change`.
