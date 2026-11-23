# Tasks

Each check runs through sluice, one suite at a time: `sluice exec main -- cargo nextest run -p sluice [--features tools] --test <suite> <test>`.

- A test of `sluice exec`, `sluice preview`, `sluice equip` or a worktree's `sluice release` runs the command with no toolchain and no task runner on its `PATH`, after `sluice host` started with them.
- The command's `PATH` holds a `git` that records each call and then runs git. The test asserts that each call that it recorded is one that `protect.rs:git` makes for the upkeep of the branch protection's hook.
- A test that starts `sluice host` gives it a state folder of its own under the test's folders, so the leak guard finds and stops it.

## 1. The protocol

- [ ] 1.1 Add the requests `exec`, `preview`, `equip` and `release` of a worktree, each with its folder, its command's arguments, `caller` and `run_env`. Add `TRACEPARENT` to the variables that `caller` carries where it does not carry it. Add the streamed answers `stdout` and `stderr`, which come before `output`. Check: `cargo nextest run -p sluice --lib host_process::protocol::tests`, with a test that each new request and answer reads back.
- [ ] 1.2 Remove `hold` (design D5). Remove each read of `results` and each send of `serve` by the commands that this change moves, and remove `results` and `serve` from the protocol where no command sends them. Check: `cargo nextest run -p sluice --test host_process an_older_hold_gets_the_host_processs_version`.

## 2. Sessions, previews and equips run in sluice host

- [ ] 2.1 Start a run's supervisor only as a worker of `sluice host`, and remove the start in a session of its own (`runs.rs:Runs::start`). Check: `cargo nextest run -p sluice --features tools --test exec a_sessions_checks_are_a_worker_of_sluice_host`, `cargo nextest run -p sluice --features tools --test previews_tools a_previews_command_is_a_worker_of_sluice_host`, and `cargo nextest run -p sluice --test host_process a_combinations_checks_are_a_worker_in_a_group_of_their_own`.
- [ ] 2.2 Answer `exec` in `sluice host`: the plan, the equip, the forecast, the slot from the host's own room in the run bound's order, the supervisor, and the run's log sent as `stdout` or `stderr` lines as it grows (design D1, D3). Print each line in the client as it comes. Check: `cargo nextest run -p sluice --features tools --test exec the_output_reaches_the_client_as_it_comes` and `a_sessions_checks_take_a_slot_of_the_bound`, and the suite `exec` whole.
- [ ] 2.3 Give a run the kept variables of `run_env` in place of the environment of `sluice host` (design D2, `profiles/checks.rs:environment`). Check: `cargo nextest run -p sluice --features tools --test exec a_session_run_takes_its_callers_path`.
- [ ] 2.4 End a session's run, give its slot back and record it stopped when its connection closes, and end a wait for a slot the same way (design D4). Check: `cargo nextest run -p sluice --features tools --test exec ctrl_c_ends_a_sessions_checks` and `a_killed_client_ends_its_checks`, and `cargo nextest run -p sluice --test host_process an_agents_check_stopped_while_it_waits_is_counted_nowhere`.
- [ ] 2.5 End a session's checks with `sluice host`, and have the client say so and exit 3 (design D4). Check: `cargo nextest run -p sluice --features tools --test exec a_stopped_sluice_host_ends_a_sessions_checks`.
- [ ] 2.6 Answer `preview` in `sluice host`, with its command as a worker that stays after the connection closes, until its release (design D1). Check: `cargo nextest run -p sluice --features tools --test previews_tools a_preview_stays_after_its_client_ends`, and the suites `previews` and `previews_tools` whole.
- [ ] 2.7 Answer `equip` in `sluice host`, its steps streamed, with the worktree's top resolved from the folder. Answer `release` of a worktree with its release command as a worker (design D1). Check: `cargo nextest run -p sluice --features tools --test equip_tools sluice_equip_with_no_tools_of_its_own` and `cargo nextest run -p sluice --features tools --test exec a_worktrees_release_runs_in_sluice_host`.
- [ ] 2.8 Remove the binding of the repository from `main.rs` for `exec`, `preview`, `equip` and a worktree's `release`. Check: `cargo nextest run -p sluice --test cli a_sessions_command_binds_no_repository`, which runs each of the four commands with the recording `git`.

## 3. Records, rules, docs and the gate

- [ ] 3.1 Amend record 0093 with this change's decisions, each with Before and Now in plain words. Check: `sluice exec main -- moon run root:changes`.
- [ ] 3.2 Rewrite the rule on a check's process group, with Before and Now: no session's run starts in a session of its own. Check: `sluice exec main -- moon run root:changes`.
- [ ] 3.3 Say in the README, the site's page on the host process and the using-sluice skill that a session's checks, a preview's command and the equip steps run in `sluice host`, and end with their client. No gate task reads a page's words, by the rule that no task reads Markdown, so the review of the diff checks it. Check: `sluice exec main -- moon run plugin-sluice:validate`.
- [ ] 3.4 Run the gate. Check: `sluice exec main`, then `devenv shell -- moon run cli:sandbox` outside the gate's sandbox, one suite at a time.
