# Verify: a-queues-home-is-written-in-its-records

Verified on `change/a-queues-home-is-written-in-its-records` at `bbc6a183`,
cut from `bolt/sluice-is-refactored-and-runs-on-pools` (merge base
`d6d11623`; the bolt's head `74deef00` adds only `6558679a`, a policy key,
and its landing), on 2026-11-15 with sluice 0.93.2 and moon 2.6.1.

## Outcome

**Not ready to archive.** The bolt's gate fails on one test of `cli:test`,
`host_process.rs`'s `a_state_folder_made_again_ends_the_host_process`, and
cargo stopped there, so the `cli:test` targets after it did not run. Every
other task of the gate passes. Every one of the 28 tasks is ticked, and
each test a task names exists where the task says. Every scenario of the
four delta specs maps to a test, and `openspec validate --strict` finds
the change valid. Task 5.6, the one task whose check the gate cannot run,
holds at this commit.

## Whole checks

`sluice exec bolt/sluice-is-refactored-and-runs-on-pools` ran the bolt's
gate, `#check-fast` (S66), once at `bbc6a183`. The root `moon.yml` changed,
so every project was affected: 30 tasks ran. The command exited 1 with 29
tasks passed and `cli:test` failed, and recorded 30 results under their keys
for `bbc6a18`.

| Task | Outcome |
|---|---|
| site:prose, site:rules, nix:lint, nix:test, nix:typecheck | passed |
| root:adrs, root:changes, root:format (`a11560d7`), root:lint, root:marketplace, root:openspec-claude, root:release, root:specs, root:toolchain, root:versions | passed |
| core:test (164), fakes:test (17), seams:test (5) | passed |
| conformance:test (every target, `homes`, `peers` and `reading` among them) | passed |
| plugin-board:engine-types, plugin-board:test, plugin-board:typecheck, plugin-board:validate | passed |
| plugin-herdr:manifest, plugin-herdr:test-sh (138, 3 skipped), plugin-herdr:test-dash (138, 3 skipped) | passed |
| plugin-sluice:hooks, plugin-sluice:lay-out, plugin-sluice:validate | passed |
| **cli:test** (`b2e63f13`) | **failed** |

moon warned on each task that it could not package task outputs into
`.moon/cache/outputs/` under the place. It recorded each result all the
same.

### The failure

`cargo test -p sluice -- --skip sandbox` ran the crate's unit tests (all
passed), `checks`, `cli` (223 passed, 0 failed), `conformance`, `equip`,
`equip_tools`, `exec` and `forge`, then `host_process`, where 38 passed and
1 failed:

```
---- a_state_folder_made_again_ends_the_host_process stdout ----
thread 'a_state_folder_made_again_ends_the_host_process' panicked at crates/sluice/tests/host_process.rs:468:5:
it wrote into the new folder: ["start.lock", "repositories"]
test result: FAILED. 38 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 87.90s
error: test failed, to rerun pass `-p sluice --test host_process`
```

The test (135, design D6 of the host process's build) moves the host
process's state folder aside and moves a fresh empty folder into its path,
then waits up to five seconds for the host process to end and asserts it
left the fresh folder empty. Here something wrote `start.lock` and the index
`repositories` into it. Those are the host process index's start lock and
its list (`crates/sluice/src/host_process/index.rs`), written by a host
process's tick or by a client reaching for or starting one. The test's own
`follow`, a wait's follower, is still held as the process ends.

This change does not touch that test or `crates/sluice/src/host_process/`.
Its one edit to `crates/sluice/src/lib.rs` is to status by number and to
the wait's `Reading::read`, neither of which writes the host's state folder.
The crates are byte for byte as they were at `9be7d303`, whose gate ran
`cli:test` whole and passed. So this reads as a race in the test, or in the
host process's end, that shows under the host's load: the gate ran
`cli:test` beside both plugin-herdr suites for about eight minutes. It is
still a failure of the gate. Whatever its cause, it is the code stage's to
fix or to account for, and this stage ran the gate only once.

Because cargo stops at the first failing target, these targets of
`cli:test` did not run in this gate: `instance`, `landed`, `local_files`
(task 1.3's `a_branchs_home_is_read_and_written_with_its_records`),
`local_refs`, `moon`, `moon_telemetry`, `overview_watch`, `peers`,
`places`, `plugin_docs`, `previews`, `previews_tools`, `protect`,
`pull_requests`, `pushed_refs`, `reader_tick`, `remote_branch`,
`remote_refs`, `runs`, `sessions`, `stack`, `telemetry`, `telemetry_logs`,
`usage` and `worktrunk`. Those built without `--features tools` run no test
of the tools tier.

## The tools tier

`#check-fast` runs no test built with `--features tools`, but the checks of
tasks 1.4 to 1.6, 2.4, 2.5, 3.2, 4.4, 4.6 and 4.7 are tests of that tier.
This stage did not run it. Between `9be7d303` and this commit, only the
root `moon.yml` (task 5.6) and this change's `tasks.md` and report differ.
At `9be7d303`, `cli:tools`' own command ran through sluice against the bolt
with `--no-fail-fast` and passed whole, 664 tests and none failed, each test
those tasks name among them.

## Tasks

All 28 tasks in `tasks.md` are ticked, and each test a task names exists
where the task says:

- 1.1: `records.rs`. 1.2: `sluice-fakes/src/lib.rs`. 1.3: `tests/local_files.rs`.
- 1.4, 1.5, 2.4, 2.5: `tests/remote_refs.rs`. 1.6, 4.6: `tests/remote_branch.rs`.
- 2.1 to 2.3, 2.5's second half, 3.1: `sluice-conformance/tests/homes.rs`.
- 3.2, 4.7's first test: `tests/peers.rs`. 3.3, 5.5: `tests/cli.rs`.
- 4.1, 4.3: `sluice-conformance/tests/peers.rs`.
- 4.4, 4.7's second test: `tests/profiles/pull_requests.rs`.
- 4.5: `instance.rs` and `policy.rs`. 5.1: `sluice-conformance/tests/reading.rs`.

In this gate, the tests of 1.1, 1.2, 2.1 to 2.3, 2.5's conformance half,
3.1, 3.3, 4.1, 4.2, 4.3, 4.5, 5.1 and 5.5, and 4.6's `--test homes
--test peers`, passed. 1.3's test did not run (see above). 2.6, 5.2, 5.3
and 5.4 hold by `root:lint`, `site:prose`, `root:adrs` and `site:rules`,
each passed. No `migrate`, `left_behind`, `forget_left_behind`,
`adopt_left_behind`, `leave_behind` or `sluice-remote/moved/` remains in
the crates or the site. The `left_behind` in `tests/profiles/places.rs` is a
place's and unrelated.

### Task 5.6

`root:format`'s inputs leave out each path `.gitignore` names and
`.ruff_cache/`. `.gitignore`'s unanchored entries (`.devenv/`,
`.moon/cache/`, `.devenv.flake.nix`, `devenv.local.nix`,
`.pre-commit-config.yaml`) are left out only at the root, which is the only
place in a place's tree where they exist. In this gate, `root:format`
passed under sluice against the bolt in this place under key `a11560d7`.
The place holds `.claude/settings.json`, `.mcp.json`, `.devenv/`,
`.moon/cache/`, `.ruff_cache/` and `target/`. A sandboxed run keeps no
manifest, so `moon run root:format` ran once in the place's shell and gave
the same key, `a11560d7`. Its manifest in the repository's moon store hashes
to the key, names `root:format`, and names 1441 inputs, each one a file of
`bbc6a183`'s tree with that file's content. That is the reading the
signer's `check_key` makes.

## Specs

`openspec validate a-queues-home-is-written-in-its-records --strict` finds
the change valid. The MODIFIED and REMOVED requirements of `peer-lead` and
`remote-records` name requirements `openspec/specs/` holds, and `queue-home`
is a new capability. Each scenario maps to a test:

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

The conformance tests in this table passed in this gate. The tests in
`remote_refs.rs`, `peers.rs` and `pull_requests.rs` are tools-tier tests,
covered as described under "The tools tier".
