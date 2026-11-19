# Tasks

Work in this change's place on its branch, cut from
`bolt/sluice-keeps-a-gates-queues-moving`, the base every check below
runs against. Run a slow test through sluice as written; a quick
`cargo check`, `cargo clippy` or one unit test may run directly. Commit
by pathspec, Conventional Commits, scoped to the part of the tree,
citing clauses by number, rules by id and this design's decisions as
D1 to D14. The gate runs no task that reads Markdown (core.21; record
0127), so a task that changes only Markdown names Vale, a writer's check
run by hand.

## 1. Rules and records

- [x] 1.1 Amend rule core.51's text and details to admit the service that keeps `sluice daemon` running where the host turns it on, and move core.31's details' "a service, unit or list of repositories a host installs or keeps for sluice" under "Not ruled out" for that service alone, each citing record 0093 as amended 2026-11-18 (D14). True when `devenv shell -- vale docs/architecture/core.md` reports nothing; the gate reads no Markdown.
- [x] 1.2 Mark record 0098 amended by record 0093's amendment of 2026-11-18, the module now declaring the daemon's service where the host turns it on, and add to record 0093 an amendment of what this build chose, D1 to D13, accepted as the choices of the change `sluice-runs-as-a-daemon`, each with the path not taken (D14). True when `devenv shell -- vale docs/adr/0093-a-hosts-queues-move-in-sluices-own-host-process-started-when-work-arrives-and-pull-requests-are-queued-on-the-forge.md docs/adr/0098-sluices-flake-gives-a-module-that-writes-the-hosts-own-settings-and-nothing-else.md` reports nothing; the gate reads no Markdown.

## 2. sluice daemon is the host process kept running

- [x] 2.1 Add `sluice daemon`, named in `sluice --help`, its help saying it keeps sluice's host process running on a machine that chooses to and that nothing needs it, dispatched in `crates/sluice/src/main.rs` before any repository is opened, as `host` is (D1). True when `cargo test -p sluice --test cli the_help_names_sluice_daemon` passes.
- [x] 2.2 Run the host process in a daemon mode whose loop never takes the idle end, `Named` giving `daemon` in `hello` and `host/version` (D1). True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- cargo test -p sluice --test host_process an_idle_daemon_keeps_running` passes.
- [x] 2.3 Show that a submission to an idle daemon lands with no other host process started. True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- cargo test -p sluice --test host_process a_daemon_lands_a_submission_with_no_other_host_process` passes.
- [x] 2.4 Note the daemon's working folder's repository in the host's index where it has sluice, before it runs as the host process (D8). True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- cargo test -p sluice --test host_process a_daemon_indexes_the_repository_it_starts_in` passes.
- [x] 2.5 Give `daemon` in the overview's `host_process` (D1). True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- cargo test -p sluice --test host_process the_overview_names_a_daemon` passes.
- [x] 2.6 Show that a daemon whose host the branch's signers name lands what another host submits to a branch whose records are on its remote, with no other command run on its host. True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- cargo test -p sluice --features tools --test peers a_daemon_lands_what_another_host_submits` passes.
- [x] 2.7 Have `sluice run`'s one line on stderr say that `sluice daemon` keeps the host process running on a machine that chooses to (D13). True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- cargo test -p sluice --test cli sluice_run_says_nothing_needs_it` passes.
- [x] 2.8 Document `sluice daemon` for any host's operator (core.26): a section of its own in `site/src/content/docs/reference/commands.md` and its "Who runs the rounds", the README's "Nothing to install" bullet and command table, the home page's "No daemon" card in `site/src/content/docs/index.mdx`, and the lines of `plugins/sluice/skills/using-sluice/SKILL.md` and `references/submit.md` that say nothing is kept running, each saying a machine may keep `sluice daemon` running and nothing needs it. True when `devenv shell -- vale site/src/content/docs/reference/commands.md site/src/content/docs/index.mdx README.md plugins/sluice/skills/using-sluice/SKILL.md plugins/sluice/skills/using-sluice/references/submit.md` reports nothing; the gate reads no Markdown.

## 3. A daemon takes the host's place

- [x] 3.1 Where a host process that is no daemon holds the host, whatever its version, have it hand over, wait for it as long as `HAND_OVER_WAIT`, and take its place (D2). True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- cargo test -p sluice --test host_process a_daemon_takes_the_place_of_a_host_process_that_ends_when_idle` passes.
- [x] 3.2 Exit 3 naming the host process's log where the holder has not ended by then (D2). True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- cargo test -p sluice --test host_process a_daemon_whose_host_process_does_not_hand_over_exits_3` passes.
- [x] 3.3 Where another daemon holds the host, ask it nothing, say once on stderr which daemon it waits for, its pid and version, and take the host's place once its lock goes (D2). True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- cargo test -p sluice --test host_process a_second_daemon_waits_and_takes_the_place_of_the_first` passes.

## 4. A daemon hands over in place

- [x] 4.1 Give `hand-over` the asker's executable, `program`, read as absent where an older command sends none, and a host process that is no daemon ending on it as before (D3). True when `cargo test -p sluice --lib a_hand_over_naming_a_program_reads_as_a_hand_over` passes.
- [ ] 4.2 Have a daemon asked to hand over let each task finish its round, let its lock and socket go, and start afresh in place as the named executable running `daemon` with the environment it started with, through `hand_over.rs`'s `replace`, writing `since` to the millisecond (D3, D4). True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- cargo test -p sluice --test host_process a_daemon_asked_to_hand_over_starts_afresh_in_place` passes.
- [ ] 4.3 Have a newer command that reaches an older daemon ask it to hand over naming its own executable, wait within `HAND_OVER_WAIT` for a host process that is not the image it asked, by pid or `since`, and use it as its own start, failing naming the log where the image it asked still answers (D4). True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- cargo test -p sluice --test host_process a_newer_client_has_an_older_daemon_start_afresh` passes.
- [ ] 4.4 Where the start afresh fails, take the lock and socket back, go on passing, and say once in the log the executable and why (D3). True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- cargo test -p sluice --test host_process a_daemon_whose_start_afresh_fails_goes_on` passes.
- [ ] 4.5 Carry `host/start.lock` across the start afresh, its descriptor named in `SLUICE_START_LOCK_FD`, which `main` takes out of the environment first, the image started afresh letting it go once its socket answers (D6). True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- cargo test -p sluice --test host_process a_submit_during_a_daemons_start_afresh_reaches_it` passes.
- [ ] 4.6 Have a daemon take up a settings change no binding takes up in place by starting afresh in place as the executable it started as (D5). True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- cargo test -p sluice --features tools --test instance a_daemon_takes_up_a_changed_telemetry_table_in_place` passes.
- [ ] 4.7 Say in "Who runs the rounds" of `site/src/content/docs/reference/commands.md`, and where `site/src/content/docs/reference/configuration.md` says a change of `[telemetry]` goes to a fresh host process, that a daemon starts afresh in its own process and keeps running. True when `devenv shell -- vale site/src/content/docs/reference/commands.md site/src/content/docs/reference/configuration.md` reports nothing; the gate reads no Markdown.

## 5. A daemon stops when told

- [ ] 5.1 End a daemon signalled with `SIGTERM` or `SIGINT` as a hand-over ends it, each task finishing its round, its lock and socket let go, exit 0, its runs going on (D7). True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- cargo test -p sluice --test host_process a_stopped_daemon_leaves_its_runs_going` passes.

## 6. The modules keep the daemon running where the host turns it on

- [ ] 6.1 Declare `services.sluice.daemon.enable`, false, in `nix/modules/options.nix`, its service declared only with `services.sluice.enable` (D12), with the cases "The daemon is off unless turned on" in `nix/tests/darwin.nix` and `nix/tests/nixos.nix`. True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- moon run nix:test` passes.
- [ ] 6.2 Declare `launchd.user.agents.sluice` in `nix/modules/darwin.nix` as D9 and D11 have it, and say so in its header, with the cases "The daemon on nix-darwin", "The daemon with no package" and "Only the daemon's service" in `nix/tests/darwin.nix`. True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- moon run nix:test` passes.
- [ ] 6.3 Add `keep <user> <program> <argument>...` to `nix/modules/activate.sh` for `sluice.service`, copied from boomdrivedev/boomdrive's, with the cases "keep: not running", "keep: another command line", "keep: the unit's command line" and "keep: another user" in `nix/tests/activate.sh`, `systemctl` and `ps` stood in for on its `PATH` (D10). True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- moon run nix:test` passes.
- [ ] 6.4 Declare `systemd.user.services.sluice` and the user's `linger` in `nix/modules/nixos.nix`, and run `keep` in the `sluice` user activation after the link where the daemon is on, as D10 and D11 have it, and say so in its header, with the case "The daemon on NixOS" in `nix/tests/nixos.nix`. True when `sluice exec bolt/sluice-keeps-a-gates-queues-moving -- moon run nix:test` passes.
- [ ] 6.5 Document the option where `site/src/content/docs/reference/configuration.md` describes the module, with the state folder's `XDG_STATE_HOME` given through the service's own options (D12), and in `site/src/content/docs/guides/executors-and-systems.md` for a box kept running for the checks, and in the README's module paragraph. True when `devenv shell -- vale site/src/content/docs/reference/configuration.md site/src/content/docs/guides/executors-and-systems.md README.md` reports nothing; the gate reads no Markdown.
