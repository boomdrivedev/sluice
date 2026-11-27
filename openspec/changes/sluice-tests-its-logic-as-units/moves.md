# Moves

Each test of `crates/sluice/tests/` that starts a process before this change, where it goes (design D1), and the test that proves its behaviour after its step lands. A row whose last column says "planned" belongs to a step that has not landed. A moved test keeps its name.


## `checks.rs` (60)

| Test | Goes to | Proven after by |
|---|---|---|
| `a_command_passes_or_fails_by_its_exit` | **End-to-end (D2)** | itself |
| `a_finished_command_run_is_timed_by_its_start_and_end` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_run_whose_folder_is_gone_reads_as_lost` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_command_over_its_limit_times_out` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `env_probe` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_command_sees_its_place_and_nothing_of_sluices` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `hosts_cache_env_probe` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `every_run_gets_the_hosts_cache_environment` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_cold_command_gets_no_cache` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_cache_holding_the_checkout_is_refused` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_command_runs_with_the_repositorys_toolchain` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `the_open_executor_refuses_what_it_cannot_hold` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `the_bound_is_the_instances` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_second_repositorys_start_waits_for_the_hosts_bound` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `an_unknown_runner_is_refused` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `the_sandbox_holds_writes_and_the_network` | **End-to-end (D2)** | itself |
| `a_sandbox_run_records_the_grant_of_its_profile` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `the_sandbox_reports_what_it_refused_a_run` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_sandbox_run_that_writes_nothing_outside_reports_no_refusal` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `two_sandbox_runs_at_once_report_only_their_own_refusals` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_sandbox_run_whose_log_cannot_be_read_reports_it_unread` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `the_sandbox_opens_the_engines_tcp_address` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `the_sandbox_lets_a_command_open_a_pseudo_terminal` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `the_sandbox_opens_the_hosts_caches` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `an_engine_that_answers_meets_a_run_that_asks_for_one` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_host_whose_engine_does_not_answer_does_not_meet_it` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_run_that_asks_for_no_engine_is_met_with_none` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_run_whose_system_was_switched_says_from_what_to_what` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_run_whose_system_stood_says_nothing` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_host_with_no_system_link_reads_nothing` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_link_gone_after_the_run_reads_no_switch` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `an_engine_at_a_tcp_address_is_looked_for_there` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `an_engine_over_tls_is_found_with_the_clients_certificate` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `docker_host_names_a_unix_socket_or_a_tcp_address` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_limit_the_executor_cannot_hold_leaves_a_run_unmet_before_any_socket` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `cannot_start_probe` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_check_that_cannot_start_records_no_fail` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `an_exit_the_policy_names_records_no_fail` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_failing_ready_before_the_run_runs_the_tests` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_failing_ready_once_a_check_failed_leaves_the_fail` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_ready_command_is_never_run` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_toolchain_that_fails_could_not_start_the_tests` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `equip_runs_the_sequence_in_order` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `equip_installs_only_what_is_pinned` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `equip_under_the_command_runner_runs_the_toolchain_alone` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `equip_stops_at_the_step_that_failed` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_toolchain_that_fails_runs_nothing_after_it` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `equip_keeps_nothing` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `equip_env_probe` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `equip_steps_get_a_runs_environment` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_run_that_runs_a_check_is_set_up` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_run_that_runs_nothing_is_never_set_up` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_run_with_a_command_is_equipped_by_its_branchs_runner` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `the_submitters_read_is_never_set_up` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_run_whose_working_copy_could_not_be_equipped_could_not_start` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `the_places_containers_are_removed_and_no_other` | **End-to-end (D2)** | itself |
| `an_engine_that_keeps_its_connections_open_is_read_to_each_answers_end` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_removal_the_engine_refuses_is_named_and_the_rest_go_on` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_host_with_no_engine_has_no_container_to_end` | A unit of `profiles/checks.rs` (D1.2) | planned |
| `a_tcp_engine_is_asked_as_a_socket_is` | A unit of `profiles/checks.rs` (D1.2) | planned |

## `cli.rs` (5)

| Test | Goes to | Proven after by |
|---|---|---|
| `set_aside_is_no_command` | In-process: a unit of the CLI over the fakes, its git counted by `World` (D1.3) | planned |
| `a_sessions_command_binds_no_repository` | In-process: a unit of the CLI over the fakes, its git counted by `World` (D1.3) | planned |
| `a_groups_wait_runs_no_git` | In-process: a unit of the CLI over the fakes, its git counted by `World` (D1.3) | planned |
| `a_change_command_binds_no_repository` | In-process: a unit of the CLI over the fakes, its git counted by `World` (D1.3) | planned |
| `push_is_no_command` | In-process: a unit of the CLI over the fakes, its git counted by `World` (D1.3) | planned |

## `equip_tools.rs` (8)

| Test | Goes to | Proven after by |
|---|---|---|
| `exec_equips_before_its_command` | **End-to-end (D2)** | itself |
| `a_preview_is_equipped_before_its_command` | In-process over the fakes (D1.3) | planned |
| `equip_installs_the_pins_before_moon_runs` | In-process over the fakes (D1.3) | planned |
| `equip_adopts_nothing` | In-process over the fakes (D1.3) | planned |
| `equip_runs_at_the_top_of_the_worktree` | In-process over the fakes (D1.3) | planned |
| `equip_refuses_a_revision_it_cannot_read` | In-process over the fakes (D1.3) | planned |
| `equip_refuses_a_folder_in_no_worktree` | In-process over the fakes (D1.3) | planned |
| `sluice_equip_with_no_tools_of_its_own` | In-process over the fakes (D1.3) | planned |

## `exec.rs` (43)

| Test | Goes to | Proven after by |
|---|---|---|
| `a_worktrees_checks_leave_out_what_its_branch_landed_since` | In-process over the fakes (D1.3) | planned |
| `a_worktrees_unstaged_edit_runs_the_checks_it_affects` | In-process over the fakes (D1.3) | planned |
| `a_worktrees_untracked_file_runs_the_checks_it_affects` | In-process over the fakes (D1.3) | planned |
| `a_worktree_at_its_branchs_revision_runs_no_check` | In-process over the fakes (D1.3) | planned |
| `a_worktrees_own_moon_yml_runs_its_project` | In-process over the fakes (D1.3) | planned |
| `a_worktrees_toolchain_runs_every_check` | In-process over the fakes (D1.3) | planned |
| `exec_opens_with_where_the_worktree_left_its_branch` | In-process over the fakes (D1.3) | planned |
| `a_second_run_on_the_same_files_replays_and_records_nothing` | In-process over the fakes (D1.3) | planned |
| `after_a_fix_exec_runs_the_fixed_check_and_replays_the_rest` | In-process over the fakes (D1.3) | planned |
| `a_pass_sluice_never_recorded_runs_again_and_is_recorded` | In-process over the fakes (D1.3) | planned |
| `exec_json_gives_since_and_each_tasks_replayed` | In-process over the fakes (D1.3) | planned |
| `a_command_gets_the_worktrees_environment_and_its_exit` | **End-to-end (D2)** | itself |
| `a_command_runs_under_the_raised_limit_on_open_files` | In-process over the fakes (D1.3) | planned |
| `sluice_exec_gets_the_hosts_cache_environment` | In-process over the fakes (D1.3) | planned |
| `an_exec_hands_its_checks_its_own_span_in_the_caller_s_trace` | In-process over the fakes (D1.3) | planned |
| `exec_tells_a_conflict_before_it_runs_and_runs_anyway` | In-process over the fakes (D1.3) | planned |
| `exec_runs_from_a_worktree_at_a_long_path` | In-process over the fakes (D1.3) | planned |
| `exec_under_the_sandbox_binds_a_socket_in_a_folder_under_its_scratch` | In-process over the fakes (D1.3) | planned |
| `a_landing_under_the_sandbox_binds_a_socket_in_a_folder_under_its_scratch` | In-process over the fakes (D1.3) | planned |
| `a_clean_run_of_the_checks_is_recorded_and_a_dirty_one_is_not` | **End-to-end (D2)** | itself |
| `exec_records_the_open_executors_grant` | In-process over the fakes (D1.3) | planned |
| `exec_records_a_fail_the_sandbox_refused_with_its_words_and_its_pass_under_a_wider_grant` | In-process over the fakes (D1.3) | planned |
| `the_next_exec_runs_a_removed_worktrees_stored_release_once` | In-process over the fakes (D1.3) | planned |
| `an_unheld_worktrees_release_runs_its_release_command` | **End-to-end (D2)** | itself |
| `an_unheld_release_with_nothing_to_free_makes_nothing` | In-process over the fakes (D1.3) | planned |
| `a_worktrees_last_result_says_its_head_outcome_and_whether_it_was_recorded` | In-process over the fakes (D1.3) | planned |
| `an_exec_waits_for_another_repositorys_run_on_the_host` | In-process over the fakes (D1.3) | planned |
| `the_overview_counts_another_repositorys_combination_in_its_room` | In-process over the fakes (D1.3) | planned |
| `exec_refuses_the_tests_of_a_branch_whose_engine_does_not_answer` | In-process over the fakes (D1.3) | planned |
| `a_check_that_fails_after_the_engine_went_is_not_run_and_a_pass_is_recorded` | In-process over the fakes (D1.3) | planned |
| `a_check_that_says_the_engine_is_down_while_it_answers_is_recorded_as_failed` | In-process over the fakes (D1.3) | planned |
| `a_check_that_fails_after_the_hosts_system_was_switched_is_not_run_and_a_pass_is_recorded` | In-process over the fakes (D1.3) | planned |
| `a_check_that_says_the_system_was_switched_while_it_stood_is_recorded_as_failed` | In-process over the fakes (D1.3) | planned |
| `exec_looks_for_the_engine_at_a_tcp_address` | In-process over the fakes (D1.3) | planned |
| `a_command_whose_host_process_ends_before_it_answers_exits_3` | In-process over the fakes (D1.3) | planned |
| `a_sessions_checks_are_a_worker_of_sluice_host` | In-process over the fakes (D1.3) | planned |
| `the_output_reaches_the_client_as_it_comes` | In-process over the fakes (D1.3) | planned |
| `a_sessions_checks_take_a_slot_of_the_bound` | In-process over the fakes (D1.3) | planned |
| `a_session_run_takes_its_callers_path` | In-process over the fakes (D1.3) | planned |
| `ctrl_c_ends_a_sessions_checks` | In-process over the fakes (D1.3) | planned |
| `a_killed_client_ends_its_checks` | In-process over the fakes (D1.3) | planned |
| `a_stopped_sluice_host_ends_a_sessions_checks` | In-process over the fakes (D1.3) | planned |
| `a_worktrees_release_runs_in_sluice_host` | In-process over the fakes (D1.3) | planned |

## `host_process.rs` (120)

| Test | Goes to | Proven after by |
|---|---|---|
| `a_second_host_process_ends_at_once` | In-process host process handler on a paused clock (D4) | planned |
| `a_socket_a_killed_host_process_left_is_replaced` | In-process host process handler on a paused clock (D4) | planned |
| `ten_reaches_at_once_start_one_host_process` | **End-to-end (D2)** | itself |
| `a_host_process_that_cannot_start_names_its_log` | In-process host process handler on a paused clock (D4) | planned |
| `a_command_starts_no_host_process_where_the_service_manager_keeps_the_daemon` | **End-to-end (D2)** | itself |
| `a_command_reaches_a_managed_daemon_that_answers` | In-process host process handler on a paused clock (D4) | planned |
| `a_started_host_process_leads_a_session_of_its_own` | In-process host process handler on a paused clock (D4) | planned |
| `a_served_repository_is_moved_by_the_host_process` | In-process host process handler on a paused clock (D4) | planned |
| `a_refused_policy_in_one_repository_holds_no_others_round` | In-process host process handler on a paused clock (D4) | planned |
| `the_host_process_ends_once_its_queues_are_empty` | **End-to-end (D2)** | itself |
| `the_host_processs_look_at_an_idle_repository_starts_nothing` | In-process host process handler on a paused clock (D4) | planned |
| `an_idle_repositorys_remote_is_asked_once_a_minute` | In-process host process handler on a paused clock (D4) | planned |
| `the_host_process_ends_with_its_state_folder` | In-process host process handler on a paused clock (D4) | planned |
| `a_state_folder_made_again_ends_the_host_process` | In-process host process handler on a paused clock (D4) | planned |
| `a_reach_while_it_ends_starts_another` | In-process host process handler on a paused clock (D4) | planned |
| `a_newer_client_has_an_older_host_process_hand_over` | **End-to-end (D2)** | itself |
| `a_host_process_starting_afresh_in_place_keeps_the_hosts_lock` | In-process host process handler on a paused clock (D4) | planned |
| `a_daemon_starting_afresh_in_place_keeps_the_hosts_lock` | In-process host process handler on a paused clock (D4) | planned |
| `a_hand_over_longer_than_a_start_is_waited_out` | In-process host process handler on a paused clock (D4) | planned |
| `an_older_command_uses_a_newer_host_process` | In-process host process handler on a paused clock (D4) | planned |
| `a_submit_with_no_host_process_running_lands` | In-process host process handler on a paused clock (D4) | planned |
| `ten_submits_at_once_start_one_host_process` | In-process host process handler on a paused clock (D4) | planned |
| `a_withdrawal_starts_the_host_process` | **End-to-end (D2)** | itself |
| `two_repositories_one_host_process` | In-process host process handler on a paused clock (D4) | planned |
| `started_from_another_repository_lands_both` | In-process host process handler on a paused clock (D4) | planned |
| `a_run_holds_nothing_of_the_command_that_started_the_host_process` | In-process host process handler on a paused clock (D4) | planned |
| `a_wait_follows_the_host_process_and_runs_no_round` | In-process host process handler on a paused clock (D4) | planned |
| `a_waits_first_pass_beside_a_live_host_process_is_the_one_it_asked_for` | In-process host process handler on a paused clock (D4) | planned |
| `a_wait_starts_a_killed_host_process_again` | In-process host process handler on a paused clock (D4) | planned |
| `a_wait_says_a_rounds_failure_once_and_goes_on` | In-process host process handler on a paused clock (D4) | planned |
| `a_step_is_the_host_processs_round` | **End-to-end (D2)** | itself |
| `a_profile_beside_the_host_process_holds_no_part` | In-process host process handler on a paused clock (D4) | planned |
| `s75_a_combination_starts_before_a_third_agents_check` | In-process host process handler on a paused clock (D4) | planned |
| `an_agents_check_stopped_while_it_waits_is_counted_nowhere` | In-process host process handler on a paused clock (D4) | planned |
| `a_killed_host_process_loses_nothing` | **End-to-end (D2)** | itself |
| `the_overview_names_the_host_process` | **End-to-end (D2)** | itself |
| `a_status_with_no_host_process_starts_it_and_reads_its_first_reading` | In-process host process handler on a paused clock (D4) | planned |
| `a_status_beside_a_slow_look_reads_as_held_and_says_so` | In-process host process handler on a paused clock (D4) | planned |
| `a_status_during_a_long_round_answers_at_once` | In-process host process handler on a paused clock (D4) | planned |
| `a_status_reads_only_the_results_it_asks_for` | In-process host process handler on a paused clock (D4) | planned |
| `the_host_process_keeps_its_repositories_across_a_new_binding` | In-process host process handler on a paused clock (D4) | planned |
| `a_submit_during_a_hand_over_reaches_the_successor` | In-process host process handler on a paused clock (D4) | planned |
| `a_successor_that_does_not_start_leaves_the_host_process_as_it_was` | In-process host process handler on a paused clock (D4) | planned |
| `an_idle_daemon_keeps_running` | In-process host process handler on a paused clock (D4) | planned |
| `a_daemon_lands_a_submission_with_no_other_host_process` | **End-to-end (D2)** | itself |
| `a_daemon_indexes_the_repository_it_starts_in` | In-process host process handler on a paused clock (D4) | planned |
| `the_overview_names_a_daemon` | In-process host process handler on a paused clock (D4) | planned |
| `a_daemon_takes_the_place_of_a_host_process_that_ends_when_idle` | In-process host process handler on a paused clock (D4) | planned |
| `a_daemon_whose_host_process_does_not_hand_over_exits_3` | In-process host process handler on a paused clock (D4) | planned |
| `a_second_daemon_waits_and_takes_the_place_of_the_first` | In-process host process handler on a paused clock (D4) | planned |
| `a_daemon_asked_to_hand_over_starts_afresh_in_place` | In-process host process handler on a paused clock (D4) | planned |
| `a_newer_client_has_an_older_daemon_start_afresh` | In-process host process handler on a paused clock (D4) | planned |
| `a_daemon_whose_start_afresh_fails_goes_on` | In-process host process handler on a paused clock (D4) | planned |
| `a_submit_during_a_daemons_start_afresh_reaches_it` | In-process host process handler on a paused clock (D4) | planned |
| `a_stopped_daemon_ends_its_runs` | In-process host process handler on a paused clock (D4) | planned |
| `a_checks_forked_copy_is_not_the_check` | In-process host process handler on a paused clock (D4) | planned |
| `a_combinations_checks_are_a_worker_in_a_group_of_their_own` | In-process host process handler on a paused clock (D4) | planned |
| `a_stopped_host_process_ends_its_runs` | **End-to-end (D2)** | itself |
| `a_killed_host_processs_runs_end_at_the_next_start` | In-process host process handler on a paused clock (D4) | planned |
| `an_upgrade_keeps_its_runs_as_children` | In-process host process handler on a paused clock (D4) | planned |
| `a_hand_over_starts_the_host_process_afresh_as_this_sluice` | In-process host process handler on a paused clock (D4) | planned |
| `a_stop_after_a_hand_over_ends_the_run` | In-process host process handler on a paused clock (D4) | planned |
| `a_test_that_leaves_a_run_fails` | **End-to-end (D2)** | itself |
| `a_clean_test_passes` | In-process host process handler on a paused clock (D4) | planned |
| `the_guard_finds_a_detached_host_process` | **End-to-end (D2)** | itself |
| `the_guard_passes_over_another_tests_process` | In-process host process handler on a paused clock (D4) | planned |
| `the_guard_ends_a_process_that_ignores_sigterm` | In-process host process handler on a paused clock (D4) | planned |
| `a_host_process_keeps_no_descriptor_its_callers_caller_left_open` | In-process host process handler on a paused clock (D4) | planned |
| `the_guard_passes_over_a_process_that_ends_by_itself` | In-process host process handler on a paused clock (D4) | planned |
| `an_idle_host_process_wakes_only_for_its_timers` | In-process host process handler on a paused clock (D4) | planned |
| `the_backstop_falls_due_at_the_minute_on_a_paused_clock` | In-process host process handler on a paused clock (D4) | planned |
| `three_idle_repositories_are_read_once_a_minute` | In-process host process handler on a paused clock (D4) | planned |
| `five_serves_during_a_reading_make_one_more` | In-process host process handler on a paused clock (D4) | planned |
| `a_burst_of_ref_moves_makes_one_reading` | In-process host process handler on a paused clock (D4) | planned |
| `a_connection_is_answered_with_no_poll_at_speed` | In-process host process handler on a paused clock (D4) | planned |
| `sigterm_stops_a_host_process_after_its_round` | In-process host process handler on a paused clock (D4) | planned |
| `a_daemon_started_afresh_answers_on_its_socket` | In-process host process handler on a paused clock (D4) | planned |
| `a_records_write_reaches_a_watch_by_its_file_event` | **End-to-end (D2)** | itself |
| `a_records_write_reaches_a_watch_by_its_file_event_at_speed` | In-process host process handler on a paused clock (D4) | planned |
| `a_status_is_answered_by_sluice_host` | In-process host process handler on a paused clock (D4) | planned |
| `a_status_runs_with_no_git` | In-process host process handler on a paused clock (D4) | planned |
| `a_status_in_no_repository_says_as_before` | In-process host process handler on a paused clock (D4) | planned |
| `the_home_view_is_answered_by_sluice_host` | In-process host process handler on a paused clock (D4) | planned |
| `a_status_during_a_long_step_answers_at_once` | In-process host process handler on a paused clock (D4) | planned |
| `a_reads_git_is_a_worker_of_sluice_host` | In-process host process handler on a paused clock (D4) | planned |
| `a_read_takes_no_slot_of_runs_max` | In-process host process handler on a paused clock (D4) | planned |
| `ten_statuses_share_one_read` | In-process host process handler on a paused clock (D4) | planned |
| `twenty_statuses_during_a_reading_make_one_more` | In-process host process handler on a paused clock (D4) | planned |
| `a_subscription_and_a_status_share_a_reading` | In-process host process handler on a paused clock (D4) | planned |
| `a_client_of_another_tool_reads_a_status_on_the_socket` | In-process host process handler on a paused clock (D4) | planned |
| `a_read_with_no_host_process_that_answers_exits_3` | In-process host process handler on a paused clock (D4) | planned |
| `a_newer_read_at_an_older_host_process_exits_3` | In-process host process handler on a paused clock (D4) | planned |
| `a_result_recorded_after_the_index_was_read_is_read` | In-process host process handler on a paused clock (D4) | planned |
| `a_watchs_folder_is_read_each_second` | In-process host process handler on a paused clock (D4) | planned |
| `a_subscription_serves_two_folders` | In-process host process handler on a paused clock (D4) | planned |
| `a_folder_added_later_shares_the_connection` | In-process host process handler on a paused clock (D4) | planned |
| `a_removed_folder_gets_no_overview` | In-process host process handler on a paused clock (D4) | planned |
| `a_gone_repository_leaves_the_subscription` | In-process host process handler on a paused clock (D4) | planned |
| `a_repository_gone_as_a_round_writes_in_it_is_gone` | In-process host process handler on a paused clock (D4) | planned |
| `sluice_run_prints_the_passes_of_its_subscription` | **End-to-end (D2)** | itself |
| `an_older_hold_gets_the_host_processs_version` | In-process host process handler on a paused clock (D4) | planned |
| `an_older_read_gets_the_host_processs_version` | In-process host process handler on a paused clock (D4) | planned |
| `an_older_follow_gets_the_host_processs_version` | In-process host process handler on a paused clock (D4) | planned |
| `a_wait_landed_while_followed_reads_no_record` | In-process host process handler on a paused clock (D4) | planned |
| `a_wait_withdrawn_while_followed_reads_no_record` | In-process host process handler on a paused clock (D4) | planned |
| `a_withdrawal_runs_no_git` | In-process host process handler on a paused clock (D4) | planned |
| `an_amend_and_a_confirmation_run_no_git` | **End-to-end (D2)** | itself |
| `a_change_with_no_host_process_that_answers_exits_3` | In-process host process handler on a paused clock (D4) | planned |
| `a_submit_runs_no_git_and_names_the_worktrees_head` | In-process host process handler on a paused clock (D4) | planned |
| `a_submit_names_its_agent` | In-process host process handler on a paused clock (D4) | planned |
| `an_older_step_gets_the_host_processs_version` | In-process host process handler on a paused clock (D4) | planned |
| `a_step_from_a_folder_runs_no_git` | In-process host process handler on a paused clock (D4) | planned |
| `a_host_process_records_the_newest_sluice_that_has_run` | In-process host process handler on a paused clock (D4) | planned |
| `an_older_command_starts_no_host_process` | In-process host process handler on a paused clock (D4) | planned |
| `an_older_host_or_daemon_starts_no_host_process` | In-process host process handler on a paused clock (D4) | planned |
| `an_older_daemon_ends_no_newer_host_process` | In-process host process handler on a paused clock (D4) | planned |
| `a_host_process_hands_over_to_no_older_sluice` | In-process host process handler on a paused clock (D4) | planned |
| `an_older_hand_over_asks_nothing` | In-process host process handler on a paused clock (D4) | planned |
| `a_command_older_than_the_newest_uses_the_host_process_as_it_is` | In-process host process handler on a paused clock (D4) | planned |
| `a_refused_hand_over_fails_at_once` | In-process host process handler on a paused clock (D4) | planned |

## `instance.rs` (13)

| Test | Goes to | Proven after by |
|---|---|---|
| `the_reserve_is_none_by_default` | A unit of `instance.rs` (D1.2) | planned |
| `the_reserve_is_read_beside_the_bound` | A unit of `instance.rs` (D1.2) | planned |
| `a_reserve_that_leaves_no_run_is_refused` | A unit of `instance.rs` (D1.2) | planned |
| `guarding_reads_only_the_forge` | A unit of `instance.rs` (D1.2) | planned |
| `discovery_is_read_from_the_forge` | A unit of `instance.rs` (D1.2) | planned |
| `one_run_holds_three_branches_to_its_bound` | A unit of `instance.rs` (D1.2) | planned |
| `three_repositories_share_the_hosts_bound` | A unit of `instance.rs` (D1.2) | planned |
| `the_host_process_takes_up_a_changed_bound_at_its_next_pass` | A unit of `instance.rs` (D1.2) | planned |
| `the_host_process_keeps_its_settings_when_the_new_ones_do_not_bind` | A unit of `instance.rs` (D1.2) | planned |
| `the_host_process_hands_a_changed_telemetry_table_to_a_fresh_one` | A unit of `instance.rs` (D1.2) | planned |
| `a_daemon_takes_up_a_changed_telemetry_table_in_place` | A unit of `instance.rs` (D1.2) | planned |
| `the_host_process_keeps_its_settings_when_a_change_it_hands_over_for_does_not_bind` | A unit of `instance.rs` (D1.2) | planned |
| `the_keepers_cores_set_each_runs_share` | A unit of `instance.rs` (D1.2) | planned |

## `landed.rs` (2)

| Test | Goes to | Proven after by |
|---|---|---|
| `a_release_failing_after_the_landing_leaves_the_submit_landed` | In-process over the fakes (D1.3) | planned |
| `a_waiting_submit_held_past_its_pass_says_what_the_pass_released` | In-process over the fakes (D1.3) | planned |

## `local_files.rs` (19)

| Test | Goes to | Proven after by |
|---|---|---|
| `hold_the_lock_until_killed` | Stays in-process; its child process goes where the contract test covers it | planned |
| `the_lock_is_one_holder_and_dies_with_it` | **End-to-end (D2)** | itself |
| `a_waiting_lock_is_taken_when_the_holder_lets_go` | Stays in-process; its child process goes where the contract test covers it | planned |
| `logs_are_kept_beside_the_records_and_only_the_last_ones` | Stays in-process; its child process goes where the contract test covers it | planned |
| `tasks_are_kept_beside_the_records_and_move_no_version` | Stays in-process; its child process goes where the contract test covers it | planned |
| `two_writes_from_one_version` | Stays in-process; its child process goes where the contract test covers it | planned |
| `a_write_without_the_lock_is_an_error` | Stays in-process; its child process goes where the contract test covers it | planned |
| `a_crash_mid_write_reads_whole_and_finishes_once` | Stays in-process; its child process goes where the contract test covers it | planned |
| `a_branchs_home_is_read_and_written_with_its_records` | Stays in-process; its child process goes where the contract test covers it | planned |
| `the_records_are_the_fake_stores_records` | Stays in-process; its child process goes where the contract test covers it | planned |
| `branch_names_round_trip` | Stays in-process; its child process goes where the contract test covers it | planned |
| `ids_count_across_the_hosts_branches` | Stays in-process; its child process goes where the contract test covers it | planned |
| `results_append_once_and_whole` | Stays in-process; its child process goes where the contract test covers it | planned |
| `a_finished_submission_moves_out_of_the_live_records_all_or_nothing` | Stays in-process; its child process goes where the contract test covers it | planned |
| `a_finished_record_put_again_replaces_it_and_leaves_the_index` | Stays in-process; its child process goes where the contract test covers it | planned |
| `a_second_read_at_one_version_walks_nothing` | Stays in-process; its child process goes where the contract test covers it | planned |
| `a_write_from_another_process_is_seen_at_the_next_read` | Stays in-process; its child process goes where the contract test covers it | planned |
| `a_stopped_writers_pending_write_is_read_whole_over_what_was_kept` | Stays in-process; its child process goes where the contract test covers it | planned |
| `a_read_beside_writes_is_one_version_whole` | Stays in-process; its child process goes where the contract test covers it | planned |

## `local_refs.rs` (20)

| Test | Goes to | Proven after by |
|---|---|---|
| `read_existing_missing_and_packed` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `a_repository_is_named_by_the_origin_it_configures` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `a_branch_in_this_hosts_refs_has_no_copy_apart` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `form_clean_is_gits_merge_with_both_parents` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `form_conflicts_name_every_path` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `form_a_chain_of_three` | **End-to-end (D2)** | itself |
| `form_errors_are_not_conflicts` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `commit_writes_the_landing` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `commit_as_names_its_author` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `a_landing_beside_reads_as_passed_to_git` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `move_to_from_expected_only` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `two_moves_from_one_revision_one_wins` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `a_refusing_hook_does_not_stop_move_to` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `read_file_reads_the_revision_not_the_worktree` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `contains_answers_ancestry` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `a_merge_base_is_where_two_revisions_part` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `contains_answers_no_for_a_commit_this_host_lacks` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `the_common_dir_is_one_from_every_worktree` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `the_default_branch_and_the_branches_read_as_git_names_them` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |
| `a_bare_repositorys_default_branch_is_its_own_heads` | Folded into the contract test `form_a_chain_of_three`, or a unit (D1.2) | planned |

## `overview_watch.rs` (15)

| Test | Goes to | Proven after by |
|---|---|---|
| `a_watch_ends_where_no_branch_holds_the_policy` | In-process over the fakes (D1.3) | planned |
| `a_watch_goes_on_where_the_policy_is_only_on_a_remotes_branch` | In-process over the fakes (D1.3) | planned |
| `a_watch_ends_where_the_policy_is_in_the_working_copy_alone` | In-process over the fakes (D1.3) | planned |
| `a_watch_goes_on_where_the_policy_is_on_a_branch_other_than_heads` | In-process over the fakes (D1.3) | planned |
| `a_readers_marks_move_while_its_document_stands` | In-process over the fakes (D1.3) | planned |
| `a_set_up_repository_with_nothing_submitted_is_watched_with_no_git` | In-process over the fakes (D1.3) | planned |
| `a_watch_ends_once_the_policy_is_removed_while_it_runs` | In-process over the fakes (D1.3) | planned |
| `a_watch_with_nothing_queued_names_held_worktrees_with_no_git` | In-process over the fakes (D1.3) | planned |
| `a_watch_reads_the_policy_again_once_the_branch_moves` | In-process over the fakes (D1.3) | planned |
| `a_followers_overview_carries_refused_writes` | In-process over the fakes (D1.3) | planned |
| `the_watch_serves_the_folders_on_its_input` | In-process over the fakes (D1.3) | planned |
| `the_watch_ends_when_its_input_ends_with_no_folder` | In-process over the fakes (D1.3) | planned |
| `the_watch_goes_on_when_its_input_ends_with_folders` | In-process over the fakes (D1.3) | planned |
| `the_watch_exits_3_when_sluice_host_ends` | In-process over the fakes (D1.3) | planned |
| `a_watch_of_one_folder_prints_as_before` | **End-to-end (D2)** | itself |

## `peers.rs` (14)

| Test | Goes to | Proven after by |
|---|---|---|
| `a_run_is_claimed_and_run_by_the_other_host` | A conformance scenario over the fakes (D1.3) | planned |
| `a_submission_lands_by_another_host` | **End-to-end (D2)** | itself |
| `an_outcome_on_the_remote_is_collected_on_another_host` | A conformance scenario over the fakes (D1.3) | planned |
| `a_host_whose_own_main_is_refused_counts_the_remotes_signers` | A conformance scenario over the fakes (D1.3) | planned |
| `a_host_runs_a_branch_only_where_its_policy_lists_the_hosts_key` | A conformance scenario over the fakes (D1.3) | planned |
| `the_other_host_runs_the_other_branch` | A conformance scenario over the fakes (D1.3) | planned |
| `each_pass_asks_origin_once_for_both_branches` | A conformance scenario over the fakes (D1.3) | planned |
| `each_watch_tick_asks_origin_once_for_both_branches` | A conformance scenario over the fakes (D1.3) | planned |
| `a_refused_ask_shows_both_branches_as_last_fetched` | A conformance scenario over the fakes (D1.3) | planned |
| `a_one_shot_overview_beside_a_current_reading_asks_origin_nothing` | A conformance scenario over the fakes (D1.3) | planned |
| `a_status_beside_a_current_reading_asks_origin_nothing` | A conformance scenario over the fakes (D1.3) | planned |
| `a_signed_record_is_verified_once` | A conformance scenario over the fakes (D1.3) | planned |
| `a_wait_follows_its_submission_to_its_queues_new_home` | A conformance scenario over the fakes (D1.3) | planned |
| `a_daemon_lands_what_another_host_submits` | A conformance scenario over the fakes (D1.3) | planned |

## `previews.rs` (10)

| Test | Goes to | Proven after by |
|---|---|---|
| `a_preview_gets_the_hosts_cache_environment` | In-process over the fakes (D1.3) | planned |
| `the_arguments` | In-process over the fakes (D1.3) | planned |
| `a_preview_returns_its_addresses_once_its_command_starts` | In-process over the fakes (D1.3) | planned |
| `an_unlanded_revision_is_refused` | In-process over the fakes (D1.3) | planned |
| `asking_again_gives_the_same_preview` | In-process over the fakes (D1.3) | planned |
| `a_preview_waits_for_room_and_never_takes_the_reserve` | In-process over the fakes (D1.3) | planned |
| `a_preview_waits_on_a_start_refused_at_the_bound` | In-process over the fakes (D1.3) | planned |
| `a_preview_stopped_while_waiting_is_taken_up_again` | In-process over the fakes (D1.3) | planned |
| `a_failed_preview_is_reported_and_released` | In-process over the fakes (D1.3) | planned |
| `a_preview_is_released_only_when_asked` | In-process over the fakes (D1.3) | planned |

## `previews_tools.rs` (13)

| Test | Goes to | Proven after by |
|---|---|---|
| `a_preview_is_released_after_its_branch_is_gone` | In-process over the fakes (D1.3) | planned |
| `a_removed_checkout_is_freed_by_the_next_listing` | In-process over the fakes (D1.3) | planned |
| `a_failed_command_is_released_and_kept_listed` | In-process over the fakes (D1.3) | planned |
| `a_released_previews_container_ends_with_it` | In-process over the fakes (D1.3) | planned |
| `a_removed_checkouts_container_ends_with_its_preview` | In-process over the fakes (D1.3) | planned |
| `an_accepted_previews_container_ends_with_it` | In-process over the fakes (D1.3) | planned |
| `a_failed_previews_labelled_container_ends_with_it` | In-process over the fakes (D1.3) | planned |
| `a_preview_on_a_host_with_no_engine_is_released_as_before` | In-process over the fakes (D1.3) | planned |
| `a_container_the_engine_will_not_remove_keeps_its_preview` | In-process over the fakes (D1.3) | planned |
| `a_gone_previews_refused_container_is_tried_again_by_the_next_listing` | In-process over the fakes (D1.3) | planned |
| `a_previews_command_is_a_worker_of_sluice_host` | In-process over the fakes (D1.3) | planned |
| `a_preview_stays_after_its_client_ends` | In-process over the fakes (D1.3) | planned |
| `the_previews_are_answered_by_sluice_host` | **End-to-end (D2)** | itself |

## `profiles/conformance.rs` (6)

| Test | Goes to | Proven after by |
|---|---|---|
| `$name` | Goes: `e2e/check.sh` and the conformance crate prove it (D2) | planned |
| `$test` | Goes: `e2e/check.sh` and the conformance crate prove it (D2) | planned |
| `peers_push_only_the_branch_and_refs_sluice` | Goes: `e2e/check.sh` and the conformance crate prove it (D2) | planned |
| `peers_read_the_last_fetched_state_when_the_remote_is_unreachable` | Goes: `e2e/check.sh` and the conformance crate prove it (D2) | planned |
| `peers_a_finished_try_is_released_while_another_run_tests` | Goes: `e2e/check.sh` and the conformance crate prove it (D2) | planned |
| `a_run_is_ended_at_its_limit_after_the_step_that_started_it` | Goes: `e2e/check.sh` and the conformance crate prove it (D2) | planned |

## `profiles/forge.rs` (63)

| Test | Goes to | Proven after by |
|---|---|---|
| `the_apps_login_is_read_once` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `the_bot_under_a_command_is_the_viewers` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `the_apps_author_is_its_bot_kept_on_the_host` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `the_apps_author_under_a_command_is_the_viewers_bot` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `an_author_github_did_not_give_is_kept_nowhere` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_kept_author_stands_while_github_cannot_be_read` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_token_is_bought_once_an_hour` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_commands_token_is_got_once_before_each_end` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_commands_token_with_no_end_is_held_until_refused` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_commands_credentials_say_what_their_token_is` | **End-to-end (D2)** | itself |
| `a_commands_token_for_another_repository_is_refused` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_refused_token_says_why` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `credentials_say_what_their_token_is_and_renew_buys_one` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_token_for_another_repository_is_refused` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `no_installation_withholds_the_credentials_and_says_what_to_do` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_redirect_is_githubs_answer_and_says_what_to_do` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `forge_check_all_set` | **End-to-end (D2)** | itself |
| `forge_check_names_an_extra_bypass` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `forge_check_names_each_failure` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `forge_check_with_a_command_all_set` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `forge_check_with_a_command_names_each_failure` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `status_says_who_pushes_the_branch` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `forge_check_exits_by_what_it_finds` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `forge_check_runs_the_hosts_command` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `forge_check_tells_a_commands_words_without_its_token` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `the_apps_pull_request_calls` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `live_pull_request_calls` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `the_pools_calls` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_read_github_could_not_answer_is_made_again` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_read_answered_503_until_the_bound_fails_unanswered` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_call_refused_its_token_is_made_on_a_fresh_one` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_call_refused_a_commands_token_is_made_on_one_got_afresh` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `apps_holding_one_store_get_a_repositorys_token_once` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_token_got_afresh_is_the_one_another_refused_takes` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_command_that_gives_the_refused_token_back_gives_none_afresh` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_command_that_gives_no_token_fails_the_call` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_write_that_sets_a_state_is_made_again` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_write_that_makes_something_is_read_back_before_it_is_made_again` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_comment_whose_first_try_cannot_connect_is_posted_once` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_dispatch_answered_502_is_made_once` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_spent_allowance_is_waited_out_to_its_reset` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `every_process_on_the_host_honours_the_pause` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_commands_token_honours_the_pause` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `the_pause_outlives_the_process` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `graphqls_allowance_is_its_own` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_429_waits_as_long_as_github_says` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_secondary_limit_naming_no_time_doubles` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `the_last_call_of_an_allowance_starts_the_pause` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `every_read_is_conditional` | **End-to-end (D2)** | itself |
| `no_token_is_kept_on_the_host` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `one_query_reads_the_open_pull_requests_whole` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `the_query_reads_fifty_to_a_page` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_refused_query_fails_in_githubs_words` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `the_list_says_whether_it_changed` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_bundle_from_the_signers_workflow_counts` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_bundle_of_githubs_own_instance_counts` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_bundle_from_a_pull_requests_ref_counts_for_nothing` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `a_bundle_no_carried_root_verifies_counts_for_nothing` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `forge_check_names_a_pools_missing_workflow` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `forge_check_reads_a_workflow_with_the_repositorys_own_steps` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `forge_check_names_own_steps_that_leave_the_run_job` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `forge_check_names_an_environment_any_branch_may_use` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |
| `forge_check_reads_the_pool_the_policy_names` | A unit of `app.rs`, `rest.rs`, `keyless.rs` or `workflows.rs` (D1.2) | planned |

## `profiles/moon.rs` (43)

| Test | Goes to | Proven after by |
|---|---|---|
| `the_tasks_a_range_affects` | **End-to-end (D2)** | itself |
| `a_projects_moon_yml_runs_its_project_and_its_dependents` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `the_workspaces_settings_and_the_policy_run_every_check` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_moon_yml_no_check_reads_lands_beside` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_plan_reads_the_paths_a_worktree_holds_beyond_its_revision` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `moon_is_given_the_runs_share_of_the_cores` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_run_shows_each_task_as_it_ends` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_session_run_shows_each_task_as_it_ends` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_project_whose_id_holds_a_slash` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_task_the_time_limit_cut_off_has_no_outcome` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `an_unaffected_candidate_runs_nothing` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_second_run_replays_under_the_same_keys` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_finished_moon_run_s_checks_are_timed` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_run_without_replay_runs_every_task` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_failure_and_its_dependent` | **End-to-end (D2)** | itself |
| `a_task_that_could_not_run_records_no_fail` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_noop_check_is_failed_by_the_task_it_depends_on` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_cleanup_dependency_that_fails_fails_no_check` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `every_required_task_of_every_tag` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_task_kept_out_of_ci_is_not_run` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `the_reading_names_the_checks_a_change_reads` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `the_command_runner_reads_every_check` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `moon_env_probe` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `moon_options_in_sluices_environment_change_nothing` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `an_old_moon_is_refused` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `helpers_are_not_results_and_allowed_failures_fail` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_sandboxed_run_reads_as_an_open_one` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_cold_test_gets_no_cache` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_cold_test_gets_no_cache_in_the_sandbox` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_key_is_checked_against_the_tree_without_running` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_key_whose_input_is_a_link_is_proven` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `proven_drops_a_key_the_tree_does_not_bear_out` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `proven_names_the_file_a_key_names_that_the_tree_does_not_hold` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_run_jobs_kept_proof_proves_its_keys_and_no_forged_one` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_pass_whose_manifest_is_gone_is_unkept_naming_the_store` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_failed_task_whose_manifest_is_gone_is_not_unkept` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_required_task_kept_out_of_ci_is_named_before_any_run` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `asking_moon_leaves_the_worktree_clean` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_key_run_in_a_bare_repositorys_worktree_is_proven` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_key_is_read_under_the_home_the_toolchain_gives_moon` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_task_moon_does_not_cache_and_its_dependent_are_proven` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_manifest_moon_kept_in_the_checkout_proves_its_key` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |
| `a_key_run_in_the_sandbox_is_proven_under_the_home_the_toolchain_gives_moon` | A unit of `profiles/moon.rs` over moon's answer as data (D1.2) | planned |

## `profiles/moon_telemetry.rs` (1)

| Test | Goes to | Proven after by |
|---|---|---|
| `moon_runs_with_otel_while_telemetry_is_on` | A unit of `profiles/moon.rs` (D1.2) | planned |

## `profiles/places.rs` (50)

| Test | Goes to | Proven after by |
|---|---|---|
| `a_git_worktree_place_is_the_candidate_and_leaves_nothing` | **End-to-end (D2)** | itself |
| `a_git_worktree_place_left_by_a_crash` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_checkout_git_left_locked_initializing_is_released` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_checkout_git_left_locked_initializing_is_made_again` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_checkout_locked_for_another_reason_is_held` | A unit of `profiles/places.rs` (D1.2) | planned |
| `releasing_a_place_ends_its_run` | A unit of `profiles/places.rs` (D1.2) | planned |
| `an_unknown_method_is_refused` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_policy_need_not_name_the_method` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_checkout_starts_with_nothing_but_its_revision` | A unit of `profiles/places.rs` (D1.2) | planned |
| `places_alive_at_once_hold_their_own_ports` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_block_with_a_port_in_use_is_passed_over` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_places_ports_are_read_from_its_record` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_release_frees_the_ports_its_record_names` | A unit of `profiles/places.rs` (D1.2) | planned |
| `ports_with_bad_names_are_refused` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_sessions_worktree_is_adopted_with_ports_it_keeps` | A unit of `profiles/places.rs` (D1.2) | planned |
| `releasing_a_sessions_place_leaves_its_worktree` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_sessions_place_is_released_under_the_policy_read_now` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_preview_is_made_kept_apart_and_released` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_preview_naming_an_unlisted_port_is_refused` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_failed_preview_is_kept_holding_nothing` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_name_another_preview_holds_takes_the_longer_revision` | A unit of `profiles/places.rs` (D1.2) | planned |
| `checked_out_lists_the_worktrees_on_a_branch` | A unit of `profiles/places.rs` (D1.2) | planned |
| `checked_out_reads_the_worktrees_as_git_lists_them` | A unit of `profiles/places.rs` (D1.2) | planned |
| `an_empty_scratch_root_reads_no_record` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_deleted_checkout_is_gone_and_freed_with_nothing_run` | A unit of `profiles/places.rs` (D1.2) | planned |
| `freeing_a_sessions_place_leaves_its_worktree` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_failed_session_release_keeps_its_place_with_what_failed` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_failed_preview_release_is_held_once_its_retries_are_spent` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_released_preview_ends_its_containers_before_its_ports_go` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_gone_previews_containers_end_after_its_stored_release` | A unit of `profiles/places.rs` (D1.2) | planned |
| `an_accepted_previews_containers_end_with_nothing_run` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_container_left_keeps_the_preview_and_its_ports` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_deleted_checkouts_stored_release_runs_once_from_its_scratch_directory` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_gone_checkout_is_freed_whatever_its_stored_release_did` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_checkout_git_no_longer_lists_is_gone` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_checkout_git_no_longer_lists_is_freed` | A unit of `profiles/places.rs` (D1.2) | planned |
| `the_users_scratch_directory_is_made_its_own` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_scratch_directory_not_the_users_own_is_refused` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_scratch_the_system_emptied_is_made_again` | A unit of `profiles/places.rs` (D1.2) | planned |
| `two_repositories_keep_their_scratch_apart` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_place_names_its_scratch_on_its_record` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_worktree_keeps_one_short_scratch` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_place_from_before_takes_the_short_scratch_at_its_next_run` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_place_in_the_repository_takes_the_short_scratch_at_its_next_run` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_place_with_a_run_going_keeps_its_former_scratch_for_it` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_former_scratch_that_cannot_be_moved_is_removed` | A unit of `profiles/places.rs` (D1.2) | planned |
| `scratch_no_record_names_is_removed` | A unit of `profiles/places.rs` (D1.2) | planned |
| `an_unreadable_record_keeps_its_scratch` | A unit of `profiles/places.rs` (D1.2) | planned |
| `another_repositorys_scratch_is_left` | A unit of `profiles/places.rs` (D1.2) | planned |
| `a_users_directory_not_its_own_is_not_swept` | A unit of `profiles/places.rs` (D1.2) | planned |

## `profiles/pull_requests.rs` (171)

| Test | Goes to | Proven after by |
|---|---|---|
| `pull_requests_need_a_remote` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `waiting_for_qa` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_draft_waits` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_pull_request_opened_by_hand` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `reviews_on_an_older_head_do_not_count` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_forks_pull_request_is_never_submitted` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_new_commit_while_queued` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `closed_while_queued` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_request_closed_under_test_drops_its_combination` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `landed_by_another_queue_while_queued` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_withdrawn_submission_whose_work_landed_is_reported_landed` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_wait_beside_a_driver_reads_a_request_another_queue_landed_from_the_branch` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_wait_on_a_request_the_operator_withdrew_exits_withdrawn` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_wait_follows_the_new_head_its_submit_withdrew_it_for` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_wait_exits_withdrawn_when_the_request_carrying_its_work_closes` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `blamed_then_retried` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `blamed_and_left_alone` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `withdrawn_by_the_operator_opened_by_hand` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `withdrawn_by_the_operator_then_retried_by_its_author` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `withdrawn_by_the_operator_then_a_new_commit` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_new_head_no_submit_pushed` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `withdrawn_by_the_operator` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `held_then_marked_ready_by_a_person` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `held_when_github_refuses_the_draft` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `one_report_comment_edited_until_landed` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `behind_a_blamed_pull_request_no_failure_is_shown` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `output_posted_as_asked_and_scrubbed` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_standing_intake_failure_is_said_once_for_each_cause_and_host` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `status_shows_this_hosts_standing_intake_refusal` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_step_prints_what_its_intake_met` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `s63_a_request_sluice_cannot_fetch_holds_no_other` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `reviews_that_cannot_be_read_hold_no_other` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_failed_list_ends_intake_and_the_queue_still_lands` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `the_host_process_buys_a_repositorys_token_once_across_its_looks` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_repository_github_no_longer_finds_is_said_once_with_what_to_do` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `the_check_names_what_is_missing` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_submits_pull_request_sent_back_says_so` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `landed_and_merged_by_landing` | **End-to-end (D2)** | itself |
| `a_landing_and_its_records_name_the_apps_bot` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_requests_post_confirms_its_outcome_on_receipt` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `the_check_names_its_host_and_queue` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `the_queue_block_moves_with_the_queue_ahead` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `the_queue_block_follows_the_queues_order` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `no_repeated_posts` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_conflict_seen_early` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_draft_whose_head_cannot_be_fetched_is_only_a_draft` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `submit_opens_then_updates_a_pull_request` | **End-to-end (D2)** | itself |
| `a_bolt_submitted_with_no_subject_is_titled_by_its_branch` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_session_submitted_with_a_subject_is_titled_by_it` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `the_description_still_carries_the_commits` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_new_head_with_a_subject_retitles_it` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_new_head_with_no_subject_keeps_the_title` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_request_opened_before_titles_changed_is_retitled` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_subject_on_a_request_opened_by_hand_leaves_its_title` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_request_sluice_opened_with_a_subject_lands_named_by_it` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_request_a_person_opened_lands_named_by_its_title` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_bolt_too_long_for_one_description` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `one_message_longer_than_github_holds` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_new_commit_submitted_again_is_described` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `submitted_again_at_the_same_head_edits_nothing` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `submit_refuses_a_dirty_worktree` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `submit_refuses_a_gate_requiring_a_task_kept_out_of_ci` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_waiting_submit_follows_its_pull_request` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `no_wait_then_status` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `status_names_the_head_waiting_for_a_review` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `status_names_a_draft` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `run_again_follows_a_pull_request_waiting_for_a_review` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `run_again_without_waiting_answers_with_a_pull_request` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `run_again_marks_a_draft_ready` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `no_wait_answers_with_its_submission` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `no_wait_waiting_for_a_review_answers_with_the_pull_request` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `no_wait_waiting_for_a_review_names_its_wait_in_words` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `no_wait_says_its_submission` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `wait_follows_a_pull_request` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `out_of_time_waiting_for_a_review_names_its_wait` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `wait_follows_a_pull_requests_submission` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_wait_on_a_request_taken_in_here_out_of_time_prints_its_record` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_wait_by_pull_request_into_another_branch_is_not_found` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_wait_by_pull_request_taken_in_here_prints_its_record` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_wait_by_another_worktrees_pull_request_hands_nothing_over` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `sent_back_keeps_the_worktree` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_landing_stands_when_its_release_then_fails` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_waiting_submit_reads_again_through_githubs_503` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `blamed_then_submitted_again_on_the_same_head` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `timed_out_then_submitted_again_on_the_same_head` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_new_head_submitted_while_queued` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `submitted_by_the_operators_ops` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `s56_a_refused_request_waits_for_its_submitter` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `s56_a_refused_request_opened_by_hand_is_taken_again_once_reviewed` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `s56_a_request_the_operator_withdrew_waits_for_a_new_head` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_group_submit_opens_every_pull_request_marked` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `two_requests_two_subjects` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `two_requests_no_subjects` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_group_submit_queues_three_pull_requests_in_order` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_group_submit_answers_with_each_submission_in_json` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_group_act_names_each_pull_requests_wait` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_group_act_stopped_names_each_open_pull_requests_wait` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_group_submit_one_short_of_a_review_is_refused` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_group_submit_refused_then_reviewed_is_taken` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_group_submit_takes_a_pull_request_held_as_a_draft` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_group_submit_leaves_a_forks_pull_request_alone` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_group_submit_with_a_dirty_worktree_pushes_nothing` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_waiting_group_submit_follows_each_pull_request` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `two_of_a_group_ready_submits_none` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_round_between_a_groups_opens_submits_none` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `one_of_a_group_closed_holds_the_rest` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `one_of_a_group_on_a_new_head_is_withdrawn_alone` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_bolts_landing_worktree_submits_the_bolt_as_github_holds_it` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_waiting_submit_ends_with_its_own_submission_when_the_branch_moves_under_it` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_waiting_submit_by_name_in_a_bare_clone_reports_another_queues_landing` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_submit_by_name_opens_the_request_from_the_branch` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_submit_by_name_names_its_pull_requests_wait` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_request_by_name_is_collected_by_no_worktree` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_waiting_submit_waits_out_the_pause` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_wait_whose_timeout_comes_first_calls_nothing` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_submit_that_does_not_wait_fails_at_once_on_the_pause` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_round_lands_through_the_pause_and_says_it_once` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `nothing_moved_asks_once_a_minute` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `passes_between_discoveries_start_no_git_and_ask_github_nothing` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `the_host_sets_the_interval` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_submit_on_the_host_asks_at_once` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_landing_asks_at_once` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_queued_pull_request_that_stands_is_read_by_the_list_alone` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `the_state_moves_and_is_posted_unread` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_draft_is_read_from_the_list_alone` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_pull_request_waiting_on_a_check_is_read_at_each_asking` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_new_head_is_read_again` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_wait_reads_its_pull_request_once_in_a_quarter_of_the_interval` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `pull_requests_need_records_on_the_remote` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `records_on_the_remote_only_the_lead_asks` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `the_host_that_keeps_main_takes_its_requests_in` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_host_the_branch_does_not_accept_takes_no_request_in` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `an_older_mark_is_read_by_nothing` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `an_older_hand_over_carried_onto_the_remote_is_taken_in_again` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_wait_on_an_older_hand_over_follows_its_pull_request` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_request_keeps_one_number_while_other_hosts_pass` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `another_host_keeps_main_and_this_hosts_step_takes_nothing_in` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_host_that_stopped_passes_main_on_after_its_lease` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_refused_main_leaves_a_bolt_landing` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_wait_across_hosts_reads_at_its_pace` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `submitted_here_taken_in_there` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_merged_request_leaves_the_follows` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `two_waits_and_a_status_read_at_one_pace` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `landed_on_another_host` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `blamed_on_another_host` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_run_github_keeps_completed_reads_by_its_block` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `an_older_sluices_check_reads_by_its_words` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `out_of_time_queued_on_the_other_host` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_wait_by_pull_request_from_the_main_checkout_follows_it_on_another_host` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `no_wait_on_a_host_that_does_not_take_it_in_names_its_wait` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `the_status_a_submit_reads_across_hosts` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_waits_words_across_hosts` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `not_yet_taken_in_across_hosts` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `queued_on_the_other_host` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `waiting_for_a_review_there` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `no_check_on_its_head_yet` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `the_keeper_and_the_check_name_different_hosts` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_status_says_a_merged_request_landed_once` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_one_shot_overview_lists_requests_taken_in_elsewhere` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_watch_reads_no_followed_request_itself` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `held_then_marked_ready_waits_to_be_taken_in` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `taken_in_again_while_github_keeps_the_check_failed` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `status_of_a_request_another_host_sent_back` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `run_again_marks_ready_a_draft_another_host_sent_back` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_submit_marks_ready_a_request_another_host_sent_back_not_a_draft` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_submit_leaves_a_request_queued_on_another_host` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `three_pull_requests_one_query` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `the_querys_reviews_count_as_before` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_refused_query_falls_back_to_the_reads_per_pull_request` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_submit_to_a_branch_that_takes_pull_requests_reads_no_forge` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |
| `a_wait_on_a_request_that_lands_reads_no_forge` | In-process over the fakes, `FileForge` and `RequestRefs`; a forge HTTP case to a unit of `rest.rs` (D3) | planned |

## `profiles/worktrunk.rs` (77)

| Test | Goes to | Proven after by |
|---|---|---|
| `wt_submit_lands_or_fails_by_its_exit` | **End-to-end (D2)** | itself |
| `only_a_landing_removes_the_worktree` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `wt_submit_conflicts_with_exit_2` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_dirty_worktree_is_refused_naming_each_path` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_detached_worktree_is_refused` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `no_wait_returns_and_status_reads_the_outcome` | **End-to-end (D2)** | itself |
| `check_writes_nothing` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `the_submitter_is_the_agent_or_the_branch` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `name_a_branchs_submitter` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `named_records_no_worktree` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_submit_records_the_pane_it_ran_in_and_its_host` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_tests_submit_records_no_pane_its_parent_is_in` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_landing_is_pushed_into_the_pane_it_was_submitted_from` | **End-to-end (D2)** | itself |
| `a_failure_is_pushed_with_its_check` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_refused_prompt_leaves_the_outcome_pending` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_submit_in_no_pane_is_pushed_nothing` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_submission_from_another_host_is_not_pushed` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_round_in_another_pane_reaches_the_recorded_server` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_submission_names_its_worktrees_path_and_the_overview_shows_it` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_resubmission_withdraws_the_first` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `run_again_follows_its_submission_and_removes_on_landing` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `run_again_once_landed_removes_at_once` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `run_again_once_landed_keeps_under_no_remove` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_first_submit_under_a_time_limit_submits_then_follows` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `run_again_blamed_keeps_the_worktree` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `run_again_after_a_blame_submits_the_same_head` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_time_limit_without_waiting_is_refused` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_no_wait_run_again_answers_with_what_stands` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_no_wait_run_again_once_landed_keeps_the_worktree` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_place_in_the_queue_where_one_stands_is_refused` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `an_unread_status_submits_nothing` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_commit_while_followed_keeps_the_worktree` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `sluice_wait_removes_nothing` | **End-to-end (D2)** | itself |
| `ctrl_c_ends_only_the_wait` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `wt_submit_releases_before_it_removes` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `wt_submit_confirms_a_landing_it_removes_the_worktree_of` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `status_shows_an_expired_blame_with_its_reader_never_confirmed` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `inbox_hands_a_landing_over_until_it_is_confirmed` | **End-to-end (D2)** | itself |
| `confirm_confirms_a_landing_by_its_key_and_says_when_again` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `confirm_json_gives_a_first_confirmation_and_the_worst_of_its_exits` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `status_json_gives_a_decided_submissions_delivery_and_none_before` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_hosts_span_of_a_day_owes_a_landing_for_a_day` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_deliveries_line_that_does_not_read_stops_the_branchs_status` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `inbox_json_lists_each_record_until_it_is_confirmed` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `inbox_says_an_expired_outcome_is_still_owed` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `inbox_new_hands_over_only_what_no_way_has_handed_over` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `inbox_delivers_a_blame_as_sluice_wait_reports_it` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_waiting_submit_hands_its_landing_over_and_inbox_hands_it_over_again` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_wait_that_confirms_on_receipt_leaves_inbox_nothing` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_wait_from_another_worktree_marks_nothing` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_wait_reports_an_outcome_the_pane_took_as_it_stands` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_wait_that_runs_out_of_time_marks_nothing` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `inbox_delivers_nothing_of_another_worktrees_outcome` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_wait_follows_the_resubmission_that_carries_its_work` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_wait_started_after_the_resubmission_landed_names_it` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_wait_exits_withdrawn_when_what_carried_its_work_failed` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_wait_out_of_time_while_it_follows_prints_its_own_record` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_wait_leaves_the_resubmissions_landing_pending` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_folder_with_no_files_checked_out_is_read_with_no_status_and_no_commit` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_wait_with_no_files_checked_out_reads_as_a_worktree_on_its_branch` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_git_directory_delivers_its_branchs_own` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_head_with_no_commit_fails_no_delivery_read` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_wait_by_name_in_a_bare_clone_says_nothing_pending` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `inbox_in_a_detached_worktree_delivers_nothing` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `inbox_json_holds_the_contracts_fields_for_each_outcome` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `inbox_in_a_herdr_pane_delivers_to_that_pane_and_no_round_pushes_it` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `of_two_tested_together_only_the_blamed_ones_pane_is_sent_the_failure` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_caller_that_never_waits_collects_a_failure_and_a_landing_by_inbox` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `submits_from_a_bare_clone` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `submits_from_a_worktree_on_another_branch` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `the_forecast_names_what_the_submit_sends` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_waiting_submit_by_name_lands` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_subject_names_the_landing_by_name` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_failure` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_conflict` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_failure_by_name` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |
| `a_conflict_by_name` | In-process over the fakes, beside `cli.rs` (D1.3) | planned |

## `protect.rs` (38)

| Test | Goes to | Proven after by |
|---|---|---|
| `every_way_git_moves_a_protected_branch_is_refused` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `what_moves_no_protected_branch_is_allowed` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `the_branchs_own_revision_decides` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_branch_with_submissions_in_its_queue_is_not_deleted` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `packing_a_protected_branch_is_allowed` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_hook_already_there_still_runs` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `apples_git_is_refused_too` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `sluices_own_move_is_not_refused` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_timers_writes_are_one_cause` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_refused_writes_count_is_in_the_state_not_the_log` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `installing_is_idempotent_and_follows_the_binary` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `sluice_protect_is_made_by_sluice_host` | **End-to-end (D2)** | itself |
| `a_tracked_hooks_directory_is_not_written` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `protect_names_a_worktree_holding_a_protected_branch` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_copy_apart_from_its_remote_is_judged_by_its_own_value` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_fetch_whose_packing_runs_the_hook_asks_nothing` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_name_that_an_ended_sluice_host_gave_is_asked` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_check_that_writes_main_is_refused` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_refusal_names_the_command_and_both_revisions` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_local_branchs_refusals_say_the_command` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `the_protection_judges_the_repository_git_names` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_transaction_the_protection_cannot_read_passes_said_once` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `settings_the_hook_does_not_know_change_nothing_it_guards` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `every_other_command_refuses_a_setting_it_does_not_know` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_policy_field_the_hook_does_not_know_changes_nothing_it_guards` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_copy_is_judged_under_fields_the_hook_does_not_know` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `settings_or_a_policy_the_hook_cannot_read_pass_said_once` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_newer_sluices_next_command_puts_back_the_hook` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_command_with_no_git_has_sluice_host_put_back_the_hook` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_removed_hook_is_not_put_back_by_a_reading_command` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_sluice_on_the_callers_path_is_not_the_one_that_judges` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `the_hook_reads_its_whole_transaction_before_it_leaves` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `the_hook_says_what_sluice_host_answers` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_persons_write_is_judged_by_sluice_host` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `the_refused_write_is_logged_by_sluice_host` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_hook_with_no_sluice_host_running_starts_it` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_hook_whose_sluice_host_does_not_answer_refuses` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |
| `a_commit_during_a_long_round_ends_at_once` | A unit of `protect.rs` and `host_process/protection.rs` (D1.2) | planned |

## `pushed_refs.rs` (61)

| Test | Goes to | Proven after by |
|---|---|---|
| `a_read_sees_what_another_clone_pushed` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_fetch_waits_for_another_fetch_of_its_repository` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_fetch_the_remote_never_answers_is_stopped_and_lets_its_lock_go` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_fetch_waits_for_the_lock_no_longer_than_a_try` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_push_the_remote_never_answers_is_stopped_and_not_pushed_again` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_race_lost_on_a_fetched_ref_is_no_refused_read` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_push_lands_and_leaves_the_copy` | **End-to-end (D2)** | itself |
| `a_push_accepted_under_the_copys_lock_is_moved` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_push_after_the_branch_moved_is_elsewhere` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `an_unreachable_remote_fails_the_read_and_the_push` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_fetch_from_a_remote_not_there_is_a_refused_read` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_refused_push_names_the_remotes_reason` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_push_the_forge_declines_reads_as_refused_by_the_forge` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_refused_push_with_the_branch_moved_reads_as_elsewhere` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `the_copy_is_read_with_the_remote_unreachable` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_copy_is_levelled_only_from_the_value_it_holds` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_levelling_fails_on_the_copys_lock` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `the_router_sends_the_copy_to_the_branchs_profile` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `the_router_keeps_a_branch_without_a_remote_local` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `the_router_sends_a_branch_to_its_remote` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `the_router_reads_a_copy_from_before_the_policy_as_origins` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `made_from_and_policy_of_read_a_copy_from_before_the_policy_as_origins` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `the_router_follows_a_later_copy_naming_a_remote` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `the_router_keeps_a_branch_no_copy_routes_local` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `the_router_refuses_two_remotes_named` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `the_router_decides_with_the_remote_out_of_reach` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_copy_refused_elsewhere_still_says_where_the_branch_is` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `the_one_revision_naming_the_remote_is_refused_elsewhere` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_remote_that_cant_be_read_refuses_naming_the_copy` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `two_remotes_are_still_refused_when_one_copy_is_refused_elsewhere` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `made_from_and_policy_of_read_a_refused_copy_as_remote_does` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_landing_names_whoever_pushes_it` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `the_router_writes_a_landing_as_its_branchs_profile` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_landing_whose_author_cannot_be_read_is_not_written` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_combination_is_one_commit_whatever_the_identity` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `an_identity_reaches_only_sluices_git` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_refused_fetch_is_run_again_with_fresh_credentials` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_remote_pointed_elsewhere_is_reached_there_at_the_next_read` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_credential_the_forge_withholds_is_a_refused_read` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_refusal_that_stands_names_every_credential` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_push_refused_on_its_token_is_made_once_more` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_header_gits_configuration_holds_is_never_sent_as_the_identity` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_credential_git_sends_of_its_own_is_named_and_never_blamed_on_the_identity` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_push_refused_on_every_token_names_each_in_order` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_push_refused_twice_with_the_time_spent_names_both` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_push_with_no_fresh_token_is_made_once` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_leased_push_refused_on_its_token_is_made_once_more` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_second_push_carries_the_first_ones_lease` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_push_refused_at_the_ref_is_made_once` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_push_with_no_credential_fails_rather_than_waits` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_push_the_remote_takes_is_one_push` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_branch_made_with_no_answer_is_what_the_remote_holds` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_branch_removed_with_no_answer_is_what_the_remote_holds` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_fetch_answered_503_twice_reads_on_the_third` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_leased_push_refused_on_two_tokens_lands_on_the_third` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_push_answered_503_before_it_was_sent_is_tried_again` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_landing_cut_after_it_was_sent_is_what_the_remote_holds` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_push_refused_on_the_hosts_credentials_is_made_once` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `the_remotes_and_their_addresses_read_as_git_answers_them` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_remote_added_after_the_profile_opened_routes_at_its_next_read` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |
| `a_branch_held_fetches_nothing_with_the_remote_out_of_reach` | A unit of `pushed_refs.rs` over a pure function (D1.2) | planned |

## `reader_tick.rs` (3)

| Test | Goes to | Proven after by |
|---|---|---|
| `a_counting_git_counts_what_status_starts` | In-process over the fakes (D1.3) | planned |
| `sluice_run_with_nothing_queued_starts_git_at_its_first_pass_alone` | In-process over the fakes (D1.3) | planned |
| `a_held_landing_is_pushed_once_a_minute` | In-process over the fakes (D1.3) | planned |

## `remote_branch.rs` (40)

| Test | Goes to | Proven after by |
|---|---|---|
| `submissions_land_on_the_remote_through_run` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_push_from_elsewhere_refuses_the_landing_and_the_next_round_lands` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_landing_pushed_but_unrecorded_is_recorded_by_the_next_round` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_push_that_never_reached_the_remote_lands_on_the_next_round` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `landed_is_written_only_once_the_remote_holds_the_landing` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_wait_on_a_landing_the_forge_refused_exits_6` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_held_landing_lands_at_the_next_round_once_the_forge_takes_it` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_held_landing_refused_with_other_words_is_held_with_them` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `push_is_no_command_on_a_remote_branch` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_submit_with_the_copy_behind_is_formed_on_the_remotes_revision` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_failed_fetch_lands_nothing_and_says_so` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_refused_fetch_is_logged_once_and_its_end_with_the_rounds` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `with_the_state_on_the_remote_both_lines_land_in_the_first_write_after` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `an_unknown_remote_lands_nothing_and_is_named` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_bolt_is_made_landed_on_and_deleted_on_its_remote` | **End-to-end (D2)** | itself |
| `a_bolt_is_made_and_deleted_with_no_remote` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_bolt_deleted_with_nothing_submitted_here_answers_its_status` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_bolt_only_fetched_here_lands_on_its_remote` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_bolt_is_made_on_origin_from_a_main_this_host_holds_from_before_the_policy` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `two_remotes_named_for_main_land_nothing_and_are_named` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `s57_a_remote_older_than_the_local_landings_holds_until_the_landing_is_pushed` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `s57_a_remote_older_than_the_local_landings_holds_until_its_revision_is_accepted` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_worktree_on_a_bolt_behind_submits_origins_revision_and_a_round_levels_it` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_branch_by_name_is_submitted_at_its_remotes_revision` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `s64_the_bolts_round_reads_past_a_copy_that_lags` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_policy_refused_on_the_remote_is_cleared_by_a_push` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `the_overview_names_a_hand_pushed_revision` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_quiet_branch_this_host_fetched_is_read_whole_by_status` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_quiet_branch_this_host_never_fetched_takes_a_submit` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_forecast_beside_a_current_reader_asks_the_remote_nothing` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_forecast_with_no_host_process_starts_it_and_fetches_nothing` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_forecast_with_no_reader_has_the_host_process_fetch_another_hosts_work` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_forecast_whose_reading_lacks_the_branch_forms_after_the_next_reading` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_forecast_beside_a_reading_that_could_not_ask_the_remote_says_so` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_wait_on_main_passes_a_bolt_whose_own_ref_is_refused` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_bounded_run_lands_main_and_exits_3_naming_the_refused_bolt` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_refused_main_hides_no_branch_this_host_has_read` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `an_idle_branch_on_a_remote_is_passed_over_with_no_git` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_branch_moved_on_its_remote_is_shown_as_last_fetched_until_a_round` | In-process over the fakes, a branch on a remote (D1.3) | planned |
| `a_branch_made_on_its_remote_runs_no_git` | In-process over the fakes, a branch on a remote (D1.3) | planned |

## `remote_refs.rs` (41)

| Test | Goes to | Proven after by |
|---|---|---|
| `a_records_commit_names_whoever_pushes_it` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `two_hosts_write_one_version_and_one_is_refused` | **End-to-end (D2)** | itself |
| `a_round_fetches_the_state_once` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `appends_and_finished_records_keep_their_places` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_finished_record_put_again_replaces_it_and_leaves_the_index` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_refused_state_fetch_is_stale_with_why` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `each_profile_keeps_a_refusal_beside_the_records` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `ids_are_numbered_once_across_hosts` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `signed_runs_are_read_verified_on_another_host` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_signed_runs_grant_is_read_from_its_statement` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_statement_verified_with_one_key_is_unverified_with_another` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `two_hosts_of_one_signer_keep_both_adds` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_host_with_no_signing_key_records_nothing_and_says_so` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `logs_are_kept_on_the_remote_for_the_last_candidates` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_sign_of_life_cut_after_it_was_sent_stands` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_state_write_refused_at_the_lease_is_one_push` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_ticks_ask_brings_the_branch_and_the_work_in_one_fetch` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_combination_only_the_remote_holds_is_fetched_and_a_place_made_at_it` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_branch_the_remote_lacks_still_gives_the_records` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_remote_out_of_reach_gives_the_records_last_fetched` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `one_ask_brings_every_branchs_records` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_refused_ask_leaves_every_branch_as_last_fetched` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_rounds_read_still_fetches_its_branch` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `each_branch_reads_runs_with_its_own_key_whichever_is_read_first` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_hosts_runs_count_only_where_the_branch_lists_its_key` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_signer_only_the_second_branch_names_and_a_key_added_later_count` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_branch_its_remote_does_not_hold_reads_no_signed_run` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_branch_on_its_remote_keeps_its_tasks_under_the_lock_its_records_took` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_branch_in_the_hosts_own_files_reads_with_its_own_list` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_branch_in_the_hosts_own_files_numbers_from_the_hosts_count` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_branchs_home_is_read_from_its_records_not_its_policy` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_host_with_no_records_of_a_branch_finds_its_home_on_the_remote` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `two_open_homes_refuse_the_branch_naming_both` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `the_first_write_writes_the_home_its_policy_chose` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_hosts_queue_moves_to_its_remote_with_its_numbers` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_number_the_remote_holds_is_carried_under_the_next` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_carry_stopped_after_the_remote_was_written_is_closed_by_the_next` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_branch_comes_home_each_host_taking_its_own_submissions` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_kms_record_verifies_by_the_listed_half_alone` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_kms_record_by_another_key_counts_for_nothing` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |
| `a_kms_record_is_never_a_hosts` | A unit of `remote_refs.rs` over a pure function, or a conformance scenario (D1.2) | planned |

## `runs.rs` (16)

| Test | Goes to | Proven after by |
|---|---|---|
| `a_run_ends_everything_it_started` | **End-to-end (D2)** | itself |
| `a_run_notes_when_its_output_grew` | A unit of `runs.rs` (D1.2) | planned |
| `found_reads_a_started_run_and_when_it_began` | A unit of `runs.rs` (D1.2) | planned |
| `a_run_still_going_is_never_finished` | A unit of `runs.rs` (D1.2) | planned |
| `a_run_over_its_limit_is_ended_and_timed_out` | A unit of `runs.rs` (D1.2) | planned |
| `a_run_whose_supervisor_died_is_lost_and_ended` | A unit of `runs.rs` (D1.2) | planned |
| `a_run_whose_folder_is_gone_is_lost` | A unit of `runs.rs` (D1.2) | planned |
| `removing_a_run_in_progress_ends_it` | A unit of `runs.rs` (D1.2) | planned |
| `a_run_has_its_own_environment_and_session` | A unit of `runs.rs` (D1.2) | planned |
| `a_command_that_cannot_start_fails_with_its_reason` | A unit of `runs.rs` (D1.2) | planned |
| `a_place_name_with_slashes_is_one_run` | A unit of `runs.rs` (D1.2) | planned |
| `removing_a_run_runs_its_release_once` | A unit of `runs.rs` (D1.2) | planned |
| `a_sessions_runs_are_released_once_and_only_together` | A unit of `runs.rs` (D1.2) | planned |
| `a_watch_is_ready_before_the_command_starts_and_ends_after_it` | A unit of `runs.rs` (D1.2) | planned |
| `a_watch_that_never_gets_ready_is_said_and_the_run_goes_on` | A unit of `runs.rs` (D1.2) | planned |
| `ending_a_run_ends_its_watch` | A unit of `runs.rs` (D1.2) | planned |

## `stack.rs` (4)

| Test | Goes to | Proven after by |
|---|---|---|
| `two_stacks_side_by_side_and_a_time_limit_leaves_nothing` | **End-to-end (D2)** | itself |
| `two_stacks_side_by_side_in_the_sandbox` | Folded into the contract test of D2 | planned |
| `a_sessions_stack_stays_between_runs_and_goes_on_release` | Folded into the contract test of D2 | planned |
| `a_previews_stack_stays_up_until_it_is_released` | Folded into the contract test of D2 | planned |

## `telemetry.rs` (9)

| Test | Goes to | Proven after by |
|---|---|---|
| `a_landing_reports_its_round_verdict_and_landing` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `a_failure_reports_its_culprit` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `a_replay_is_counted` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `a_replay_counts_the_time_it_saved` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `a_conflict_is_counted` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `a_refused_landing_is_counted` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `a_submission_s_end_is_measured` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `telemetry_is_off_unless_asked` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `a_submits_trace_names_its_callers_span` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |

## `telemetry_logs.rs` (24)

| Test | Goes to | Proven after by |
|---|---|---|
| `a_failure_s_output_is_sent_under_its_verdict_scrubbed` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `a_summary_says_where_the_output_is_kept_and_a_warning_is_sent_whole` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `a_failed_preview_s_output_is_sent_when_it_is_released` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `at_off_no_log_record_is_sent` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `a_round_s_spans_carry_what_they_are_of` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `a_round_is_one_trace_rooted_at_sluice_round` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `a_command_nests_under_its_caller_s_trace` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `a_round_links_to_the_trace_its_work_was_submitted_under` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `a_submission_s_life_is_a_span_in_the_trace_it_was_submitted_under` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `an_exec_s_span_holds_each_check_it_ran` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `an_exec_s_command_is_one_check_named_scrubbed` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `a_run_s_checks_join_the_round` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `with_the_tables_collector_a_run_gets_it_and_none_of_the_callers` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `with_traces_off_a_run_s_checks_join_no_trace` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `telemetry_goes_to_the_endpoint_as_otlp_over_http` | **End-to-end (D2)** | itself |
| `the_tables_collector_takes_sluices_telemetry_and_none_of_the_callers` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `with_no_collector_in_the_table_the_environment_names_it` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `the_table_turns_traces_off_whatever_the_environment_says` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `with_traces_off_metrics_and_logs_still_go_to_the_endpoint` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `with_metrics_off_traces_and_logs_still_go_to_the_endpoint` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `with_logs_off_traces_and_metrics_still_go_to_the_endpoint` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `with_the_sdk_disabled_nothing_is_sent` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `with_traces_off_a_failure_s_record_names_no_span` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |
| `a_collector_out_of_reach_fails_nothing` | A unit of `telemetry.rs` over the SDK's in-memory exporter (D1.2) | planned |

## `usage.rs` (12)

| Test | Goes to | Proven after by |
|---|---|---|
| `an_unknown_flag_exits_usage_on_stderr` | A unit of the CLI's parser (D1.2) | planned |
| `an_unknown_command_exits_usage` | A unit of the CLI's parser (D1.2) | planned |
| `a_missing_argument_exits_usage` | A unit of the CLI's parser (D1.2) | planned |
| `a_subject_with_check_or_status_exits_usage` | A unit of the CLI's parser (D1.2) | planned |
| `help_and_version_exit_zero` | A unit of the CLI's parser (D1.2) | planned |
| `submit_help_lists_the_exits_and_not_the_telemetry_flags` | A unit of the CLI's parser (D1.2) | planned |
| `wait_help_lists_the_exits_and_a_bad_timeout_exits_usage` | A unit of the CLI's parser (D1.2) | planned |
| `branch_accept_help_has_an_example` | A unit of the CLI's parser (D1.2) | planned |
| `only_the_root_help_names_the_telemetry_flags` | A unit of the CLI's parser (D1.2) | planned |
| `the_root_help_names_opentelemetrys_off_switches` | A unit of the CLI's parser (D1.2) | planned |
| `the_root_help_names_the_hosts_telemetry_table` | A unit of the CLI's parser (D1.2) | planned |
| `a_command_still_takes_the_telemetry_flags` | A unit of the CLI's parser (D1.2) | planned |

1001 tests; 47 stay end-to-end.
