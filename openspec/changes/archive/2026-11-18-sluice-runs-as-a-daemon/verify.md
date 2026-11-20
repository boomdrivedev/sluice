# Verify: sluice-runs-as-a-daemon

Verified 2026-11-18 on `change/sluice-runs-as-a-daemon` at `bed116b`,
cut from `bolt/sluice-keeps-a-gates-queues-moving` (`b6c30e7`), with
sluice 0.95.1 and the OpenSpec `devenv.nix` pins (1.14.3).

**Verdict: the work holds.** The gate passes, OpenSpec validates the
change and reads every task done, and every requirement and scenario of
its six specs is met by code and a test that passed.

## The gate

`sluice exec bolt/sluice-keeps-a-gates-queues-moving`, run once: exit 0.

    running the checks bed116b affects against bolt/sluice-keeps-a-gates-queues-moving (b6c30e7)
    passed: 8 tasks
    recorded 8 results under their keys for bed116b

The tasks were `cli:test`, `nix:lint`, `nix:test`, `nix:typecheck`,
`plugin-sluice:validate`, `root:changes`, `root:format` and `root:lint`.
`cli:test` ran 840 tests with no failure, every test this change added
among them. `nix:test` said `nix: test: every case holds`, and it runs
`nix/tests/darwin.nix`, `nixos.nix` and `activate.sh`, where the daemon's
cases are. `root:changes` archives this change into a scratch copy of the
specs and validates what it writes (core.61). The run also printed moon's
`Failed to package task outputs into archive` warning for each task. That
is moon's cache of outputs, and no check read it.

The bolt's gate is `#check-fast`, which leaves out two of the tests the
tasks name: `peers` and `instance` run under `--features tools`, in
`#check-tools`. They ran on their own, as tasks 2.6 and 4.6 name them:
`sluice exec bolt/sluice-keeps-a-gates-queues-moving -- cargo test -p
sluice --features tools --test peers --test instance a_daemon_`, exit 0:

    test bound::a_daemon_takes_up_a_changed_telemetry_table_in_place ... ok
    test a_daemon_lands_what_another_host_submits ... ok

## OpenSpec

- `openspec validate sluice-runs-as-a-daemon --strict --no-interactive`:
  `Change 'sluice-runs-as-a-daemon' is valid`, exit 0.
- `openspec instructions apply --change sluice-runs-as-a-daemon --json`:
  `state` `all_done`, progress 26 of 26 complete, 0 remaining.

## Tasks

| Task | Check | Outcome |
|---|---|---|
| 1.1, 1.2 | core.51 and core.31's details admit the daemon's service; record 0098 marked amended; record 0093's build amendment, D1 to D13 | Done; Vale below |
| 2.1 | `the_help_names_sluice_daemon` | passed (gate) |
| 2.2 | `an_idle_daemon_keeps_running` | passed (gate) |
| 2.3 | `a_daemon_lands_a_submission_with_no_other_host_process` | passed (gate) |
| 2.4 | `a_daemon_indexes_the_repository_it_starts_in` | passed (gate) |
| 2.5 | `the_overview_names_a_daemon` | passed (gate) |
| 2.6 | `a_daemon_lands_what_another_host_submits` | passed (tools run) |
| 2.7 | `sluice_run_says_nothing_needs_it` | passed (gate) |
| 2.8, 4.7, 6.5 | the README, the site's pages and the using-sluice skill name `sluice daemon`, its start afresh and `services.sluice.daemon.enable` | Done; Vale below |
| 3.1 | `a_daemon_takes_the_place_of_a_host_process_that_ends_when_idle` | passed (gate) |
| 3.2 | `a_daemon_whose_host_process_does_not_hand_over_exits_3` | passed (gate) |
| 3.3 | `a_second_daemon_waits_and_takes_the_place_of_the_first` | passed (gate) |
| 4.1 | `a_hand_over_naming_a_program_reads_as_a_hand_over` | passed (gate) |
| 4.2 | `a_daemon_asked_to_hand_over_starts_afresh_in_place` | passed (gate) |
| 4.3 | `a_newer_client_has_an_older_daemon_start_afresh` | passed (gate) |
| 4.4 | `a_daemon_whose_start_afresh_fails_goes_on` | passed (gate) |
| 4.5 | `a_submit_during_a_daemons_start_afresh_reaches_it` | passed (gate) |
| 4.6 | `a_daemon_takes_up_a_changed_telemetry_table_in_place` | passed (tools run) |
| 5.1 | `a_stopped_daemon_leaves_its_runs_going` | passed (gate) |
| 6.1 to 6.4 | `moon run nix:test`, with the cases each task names | passed (gate) |

**Vale.** The Markdown tasks are true when `devenv shell -- vale`
reports nothing on the files they name. Run over all ten files, Vale
reports 5 errors, 124 warnings and 360 suggestions. The same files at the
base report the same alerts: each file's alerts, by severity, rule and
matched text, are equal at `b6c30e7` and at `bed116b`. So the change adds
no alert, and those it leaves were there before it. No task here asks
for the prose it did not touch to be cleared, and the gate reads no
Markdown (core.21; record 0127).

## Requirements and scenarios

Each scenario names its test, and each test asserts what its scenario
says. The code was read beside it.

**host-daemon** (new)
- *sluice daemon is the host process kept running*: `Command::Daemon`
  dispatched in `main.rs` before any repository is opened;
  `host_process::daemon` runs `run_holding` with `daemon` set, and
  `serve` skips the idle end for a daemon (D1). It ends only on a signal,
  a gone state folder or a failed taking of the place. Scenarios "Idle,
  it keeps running" and "A submission to an idle daemon": met.
- *A daemon moves what other machines submit*: "A gate box lands another
  machine's submission": met (`peers`).
- *A daemon started in a repository adds it to the index*: `note_here`
  (D8). "Started in a repository" and "Started in no repository": met.
- *A daemon takes the host's place from a host process that ends when
  idle*: `take_place` and `handed_over` ask the holder `hand-over` under
  the start lock, wait `HAND_OVER_WAIT`, and fail naming the log (D2).
  Both scenarios: met.
- *A daemon waits behind another daemon*: it asks nothing, says once on
  stderr which daemon it waits for, by pid and version, and takes the
  lock only when the start lock is free. "A second daemon": met.
- *A newer sluice's command has a daemon start afresh in place*:
  `Request::HandOver { program }`, `Client::afresh`, which tells the new
  image by pid or `since` to the millisecond (D4), and `start_afresh` by
  `hand_over::replace` (D3). Both scenarios: met.
- *A daemon takes up a settings change by starting afresh in place*:
  `Taken::HandOver` in daemon mode calls `start_afresh` as the executable
  it started as (D5). "A changed collector": met (`instance`).
- *A command during a daemon's start afresh reaches the daemon*: the
  start lock is held across `execve` through `SLUICE_START_LOCK_FD`,
  which `main` takes out of the environment first, and dropped once the
  socket answers (D6). "A submit during the start afresh": met.
- *A daemon whose start afresh fails goes on as it was*: the lock and
  socket are taken back, tasks resume, and one warning names the
  executable and why. A newer command fails naming the log once the wait
  has passed (`Client::afresh`). "An executable that is not there": met.
- *A daemon stops when told*: `stop_on_interrupt` and
  `stop_on_terminate` set the flag, and `serve` ends through `stop_all`
  (D7). "Stopped while a run goes on": met.
- *sluice --help names sluice daemon*: "The help": met.

**daemon-service** (new)
- *Off unless the host turns it on*: `services.sluice.daemon.enable`, a
  `mkEnableOption` inside `mkIf cfg.enable`. Both "Unset" scenarios: met.
- *On nix-darwin a launchd agent of the primary user*:
  `launchd.user.agents.sluice` as D9 has it. "The agent": met.
- *On NixOS a systemd user unit*: `systemd.user.services.sluice` and
  `linger`, as D10 has it. "The unit": met.
- *The user's activation restarts a changed daemon*: `activate.sh keep`,
  run after the link in the `sluice` user activation. All four `keep:`
  scenarios: met.
- *The daemon's service runs the module's sluice*: the package's
  `bin/sluice`, or `sluice` from `PATH` where the package is null. "No
  package": met.

**host-process** (modified)
- *One host process for each user*: `run` still ends a second `sluice
  host` at once. Its scenarios stand, in the existing tests, which passed.
- *The host process ends once nothing is in play*: unchanged for a host
  process that is no daemon. "A daemon's queues empty": met
  (`an_idle_daemon_keeps_running`).
- *A newer sluice's command has an older host process hand over*:
  `Client::reach` hands over a host process that is no daemon as before,
  and calls `afresh` for a daemon. The existing scenarios passed.
- *The overview names the host process*: `room_and_process` gives
  `daemon`. "A daemon": met.

**settings-take-up** (modified)
- *Work in play never waits for a command across the hand-over*: "A
  submit during the hand-over" (`a_submit_during_a_hand_over_reaches_the_successor`)
  and "A submit during a daemon's start afresh": met.

**host-driver** (modified)
- *sluice run says nothing needs it*: `Following::start`'s line names
  `sluice daemon`. "Started by hand": met.

**host-settings-module** (modified)
- *The module installs the package and nothing else*: the darwin and
  NixOS cases "The flake's sluice", "A host's own sluice", "No package",
  "Nothing else on the host" and "Only the daemon's service": met
  (`nix:test`).

## Rules

The diff keeps core.1 (no task runner, service manager or forge named in
the core: `launchd` and `systemd` appear only in `nix/modules/`), core.31
and core.51 as amended here, and core.54's hand-over through the start
lock. core.31's own text still says "nothing is installed for sluice".
Its details now rule the daemon's service in, on a host that turns it on,
which D14 chose as the place for that change.
