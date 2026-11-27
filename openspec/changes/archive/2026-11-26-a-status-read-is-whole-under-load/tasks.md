# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. A read of the records is one version whole

- [x] 1.1 In `crates/sluice/src/profiles/local_files.rs`, read `read` and `read_all_finished` through `whole`, which lists and reads the files again where a write ended during the read (D1). Check: `sluice exec main -- cargo nextest run -p sluice --test local_files a_read_beside_writes_is_one_version_whole`, which fails without the change.

## 2. A read of GitHub goes only into the follow that it claimed

- [x] 2.1 In `read_followed` in `crates/sluice/src/profiles/github/pulls.rs`, write what was read only into the follow whose `read_at` is the read's claim (D2). Check: `sluice exec main -- cargo nextest run -p sluice --lib a_follow_let_go_while_it_is_read_stays_let_go`, which fails without the change.

## 3. The change

- [x] 3.1 Run `openspec validate a-status-read-is-whole-under-load --strict`. Check: `sluice exec main -- moon run root:specs root:changes`.
- [x] 3.2 Archive the change with `openspec archive a-status-read-is-whole-under-load`.
