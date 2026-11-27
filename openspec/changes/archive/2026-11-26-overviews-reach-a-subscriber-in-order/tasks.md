# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. An older overview is not sent after a newer one

- [x] 1.1 In `crates/sluice/src/host_process/joined.rs`, keep the moment when each call began with its answer. `ask_dated` gives it (D1). Check: `sluice exec main -- cargo test -p sluice --lib host_process::joined`.
- [x] 1.2 In `crates/sluice/src/host_process/reads.rs` and `tasks.rs`, give that moment with each overview. Do not send an overview whose read began before the read of the last one sent (D2). Check: `sluice exec main -- cargo test -p sluice --lib a_document_read_that_ends_late_is_not_sent_after_a_newer_one`, which fails without the change.

## 2. The change

- [x] 2.1 Run `openspec validate overviews-reach-a-subscriber-in-order --strict`. Check: `sluice exec main -- moon run root:specs root:changes`.
- [x] 2.2 Archive the change with `openspec archive overviews-reach-a-subscriber-in-order`.
