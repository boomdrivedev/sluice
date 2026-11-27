# Tasks

Each check runs through sluice: `sluice exec main -- <command>`. A test that starts `sluice host` gives it a state folder of its own under the test's folders, so the leak guard finds and stops it. No test asserts a bound on real time: each waits for a state.

## 1. The setting

- [x] 1.1 Read `[daemon] managed` in the host's settings, a boolean, false where unset, each other key of `[daemon]` refused (design D1, D3). Check: `cargo nextest run -p sluice --lib instance::tests`, with a test that the table reads, and one that refuses a value that is no boolean.
- [x] 1.2 For a command, read `[daemon] managed` alone, from the command's own environment, and pass over every other table. Give false for a file that is not there or that cannot be read (design D3). Check: the same tests, with a test of a file that holds `[runs]` and a key no table knows.

## 2. The client

- [x] 2.1 Where `[daemon] managed` is true and no host process answers, start nothing. Fail at once with words that name the socket, the setting and the log (design D2). Check: `cargo nextest run -p sluice --test host_process a_command_starts_no_host_process_where_the_service_manager_keeps_the_daemon` and `a_command_reaches_a_managed_daemon_that_answers`.
- [x] 2.2 Start `sluice host` through process-wrap's `StdCommandWrap` and `ProcessSession`, and remove the command's own `pre_exec` step (design D4). Check: `cargo nextest run -p sluice --test host_process a_started_host_process_leads_a_session_of_its_own`, and the suite `host_process` whole.

## 3. The modules

- [x] 3.1 Declare `services.sluice.settings.daemon.managed`, and set it to true by default where `services.sluice.enable` and `services.sluice.daemon.enable` are set (design D1). Check: `sluice exec main -- moon run nix:test`, with "The daemon says it is managed" in `nix/tests/darwin.nix` and `nix/tests/nixos.nix`.

## 4. Records, rules, docs and the gate

- [x] 4.1 Amend record 0093 with this change's decisions, each with Before and Now. Add to the details of core.31 and core.51 what they do not rule out. Check: `sluice exec main -- moon run root:changes`.
- [x] 4.2 On the site's page of the host's settings, say what `[daemon] managed` does. On the page of commands, say that a command then starts no host process. No gate task reads a page's words, so the review of the diff checks it.
- [x] 4.3 Run the projects' tasks, then submit. Check: `sluice exec main -- moon run cli:test nix:test root:changes root:specs`.
