# Tasks

Each check runs through sluice: `sluice exec main -- <command>`. A test is a test of `crates/sluice/tests/host_process.rs`, which the task `cli:test` runs.

## 1. The host process records the newest sluice that has run

- [x] 1.1 Add `host/newest`, `Newest`, `Paths::newest`, `Paths::newest_run` and `raise_newest` to `crates/sluice/src/host_process/mod.rs`, and raise the record as a host process starts, after `host/version` and before its socket answers (D1). Check: `sluice exec main -- cargo nextest run -p sluice --test host_process a_host_process_records_the_newest_sluice_that_has_run`.

## 2. A sluice older than the record starts no host process

- [x] 2.1 Add `Paths::refused_start`, with the words of D2. Read it in `Client::start` under the start lock, and in `run` and `daemon` before each takes the host's lock, except in an image started afresh in place (D2). Check: the tests `an_older_command_starts_no_host_process`, `an_older_host_or_daemon_starts_no_host_process` and `an_older_daemon_ends_no_newer_host_process`.

## 3. A host process hands over to no older sluice

- [x] 3.1 In `answer`, refuse a hand-over whose `hello` names a sluice older than the record or than the host process, with an error, a line in the log, and nothing changed (D3). Check: the test `a_host_process_hands_over_to_no_older_sluice`.
- [x] 3.2 In `Client::reach`, ask for no hand-over where the command is older than the record. In `Client::hand_over_here`, fail with the words of D2 before asking anything. In `Client::afresh` and `Client::hand_over_here`, fail at once with the words of an error that answers the hand-over (D3). Check: the tests `an_older_hand_over_asks_nothing`, `a_command_older_than_the_newest_uses_the_host_process_as_it_is` and `a_refused_hand_over_fails_at_once`, with `a_newer_client_has_an_older_host_process_hand_over`, `a_hand_over_starts_the_host_process_afresh_as_this_sluice` and `an_upgrade_keeps_its_runs_as_children` still passing.

## 4. The claim and the change

- [x] 4.1 Propose a new version of `a-newer-command-replaces-an-older-host-process` that amends `c8ef83408996`, for the user to accept.
- [x] 4.2 Run `openspec validate an-older-client-starts-no-older-host --strict` and `boom terms check` on the change's files. Check: `sluice exec main -- moon run root:specs root:changes`.
- [x] 4.3 Archive the change with `openspec archive an-older-client-starts-no-older-host`, so `openspec/specs/host-process/spec.md` holds its requirements.
