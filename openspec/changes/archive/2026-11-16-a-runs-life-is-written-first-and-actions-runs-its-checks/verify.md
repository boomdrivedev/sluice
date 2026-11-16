# Verify: a-runs-life-is-written-first-and-actions-runs-its-checks

Verified on `change/a-runs-life-is-written-first-and-actions-runs-its-checks`
at `8fa768bca`, against its base `bolt/sluice-is-refactored-and-runs-on-pools`
at `e56a904ba`, on 2026-11-16. This is the second verify. The first, at
`b21016d50`, found the work short (`6d1626aa2`), and the code stage fixed
each of its findings in `99262efee` to `8fa768bca`.

**Verdict: the work holds.** The gate passes, every task is ticked, and
every finding of the first verify is fixed in the code or, where the
code was right, in the change's words. The change is archived with this
report.

## The whole checks

`sluice exec bolt/sluice-is-refactored-and-runs-on-pools`, run once,
exited 0. It ran the bolt's gate, `#check-fast`, as `.config/sluice.toml`
names it, over the checks `8fa768b` affects:

- 15 tasks passed: `site:rules`, `site:prose`, `plugin-sluice:validate`,
  `nix:typecheck`, `nix:test`, `root:adrs`, `root:changes`, `root:lint`,
  `root:format`, `root:versions`, `seams:test`, `core:test`,
  `fakes:test`, `conformance:test` and `cli:test`. `root:specs` was not
  among them: `openspec/specs/` is as it stands at the base.
- 1404 tests passed, none failed and none was ignored. `site:prose` gave
  suggestions only.
- sluice recorded the 15 results under their keys for `8fa768b`.

`#check-tools` and `#check-e2e` are main's, run when the bolt merges onto
main. The tests of theirs that the tasks name as their checks were run
once through `sluice exec bolt/sluice-is-refactored-and-runs-on-pools --
cargo test -p sluice --features tools`, and all 18 passed: `pushed_refs`'
three of task 6.1, `remote_branch`'s three of tasks 6.5, 6.6 and 6.8,
`forge.rs`' `the_pools_calls`, its four `a_bundle_` tests and its two
`forge_check_` tests (tasks 7.1, 7.3, 8.1, 8.3, 9.6), `places.rs`' three
`locked` tests (task 5.1), `pull_requests_need_records_on_the_remote`
(task 9.9) and `moon.rs`' `a_run_jobs_kept_proof_proves_its_keys_and_no_forged_one`
(task 9.8).

## Tasks

Every task from 1.1 to 10.7 is ticked, the fixes' tasks 2.5, 4.5, 6.7,
6.8, 7.3, 7.4, 8.3, 9.8, 9.9, 9.10 and 10.7 among them. Every test the
tasks and the delta specs name is defined where they say. The names tasks
2.2 and 6.5 replace are gone, as those tasks ask.

## The change in the tree

The archive the code stage made before the first verify is undone
(`7ee6ba199`): `openspec/specs/` is as it stands at the base, and the
change's delta specs carry the words its fixes changed. `actions-pool` is
archived with `--skip-specs` (task 10.6), and `openspec list` names it
nowhere. `openspec validate a-runs-life-is-written-first-and-actions-runs-its-checks --strict`
passes.

## Requirements and scenarios

The 14 delta specs were read against the code and the tests they name
in the first verify. This verify reads again each place that report
found short, and each requirement and scenario whose words the fixes
changed (actions-pool, refused-landing, run-progress, run-signing and
submission-wait). Each holds.

### The first verify's findings

1. **A checkout released before its run's end** is fixed. `reopen_made`,
   a start refused at the bound, `not_started` and `unmade` now write the
   run `ended` with its checkout still named, then release it through
   `release_ended`, which writes `closed` once the release frees it and
   otherwise leaves the run to `perform_closes` (`step.rs:1105-1146`,
   `1626-1773`). `a_checkout_is_released_only_once_its_run_is_written_ended`
   stops each of the three cases right after the release and reads the
   run `ended`, then `closed` and released once. run-life's "Nothing acts
   on a run before the state naming the act is written", design D3 and
   rule core.61 hold.
2. **A pool's dispatch** sends `claim`, `branch`, `combination`, `system`
   and `runner` alone (`actions.rs`, `run_inputs`), and a wake dispatches
   nothing. `a_dispatch_sends_the_inputs_sluice_yml_declares` reads the
   inputs from the rendered `sluice.yml`, and `the_pools_calls` refuses an
   undeclared input with GitHub's 422.
3. **A key the tree does not bear out** under a tag gate is left without
   a result: `sluice job record --from` judges the report again (`kept`,
   shared with `sluice job sign`), and `end_claimed` writes each left-out
   check with no outcome beside the tag, so the run is undecided and its
   follow-up holds the check.
   `job_record_leaves_a_key_the_tree_does_not_bear_out_without_a_result`
   runs it on `#gate`, and `a_run_jobs_kept_proof_proves_its_keys_and_no_forged_one`
   tests the moon profile's `keep_proof` and `proven_from` on a real run.
   The template's sign job passes `--from`.
4. **`sluice push` after a withdrawal** lands nothing withdrawn.
   `Line::drop_left_landing` drops a held landing in the write that takes
   one of its submissions out (`withdraw_in`, `admit`, `drop_withdrawn`),
   and `push_held` drops one left over, pushing nothing
   (`PushedHeld::Dropped`). `a_push_after_a_withdrawal_lands_nothing_withdrawn`
   and `a_push_drops_a_held_landing_one_of_whose_submissions_left` assert
   it, and refused-landing's new scenario "A push after a withdrawal"
   names them.
5. **Each run's state** is read by whether a live executor may take it:
   the lead's `poll` marks an open run `no_executor`, writing only a
   change, and `overview::run_time` gives each run's state in order by that
   mark. `a_waiting_run_reads_whether_a_live_executor_may_take_it` takes
   the one host of `macos-arm64` live, lapsed and back. A run naming no
   system that no executor may take reads `waiting for a host since
   <time>`, there being no system to name.
6. **`pull_requests_need_records_on_the_remote`** expects the refusal's
   words as `NoIntake` gives them (`policy.rs:183-192`), character for
   character.
7. **The log line** is `forge_refused` in the refused-landing spec, as in
   the code, the state reference and record 0124.
8. **`sluice status <n>`** of a refused submission gives `held: the forge
   refused its landing:` with the forge's words, in its row and its
   report (`landing_refusal`, from the landing or its `forge_refused`
   line).
9. **A submission behind a held landing** reads `waits behind the landing
   the forge refused of #1 …` in its row.
10. **A wait's document** on a refused submission carries `refused`, the
    forge's `words` and `at`. `a_landing_the_forge_refused_is_told_with_its_words`
    runs a waiting `sluice submit` refused while it waits, exit 6 with the
    words, the wait's document and both rows; `a_wait_on_a_landing_the_forge_refused_exits_6`
    reads `refused.words`.
11. **`sluice init actions`** names each system GitHub hosts no runner for
    and the `[systems]` it appends, and the actions-pool spec and D14 now
    name the signer's ref as the remote's default branch, else `main`.
    `init_actions_writes_both_workflows_and_the_pool` runs the scenario's
    policy, `linux-x64`, `macos-arm64` and `riscv`, and reads the signer's
    `github = { workflow, ref }` and the pool's runners.
12. **A cancelled workflow run**: the sign job runs `if: ${{ !cancelled()
    && github.event_name == 'workflow_dispatch' }}`, so a cancel skips it,
    and the lead's `lapse` ends the run saying `<claim>'s job ended with no
    record of its run`. `a_pools_job_ended_with_no_record_is_a_lapsed_claim`
    reads the why, no result, nobody blamed and the follow-up claimed again.
13. **A bundle that does not verify**: run-signing's scenario and D13 now
    say the reading process's warning names the identity its certificate
    carried, which `verify`'s error does (`remote_refs.rs:584-593`), and
    `a_bundle_from_a_pull_requests_ref_counts_for_nothing` asserts the
    identity in it.

### Test gaps where the code holds

None of these bears on a scenario's THEN; each stands from the first
verify, and none is a task's.

- run-life's "A late report": `s20_a_lapsed_claim_runs_again_elsewhere_and_its_late_report_is_not_written`
  checks the claims and the landing, not run 0's end or that b recorded
  nothing.
- host-requirements' two scenarios: `engine.rs` reads the why from the
  records, not the status line `progress.rs` prints from it.
- run-outcome's "A test running when the limit came": that its log is
  kept is not asserted.
- run-claims' "A long run keeps its claim": `peers.rs` covers 10 minutes
  and does not reach the report.
- pull-request-intake's "Passed before it was ready": `sluice job record
  --head` has no command-line test.
- run-signing's "A forge job's result", "A sign job on main" and "A
  private repository's bundle": no sluice run statement signed keylessly
  is read through the records and the gate; record 0119's amendment
  leaves that bundle, and the live trial, to a trial of their own once a
  release holds `sluice job`.

### A departure older than this change

pull-request-intake's "Status shows the refusal" asks that `sluice
overview` name a refused main; record 0125 has the overview list nothing
of a main with no records. The scenario is not this change's, and
neither its words nor the code moved here. The statement or the scenario
is to be reconciled with record 0125 by a change of its own.
