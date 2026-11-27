# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. The host's lock goes to the new image

- [x] 1.1 In `crates/sluice/src/host_process/mod.rs`, hand the host's lock to the image started afresh by `SLUICE_LOCK_FD`, and take it in `take_carried`, `run` and `daemon` (D1). Check: `sluice exec main -- cargo nextest run -p sluice --test host_process -E 'test(keeps_the_hosts_lock)'`, which fails without the change.

## 2. A kept read of a request stands while its entry stands

- [x] 2.1 In `crates/sluice/src/profiles/github/pulls.rs`, keep the entry beside each read in `follow` and `holds`, and read again where it moved (D2). Check: `sluice exec main -- cargo nextest run -p sluice --features tools --test pull_requests -E 'test(a_wait_exits_withdrawn_when_the_request_carrying_its_work_closes)'`, which fails without the change.

## 3. Reports before documents

- [x] 3.1 In `crates/sluice/src/host_process/subscription.rs`, bias the loop's `select!`, reports before documents (D3). Check: `sluice exec main -- cargo nextest run -p sluice --lib -E 'test(a_report_goes_before_the_document_read_after_it)'`, which fails without the change.

## 4. One write of a hooks folder at a time

- [x] 4.1 In `crates/sluice/src/protect.rs`, take one lock of the process in `install_at`, `reinstall_at` and `remove` (D4). Check: `sluice exec main -- cargo nextest run -p sluice --lib -E 'test(installs_at_once_each_end_and_keep_the_hook_there)'`, which fails without the change.

## 5. Tests that read what they mean

- [x] 5.1 `sluice_run_with_nothing_queued_starts_git_at_its_first_pass_alone` waits for the watch's first document, and says every git start and the host's log where it fails (D5). Check: `sluice exec main -- cargo nextest run -p sluice --test reader_tick`.
- [x] 5.2 `an_upgrade_keeps_its_runs_as_children` reads the host's log at the hand-over (D5). Check: `sluice exec main -- cargo nextest run -p sluice --test host_process -E 'test(an_upgrade_keeps_its_runs_as_children)'`.
- [x] 5.3 The `states` helper of `crates/sluice/tests/instance.rs` fails with the status's output (D5). Check: `sluice exec main -- cargo nextest run -p sluice --features tools --test instance`.

- [x] 5.4 `a_waiting_submit_by_name_in_a_bare_clone_reports_another_queues_landing` runs a round before it reads the record (D5). Check: `sluice exec main -- cargo nextest run -p sluice --features tools --test pull_requests -E 'test(a_waiting_submit_by_name_in_a_bare_clone)'`.

## 6. The change

- [x] 6.1 Run `openspec validate its-tests-hold-under-load --strict`. Check: `sluice exec main -- moon run root:specs root:changes`.
- [x] 6.2 Archive the change with `openspec archive its-tests-hold-under-load`.
