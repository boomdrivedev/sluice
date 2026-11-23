# Tasks

Each check runs through sluice, one suite at a time: `sluice exec main -- cargo nextest run -p <crate> [--features tools] --test <suite> <test>`.

- A test that says a command "ran no git but the upkeep of the hook" runs the command with a `git` on its `PATH` that records each call and then runs git. It asserts that each call that it recorded is one that `protect.rs:git` makes for the upkeep of the branch protection's hook. `sluice host` starts with the real git.
- A test that says a command "read no forge" runs it with `SLUICE_FORGE_API` naming an address that refuses each call, after `sluice host` started with the test's forge.
- A test that starts `sluice host` gives it a state folder of its own under the test's folders, so the leak guard finds and stops it.

## 1. The protocol

- [x] 1.1 Add the requests `submit`, `amend`, `withdraw`, `confirm`, `release` of a checkout or a preview, `branch`, `protect` and `previews`, each with its folder, its command's arguments and `caller`. Check: `cargo nextest run -p sluice --lib host_process::protocol::tests`, with a test that each new request reads back.
- [x] 1.2 Have `step` and `pass` name a folder, and answer a request that no command of this release sends with the error that names both versions (design D5). Check: `cargo nextest run -p sluice --test host_process an_older_step_gets_the_host_processs_version` and `a_step_from_a_folder_runs_no_git`.
- [x] 1.3 Add `TRACEPARENT` to the variables that `caller` carries, and have `caller::var` answer it in a request's call (design D1). Check: `cargo nextest run -p sluice --test host_process a_submit_names_its_agent` and `cargo nextest run -p sluice --test telemetry a_submits_trace_names_its_callers_span`.

## 2. sluice host makes each change

- [x] 2.1 Answer `amend`, `withdraw`, `confirm`, `release` of a checkout or a preview, `branch` and `protect` with one `spawn_blocking` call each. The call binds the repository as the host holds it and renders the command's output, and `sluice host` then tells the repository's scheduler of the change (design D1). Send each from `main.rs` before the command binds a repository. Check:
  - `cargo nextest run -p sluice --test host_process a_withdrawal_runs_no_git`, `an_amend_and_a_confirmation_run_no_git` and `a_change_with_no_host_process_that_answers_exits_3`
  - `cargo nextest run -p sluice --features tools --test remote_branch a_branch_made_on_its_remote_runs_no_git`
  - `cargo nextest run -p sluice --features tools --test protect sluice_protect_is_made_by_sluice_host`.
- [x] 2.2 Write a submit's submission in `sluice host`, from each worktree's folder that `submit` names, and answer with its number. A waiting submit then sends `wait`. Check: `cargo nextest run -p sluice --test host_process a_submit_runs_no_git_and_names_the_worktrees_head`, `a_submit_with_no_host_process_running_lands` and `ten_submits_at_once_start_one_host_process`.
- [x] 2.3 Make a pull request's submit in `sluice host`: the push as a git worker, and the request opened or updated, marked ready and taken in (design D2). Check: `cargo nextest run -p sluice --test pull_requests a_submit_to_a_branch_that_takes_pull_requests_reads_no_forge`.
- [x] 2.4 Take each step of a wait on a pull request, of a wait on a branch that takes pull requests, and of a group's wait in `sluice host`, on the subscription of `wait` (design D2). Check: `cargo nextest run -p sluice --test pull_requests a_wait_on_a_request_that_lands_reads_no_forge`, `cargo nextest run -p sluice --test cli a_groups_wait_runs_no_git`, and the suites `pull_requests` and `reader_tick` whole.
- [x] 2.5 Answer `previews` as a read (design D1). Check: `cargo nextest run -p sluice --features tools --test previews_tools the_previews_are_answered_by_sluice_host`.
- [x] 2.6 Remove the binding of the repository from `main.rs` for each command that this change moves. Remove each send of `serve` by those commands, and remove `serve` from the protocol where no command sends it (design D5). Check: `cargo nextest run -p sluice --test cli a_change_command_binds_no_repository`, which runs each change command of this change and each read with the recording `git`.

## 3. A held landing is pushed again at each round

- [x] 3.1 Push a held landing again at each round of its branch, before the round forms anything, as the same landing. Land it where the forge takes it. Where the forge refuses it with other words, write them and log one more `forge_refused` line (design D3). Check: `cargo nextest run -p sluice-conformance --test landing a_landing_the_forge_refused_is_pushed_at_each_round_and_logged_once`, `a_held_landing_lands_once_the_forge_takes_it`, `a_held_landing_refused_with_other_words_logs_them_once`, `a_round_after_a_withdrawal_lands_nothing_withdrawn` and `the_submissions_behind_a_held_landing_wait`.
- [x] 3.2 Take the rounds of a branch whose one piece of work in play is a held landing at the pace of a repository with nothing in play (design D3). Check: `cargo nextest run -p sluice --test reader_tick a_held_landing_is_pushed_once_a_minute`, with a paused clock.
- [x] 3.3 Remove `Command::Push`, its request and its code (`lib.rs:2863`, `lib.rs:7481`). Check: `cargo nextest run -p sluice --test cli push_is_no_command`, and the suite `remote_branch` whole, with the tests of `sluice push` removed.

## 4. The set-aside goes

- [x] 4.1 Remove the command that sets a result aside, `Records::set_aside`, `set-aside.jsonl`, the remote ref `refs/sluice/set-aside`, and each read of them (`sluice-core/src/records.rs`, `ops.rs`, `verdict_view.rs`, `sluice-seams`, `sluice-fakes`, `profiles/local_files.rs`, `profiles/remote_refs.rs`, `profiles/records.rs`, `profiles/github`, `job.rs`, `words.rs`) (design D4). Check: `cargo nextest run -p sluice --test cli set_aside_is_no_command`, `cargo nextest run -p sluice-conformance --test could_not_run an_earlier_releases_set_aside_is_read_by_nothing`, and the suites `remote_refs` and `could_not_run` whole.
- [x] 4.2 Have a flaky failure's report name the result that it contradicts and when it was recorded, say that the same head lands once a real fix changes what the check reads, and say that a fail the host caused is a fault of sluice. It names no step of the operator's (design D4). Check: `cargo nextest run -p sluice --test cli a_flaky_failure_names_no_operators_step` and `a_flaky_reports_way_on_names_no_operators_step`, and `cargo nextest run -p sluice-core --lib verdict_view`.
- [x] 4.3 Remove the guard's refusal of a set-aside from the Claude Code plugin (`plugins/sluice/scripts/guard.sh`), and its case from `plugins/sluice/tests/hooks.sh`. Check: `sluice exec main -- moon run plugin-sluice:test`.

## 5. Records, rules, docs and the gate

- [x] 5.1 Amend records 0124, 0077, 0031, 0043, 0044 and 0093 with this change's decisions, each with Before and Now in plain words. Check: `sluice exec main -- moon run root:changes`.
- [x] 5.2 Rewrite the rules that this change departs from, each with Before and Now. Check: `sluice exec main -- moon run root:changes`.
  - the rule that a landing the forge refuses is held and never pushed again by itself
  - the rule that each check run is recorded before any verdict is read, for its parts on a set-aside
  - the rule that sluice runs no readiness command, for its part on the operator's set-aside
  - the rule that each signed record is of a catalogued kind
- [x] 5.3 Say in the README, the site's pages `reference/commands.md`, `reference/state.md`, `reference/configuration.md` and `architecture/rules.md`, and the using-sluice skill's references `verdicts.md` and `exec.md` that each change command is a client of `sluice host`, that a held landing is pushed again at each round, and that no result is set aside. No gate task reads a page's words, by the rule that no task reads Markdown, so the review of the diff checks it. Check: `sluice exec main -- moon run plugin-sluice:validate`.
- [ ] 5.4 Run the gate. Check: `sluice exec main`, then `devenv shell -- moon run cli:sandbox` outside the gate's sandbox, one suite at a time.
