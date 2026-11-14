# Verify: a-queues-home-is-written-in-its-records

Verified on `change/a-queues-home-is-written-in-its-records` at `9be7d303`,
cut from `bolt/sluice-is-refactored-and-runs-on-pools` (merge base
`d6d11623`; the bolt has since gained only `6558679a`, a policy key, and its
landing), on 2026-11-14 with sluice 0.93.2.

## Outcome

**Ready to archive.** The bolt's gate passes whole, and so does the tools
tier that main's gate adds. Every one of the 27 tasks is ticked, and its
check passes. Every scenario of the four delta specs has a test that
passes, and `openspec validate --strict` finds the change valid.

## Whole checks

`sluice exec bolt/sluice-is-refactored-and-runs-on-pools` ran the bolt's
gate, `#check-fast` (S66), once at `9be7d303`. It exited 0 with 12 tasks
passed, and recorded 12 results under their keys for `9be7d30`:

| Task | Outcome |
|---|---|
| site:prose, site:rules, nix:typecheck, root:adrs, root:changes | passed |
| root:format, root:lint | passed |
| core:test (164), fakes:test (17), seams:test (5) | passed |
| conformance:test (every target, `homes`, `peers` and `reading` among them) | passed |
| cli:test (`--test cli`: 223 passed, 0 failed; every other target whole) | passed |

moon warned on each task that it could not package task outputs into
`.moon/cache/outputs/` under the place. It recorded each pass all the same,
and nothing in this change touches that.

## The tools tier

`#check-fast` runs no test built with `--features tools`, but the checks of
tasks 1.4 to 1.6, 2.4, 2.5, 3.2, 4.4, 4.6 and 4.7 are tests of that tier.
So `cli:tools`' own command ran once through sluice against the bolt, with
`--no-fail-fast`:
`sluice exec bolt/sluice-is-refactored-and-runs-on-pools -- cargo test -p sluice --features tools --no-fail-fast --test instance … --test overview_watch -- --skip sandbox`.
It exited 0, with 664 tests passed and none failed:

| Target | Passed |
|---|---|
| conformance | 60 |
| equip_tools | 7 |
| exec | 17 (3 sandbox tests skipped) |
| forge | 48 |
| instance | 12 |
| landed | 1 |
| local_refs | 17 |
| moon | 32 (3 sandbox tests skipped), and 1 more |
| overview_watch | 10 |
| peers | 13 |
| places | 47 |
| previews_tools | 10 |
| protect | 28 |
| pull_requests | 157 |
| pushed_refs | 53 |
| remote_branch | 34 |
| remote_refs | 37 |
| stack | 3 (1 sandbox test skipped) |
| worktrunk | 77 |

Each test a tools-tier task names passed there:

- 1.4: `a_branchs_home_is_read_from_its_records_not_its_policy`,
  `a_host_with_no_records_of_a_branch_finds_its_home_on_the_remote` and
  `two_open_homes_refuse_the_branch_naming_both`.
- 1.5: `the_first_write_writes_the_home_its_policy_chose`.
- 1.6: `remote_branch.rs`'s `an_idle_branch_on_a_remote_is_passed_over_with_no_git`,
  which the change leaves as it is at the base, with 1.4's, 1.5's and 2.5's
  home tests.
- 2.4: `a_hosts_queue_moves_to_its_remote_with_its_numbers`,
  `a_number_the_remote_holds_is_carried_under_the_next` and
  `a_carry_stopped_after_the_remote_was_written_is_closed_by_the_next`.
- 2.5: `a_branch_comes_home_each_host_taking_its_own_submissions`.
- 3.2: `a_wait_follows_its_submission_to_its_queues_new_home`.
- 4.4: `a_host_the_branch_does_not_accept_takes_no_request_in`.
- 4.6: `remote_branch.rs`'s `a_policy_refused_on_the_remote_is_cleared_by_a_push`,
  which the change leaves as it is at the base.
- 4.7: `peers.rs`'s `a_run_is_claimed_and_run_by_the_other_host`, whose lead
  is trusted but its signer may claim only a system the gate does not ask
  for (79), and `pull_requests.rs`'s
  `the_keeper_and_the_check_name_different_hosts`, whose host C signs as
  `b`.

## Tasks

All 27 tasks in `tasks.md` are ticked, and each test a task names exists
where the task says. Each one passed in the run that holds it:

- In `#check-fast`, the 33 tests of 1.1 to 1.3, 2.1 to 2.3, 2.5's
  conformance half, 3.1, 3.3, 4.1, 4.3, 4.5, 5.1 and 5.5, and 4.6's
  `--test homes --test peers` whole.
- In the tools run, the 14 tests of 1.4 to 1.6, 2.4, 2.5, 3.2, 4.4, 4.6
  and 4.7.

2.6, 5.2, 5.3 and 5.4 hold by `root:lint`, `site:prose`, `root:adrs` and
`site:rules`, each passed in the gate.

The two code fixes behind 1.6 and 4.6 are as their tasks say:

- `Records::held_home` (`crates/sluice/src/profiles/records.rs`) reads a
  remote's `home.json` through `remote_refs::fetched_home`, from the ref
  this host last fetched, with no profile of the remote opened (129; rule
  core.31).
- `Sluice::keeps` (`crates/sluice-core/src/step.rs`) reads the rules at the
  branch's revision before it answers for a home in this host's own files,
  so a refused policy refuses the round first (124, 56; record 0069; rule
  core.15).

## Specs

`openspec validate a-queues-home-is-written-in-its-records --strict` finds
the change valid. The MODIFIED and REMOVED requirements of `peer-lead` and
`remote-records` name requirements `openspec/specs/` holds, and `queue-home`
is a new capability. Each scenario maps to a test that passed:

| Spec: scenario | Test |
|---|---|
| queue-home: The first submission writes the home; A branch with no signers | `the_first_write_writes_the_home_its_policy_chose` (both cases) |
| queue-home: A policy that names another home does not move the reading | `a_branchs_home_is_read_from_its_records_not_its_policy` |
| queue-home: A host with no records of the branch | `a_host_with_no_records_of_a_branch_finds_its_home_on_the_remote` |
| queue-home: Two open homes | `two_open_homes_refuse_the_branch_naming_both` |
| queue-home: A move waits for a combination under test | `a_policy_naming_another_home_writes_a_move_and_opens_no_combination`, `a_move_waits_for_the_combination_under_test` |
| queue-home: The policy names the open home again | `a_policy_naming_the_open_home_again_ends_the_move` |
| queue-home: A host's queue moves to origin | `a_move_carries_every_live_submission_with_its_number_and_state`, `a_hosts_queue_moves_to_its_remote_with_its_numbers` |
| queue-home: A submit racing the carry | `a_submit_racing_the_carry_is_written_in_the_new_home`, `a_submission_made_while_moving_is_carried` |
| queue-home: Stopped after the new home was written | `a_move_stopped_after_the_new_home_was_written_is_closed_by_the_next_round`, `a_carry_stopped_after_the_remote_was_written_is_closed_by_the_next` |
| queue-home: Two hosts' submissions come home | `a_branch_comes_home_each_host_taking_its_own_submissions`, `a_round_carries_its_hosts_submissions_home_before_it_reads_the_branch` |
| queue-home: A host that has not come back | `a_submission_left_for_its_host_says_it_waits_for_it` |
| queue-home: Numbers past the remote's count | `a_hosts_queue_moves_to_its_remote_with_its_numbers` |
| queue-home: Two hosts each held #52 | `a_number_the_remote_holds_is_carried_under_the_next`, `a_status_of_a_carried_number_names_the_number_it_has_now` |
| queue-home: A submission landed before the move | `a_submission_finished_before_the_move_is_read_in_the_closed_home` |
| queue-home: A host's settings name a lead; A policy names how a branch runs | `no_host_setting_chooses_how_a_queue_runs`, `no_policy_key_chooses_how_a_queue_runs` |
| queue-home: A move waiting | `the_overview_names_each_branchs_home_and_its_move`, and 5.5's status tests |
| submission-wait: The queue moves while a wait follows; Carried under another number | `a_wait_follows_its_submission_to_its_queues_new_home` |
| peer-lead: The former lead stops after a takeover; A lead that went quiet; Clocks that disagree | `a_quiet_lead_is_taken_after_the_lease_by_the_takers_own_observation`, `a_lead_that_writes_keeps_the_branch` (conformance `peers`) |
| peer-lead: A lead the branch no longer accepts | `a_lead_the_branch_no_longer_accepts_is_taken_after_the_lease` |
| peer-lead: A session host submits to a bolt held on origin | `a_host_whose_signature_the_branch_does_not_accept_never_leads` |
| peer-lead: A host with an unlisted key | `a_host_with_an_unlisted_key_never_leads`, `a_host_the_branch_does_not_accept_takes_no_request_in` |
| peer-lead: A bolt whose signers went | `any_host_leads_a_remote_home_whose_policy_names_no_signer` |
| peer-lead: A run longer than the lease | `a_lead_keeps_its_branch_through_a_run_longer_than_the_lease` |
| peer-lead: Nothing live | `a_lead_with_no_live_run_writes_nothing_to_keep_it` |
| remote-records: Signers move the state to the remote; No signers, no change | `the_first_write_writes_the_home_its_policy_chose` |

The REMOVED requirement's code is gone. No `migrate`, `left_behind`,
`forget_left_behind`, `adopt_left_behind`, `leave_behind` or
`sluice-remote/moved/` remains (the `left_behind` in
`tests/profiles/places.rs` is a place's, unrelated), and `root:lint` passes.
