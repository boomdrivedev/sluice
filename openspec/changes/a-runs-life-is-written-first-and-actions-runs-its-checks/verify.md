# Verify: a-runs-life-is-written-first-and-actions-runs-its-checks

Verified on `change/a-runs-life-is-written-first-and-actions-runs-its-checks`
at `b21016d50`, against its base `bolt/sluice-is-refactored-and-runs-on-pools`
at `e56a904ba`, on 2026-11-16.

**Verdict: the work falls short.** The gate passes. All tasks are ticked,
and every test the tasks name exists. But six findings contradict the
change's specs, design or rules, and one test the gate leaves to main fails
by reading. The change goes back to its code stage.

The code stage archived the change before this stage ran (`85be5d214`), so
its delta specs are already merged into `openspec/specs/`. This stage
archives nothing further and undoes nothing. Fixing a finding that changes
a requirement's wording needs that archive undone, or a change of its own.

## The whole checks

`sluice exec bolt/sluice-is-refactored-and-runs-on-pools`, run once, exited
0. It ran the bolt's gate, `#check-fast`, as `.config/sluice.toml` names
it:

- 16 tasks passed: `site:rules`, `site:prose`, `plugin-sluice:validate`,
  `nix:typecheck`, `nix:test`, `root:adrs`, `root:changes`, `root:lint`,
  `root:format`, `root:specs`, `root:versions`, `seams:test`, `core:test`,
  `fakes:test`, `conformance:test` and `cli:test`.
- 1397 tests passed, none failed and none was ignored.
- sluice recorded the results under their keys for `b21016d`.
- `openspec validate --specs --strict` also passed: 83 items.

`#check-tools` and `#check-e2e` belong to main's gate and were not run.
By reading, one of their tests fails (finding 6 below).

## Tasks

Every task from 1.1 to 10.6 is ticked. Every test the tasks name is defined
where they say. The old names that tasks 2.2 and 6.5 replace are gone, as
those tasks ask.

## Requirements and scenarios

The 14 specs hold 49 requirements and 94 scenarios. Each scenario was read
against the code that implements it and against the test it names. Where
nothing is noted below, the scenario holds and its test asserts its THEN
clauses.

### Findings: behaviour contradicts the change

1. **A checkout is released while its run is still `claimed`.** Four paths
   release the checkout first and write the run's end afterwards:

   - `reopen_made` (`crates/sluice-core/src/step.rs:1090-1108`, also
     reached from `start_claimed`);
   - `start_run`'s start refused at the bound (`step.rs:1414-1421`);
   - `not_started` (`step.rs:1580-1605`);
   - `unmade` (`step.rs:1653-1680`).

   This breaks run-life's "Nothing acts on a run before the state naming
   the act is written", which says "its checkout released only once the run
   is written `ended`". It also breaks design D3, whose act table has a
   release named by `ended`, and which says "otherwise the run ends with no
   outcome … its checkout released after the end". And it breaks rule
   core.61. No test covers the order.

2. **A pool's dispatch sends an input `sluice.yml` does not declare.**
   `Actions::launch` sends `"mode": "run"` (`crates/sluice/src/profiles/github/actions.rs:99`).
   The template's `workflow_dispatch` declares only `claim`, `branch`,
   `combination`, `system` and `runner` (`workflows/sluice.yml:11-28`).
   GitHub refuses a dispatch that carries an input the workflow does not
   declare. So every live run on a pool would be refused, and ended as a
   refused dispatch.

   `wake` (`actions.rs:168`) has the same fault. Every submit to a signed
   branch with a pool calls it (`crates/sluice-core/src/ops.rs:703-715`).
   It sends `mode: round` and leaves out the required `combination` and
   `runner`.

   The fake pool accepts any input, and `forge.rs`'s `the_pools_calls`
   asserts that `mode` is sent. This falls under actions-pool's "A pool's
   run is claimed before its job is dispatched".

3. **On a branch whose gate is a tag, a key the tree does not bear out
   still lets the tag pass.** The job's end, `end_claimed`
   (`step.rs:1209-1220`), writes a given `#tag` `passed` whenever anything
   was signed. `unsigned_checks` reads a tag as borne out once every signed
   result counts (`step.rs:5396-5399`). So a key `sluice job sign` left out
   vanishes, and the candidate can pass and land without it.

   This contradicts actions-pool's "A key the tree does not bear out": "the
   check reads as left without a result". `job_sign_signs_only_what_the_tree_bears_out`
   asserts only the statement. The moon profile's real `proven_from` and
   `keep_proof` (`crates/sluice/src/profiles/checks.rs:2398`, `:2440`) have
   no test.

4. **`sluice push` lands withdrawn work.** `push_held` (`step.rs:842-886`)
   does not drop a held landing one of whose submissions has left. A
   refused landing is dropped only by a round's `drop_withdrawn`
   (`step.rs:2056-2075`), and `withdraw_in` does not drop it. Then
   `perform_landing`'s `Moved` arm (`step.rs:648-662`) writes every one of
   the landing's submissions `Landed`.

   So a `sluice push` between a withdrawal and the next round pushes the
   withdrawn work, and rewrites its withdrawn record as landed. That is
   against refused-landing's "A held landing's submissions are their
   submitters'" and rule core.48. No test covers it.

5. **A combination with no run running does not give each run's state as
   specified.** `overview::run_time` (`crates/sluice-core/src/overview.rs:368-386`)
   gives one run's state: the first claimed run, else the first open one.
   It chooses between `waiting for a host of <system>` and `waiting for
   room` by whether the run names a system. The spec chooses by whether a
   live executor may take the run.

   So a run with a system that waits for room on a live host of that system
   reads `waiting for a host of <system>`. A run named on no system that
   every host turned down reads `waiting for room`. This breaks
   run-progress's "A combination with no run running reads each run's
   state" and run-life's "A status reads each run's state".
   `a_run_waiting_for_room_says_since_when` passes only because its run
   names no system.

6. **A test of main's gate fails, by reading.** `pull_requests_need_records_on_the_remote`
   (`crates/sluice/tests/profiles/pull_requests.rs:5461-5476`) expects the
   refusal's old words, ending "so its state is kept on origin (137)".
   `c665630b3` changed `NoIntake`'s words (`crates/sluice-core/src/policy.rs:183-192`)
   to end "kept on origin, as `sluice init actions` names a pool's signer
   (137)". The test runs only in `cli:tools` (`#check-tools`), so the
   bolt's gate does not see it. It is the one command-line test of
   pull-request-intake's "Pull requests need records on the remote" and
   "Status shows the refusal".

### Findings: the code departs from a scenario's words

7. **The log line's name.** refused-landing's "Refused every round" names
   the line `landing-refused`, and so does the merged
   `openspec/specs/refused-landing/spec.md`. The code writes
   `forge_refused`, as task 6.2, the state reference and record 0124 do.

8. **`sluice status <n>` for a refused submission.** refused-landing's "The
   submitter's notice" asks for `held: the forge refused its landing:` with
   the forge's words. The submission's row gives the phrase with no words
   (`crates/sluice/src/lib.rs:6241`). The words appear only in the branch's
   held line (`lib.rs:2633`). No test runs the status.

9. **A submission behind a held landing.** refused-landing's "The
   submissions behind it" asks that its status say it waits behind the
   landing the forge refused. Its row prints `after` and nothing more
   (`lib.rs:6239`), and no test reads it.

10. **A wait's document on a refused landing.** submission-wait's "A
    landing the forge refused" asks for the submission's record with the
    landing's refusal. The document is the bare record, which carries no
    refusal (`lib.rs:4260`; `owed.rs:572-577`), and the test asserts only
    `state`. The refusal arriving during the wait (`lib.rs:3993`) and exit
    6 for a waiting submit are untested.

11. **`sluice init actions`.** actions-pool's "sluice init actions names
    the pool and its signer" names `sluice.yml` "on the branch it runs
    from". `signer_ref` reads the remote's default branch instead
    (`crates/sluice/src/init.rs:108`). A system with no GitHub-hosted
    runner is dropped with nothing said (`init.rs:80-85`). Where the policy
    has no `[systems]`, a `[systems]` table is appended, which the spec
    does not name.

    `init_actions_writes_both_workflows_and_the_pool` uses a policy with no
    systems, not the scenario's `linux-x64` and `macos-arm64`. It asserts
    only that a `forge` signer exists, not its `github = { workflow, ref }`.

12. **A cancelled workflow run.** The scenario says the run ends "saying
    its job ended with no record". The run is written `<claim>'s claim
    lapsed`. `a_pools_job_ended_with_no_record_is_a_lapsed_claim` asserts
    that why. It does not assert that nothing was recorded or that nobody
    was blamed. Also, the template's sign job runs `if: always()`, so on a
    plain cancel the sign job records the run with no outcome, and the
    lapse path is not taken.

13. **A bundle that does not verify.** run-signing's "A pull request's
    workflow" says the log names the identity the bundle carried. That
    identity goes only to a tracing warning
    (`crates/sluice/src/profiles/remote_refs.rs:587`), not to the branch's
    log, and no test asserts it.

### Test gaps where the code holds

- run-life's "A late report":
  `s20_a_lapsed_claim_runs_again_elsewhere_and_its_late_report_is_not_written`
  (`crates/sluice-conformance/tests/peers.rs:299`) checks only the claims
  and the landing once b returns. It does not check that run 0's end is
  still the lead's, or that b recorded nothing.
- host-requirements' two scenarios: no test reads the submission's status
  line "no host that could run its tests as its policy asks took them"
  (`crates/sluice/src/progress.rs:61-79`). The tests read only the log.
- run-outcome's "A test running when the limit came": that its log is kept
  is not asserted.
- run-claims' "A long run keeps its claim": `peers.rs:399` covers 10
  minutes and does not reach the report.
- pull-request-intake's S40, "Passed before it was ready": `peers.rs:1326`
  uses one system and no submission ahead, and writes the head's results
  directly. `sluice job record --head` (`crates/sluice/src/job.rs:207-235`)
  has no test.
- run-signing's "A forge job's result", "A sign job on main" and "A
  private repository's bundle": no test verifies a sluice run statement
  signed keylessly through the records and the gate. The bundles tested
  are third-party ones, read by `keyless::verify` alone. Record 0119's
  amendment and `a14bd59f9` leave that bundle, and the live trial, to a
  trial of their own once a release holds `sluice job`.
- refused-landing: `leased`'s branch-holds-the-landing case is untested.
  When `sluice push` is refused again, the submitter's delivery keeps the
  first words.

### A carried departure, older than this change

pull-request-intake's "Status shows the refusal" asks that `sluice overview`
name a refused main. The scenario stands unchanged from before this change.
Record 0125's choice, which the test's own comment cites, is that the
overview lists nothing of a main with no records. The statement or the
scenario should be reconciled with record 0125.

## What goes back

Findings 1 to 6 are for the code stage to fix, each with a test that
asserts the scenario. Findings 7 to 13 need either the code or the
scenario's words to change. Where the words change, the change carries it,
as CLAUDE.md's Specs asks.
