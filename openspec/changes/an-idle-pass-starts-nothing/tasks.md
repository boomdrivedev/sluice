# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. The hooks folder is found once

- [x] 1.1 In `crates/sluice/src/host_process/protection.rs`, find a folder's hooks folder in `Hooks::found`, one finding at a time, and keep it only where it is found. Check: `sluice exec main -- cargo test -p sluice --lib host_process::protection`.

## 2. A refused worker is said once for each cause

- [x] 2.1 In `crates/sluice/src/host_process/tasks.rs`, keep the words logged in `said`, apart from `refused`, and remove them where a worker binds. Check: `sluice exec main -- cargo test -p sluice --test host_process the_host_processs_look_at_an_idle_repository_starts_nothing`.

## 3. The change

- [x] 3.1 Run `openspec validate an-idle-pass-starts-nothing --strict`. Check: `sluice exec main -- moon run root:specs root:changes`.
- [x] 3.2 Archive the change with `openspec archive an-idle-pass-starts-nothing`.
