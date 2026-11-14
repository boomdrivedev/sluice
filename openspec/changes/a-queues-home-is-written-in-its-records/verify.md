# Verify: a-queues-home-is-written-in-its-records

Verified on `change/a-queues-home-is-written-in-its-records` at `350ab645`,
cut from `bolt/sluice-is-refactored-and-runs-on-pools` (merge base
`d6d11623`), on 2026-11-14 with sluice 0.93.2.

## Outcome

**Not ready to archive.** The bolt's gate fails: `cli:test` has 7 failing
tests in `crates/sluice/tests/cli.rs`. The tasks, specs and tests of the change
are otherwise in place.

## Whole checks

`sluice exec bolt/sluice-is-refactored-and-runs-on-pools`, the bolt's gate
`#check-fast` (S66), run once at `350ab645`. Its 12 results are recorded under
their keys for `350ab64`. It exited 1, with 11 tasks passed and 1 failed:

| Task | Outcome |
|---|---|
| site:prose, site:rules, nix:typecheck, root:adrs, root:changes | passed |
| root:format, root:lint | passed |
| core:test (164), fakes:test (17), seams:test (5) | passed |
| conformance:test (every target, `homes`, `peers` and `reading` among them) | passed |
| cli:test | **failed**: `--test cli` had 215 passed and 7 failed. Every other cli target passed, including `remote_refs`, `local_files`, `peers` and `pull_requests`. |

### The failures

**A. The new home line breaks the status tests that read lines by position
(task 5.1, commit `368abc11`).** `sluice status <branch>` now prints `its
queue is in this host's own files` as its second line. Five tests still
expect the line that followed the header, or the refused-writes block, at a
fixed position, and fail on the extra line:

- `status_lists_the_queue` (cli.rs:1394): `lines[1]` is the home line, where
  `  testing bolt/concurrent-units + #3 unit/forecast · …` is expected.
- `status_and_overview_show_a_hold_while_it_lasts` (cli.rs:7075).
- `status_shows_this_hosts_copy_behind_while_it_lasts` (cli.rs:7917).
- `status_shows_the_writes_its_protection_refused` (cli.rs:8032).
- `status_tells_a_local_branchs_refused_writes_what_to_do` (cli.rs:8088).

The line is the spec's ("A branch's status names its home and its move"),
so these tests need updating for it.

**B. A wait out of time no longer prints the reader's record (task 3.2,
commit `3a9d91e3`).** `Reading::read` now takes the reader document's entry
only where `sluice_core::home::same_work` holds. Otherwise it reads the record
again by number. Two tests still expect the entry from the reader's document,
and get the record read afresh from the records:

- `wait_out_of_time_prints_the_readers_record` (cli.rs:2952): expected #1
  `testing` from `unit/r` at `d46f5d3…`, as the reader's document holds it;
  got #1 `queued` from `unit/a` at `e543184…`.
- `a_follower_wait_out_of_time_on_a_failure` (cli.rs:3713): the same for #7.

In both tests, the reader's document (`one_testing`) gives the number to
different work than the submission the test made, and `same_work` now rejects
that entry. Two fixes would work: make these fixtures name the same work, or
narrow the filter so a reader's record of the same submission is still
followed (129, core.25). The code stage chooses between them. `same_work`
must still keep a wait off another submission under its number (core.48).

## Tasks

All 21 tasks in `tasks.md` are ticked. Each test a task names exists where it
says:

- 1.1 `records::tests::a_home_document_reads_as_written`,
  `a_branch_with_no_home_document_has_no_home` (sluice-core).
- 1.2 `a_fake_branch_is_read_in_the_home_its_records_name`,
  `a_fake_carry_moves_the_live_records_once` (sluice-fakes).
- 1.3 `a_branchs_home_is_read_and_written_with_its_records` (local_files.rs).
- 1.4 `a_branchs_home_is_read_from_its_records_not_its_policy`,
  `a_host_with_no_records_of_a_branch_finds_its_home_on_the_remote`,
  `two_open_homes_refuse_the_branch_naming_both` (remote_refs.rs).
- 1.5 `the_first_write_writes_the_home_its_policy_chose` (remote_refs.rs).
- 2.1 to 2.3, 2.5, 3.1: `homes.rs`, all eleven tests the tasks name.
- 2.4 `a_hosts_queue_moves_to_its_remote_with_its_numbers`,
  `a_number_the_remote_holds_is_carried_under_the_next`,
  `a_carry_stopped_after_the_remote_was_written_is_closed_by_the_next`
  (remote_refs.rs).
- 2.5 `a_branch_comes_home_each_host_taking_its_own_submissions`
  (remote_refs.rs).
- 2.6 `Records::migrate`, the records' `Moved`, the `sluice-remote/moved/`
  notes, `State::left_behind`, `forget_left_behind`, `adopt_left_behind`,
  the fakes' `leave_behind`, and the tests
  `a_submission_left_behind_is_submitted_again_naming_its_old_id` and
  `the_first_hosts_queue_moves_and_the_others_is_left_behind` are gone
  (`99f65425`). The `Moved` still in `pushed_refs.rs` and `local_refs.rs` is
  the branch seam's, and stays. `root:lint` passes.
- 3.2 `a_wait_follows_its_submission_to_its_queues_new_home` (peers.rs).
  It passes, but the same commit breaks failure B.
- 4.1, 4.3 `a_host_whose_signature_the_branch_does_not_accept_never_leads`,
  `a_host_with_an_unlisted_key_never_leads`,
  `a_lead_the_branch_no_longer_accepts_is_taken_after_the_lease`,
  `any_host_leads_a_remote_home_whose_policy_names_no_signer`,
  `a_lead_keeps_its_branch_through_a_run_longer_than_the_lease`,
  `a_lead_with_no_live_run_writes_nothing_to_keep_it` (conformance peers.rs).
- 4.2 the conformance suite passes whole.
- 4.4 `a_host_the_branch_does_not_accept_takes_no_request_in`
  (pull_requests.rs).
- 4.5 `no_host_setting_chooses_how_a_queue_runs` (instance.rs),
  `no_policy_key_chooses_how_a_queue_runs` (policy.rs).
- 5.1 `the_overview_names_each_branchs_home_and_its_move` (reading.rs). The
  CLI prints `its queue is …`, and `--json` gives `home`. This line causes
  failure A.
- 5.2 the site's state reference names `home.json`, `carried_from`, `kept`,
  `moving`, `stayed` and `carried`, and nothing names `sluice-remote/moved/`.
  `site:prose` passes.
- 5.3 record 0122 has its amendment of 2026-11-14, and record 0116 says that
  record 0122 replaces its routing half. `root:adrs` passes.
- 5.4 `docs/architecture/core.md` carries core.59 and core.60, and core.27's
  details point the records' home to core.59. `site:rules` passes.

## Specs

`openspec validate a-queues-home-is-written-in-its-records --strict`: valid.
Every MODIFIED and REMOVED requirement names a requirement that
`openspec/specs/peer-lead` and `openspec/specs/remote-records` hold, so the
archive will reconcile.

Each scenario has a test, and every one of those tests passes:

- **queue-home.** "The first submission writes the home" and "A branch with no
  signers" are covered by `the_first_write_writes_the_home_its_policy_chose`.
  "A policy that names another home does not move the reading", "A host with
  no records of the branch" and "Two open homes" are covered by 1.4's three
  tests. "A move waits for a combination under test" and "The policy names
  the open home again" are covered by 2.1's tests and
  `a_move_waits_for_the_combination_under_test`. "A host's queue moves to
  origin" is covered by
  `a_move_carries_every_live_submission_with_its_number_and_state`. "A submit
  racing the carry" and "Stopped after the new home was written" are covered
  by `homes.rs`, with the binding's carry in `remote_refs.rs`. "Two hosts'
  submissions come home" is covered in both `homes.rs` and `remote_refs.rs`.
  "A host that has not come back" is covered by
  `a_submission_left_for_its_host_says_it_waits_for_it`, and `sluice status
  <n>` prints `waiting for <host> to take it home` (lib.rs:2000). "Numbers
  past the remote's count" is covered by
  `a_hosts_queue_moves_to_its_remote_with_its_numbers`. "Two hosts each held
  #52" is covered by `a_status_of_a_carried_number_names_the_number_it_has_now`
  and `a_number_the_remote_holds_is_carried_under_the_next`. "A submission
  landed before the move" is covered by
  `a_submission_finished_before_the_move_is_read_in_the_closed_home`. The two
  "No setting names how a queue runs" scenarios are covered by 4.5, and "A
  move waiting" by 5.1.
- **submission-wait.** "The queue moves while a wait follows" and "Carried
  under another number" are covered by
  `a_wait_follows_its_submission_to_its_queues_new_home`.
- **peer-lead.** The takeover, lease, clock, no-longer-accepted,
  unaccepted-host, unlisted-key, no-signer, long-run and nothing-live
  scenarios are covered by the conformance `peers.rs` tests above, with intake
  in `pull_requests.rs`.
- **remote-records.** "Signers move the state to the remote" and "No signers,
  no change" are covered by
  `the_first_write_writes_the_home_its_policy_chose`. The removed requirement's
  tests are gone, as 2.6 says.

## What goes back to the code stage

Fix the 7 `cli:test` failures, A and B above, then run
`sluice exec bolt/sluice-is-refactored-and-runs-on-pools` again before this
change is verified and archived.
