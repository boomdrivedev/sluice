# Verify: a-queues-home-is-written-in-its-records

Verified on `change/a-queues-home-is-written-in-its-records` at `661118f0`,
cut from `bolt/sluice-is-refactored-and-runs-on-pools` (merge base
`d6d11623`; the bolt has since gained only `6558679a`, a policy key, and its
landing), on 2026-11-14 with sluice 0.93.2.

## Outcome

**Not ready to archive.** The bolt's gate passes whole. But the change breaks
four tests of the tools tier, `cli:tools`, which `#check-tools` (main's gate)
runs when the bolt merges onto main. All four pass at the merge base, and all
four fail again when rerun once at `661118f0`, so the change causes them;
they are not flakes. Every task is ticked. Every scenario's test passes.

## Whole checks

`sluice exec bolt/sluice-is-refactored-and-runs-on-pools`, the bolt's gate
`#check-fast` (S66), run once at `661118f0`. It exited 0 with 12 tasks passed
and recorded 12 results under their keys for `661118f`:

| Task | Outcome |
|---|---|
| site:prose, site:rules, nix:typecheck, root:adrs, root:changes | passed |
| root:format, root:lint | passed |
| core:test, fakes:test, seams:test | passed |
| conformance:test (`homes`, `peers`, `reading` and every other target) | passed |
| cli:test (`--test cli`: 223 passed, 0 failed) | passed |

moon warned on each task that it could not package task outputs into
`.moon/cache/outputs/` under the place. Each pass was recorded all the same,
and nothing in this change touches that.

## The tools tier

`#check-fast` runs no test built with `--features tools`. Its `remote_refs`,
`peers` and `pull_requests` targets ran zero tests. But the checks of tasks
1.4, 1.5, 2.4, 2.5, 3.2 and 4.4 are tests of that tier. So they were run
through sluice against the bolt: first `--test remote_refs --test peers
--test pull_requests`, then, since cargo stops at the first binary that
fails, every other target of `cli:tools` with `--no-fail-fast`.

Every test a task names passed there:

- `remote_refs`: 37 passed. These include 1.4's three tests, 1.5's, 2.4's
  three and 2.5's.
- `peers`: `a_wait_follows_its_submission_to_its_queues_new_home` (3.2)
  passed.
- `pull_requests`: `a_host_the_branch_does_not_accept_takes_no_request_in`
  (4.4) passed.
- Every other target of the tier passed whole: `instance`, `local_refs`,
  `pushed_refs`, `forge`, `moon`, `places`, `worktrunk`, `protect`,
  `conformance`, `stack`, `exec`, `previews_tools`, `equip_tools`, `landed`
  and `overview_watch`.

Four tests failed. Each passes at `d6d11623` in a worktree of that commit
and fails again at `661118f0`, both run once through sluice:

### A. Tests whose leading host D8 no longer accepts

The code is as design D8 has it. These tests still expect an unaccepted host
to lead or take requests in, as the conformance tests 4.2 rewrote did:

- **`peers.rs` `a_run_is_claimed_and_run_by_the_other_host`**
  (crates/sluice/tests/peers.rs:213). Host `a` signs as `a` under
  `trust = ["b"]`. `Sluice::keeps` (crates/sluice-core/src/step.rs:353) now
  reads `a` as not accepted, so `a`'s `sluice step main` opens no
  combination. The assertion that `refs/sluice/combinations/main/` exists
  fails. The test means to show that a run is claimed and run by the host
  that does not lead (75, 82, 78). 4.2's way would keep that: a lead the
  branch accepts that has no room.
- **`pull_requests.rs` `the_keeper_and_the_check_name_different_hosts`**
  (crates/sluice/tests/profiles/pull_requests.rs:6600). Host `c`, made by
  `g.other("c", "", "")`, has no `[signing]` table, so D8 and 4.4 have it
  take none of `main`'s requests in. The test still expects
  `taken_in_by: {host: "c"}`. It panics first, in `follow_due`
  (pull_requests.rs:6109): `sluice/forge/follows/o/r/main.json` does not
  exist (`No such file or directory`), so host A never wrote a follow of the
  request.

### B. Behaviour the change regressed

- **`remote_branch.rs` `an_idle_branch_on_a_remote_is_passed_over_with_no_git`**
  (crates/sluice/tests/remote_branch.rs:2119). This breaks the reader-tick
  rule (129, core.31). With nothing queued, `sluice run --rounds 6` starts 8
  git processes where `--rounds 1` starts 3. Each pass after the first runs
  another `fetch … origin +refs/sluice/hosts/*:refs/sluice-fetched/origin/hosts/*`.
  An idle branch on a remote now fetches every pass.
- **`remote_branch.rs` `a_policy_refused_on_the_remote_is_cleared_by_a_push`**
  (crates/sluice/tests/remote_branch.rs:1486). This breaks 124 and 56
  (record 0069). Origin's `bolt/x` and this host's own copy both hold a
  refused `[[checkout.cache]]` policy. The round is refused as it should be,
  naming the branch, the revision and the table, and it runs nothing. But
  afterwards this host's own `bolt/x` is no longer at the refused revision,
  so the round moved something it must not.

## Tasks

All 23 tasks in `tasks.md` are ticked, 3.3 and 5.5 among them, and each test
a task names exists where the task says. Each one passed in the run that
holds it:

- In `#check-fast`, all 33 tests of 1.1 to 1.3, 2.1 to 2.3, 2.5's
  conformance half, 3.1, 3.3, 4.1, 4.3, 4.5, 5.1 and 5.5.
- In the tools run, all 10 tests of 1.4, 1.5, 2.4, 2.5, 3.2 and 4.4.

The left-behind design 2.6 removes is gone: no `migrate`, `left_behind`,
`forget_left_behind`, `adopt_left_behind`, `leave_behind` or
`sluice-remote/moved/` remains (the `left_behind` in
`tests/profiles/places.rs` is a place's, unrelated), and `root:lint` passes.
5.2 to 5.4 hold, by `site:prose`, `root:adrs` and `site:rules`.

## Specs

`openspec validate a-queues-home-is-written-in-its-records --strict`: valid.
Each scenario of `queue-home`, `submission-wait`, `peer-lead` and
`remote-records` maps to the tests its tasks name, and every one of those
tests passes. That includes "A move waiting", which the status line `its
queue is …` (crates/sluice/src/lib.rs:6112) and `--json`'s `home` give. The
four failing tests above belong to no scenario of this change. They are
older behaviour the change was to keep (A's, by rewriting them as 4.2 did
the conformance tests; B's, by leaving it as it was).

## What goes back to the code stage

Make the four tests pass, then run
`sluice exec bolt/sluice-is-refactored-and-runs-on-pools` again, together
with the tools targets
`cargo test -p sluice --features tools --test peers --test pull_requests --test remote_branch`
run through sluice against the bolt, before this change is verified and
archived:

- Rewrite A's two tests for D8, each with a lead or an intake host the
  branch accepts, keeping what they show.
- Fix B's two regressions in the code: an idle branch on a remote fetches
  nothing after its first pass, and a round refused on the policy moves
  neither the remote's branch nor this host's own copy.
