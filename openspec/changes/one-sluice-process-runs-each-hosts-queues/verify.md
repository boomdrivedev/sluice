# Verify: one-sluice-process-runs-each-hosts-queues

The branch `change/one-sluice-process-runs-each-hosts-queues` was verified at
`683ba07c` on 2026-11-14. It was cut from
`bolt/sluice-is-refactored-and-runs-on-pools` and holds that bolt at
`7ce112b4`, the bolt's revision when the gate ran.

Outcome: **not archived.** The bolt's gate failed: `cli:test` failed 2 of
its 221 tests. Five requirements the change adds or modifies are not met,
one of them shown by five failing tests of the tools tier. All 89 tasks are
ticked, but tasks 3.4, 6.3, 6.6 and 8.3 do not hold as written. The change
goes back to its code stage.

## The whole checks

The gate is `sluice exec bolt/sluice-is-refactored-and-runs-on-pools`
(sluice 0.93.2), the bolt's `#check-fast`. It ran once on `683ba07` and
**failed** with exit 1.

```
running the checks 683ba07 affects against bolt/sluice-is-refactored-and-runs-on-pools (7ce112b)
...
test result: FAILED. 219 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 89.57s
...
Tasks: 14 completed, 1 failed
 Time: 2m 16s 890ms

failed: cli:test
recorded 15 results under their keys for 683ba07
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
| `site:prose` | passed |
| `site:rules` | passed |
| `core:test` | passed (161 tests) |
| `seams:test` | passed |
| `fakes:test` | passed |
| `conformance:test` | passed |
| `plugin-sluice:validate` | passed |
| `cli:test` | **failed**: 219 passed, 2 failed |

The two failures in `crates/sluice/tests/cli.rs`:

```
---- a_refusal_under_json_is_an_error_document stdout ----
thread 'a_refusal_under_json_is_an_error_document' panicked at crates/sluice/tests/cli.rs:59:6:
called `Result::unwrap()` on an `Err` value: could not read what a forecast of bolt/none forms from: the host process's reading did not bring a revision of bolt/none to this host

---- check_forecasts_a_branch_with_no_worktree stdout ----
thread 'check_forecasts_a_branch_with_no_worktree' panicked at crates/sluice/tests/cli.rs:59:6:
called `Result::unwrap()` on an `Err` value: could not read what a forecast of bolt/concurrent-units forms from: the host process's reading did not bring a revision of bolt/none to this host
```

The `#check-tools` tier is main's gate, not the bolt's, so the gate ran none
of it. That tier holds the binary suites that most of the tasks of groups
1, 5, 6, 7 and 8 name (`places`, `peers`, `remote_branch`, `instance`,
`pull_requests`, `overview_watch`). Its whole suite was not run. To confirm
finding 2 below, five of its tests were run once through sluice:

```
sluice exec bolt/sluice-is-refactored-and-runs-on-pools -- cargo test -p sluice --features tools --test pull_requests -- held_then_marked_ready_waits_to_be_taken_in two_waits_and_a_status_read_at_one_pace a_waits_words_across_hosts not_yet_taken_in_across_hosts a_wait_across_hosts_reads_at_its_pace
test result: FAILED. 0 passed; 5 failed; 0 ignored; 0 measured; 151 filtered out; finished in 59.31s
```

## What falls short

Each finding below was confirmed in the code. For finding 2, the tests
named above also fail.

1. **A forecast of a branch that exists nowhere is an error, not a
   refusal (the gate's two failures).** `forecast`'s unchanged requirement
   "A branch is forecast with no worktree of it" has the scenario "No such
   branch": the forecast says `no branch bolt/gone to forecast` and exits 2.
   `cli.rs:1030` also asks for the refusal `no branch bolt/none` with its
   next steps. In held mode, `Sluice::forecast_branch` and `forecast_head`
   (`crates/sluice-core/src/ops.rs:840-866`) and `forecast_read`
   (`crates/sluice/src/lib.rs:2712-2716`) return `Unheld` for any branch
   missing from the host. `forecast` (`lib.rs:2642-2654`) asks for a reading
   afresh and then fails with exit 3. The delta's "where that reading does
   not hold the branch … exit 3" has to be reconciled with this refusal:
   once a reading is current, a branch that no remote and no local ref names
   is no branch, not something unheld.

2. **A wait on a host that does not keep the branch never follows the
   request across hosts.** `Pulls::follow` takes `follow_across` only where
   `away` names the branch (`crates/sluice/src/profiles/github/pulls.rs:3032`).
   Only an intake sets `away` (`pulls.rs:727`). A wait runs no intake
   (`lib.rs:5131`) and calls `pulls.follow` directly (`lib.rs:5147`), so it
   reads the request as its keeper would. It names no host, gives no
   `draft`, reads GitHub at each process's own pace rather than the host's,
   and keeps nothing in `forge/follows/`. Not met:
   - `request-intake-host`: "A wait whose pull request another host takes in
     runs nothing for it", "A host keeps the pull requests it follows" (its
     pace) and "A wait reads a held pull request by its draft mark".
   - `submission-wait`: "A wait across hosts names where its pull request
     stands".

   Task 8.3's tests fail: `a_wait_across_hosts_reads_at_its_pace` (7 reads),
   `two_waits_and_a_status_read_at_one_pace` (15 reads),
   `a_waits_words_across_hosts`, `not_yet_taken_in_across_hosts` (`draft`
   null) and `held_then_marked_ready_waits_to_be_taken_in` (`waiting`, not
   `waiting to be taken in by b`).

3. **A wait's first pass is not the host process's pass it asked for.**
   `submission-wait`'s "A timeout returns the submission as it stands" says
   a wait takes "the host process's pass it asked for when it began to
   follow". Its scenario "Out of time as it starts" has `--timeout 0s` find
   `testing`. When a host process passes on its own tick, a follow wakes a
   pass only for a watch (`crates/sluice/src/host_process/tasks.rs:441`).
   The client reads no first answer (`host_process/client.rs:338-348`), and
   `Follower::tick` asks no pass (`host_process/ask.rs:340`). So `wait_on`
   checks its deadline on its second loop and returns the record as it
   stood, which may be `queued` (`lib.rs:3774-3788`). This contradicts the
   doc comment at `lib.rs:3682` and D9. Every test runs passes only when
   asked (`SLUICE_ROUND_SLEEP=record`, or `FakeHost`), and that mode asks a
   pass first, which hides the gap. The review also reports the same race
   for a watch, in `overview-reader`'s "sluice set up after a watch ended":
   the watch's first read can find an earlier reader's `set_up: false`
   document before the host process's first pass replaces it.

4. **A one-shot overview and the home view are not printed from the
   reader's document.** `overview-reader`'s "A one-shot read is answered
   from the host process's last reading" and its scenario "A one-shot
   overview beside a current reading" ask for "the reader's document of
   that reading", and so does task 6.3 ("text rendered from it as the
   watch's is"). Instead `overview::run` rebuilds the document from the
   records in held mode (`crates/sluice/src/overview.rs:239-249`), and so
   does `home.rs:68`. Nothing is fetched, but the document is not the
   reader's. `cli.rs:2743`'s `one_shot_overview_reads_the_records_beside_a_reader`
   still asserts the removed requirement "A one-shot overview reads the
   records itself".

5. **A forecast beside a ticking reader whose document lacks the branch
   fetches.** Where a reader keeps its tick, the command's `Sluice` is not
   held (`lib.rs:1476`). Where that reader's document lacks the branch or a
   revision, `forecast_read` returns `None` (`lib.rs:2696-2701`), and the
   forecast forms as a round reads (`lib.rs:2631-2641`), fetching on disk.
   It asks no reading afresh, gives no exit 3, and says nothing of the
   remote's refusal on stderr. This breaks the requirement "A forecast with
   no current reader has the host process read" ("SHALL fetch nothing
   itself") and its scenario "The reader could not reach the remote", and so
   task 6.6. `cli.rs:748`'s `a_forecast_with_no_revision_reads_as_a_round`
   asserts the old behaviour.

## Smaller departures

These were confirmed in the code. None would block the change alone.

- **`sluice step` beside an older `sluice run`.** `Job::Step`
  (`host_process/tasks.rs:780`) runs `pass::step` without the
  `Driver::try_take` that a pass makes (`tasks.rs:860`). The branch's lock
  keeps it safe, but `host-process`'s "An older sluice run keeps the
  repository it drives" says the host process runs no round of that
  repository while the run lives.
- **Task 3.4's room handle.** `RoomHandle` has only `Here`
  (`crates/sluice/src/profiles/checks.rs:396-407`). A profile bound with no
  handle builds a `Room` of its own over `running/` (`checks.rs:299-311`),
  where the task says the host process's socket. `sluice exec` and
  `sluice preview` hold over the socket by another path, so no production
  process decides by itself today. The tree does not enforce that.

The review also reports these from the code. They were not run:

- The host process notes in its index any repository it is asked about,
  without reading that it has sluice (`host_process/mod.rs:856-930`, `note`
  at `:967`). Such a repository leaves the index only when a host process
  starts.
- On a host that does not keep the branch, a followed request the queue
  holds at its head is passed over by `across_rows` but not removed from
  the follows (`pulls.rs:3407-3413`). The requirement removes it.
- `pull_requests_need_records_on_the_remote` finds that the overview lists
  nothing of a refused `main` with no records (`overview.rs:374`).
  `pull-request-intake`'s refusal scenario has the overview show the
  refusal.
- A look's read marks are kept only in the host process's memory
  (`tasks.rs:276-293`). The scenario "The tick is named" says `"tick": 3`
  once only a wait follows, and that half has no test.
- In the claim loop (`crates/sluice-core/src/step.rs:3546-3590`), `update`
  runs its closure again on a refused write, and the second run gets
  `part.take() == None`. For a moment, a run can be written as starting
  with no part. The next `perform_starts` opens it again.
- Two tests contradict their specs:
  - `a_watch_reads_no_followed_request_itself` (`pull_requests.rs:6672`)
    asserts no GitHub read while a row is due, but the host process's pass
    reads a due request (`pass.rs:200`).
  - The test of "A branch that takes pull requests between discoveries"
    asserts one start more for six passes than for one, where the scenario
    says exactly as many.
- Stale words about the mark and the hand-over remain:
  - `lib.rs:950-986`, `5119` and `5533`
  - `pulls.rs:3155-3162`
  - `reader.rs:1-12`
  - an empty `impl OnGitHub {}` at `pull_requests.rs:7205-7213`
  - `main.rs:314-318`'s "took up … in this sluice run started afresh"

## Requirements and scenarios

There are 16 delta specs with 123 requirements and 188 scenarios. They were
read against the code and its tests, without running the tools tier.

| Spec | Verdict |
|---|---|
| `host-process` | Met but for the step's `.driver` departure above. Several scenarios have thin tests: "A watch keeps it" has none; "The queue empties" allows 10 s for two ticks; "The host's sluice is upgraded" never lands a submission. |
| `host-driver` | Met. The removed driver is gone (`drive`, `Driving`, `session_capacity`). |
| `standing-executor` | Met. |
| `obsolete-combinations` | Met. `Requester::HandOver` and `drop_obsolete` are gone. |
| `run-signing` | Met. |
| `place-ports` | Met. |
| `run-bound` | Met, with the claim loop's moment above. "A start that fails" and the S19 pool job have thin tests. |
| `settings-take-up` | Met. "A submit during the hand-over" does not force the race. |
| `submission-wait` | Not met: findings 2 and 3. |
| `request-intake-host` | Not met: finding 2. Its REMOVED requirements are gone: no mark refs, `put_intake_mark`, `hand_over`, `drop_obsolete` or `Taker`. |
| `pull-request-intake` | Met but for the overview's refusal above. |
| `remote-read-log` | Met, with thin tests of each refusal ending on its own. |
| `overview-reader` | Not met: finding 4, and the watch's race in finding 3. |
| `overview` | Met in code. Its watch test contradicts the spec. |
| `forecast` | Not met: findings 1 and 5. |
| `reader-tick` | Met but for the test that contradicts its scenario. |

## Tasks

All 89 tasks are ticked. These do not hold as written:

- **3.4**: there is no socket room handle.
- **6.3**: the one-shot overview and home view are not rendered from the
  reader's document (finding 4).
- **6.6**: a forecast beside a ticking reader fetches (finding 5).
- **8.3**: its named tests fail (finding 2).

The gate's `cli:test` failure (finding 1) belongs to no task's own test,
but every task "leaves the tree building and its named check passing", and
`cli` no longer passes.
