# Tasks

Each check runs through sluice, one suite at a time: `sluice exec main -- cargo nextest run -p sluice --features tools --test <suite> <test>`. A test that starts `sluice host` gives it a state folder of its own under the test's folders, so the leak guard finds and stops it. A test of the hook asserts that the hook read no file of the records.

## 1. The protocol

- [x] 1.1 Add the request `reference-transaction`, with the transaction's lines, the folder, `GIT_DIR` and git's command line, answered with `output`. Check: `cargo nextest run -p sluice --lib host_process::protocol::tests`, with a test that the request reads back.

## 2. The git hook asks sluice host

- [x] 2.1 Send the transaction from the hook as `reference-transaction`, and exit with the code that `sluice host` answers (design D1). Check: `cargo nextest run -p sluice --features tools --test protect a_persons_write_is_judged_by_sluice_host` and `a_hook_with_no_sluice_host_running_starts_it`.
- [x] 2.2 Refuse the transaction where `sluice host` does not answer within ten seconds, with words that say so and name `sluice protect --remove` (design D2). Check: `cargo nextest run -p sluice --features tools --test protect a_hook_whose_sluice_host_does_not_answer_refuses`.
- [x] 2.3 Judge the transaction in `sluice host` from the state that it keeps, on the connection's task with no wait for a round or a reading, and write each refused write under the branch's lock (design D1). Check: `cargo nextest run -p sluice --features tools --test protect the_refused_write_is_logged_by_sluice_host` and `a_commit_during_a_long_round_ends_at_once`, and the suite `protect` whole.
- [x] 2.4 Give each git command of `sluice host` `SLUICE_HOST_PROCESS`. Take it out of each check, session command, preview command, release command and equip step. Let a transaction that carries a live value go with no ask (design D3). Check: `cargo nextest run -p sluice --features tools --test protect a_fetch_whose_packing_runs_the_hook_asks_nothing`, `a_check_that_writes_main_is_refused` and `a_name_that_an_ended_sluice_host_gave_is_asked`.
- [x] 2.5 Put back the hook in `sluice host` by its own path, for each repository that it binds for a request, and install it where a submit, a wait or a step names the repository. Remove `protection` from `main.rs` (design D4). Check: `cargo nextest run -p sluice --features tools --test protect a_command_with_no_git_has_sluice_host_put_back_the_hook`, `a_newer_sluices_next_command_puts_back_the_hook`, `a_removed_hook_is_not_put_back_by_a_reading_command` and `a_sluice_on_the_callers_path_is_not_the_one_that_judges`.

## 3. Records, rules, docs and the gate

- [ ] 3.1 Amend records 0110 and 0093 with this change's decisions, each with Before and Now in plain words. Check: `sluice exec main -- moon run root:changes`.
- [ ] 3.2 Rewrite the rules that this change departs from, each with Before and Now. Check: `sluice exec main -- moon run root:changes`.
  - the rule that `sluice host` alone reads a repository's records, for the hook and for a command that puts back the hook
  - the rule on a write that the protection refused, for the hook's own write of it
  - the rule on sluice's one retry policy, for a run of sluice under sluice's own fetch
- [ ] 3.3 Say in the README, the site's page on branch protection and the using-sluice skill that the hook asks `sluice host`, and that it refuses a write where `sluice host` does not answer. No gate task reads a page's words, by the rule that no task reads Markdown, so the review of the diff checks it. Check: `sluice exec main -- moon run plugin-sluice:validate`.
- [ ] 3.4 Run the gate. Check: `sluice exec main`, then `devenv shell -- moon run cli:sandbox` outside the gate's sandbox, one suite at a time.
