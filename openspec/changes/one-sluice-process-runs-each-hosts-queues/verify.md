# Verify: one-sluice-process-runs-each-hosts-queues

The branch `change/one-sluice-process-runs-each-hosts-queues` was verified at
`d9ab4466` on 2026-11-14. It was cut from
`bolt/sluice-is-refactored-and-runs-on-pools` and holds that bolt at
`7ce112b4`, the bolt's revision when the gate ran.

Outcome: **archived.** The bolt's gate passes whole, `cli:test`'s
integration binaries among it, and main's tools tier passes whole in all
19 of its suites. The change validates strictly, all 120 tasks are
ticked, and every requirement and scenario of its 16 delta specs holds.
A trial archive in a scratch copy applied every delta cleanly, and all 85
specs and changes then validated strictly.

## The whole checks

The gate is `sluice exec bolt/sluice-is-refactored-and-runs-on-pools`
(sluice 0.93.2), the bolt's `#check-fast`. It ran once on `d9ab446` and
**passed** with exit 0 in 3m 19s.

```
running the checks d9ab446 affects against bolt/sluice-is-refactored-and-runs-on-pools (7ce112b)
...
Tasks: 15 completed
 Time: 3m 12s 553ms

passed: 15 tasks
recorded 1 result under its key for d9ab446
```

| Task | Outcome |
|---|---|
| `root:format` | passed |
| `root:lint` | passed |
| `root:adrs` | passed |
| `root:changes` | passed (7 items) |
| `root:versions` | passed |
| `nix:typecheck` | passed |
| `nix:test` | passed |
| `site:prose` | passed (0 errors, 0 warnings, 59 suggestions) |
| `site:rules` | passed |
| `core:test` | passed (161 tests) |
| `seams:test` | passed (5 tests) |
| `fakes:test` | passed (15 tests) |
| `conformance:test` | passed |
| `plugin-sluice:validate` | passed |
| `cli:test` | passed in 3m 9s: the library's 307, `cli`'s 222, `host_process`'s 39 in 67.8s, and every other integration binary |

moon warned for each task that it could not package the task's outputs
into the worktree's `.moon/cache/outputs`, and that it skipped the Rust
dependency install for want of a connection. Neither changed a task's
outcome.

`openspec validate one-sluice-process-runs-each-hosts-queues --strict`:
valid.

## The tools tier

`#check-tools` is main's gate, not the bolt's, run when the bolt merges
onto main. The tasks name its suites, so it was run once, whole, through
sluice against the bolt, as `cli:tools` runs it, and **passed** with exit 0
in 22m 3s:

```
sluice exec bolt/sluice-is-refactored-and-runs-on-pools -- cargo test --no-fail-fast -p sluice --features tools --test instance --test local_refs --test pushed_refs --test remote_refs --test peers --test forge --test pull_requests --test remote_branch --test moon --test places --test worktrunk --test protect --test conformance --test stack --test exec --test previews_tools --test equip_tools --test landed --test overview_watch -- --skip sandbox
conformance:    ok. 60 passed
equip_tools:    ok. 7 passed
exec:           ok. 17 passed
forge:          ok. 48 passed
instance:       ok. 12 passed
landed:         ok. 1 passed
local_refs:     ok. 17 passed
moon:           ok. 32 passed
overview_watch: ok. 10 passed
peers:          ok. 12 passed
places:         ok. 47 passed
previews_tools: ok. 10 passed
protect:        ok. 28 passed
pull_requests:  ok. 156 passed (745.76s)
pushed_refs:    ok. 53 passed
remote_branch:  ok. 34 passed
remote_refs:    ok. 30 passed
stack:          ok. 3 passed
worktrunk:      ok. 77 passed
exit=0
```

## The tests that failed under load

Three tests passed alone and failed under load at `c6553b10`. Each now
passes in the runs above:

- **6.24**: `host_process::tasks::tests::a_reading_under_way_when_told_is_read_again`
  passed in the gate's library run. `look_at` now marks the reading begun
  under the lock that shows the look under way, before its thread starts
  (`crates/sluice/src/host_process/tasks.rs:277`), so a `serve` can no
  longer fall between the two. The marks are cloned out of their lock
  before `looking` is asked, so the new order, `looking` then
  `readings`, meets no inverse anywhere.
- **7.11, 7.12**: `a_successor_that_does_not_start_leaves_the_host_process_as_it_was`,
  `the_host_process_keeps_its_repositories_across_a_new_binding` and
  `a_submit_during_a_hand_over_reaches_the_successor` passed in the gate's
  `host_process` run. The test now replaces the executable only once the
  run's check has marked that it began, gives the held run ten minutes,
  and reads a host process that ended once its queue emptied as the one
  host process.
- **4.14**: `the_host_process_ends_once_its_queues_are_empty` passed. It
  now reads the ended process's socket as gone, or as answered by a host
  process a later command started.
- **8.25**: `landed_on_another_host` passed in the tools tier.
  `took_nothing_in` now tells a submission by its id, branch, head,
  submitter and time, so a reading one write of B's older shows nothing A
  took in.

The code stage's repeated runs of these tests, 24 to 200 at a time,
were not repeated here.

## Smaller departures

These were confirmed in the code. None blocks the change.

- **A status after a change during a long pass waits out its bound.**
  Once a command sends `serve`, the reading under way is not current.
  A status then waits for a reading begun after it, up to `read_for()`,
  which is `retry::COMMAND_FOR` and `READ_TICK`, 33 seconds. After that
  it reads the records held and says when the repository was last read.
  "A status during a long round" answers at once, but its scenario has
  no command between.
- **forge-allowance's "A wait whose timeout comes first" names
  `sluice wait 77`** (`openspec/specs/forge-allowance/spec.md:79`).
  Under record 0093's decision 24, a wait on a submission meets no
  pause, so the scenario's WHEN never arises, though its THEN still
  holds: the wait exits 5 at its timeout having called GitHub nothing.
  Task 5.24's test waits on the pull request its worktree opened. The
  change does not amend the scenario.
- **The claim loop's moment with no part.** In
  `crates/sluice-core/src/step.rs:3570`, `update` runs its closure
  again on a refused write, and the second run gets
  `part.take() == None`.
- **The host process notes every repository it is asked about.**
  `host_process/mod.rs:971` notes a repository without reading whether
  it has sluice.
- **Stale words about the intake mark and the hand-over.** They remain
  in:
  - `lib.rs:966`, `:981` and `:997`
  - the empty `impl OnGitHub {}` at `pull_requests.rs:7263`

## Requirements and scenarios

There are 16 delta specs. They were read against the code and its tests,
and the results come from the runs above.

| Spec | Verdict |
|---|---|
| `host-process` | Met. `peers` passes whole, two hosts landing (4.13) and a signed record verified once (6.9) among it. "The queue empties" holds (4.14). |
| `host-driver` | Met. |
| `standing-executor` | Met. |
| `obsolete-combinations` | Met. |
| `run-signing` | Met. |
| `place-ports` | Met. `places`, `previews_tools` and `stack` pass whole, reading ports held by the port (1.5). |
| `run-bound` | Met, with the claim loop's moment. The keeper's cores pass (7.10). |
| `settings-take-up` | Met. "A successor that does not answer" and "A submit during the hand-over" pass in the gate (7.11, 7.12). |
| `submission-wait` | Met. A wait meets a pause only on the request it follows (5.24). |
| `request-intake-host` | Met. "Landed on another host" and "Blamed on another host" pass (8.25). A submit's request is held for the next submit, as the requirement's "one held for the next submit" reads. |
| `pull-request-intake` | Met. A round that stepped shows what it sent back with nothing asked of GitHub (8.24), and forge-reads' scenarios pass. |
| `remote-read-log` | Met. |
| `overview-reader` | Met. "A one-shot read reads what a command changed" passes in the gate (6.23, 6.24). The one-shot overview beside a current reading asks origin nothing. |
| `overview` | Met. |
| `forecast` | Met. `remote_branch` passes whole (6.19). |
| `reader-tick` | Met (5.25). |

The main specs the change does not amend, forge-reads,
pull-request-submit and request-intake-host among them, hold in the tools
tier.

## Tasks

All 120 tasks are ticked, and none is open. Every test and moon task a
task names passed in the gate or the tools tier above, among them the
last tasks' checks: 4.14, 6.24, 7.11, 7.12 and 8.25. Task 9.1's
`cargo clippy -p sluice --all-targets --features tools -- -D warnings`,
which `root:lint` does not cover since it builds without `tools`, was run
once and passed with no warning. Task 10.1's strict validation passes,
and so does 10.2's trial archive in a scratch copy.
