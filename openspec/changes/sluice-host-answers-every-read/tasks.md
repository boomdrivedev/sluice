# Tasks

Each check runs through sluice, one suite at a time: `sluice exec main -- cargo nextest run -p sluice --test <suite> <test>`. A test of a read that starts `sluice host` gives it a state folder of its own under the test's folders, so the guard finds and stops it. A test that says a command "ran no git" runs the command with no git on its `PATH`, after `sluice host` started with git.

## 1. The protocol

- [ ] 1.1 Add the requests `status`, `overview`, `home`, `forecast` and `inbox`, each with its folder and its command's arguments, and the answer `output` with `stdout`, `stderr` and `code`. Keep each request and answer of today's protocol as it is. Check: `cargo nextest run -p sluice --lib host_process::protocol::tests`, with a test that today's requests read as before and the new ones read back.
- [ ] 1.2 Add `subscribe`, its `add` and `remove` lines, and the answer `document` with `folder` and `overview`. Check: `cargo nextest run -p sluice --lib host_process::protocol::tests::a_subscription_reads_back`.

## 2. sluice host answers each read

- [ ] 2.1 Resolve a folder in `sluice host` with a git worker, and join the asks of one folder that come while its resolution is in flight. Check: `cargo nextest run -p sluice --test host_process a_status_runs_with_no_git` and `a_status_in_no_repository_says_as_before`.
- [ ] 2.2 Give each repository's scheduler a reading thread beside its worker, with a binding that holds what the host holds and fetches nothing. Answer `status`, `overview`, `home`, `forecast` and `inbox` on it with the command's own rendering. Check: `cargo nextest run -p sluice --test host_process a_status_is_answered_by_sluice_host`, `the_home_view_is_answered_by_sluice_host` and `a_status_during_a_long_round_answers_at_once`.
- [ ] 2.3 Join the asks of one read that come while it is in flight, and start one more read after a command told of a change. Check: `cargo nextest run -p sluice --test host_process ten_statuses_share_one_read`, `twenty_statuses_during_a_reading_make_one_more` and `a_subscription_and_a_status_share_a_reading`.
- [ ] 2.4 Answer a read past its bound from the state that `sluice host` last kept, with when it last read the repository on `stderr`. Check: `cargo nextest run -p sluice --test host_process a_status_beside_a_slow_look_reads_as_held_and_says_so`, changed to assert that the status ran no git.
- [ ] 2.5 Answer another tool's client on the socket. Check: `cargo nextest run -p sluice --test host_process a_client_of_another_tool_reads_a_status_on_the_socket`, whose client speaks the protocol from the test and starts no sluice command.

## 3. The read commands are clients

- [ ] 3.1 Send `status`, `overview` without `--watch`, the home view, `submit --check` and `inbox` to `sluice host` from `main.rs` before the command binds a repository, print the answer, and exit with its code. Remove the one-shot read's own reading of the records (`lib.rs:run`, `overview.rs:run`, `home.rs:run`). Check: `cargo nextest run -p sluice --test host_process a_status_with_no_host_process_starts_it_and_reads_its_first_reading`, `cargo nextest run -p sluice --test peers a_one_shot_overview_beside_a_current_reading_asks_origin_nothing` and `cargo nextest run -p sluice --test cli a_one_shot_overview_beside_a_reader_prints_its_document`, each changed to assert that the command read no record.
- [ ] 3.2 Exit 3 where no `sluice host` answers or it ends before it answers, and name the version of an older `sluice host` that does not know the request. Check: `cargo nextest run -p sluice --test host_process a_read_with_no_host_process_that_answers_exits_3` and `a_newer_read_at_an_older_host_process_exits_3`.
- [ ] 3.3 Form each forecast in `sluice host`. Check: `cargo nextest run -p sluice --test remote_branch a_forecast_with_no_host_process_starts_it_and_fetches_nothing`, `a_forecast_with_no_reader_has_the_host_process_fetch_another_hosts_work`, `a_forecast_whose_reading_lacks_the_branch_forms_after_the_next_reading` and `a_forecast_beside_a_reading_that_could_not_ask_the_remote_says_so`, and `cargo nextest run -p sluice --test cli a_forecast_lacking_a_head_is_a_read_it_could_not_make`, each asserting that the command fetched nothing.
- [ ] 3.4 Remove the read of the results in the command's own process from `profiles/records.rs:results_for`. Check: `cargo nextest run -p sluice --test exec results_with_no_host_process_that_answers_exit_3`, `cargo nextest run -p sluice --test peers a_signed_record_is_verified_once`, and `cargo nextest run -p sluice --test host_process a_status_reads_only_the_results_it_asks_for` and `a_result_recorded_after_the_index_was_read_is_read`.

## 4. Subscriptions

- [ ] 4.1 Serve `subscribe`: the overview of each folder at its addition and at each change in a part other than its time, the newest only, from each repository's `watch` channel. Keep writing the reader's document while anything follows the repository. Check: `cargo nextest run -p sluice --test host_process a_subscription_serves_two_folders`, `a_folder_added_later_shares_the_connection` and `a_removed_folder_gets_no_overview`, and `cargo nextest run -p sluice --lib host_process::tasks::tests::a_slow_subscriber_gets_the_newest_overview`.
- [ ] 4.2 Send the overview that says a folder's repository is gone or has no sluice, and remove the folder. Check: `cargo nextest run -p sluice --test host_process a_gone_repository_leaves_the_subscription`.
- [ ] 4.3 Add `--stdin` to `sluice overview --json --watch`, and make the watch of one folder a subscription. Check: `cargo nextest run -p sluice --test overview_watch the_watch_serves_the_folders_on_its_input` and `a_watch_of_one_folder_prints_as_before`, and the suite `overview_watch` whole.
- [ ] 4.4 Make `sluice wait` and a waiting `sluice submit` follow a subscription, and ask `status` of their number where the document no longer holds it. Check: `cargo nextest run -p sluice --test host_process a_wait_landed_while_followed_reads_no_record` and `a_wait_withdrawn_while_followed_reads_no_record`, and the suite `reader_tick` whole.

## 5. An older command

- [ ] 5.1 Prove that an older command's requests are answered as before. Check: `cargo nextest run -p sluice --test host_process an_older_read_is_answered_as_before` and `an_older_watch_reads_the_readers_document`, whose clients send today's requests from the test.

## 6. Records and the gate

- [ ] 6.1 Amend the record of the host process with this change's decisions, each with Before and Now in plain words. Rewrite in the same form the rule that a watch and a wait follow one reader, and the rule that each read goes through `sluice host`. Check: `sluice exec main -- moon run root:changes`.
- [ ] 6.2 Run the gate. Check: `sluice exec main`, then `devenv shell -- moon run cli:sandbox` outside the gate's sandbox, one suite at a time.
