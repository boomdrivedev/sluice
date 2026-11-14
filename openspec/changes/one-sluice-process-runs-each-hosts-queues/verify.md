# Verify: one-sluice-process-runs-each-hosts-queues

The branch `change/one-sluice-process-runs-each-hosts-queues` was verified at
`59225774` on 2026-11-14. It was cut from
`bolt/sluice-is-refactored-and-runs-on-pools` and holds that bolt at
`7ce112b4`, the bolt's revision when the gate ran.

Outcome: **not archived.** The bolt's gate passed. The change validates
strictly, all 110 tasks are ticked, and every task's named check passes.
But 9 tests of the tools tier fail in two of the five suites the tasks
name, and each failed again when run a second time by name. Main's
`#check-tools` runs that tier when the bolt lands on main. Eight of the
nine follow from task 8.23's intake after a round. That intake asks
GitHub again after every round, where 8.23 and design D5 say it asks
nothing when the round left the branch and its records as they were. It
also posts a blamed request's held state before any host sees it
failed. The change goes back to its code stage.

The rest of main's tools tier, which no task names, was run once too.
21 more of its tests fail, in `forge`, `previews_tools`, `stack` and
`worktrunk`. Nineteen of them assert behaviour that this change's design
replaces, and those tests were not updated with it. Three causes cover
the nineteen: a place's ports held under `ports/held/` (1.1), a status
that fetches nothing (D16), and a `--no-wait` submit whose `serve` the
host process passes at once (D15). Two were not traced.

## The whole checks

The gate is `sluice exec bolt/sluice-is-refactored-and-runs-on-pools`
(sluice 0.93.2), the bolt's `#check-fast`. It ran once on `5922577` and
**passed** with exit 0.

```
running the checks 5922577 affects against bolt/sluice-is-refactored-and-runs-on-pools (7ce112b)
...
Tasks: 15 completed
 Time: 3m 46s 647ms

passed: 15 tasks
recorded 15 results under their keys for 5922577
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
| `seams:test` | passed (5 tests) |
| `fakes:test` | passed (15 tests) |
| `conformance:test` | passed |
| `plugin-sluice:validate` | passed |
| `cli:test` | passed: the library's 306, `cli.rs`'s 222 and `host_process.rs`'s 39 among them |

Every fast-tier test a task names passed in that run, among them
`a_read_after_a_change_reads_it` and
`a_read_is_answered_once_a_reading_is_current` (5.21, 6.20),
`a_one_shot_overview_beside_a_reader_prints_its_document` and
`the_home_view_beside_a_reader_counts_from_its_document` (6.16, 6.20),
`a_waits_first_pass_beside_a_live_host_process_is_the_one_it_asked_for`
(5.21), `s75_a_combination_starts_before_a_third_agents_check` (5.23),
`a_step_of_a_repository_an_older_sluice_run_drives_runs_no_round` (4.12),
`a_status_with_no_host_process_starts_it_and_reads_its_first_reading`
(6.18), the conformance tests of 2.2 to 2.6, and the core tests of
group 8.

moon warned for each of the 15 tasks that it could not package the
task's outputs into the worktree's `.moon/cache/outputs`. The gate
passed regardless.

`openspec validate one-sluice-process-runs-each-hosts-queues --strict`:
valid.

## The tools tier the tasks name

`#check-tools` is main's gate, not the bolt's. The tasks name five of its
suites: `instance`, `peers`, `places`, `pull_requests` and
`remote_branch`. They were run once together through sluice against the
bolt:

```
sluice exec bolt/sluice-is-refactored-and-runs-on-pools -- cargo test --no-fail-fast -p sluice --features tools --test instance --test peers --test places --test pull_requests --test remote_branch
instance:       ok. 12 passed; 0 failed
peers:          FAILED. 11 passed; 1 failed
places:         ok. 47 passed; 0 failed
pull_requests:  FAILED. 148 passed; 8 failed
remote_branch:  ok. 34 passed; 0 failed
exit=101
```

Every test a task names in these suites passed in that run. That
includes the checks of 4.13 (`peers`' two hosts land), 5.24 (a wait's
pause), 5.25 (passes between discoveries), 6.19 (`remote_branch
forecast`), 6.20 (`a_bolt_is_made_and_deleted_with_no_remote`), 7.10 (the
keeper's cores) and 8.23 (a blamed request marked a draft, the step's
words on GitHub's refusal).

The nine failing tests were then run once more, by name with `--exact`,
through sluice against the bolt. All nine failed again with the same
assertions.

## The rest of the tools tier

The tasks name no other suite of `cli:tools`. The rest of the tier was
run once through sluice against the bolt, as `cli:tools` runs it, with
`sandbox` tests skipped:

```
sluice exec bolt/sluice-is-refactored-and-runs-on-pools -- cargo test --no-fail-fast -p sluice --features tools --test local_refs --test pushed_refs --test remote_refs --test forge --test moon --test worktrunk --test protect --test conformance --test stack --test exec --test previews_tools --test equip_tools --test landed --test overview_watch -- --skip sandbox
conformance:    ok. 60 passed
equip_tools:    ok. 7 passed
exec:           ok. 17 passed
forge:          FAILED. 47 passed; 1 failed
landed:         ok. 1 passed
local_refs:     ok. 17 passed
moon:           ok. 32 passed
overview_watch: ok. 10 passed
previews_tools: FAILED. 1 passed; 9 failed
protect:        ok. 28 passed
pushed_refs:    ok. 53 passed
remote_refs:    ok. 30 passed
stack:          FAILED. 0 passed; 3 failed
worktrunk:      FAILED. 69 passed; 8 failed
exit=101
```

These failures were read from their assertions and were not run on the
bolt. The change has no commits to `previews_tools.rs`, `worktrunk.rs`
or `forge.rs`. It has two to `stack.rs`, but they leave the listings
below as they were.

## What falls short

### 1. The intake after a round asks GitHub again

Task 8.23 has a pass and a step take a branch's intake again after a
round that stepped (`host_process/pass.rs:180` and `:283`). Its "True
when" and design D5 say that intake "asks GitHub nothing where the round
left the branch and its records as they were".

`Step::Stepped` is every round that was not busy, led, contended or
held, so the second intake runs after every ordinary round. Where a pull
request out of draft waits on its conditions, it asks the GraphQL query
again. One `sluice step main` then makes two queries. That contradicts
forge-reads' "Three pull requests, one query" and rule core.32 (one query
per branch). These three tests fail on it:

- `three_pull_requests_one_query` (`pull_requests.rs:7280`):
  `g.gh.queries()` is 2 after one step where it asks 1.
- `the_querys_reviews_count_as_before` (`:7302`): 2 where it asks 1.
- `a_refused_query_falls_back_to_the_reads_per_pull_request` (`:7329`):
  2 where it asks 1.

`a_draft_is_read_from_the_list_alone` (`:5223`) counts 4 lists after
three steps, where it asks 3. The first step's second intake asks the
list again.

### 2. What the intake after a round moves a pass earlier

Two tests find GitHub asked or posted one step sooner than they assert:

- `a_landing_asks_at_once` (`pull_requests.rs:5134`): the step that
  lands already asks for main's pull requests in its second intake, so
  the next step asks nothing (`3` where it asks `4`). forge-reads' "A
  landing asks at once" says the next pass asks.
- `the_state_moves_and_is_posted_unread` (`:5208`): the first step's
  second intake already posts `testing`, so the second step posts
  nothing (`2` where it asks `3`). forge-reads' "The state moves" still
  holds: the check is posted once, saying testing, unread.

Either the tests or the timing has to change.

### 3. A request blamed on another host is never posted as failed

`blamed_on_another_host` (`pull_requests.rs:6069`): A's wait exits as a
failed submission, but sluice's check on the request's head says
`waiting: the next submit`, not `failed: ` and the blamed check. B's
round blames the request, and the same pass's second intake marks it a
draft and posts the held state. No host then sees the failure posted.
request-intake-host's "Blamed on another host" starts from the check
saying `failed: ` and the blamed check, and its wait names what the
check names. The test asserts that the posted words begin `failed: `.

### 4. A request queued on another host is never seen queued

`a_submit_leaves_a_request_queued_on_another_host`
(`pull_requests.rs:7205`): B takes the request in, tests it and lands
it, and its check never says `queued: next on main` between passes. The
test waits for that state, so it panics with `B never queued it`, the
submission landed. pull-request-submit's "Still queued on the other
host" needs B's check to say `queued: next on main`. The intake after
the round posts what the round moved it to in the same pass. The test's
setup needs a way to hold B's request queued, or the scenario needs
another reading.

### 5. A one-shot overview beside a current reading fetches

`peers.rs`'s
`a_one_shot_overview_beside_a_current_reading_asks_origin_nothing` fails
at `:727`. While `sluice overview --json` runs, right after
`sluice status main`, git counts one start of the remote's one ask:

```
fetch --quiet --no-tags --no-write-fetch-head --prune origin +refs/sluice/state/*:... +refs/heads/*:refs/remotes/origin/* ... +refs/sluice/runs/*:refs/sluice-fetched/origin/runs/*
```

The test is unchanged since it last passed at `e7e13952`. Since then,
`ccabbd07` changed when a reading is current (a `serve` makes every
earlier reading not current) and when the overview prints the reader's
document (`looked`). `171bb682` gave a step the remote's one ask. The
cause was not traced. It failed on both runs.

### 6. The rest of the tools tier reads the design before this change

21 tests outside the suites the tasks name fail:

- **A place's ports are held under `ports/held/` (task 1.1, D18).**
  Twelve tests list `$XDG_STATE_HOME/sluice/ports` as blocks of ports.
  They find the `held` folder, and the stack tests parse each name as a
  block number. These are `previews_tools.rs`'s nine, each failing at
  `assert!(s.held().is_empty())` (`previews_tools.rs:118`'s helper), and
  `stack.rs`'s three:
  - `a_previews_stack_stays_up_until_it_is_released` (`:604`, `blocks of ports still held: ["held"]`)
  - `two_stacks_side_by_side_and_a_time_limit_leaves_nothing` (`:312`, the same)
  - `a_sessions_stack_stays_between_runs_and_goes_on_release` (`:468`, `ParseIntError` on `held`)
- **A status fetches nothing (D16).** `forge.rs`'s
  `status_says_who_pushes_the_branch` (`:929`) asks that `sluice status
  main` fetch with the app's token. It mints no token now (`0` where it
  asks `1`).
- **A `--no-wait` submit's `serve` is passed at once (D15).** Six
  `worktrunk.rs` tests read the submission `testing` where they ask
  `queued`:
  - `a_no_wait_run_again_answers_with_what_stands` (`:1015`)
  - `a_place_in_the_queue_where_one_stands_is_refused` (`:1056`)
  - `no_wait_returns_and_status_reads_the_outcome` (`:374`)
  - `status_gives_the_inbox_words::a_failure_by_name` (`:3139`)
  - `status_json_gives_a_decided_submissions_delivery_and_none_before` (`:1666`)
  - `run_again_once_landed_removes_at_once` (`:888`), which lands where the test expects a wait
- **Two `worktrunk.rs` failures were not traced.**
  - `a_submission_from_another_host_is_not_pushed` (`:752`) finds herdr
    called, where it asks for no call.
  - `of_two_tested_together_only_the_blamed_ones_pane_is_sent_the_failure`
    (`:2738`) finds the two submissions not tested together.

Main's `#check-tools` runs all of these when the bolt lands.

## Smaller departures

These were confirmed in the code. None would block the change alone.

- **A reading begun before a `serve` can still answer a read.**
  `Marks::current` (`host_process/tasks.rs:92`) refuses a reading that
  began before the last `serve`. But `Tasks::read`'s wait returns any
  reading that ends after the read was asked (`:364`), even one begun
  before that `serve`. The window is narrow: a pass builds its reader's
  document from the records as the host holds them at its end. It is
  still the case "A one-shot read reads what a command changed" rules
  out.
- **A status after a change during a long pass waits out its bound.**
  Once a command sends `serve`, the reading under way is not current. A
  status then waits for the pass to end, up to `read_for()`, which is
  33 seconds (`retry::COMMAND_FOR` 30 seconds and `READ_TICK` 3
  seconds). After that it reads the records held and says when the
  repository was last read. "A status during a long round" answers at
  once, but its scenario has no command between.
- **forge-allowance's "A wait whose timeout comes first" names
  `sluice wait 77`.** Under record 0093's decision 24, a wait on a
  submission meets no pause. Its test now waits on the pull request its
  worktree's submit opened. The change does not amend the scenario.
- **The claim loop's moment with no part.** In
  `crates/sluice-core/src/step.rs:3570`, `update` runs its closure again
  on a refused write, and the second run gets `part.take() == None`.
- **The host process notes every repository it is asked about.**
  `host_process/mod.rs:971` notes a repository without reading whether
  it has sluice.
- **Stale words about the intake mark and the hand-over.** They remain
  in:
  - `lib.rs:955`, `:970`, `:5151` and `:5565`
  - the empty `impl OnGitHub {}` at `pull_requests.rs:7248`

## Requirements and scenarios

There are 16 delta specs. They were read against the code and its tests,
and the tools-tier results come from the runs above.

| Spec | Verdict |
|---|---|
| `host-process` | Met. A step takes the remote's one ask, and two hosts land (4.13). |
| `host-driver` | Met. |
| `standing-executor` | Met. |
| `obsolete-combinations` | Met. |
| `run-signing` | Met. |
| `place-ports` | Met. `places` passes whole. |
| `run-bound` | Met, with the claim loop's moment. The keeper's cores pass (7.10). |
| `settings-take-up` | Met. |
| `submission-wait` | Met. A wait meets a pause only on the request it follows (5.24). |
| `request-intake-host` | Met as the delta words it. The main spec's "Blamed on another host" does not hold (finding 3). |
| `pull-request-intake` | Met: a blamed request is marked a draft (8.23). Its intake after a round asks GitHub again (finding 1). |
| `remote-read-log` | Met. |
| `overview-reader` | Met in its scenarios, with the window above. A one-shot overview after a branch is removed shows it gone (6.20). Finding 5 is unresolved. |
| `overview` | Met. |
| `forecast` | Met. `remote_branch forecast` passes (6.19). |
| `reader-tick` | Met. Its test now asserts what its scenario says (5.25). |

The change breaks scenarios of three main specs it does not amend:
forge-reads (findings 1 and 2), request-intake-host (finding 3) and
pull-request-submit (finding 4).

## Tasks

All 110 tasks are ticked, and every task's named check passes. One does
not hold as written:

- **8.23**: the intake after a round asks GitHub where the round left
  the branch and its records as they were (finding 1).

The other failing tests are named by no task. But every task "leaves the
tree building and its named check passing". The tasks name these
suites, and main's `#check-tools` runs them when the bolt lands.
