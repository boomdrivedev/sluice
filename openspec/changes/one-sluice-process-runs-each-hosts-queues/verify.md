# Verify: one-sluice-process-runs-each-hosts-queues

The branch `change/one-sluice-process-runs-each-hosts-queues` was verified at
`c6553b10` on 2026-11-14. It was cut from
`bolt/sluice-is-refactored-and-runs-on-pools` and holds that bolt at
`7ce112b4`, the bolt's revision when the gate ran.

Outcome: **not archived.** The bolt's gate **failed**: `cli:test` failed
on `host_process::tasks::tests::a_reading_under_way_when_told_is_read_again`,
task 6.23's own test. That library failure stopped `cargo test`, so
`cli:test`'s integration binaries never ran in the gate. Run once
afterwards, they fail `host_process.rs`'s
`a_successor_that_does_not_start_leaves_the_host_process_as_it_was`,
task 7.6's check. Main's tools tier, run once, fails
`pull_requests.rs`'s `landed_on_another_host`, which reads task 8.21's
`took_nothing_in`.

Each of the three passed when run again alone, by name. All three
depend on timing, and the gate needs them to pass every time. The
change goes back to its code stage.

The change validates strictly and all 115 tasks are ticked. Apart from
those three tests, every task's named check passes. Main's tools tier
passes whole in 18 of its 19 suites, among them every test the last
tasks (1.5, 5.26, 6.22, 8.24) name.

## The whole checks

The gate is `sluice exec bolt/sluice-is-refactored-and-runs-on-pools`
(sluice 0.93.2), the bolt's `#check-fast`. It ran once on `c6553b1` and
**failed** with exit 1.

```
running the checks c6553b1 affects against bolt/sluice-is-refactored-and-runs-on-pools (7ce112b)
...
Tasks: 14 completed, 1 failed
 Time: 26s 554ms

failed: cli:test
recorded 15 results under their keys for c6553b1
```

| Task | Outcome |
|---|---|
| `root:format` | passed |
| `root:lint` | passed |
| `root:adrs` | passed |
| `root:changes` | passed |
| `root:versions` | passed |
| `nix:typecheck` | passed |
| `nix:test` | passed |
| `site:prose` | passed (suggestions only) |
| `site:rules` | passed |
| `core:test` | passed (161 tests) |
| `seams:test` | passed (5 tests) |
| `fakes:test` | passed (15 tests) |
| `conformance:test` | passed |
| `plugin-sluice:validate` | passed |
| `cli:test` | **failed**: the library's 306 passed and 1 failed. No integration binary ran. |

The library failure:

```
---- host_process::tasks::tests::a_reading_under_way_when_told_is_read_again stdout ----
thread '...' panicked at crates/sluice/src/host_process/tasks.rs:1309:9:
assertion `left == right` failed: looked again once told
  left: 1
 right: 2
```

moon warned for each task that it could not package the task's outputs
into the worktree's `.moon/cache/outputs`.

`openspec validate one-sluice-process-runs-each-hosts-queues --strict`:
valid.

### What the gate did not reach

`cli:test` is `cargo test -p sluice -- --skip sandbox`. Once the library
failed, cargo ran none of the integration binaries. Those binaries were
run once through sluice against the bolt, with nothing else run again.
The tools tier was running beside them:

```
sluice exec bolt/sluice-is-refactored-and-runs-on-pools -- cargo test --no-fail-fast -p sluice --test '*' -- --skip sandbox
cli:          ok. 222 passed
host_process: FAILED. 38 passed; 1 failed (98.84s)
checks, forge, instance, local_files, moon_telemetry, plugin_docs,
previews, reader_tick, runs, sessions, telemetry, telemetry_logs,
usage:        ok
exit=101
```

```
---- a_successor_that_does_not_start_leaves_the_host_process_as_it_was stdout ----
thread '...' panicked at crates/sluice/tests/host_process.rs:33:9:
#1 lands: not within 60s
```

## The tools tier

`#check-tools` is main's gate, not the bolt's. The tasks name nine of
its suites. The whole tier was run once through sluice against the bolt,
as `cli:tools` runs it:

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
pull_requests:  FAILED. 155 passed; 1 failed (603.02s)
pushed_refs:    ok. 53 passed
remote_branch:  ok. 34 passed
remote_refs:    ok. 30 passed
stack:          ok. 3 passed
worktrunk:      ok. 77 passed
exit=101
```

```
---- landed_on_another_host stdout ----
thread '...' panicked at crates/sluice/tests/profiles/pull_requests.rs:6793:5:
A took #1 in: [{... "id": 1, "state": "landing", "tested": {...} ...}]
  beside B's [{... "id": 1, "state": "landed", "landed_as": "fea5044...", ...}]
```

Every test task 8.24 names passed in that run:
`three_pull_requests_one_query`, `the_querys_reviews_count_as_before`,
`a_refused_query_falls_back_to_the_reads_per_pull_request`,
`a_draft_is_read_from_the_list_alone`, `a_landing_asks_at_once`,
`the_state_moves_and_is_posted_unread`, `blamed_on_another_host`,
`a_submit_leaves_a_request_queued_on_another_host`,
`a_submits_pull_request_sent_back_says_so` and
`held_when_github_refuses_the_draft`. So did:

- task 1.5's `previews_tools` and `stack`;
- task 5.26's `worktrunk` and task 6.22's `forge`;
- task 6.23's `peers`, among it
  `a_one_shot_overview_beside_a_current_reading_asks_origin_nothing`.

## The three failures run again

The three failing tests were then run once more together, by name with
`--exact`, through sluice against the bolt, with nothing beside them.
All three passed:

```
sluice exec bolt/sluice-is-refactored-and-runs-on-pools -- cargo test --no-fail-fast -p sluice --features tools --lib --test host_process --test pull_requests -- --exact host_process::tasks::tests::a_reading_under_way_when_told_is_read_again a_successor_that_does_not_start_leaves_the_host_process_as_it_was landed_on_another_host
lib:           a_reading_under_way_when_told_is_read_again ... ok (1.00s)
host_process:  a_successor_that_does_not_start_leaves_the_host_process_as_it_was ... ok (19.46s)
pull_requests: landed_on_another_host ... ok (13.39s)
exit=0
```

So each fails only under load, and each must pass every time before the
gate can pass.

## What falls short

### 1. Task 6.23's test races the look's start (the gate's failure)

`a_reading_under_way_when_told_is_read_again` (`tasks.rs:1288`) starts a
look, waits for `tasks.looking(&repo)`, then calls `tasks.told(&repo)`.
It expects that look to have begun before the `serve`, so the read looks
a second time.

But `look_at` (`tasks.rs:274`) puts the repository in `looking` before
it spawns the look's thread. That thread then marks `began` in
`begin_reading`. Under load, `told` can land between the two, so `began`
comes after `told`. `Marks::read_since` then counts the first look as
begun after the `serve` and answers from it after one look (`left: 1`).

The code reads what the `serve` changed here, since the look's read of
the records starts after `begin_reading`. The test's sign that a look is
under way is wrong. Either the test waits for the look to have begun
(the in-play closure signalling it), or `look_at` marks `began` before
it spawns. Task 6.23's check failed in the gate.

### 2. A host process left by a successor that did not start lands late under load

`a_successor_that_does_not_start_leaves_the_host_process_as_it_was`
(`host_process.rs:1665`) is task 7.6's check. Run beside the tools tier,
it reached the warning that the successor did not start. Then `#1` did
not land within 60 seconds of `go` being written. Alone, the whole test
takes 19 seconds.

The cause was not traced. One lead: the branch's `timeout = "1m"` is
counted from the run's start, and the time between `#1` testing and `go`
includes the attempted hand-over. That hand-over lets each task finish
its pass within `FINISH_FOR` (60 seconds) and waits ten seconds for a
`hello`. Under load, the run's limit could pass before the run sees
`go`, so `#1` never lands as tested. The resumed host process may also
take a slow first pass.

### 3. A's status reads a reading taken while B was landing

`landed_on_another_host` (`pull_requests.rs:5990`) ends with
`took_nothing_in` (task 8.21). That asserts every submission A's status
shows is one B's shows, compared whole. A's status showed `#1` as
`landing`, with no `landed_as`. B's showed it `landed`.

Since D16, a status fetches nothing. It reads main's records on the
remote as A's host process last fetched them, a reading current within
its tick. A's last fetch came between B's write of `landing` and its
write of `landed`. A took nothing in: the record it shows is B's, one
write older. But the helper reads that as A's own.

`request-intake-host`'s "Landed on another host" holds: A's submit
exited 0, reporting it landed. Task 8.21's reading that A's status
shows "nothing of the request B's does not" does not hold within a tick
of B's landing. Either `took_nothing_in` waits for A's reading to catch
up, or it compares submissions by id and by who made them, not whole.

## Smaller departures

These were confirmed in the code. None would block the change alone.

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
  pause. Task 5.24's test waits on the pull request its worktree's
  submit opened. The change does not amend the scenario.
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
  - the empty `impl OnGitHub {}` at `pull_requests.rs:7257`

## Requirements and scenarios

There are 16 delta specs. They were read against the code and its tests,
and the results come from the runs above.

| Spec | Verdict |
|---|---|
| `host-process` | Met. `peers` passes whole, two hosts landing (4.13) and a signed record verified once (6.9) among it. |
| `host-driver` | Met. |
| `standing-executor` | Met. |
| `obsolete-combinations` | Met. |
| `run-signing` | Met. |
| `place-ports` | Met. `places`, `previews_tools` and `stack` pass whole, reading ports held by the port (1.5). |
| `run-bound` | Met, with the claim loop's moment. The keeper's cores pass (7.10). |
| `settings-take-up` | Met in the code. Its scenario "A successor that does not answer" fails under load (finding 2). |
| `submission-wait` | Met. A wait meets a pause only on the request it follows (5.24). |
| `request-intake-host` | Met. "Blamed on another host" holds for a request opened by hand, whose check says `failed: `. A submit's request is held for the next submit, as the requirement's "one held for the next submit" reads. |
| `pull-request-intake` | Met. A round that stepped shows what it sent back with nothing asked of GitHub (8.24), and forge-reads' scenarios pass. |
| `remote-read-log` | Met. |
| `overview-reader` | Met. "A one-shot read reads what a command changed" holds in the code (6.23), but its test races (finding 1). The one-shot overview beside a current reading asks origin nothing. |
| `overview` | Met. |
| `forecast` | Met. `remote_branch` passes whole (6.19). |
| `reader-tick` | Met (5.25). |

The main specs the change does not amend hold in the tools tier:
forge-reads, pull-request-submit and request-intake-host. The one
exception is finding 3's reading of A's status within a tick of B's
landing.

## Tasks

All 115 tasks are ticked. Every task's named check passes except these,
each of which passes alone and fails under load:

- **6.23**: `a_reading_under_way_when_told_is_read_again` failed in the
  gate (finding 1).
- **7.6**: `a_successor_that_does_not_start_leaves_the_host_process_as_it_was`
  failed (finding 2).
- **8.21**: the `took_nothing_in` it names fails `landed_on_another_host`
  (finding 3). 8.21's own named tests passed.
