# Verify: a-queues-home-is-written-in-its-records

Verified on `change/a-queues-home-is-written-in-its-records` at `6ec031c0`,
cut from `bolt/sluice-is-refactored-and-runs-on-pools`, whose head
`48a9c3d5` it holds, on 2026-11-16 with sluice 0.93.2.

## Outcome

**Ready to archive.** The bolt's gate passes at `6ec031c`, and so do the
tools-tier targets that hold the tasks' tools tests. All 29 tasks are
ticked. Every test a task names exists where the task says and passed in
this stage. Every requirement and scenario of the four delta specs is
met by the build and shown by a test. `openspec validate --strict` finds
the change valid.

## Whole checks

`sluice exec bolt/sluice-is-refactored-and-runs-on-pools` ran once at
`6ec031c`. It ran the bolt's gate, `#check-fast` (S66), over the checks
`6ec031c` affects against the bolt's `48a9c3d`. It exited 0 with 12 tasks
passed and none failed, and sluice recorded the 12 results under their
keys for `6ec031c`:

| Task | Outcome |
|---|---|
| nix:typecheck, site:prose, site:rules | passed |
| root:adrs, root:changes (7 of 7), root:format, root:lint | passed |
| core:test (164), fakes:test (17), seams:test (5) | passed |
| conformance:test (every target, `homes`, `peers` and `reading` among them) | passed |
| cli:test (`10d4525f`) | passed |

`cli:test` ran every target of `cargo test -p sluice -- --skip sandbox`
with no failures: `host_process` 39 of 39, `cli` 310, `local_files`
among them with task 1.3's test. moon warned on each task that it could
not package outputs into `.moon/cache/outputs/` under the place, and
that it had no internet connection for its dependency install; neither
touched an outcome.

The tools tier is main's gate, not the bolt's, so `#check-fast` builds
no test with `--features tools`. Tasks 1.4 to 1.6, 2.4, 2.5, 3.2, 4.4,
4.6 and 4.7 name checks in that tier. Their four targets ran whole,
once, through sluice against the bolt: `sluice exec
bolt/sluice-is-refactored-and-runs-on-pools -- cargo test -p sluice
--features tools --test remote_refs --test remote_branch --test peers
--test pull_requests`. It exited 0:

| Target | Passed | Failed |
|---|---|---|
| peers | 13 | 0 |
| pull_requests | 157 | 0 |
| remote_branch | 34 | 0 |
| remote_refs | 37 | 0 |

Every test those tasks name is in these targets.

## Tasks

All 29 tasks in `tasks.md` are ticked, none open. Each test a task names
exists where the task says:

- 1.1: `sluice-core/src/records.rs`. 1.2: `sluice-fakes/src/lib.rs`.
  1.3: `tests/local_files.rs`.
- 1.4, 1.5, 2.4, 2.5: `tests/remote_refs.rs`. 1.6, 4.6:
  `tests/remote_branch.rs`.
- 2.1 to 2.3, 2.5's second half, 3.1: `sluice-conformance/tests/homes.rs`.
- 3.2, 4.7's first test: `tests/peers.rs`. 3.3, 5.5: `tests/cli.rs`.
- 4.1, 4.3: `sluice-conformance/tests/peers.rs`.
- 4.4, 4.7's second test: `tests/profiles/pull_requests.rs`.
- 4.5: `sluice/src/instance.rs` and `sluice-core/src/policy.rs`. 5.1:
  `sluice-conformance/tests/reading.rs`.

The tests that go with 1.4 and 2.6,
`the_first_hosts_queue_moves_and_the_others_is_left_behind` and
`a_submission_left_behind_is_submitted_again_naming_its_old_id`, are gone.
No `migrate`, `left_behind`, `forget_left_behind`, `adopt_left_behind`,
`leave_behind` or `sluice-remote/moved/` is left in the crates or the
site, but for a place's own `left_behind` in `tests/profiles/places.rs`,
which is not this change's.

Tasks 2.6, 5.2, 5.3, 5.4 and 5.7 hold by `root:lint`, `site:prose`,
`root:adrs`, `site:rules` and `root:changes`, each of which passed in the
gate. Record 0122 has its build amendment of 2026-11-14, and record
0116's routing half is marked replaced by it (5.3). Rules core.59 and
core.60 are in `docs/architecture/core.md`, and core.27's details read
with core.59 (5.4). `site/src/content/docs/reference/state.md` names
`home.json`, `carried_from`, `kept` and the log lines `moving`, `stayed`
and `carried`; `commands.md`'s `sluice status` names the home line (5.2).
Task 5.6's `moon.yml` reached the bolt through #86 and is this branch's.

Task 5.7 holds. The queue-home requirement "One queue in two homes is
told apart from a queue not yet moved" and design D2 refuse a branch
only where this host's own home is open while a live remote home came
from this host's own files. That is what `find_route` in
`crates/sluice/src/profiles/records.rs` does: an own home `open` is
refused only where a live remote home's `came_from` names `here` and
this host; otherwise it routes to this host's own files. Record 0122's
amendment and rule core.59 say the same. Task 3.1 names the lookup order
`Sluice::reach` in `crates/sluice-core/src/lib.rs` keeps: the pointers of
the closed homes this host reached the branch through first, then the
open home, then those closed homes' own live or finished records.

## Specs

`openspec validate a-queues-home-is-written-in-its-records --strict` finds
the change valid. The MODIFIED and REMOVED requirements of `peer-lead`
and `remote-records` name requirements `openspec/specs/` holds, and
`queue-home` is a new capability. Each scenario maps to a test that
passed in this stage:

| Spec: scenario | Test |
|---|---|
| queue-home: The first submission writes the home; A branch with no signers | `the_first_write_writes_the_home_its_policy_chose` (both cases) |
| queue-home: A policy that names another home does not move the reading | `a_branchs_home_is_read_from_its_records_not_its_policy` |
| queue-home: A host with no records of the branch | `a_host_with_no_records_of_a_branch_finds_its_home_on_the_remote` |
| queue-home: Two open homes | `two_open_homes_refuse_the_branch_naming_both`: origin's home came from this host; a read, a lock and a write of `bolt/x` refused naming both homes, `main` read as before |
| queue-home: a queue not yet moved, read in this host's own files and moved by its round | `a_status_of_a_carried_number_names_the_number_it_has_now`: host b's own home open beside origin's that a policy opened, b's step carrying its #52 |
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
