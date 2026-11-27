# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. A repository that the scan finds gone is said gone

- [x] 1.1 `Tasks::forget` sends the document that says the repository is gone on its channel, through `send_gone`, in `crates/sluice/src/host_process/tasks.rs`. Check on Linux, where it failed in two runs of three: `cargo nextest run -p sluice --test host_process -E 'test(=a_repository_gone_as_a_round_writes_in_it_is_gone)'`, five runs.

## 2. The daemon's test asks for the lock on one descriptor

- [x] 2.1 `keeps_the_hosts_lock` opens the host's lock one time and asks for it on that descriptor, in `crates/sluice/tests/host_process.rs`. Check on Linux, where it failed each time: `cargo nextest run -p sluice --test host_process -E 'test(keeps_the_hosts_lock)'`, five runs.

## 3. The checks

- [x] 3.1 Run the whole `host_process` binary on Linux: `cargo nextest run -p sluice --test host_process -E 'not test(speed)'`.
- [x] 3.2 Run the projects' tasks one at a time: `sluice exec main -- moon run cli:test`, then `root:lint`, `root:format`, `root:specs` and `root:changes`.

## 4. The change

- [x] 4.1 Run `openspec validate the-host-process-tests-pass-on-linux --strict`.
- [x] 4.2 Archive the change with `openspec archive the-host-process-tests-pass-on-linux`.
