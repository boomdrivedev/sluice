# Verify: one-sluice-process-runs-each-hosts-queues

The branch `change/one-sluice-process-runs-each-hosts-queues` was verified at
`e7e13952` on 2026-11-14. It was cut from
`bolt/sluice-is-refactored-and-runs-on-pools` and holds that bolt at
`7ce112b4`, the bolt's revision when the gate ran.

Outcome: **not archived.** The bolt's gate passed. The change validates
strictly, and all 102 tasks are ticked. But 13 tests of the tools tier fail
in the five suites the tasks name. Main's `#check-tools` runs that tier when
the bolt lands on main. One of the 13 is task 6.15's named check, so 6.15
does not hold as written. Two of them show behaviour the specs rule out. The
change goes back to its code stage.

## The whole checks

The gate is `sluice exec bolt/sluice-is-refactored-and-runs-on-pools`
(sluice 0.93.2), the bolt's `#check-fast`. It ran once on `e7e1395` and
**passed** with exit 0.

```
running the checks e7e1395 affects against bolt/sluice-is-refactored-and-runs-on-pools (7ce112b)
...
Tasks: 15 completed
 Time: 3m 22s 740ms

passed: 15 tasks
recorded 15 results under their keys for e7e1395
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
| `cli:test` | passed: the library's 305, `cli.rs`'s 222 and `host_process.rs`'s 39 among them |

The fast-tier tests that tasks 3.4, 3.5, 4.12, 5.21, 5.23, 6.14, 6.16,
6.18 and 8.22 name all passed in that run.

`openspec validate one-sluice-process-runs-each-hosts-queues --strict`:
valid.

## The tools tier

`#check-tools` is main's gate, not the bolt's, so the bolt's gate runs none
of it. The tasks name five of its suites: `instance`, `peers`, `places`,
`pull_requests` and `remote_branch`. Each was run once through sluice
against the bolt.

The first run stopped at `instance`, cargo's first failing target. The
other four were then run with `--no-fail-fast`:

```
sluice exec bolt/sluice-is-refactored-and-runs-on-pools -- cargo test -p sluice --features tools --test instance --test peers --test places --test pull_requests --test remote_branch
instance:       FAILED. 11 passed; 1 failed

sluice exec bolt/sluice-is-refactored-and-runs-on-pools -- cargo test --no-fail-fast -p sluice --features tools --test peers --test places --test pull_requests --test remote_branch
peers:          FAILED. 10 passed; 2 failed
places:         ok. 47 passed; 0 failed
pull_requests:  FAILED. 148 passed; 8 failed
remote_branch:  FAILED. 32 passed; 2 failed
```

Before those whole runs, the tests that tasks 5.22, 6.3, 6.6, 6.15, 8.3,
8.20 and 8.21 name were run once by name. Every one passed except 6.15's
`remote_branch forecast` filter, which failed one test, the same one the
whole run fails.

## What falls short

### 1. A forecast beside a current reader: task 6.15's check

`remote_branch.rs`'s `a_forecast_beside_a_current_reader_asks_the_remote_nothing`
fails at `remote_branch.rs:1792`:

```
a reader of /private/tmp/sluice-501/.../sluice-test-23266-64-forecast-reader/.git already
```

The test steps `bolt/y` with `sluice step` until it lands, then takes the
reader's lease itself to write a reader's document. Since task 4.12,
`Job::Step` calls `Task::drives`, which takes the repository's driver lock
(`host_process/tasks.rs:807`, `Driver::try_take`), and the task keeps it.
`Reader::take` makes no process the lead while a driver lives
(`reader.rs:107-112`), so the test finds itself a follower and panics.

The test never ends the host process its steps started.
`support::end_host_process` is there for that. Task 6.15's "True when"
names `remote_branch forecast` passing, so 6.15 does not hold.

### 2. A one-shot overview shows a removed branch as present

`remote_branch.rs`'s `a_bolt_is_made_and_deleted_with_no_remote` fails at
`:943`. After `bolt/x` is deleted, `sluice status bolt/x` says
`bolt/x is gone, last at 83b56d6`. But `sluice overview` prints:

```
bolt/x at 83b56d6 · 0 in the queue · testing 0 at once (up to 4)
```

The one-shot overview now prints the reader's document whenever a reader
keeps its tick (`overview.rs:244`, `reader_document`). That document comes
from a pass before the deletion, and the reading it gives counts as current
within its tick. So the overview names a branch that the status says is
gone, which rule core.11 rules out. `overview-reader`'s "A one-shot read is
answered from the host process's last reading" does not hold here either.

### 3. A blamed pull request is no longer marked a draft

Four `pull_requests.rs` tests find the blamed request open and not a
draft:

- `a_submits_pull_request_sent_back_says_so` (`:1735`)
- `blamed_then_submitted_again_on_the_same_head` (`:3262`)
- `run_again_marks_a_draft_ready` (`:2909`)
- `submitted_by_the_operators_ops` (`:3368`, `g.pull_from("unit/unit-1").draft`)

For example:

```
Pull { number: 1, ..., title: "unit/blamed", draft: false, open: true, ... }
```

`held_when_github_refuses_the_draft` fails at `:1318` on the step's
output:

```
main: stepped
sluice: warning: taking in main's pull requests: pull/1: it waits for the next submit, and GitHub would not mark it a draft: POST /graphql: Resource not accessible by integration
```

The cause was not traced.

### 4. A wait's timeout and a pause

These two `pull_requests.rs` tests fail:

- `a_wait_whose_timeout_comes_first_calls_nothing` (`:4817`) exits 0, where
  the test asks for exit 5. Its wait printed the submission through
  `passed · landed as 5231c56`, so the wait landed it rather than timing
  out first.
- `a_waiting_submit_waits_out_the_pause` (`:4802`) panics with `landed`.

Both fit task 5.21: a wait's first tick now waits for the pass its follow
asked for. They also fit task 5.22, a pass's time set to its asker's. The
cause was not traced.

### 5. The test of passes between discoveries contradicts its scenario

`passes_between_discoveries_start_no_git_and_ask_github_nothing` fails at
`pull_requests.rs:5030`:

```
assertion `left == right` failed: one: [..2 starts..]; six: [..2 starts..]
  left: 2
 right: 7
```

`reader-tick`'s scenario "A branch that takes pull requests between
discoveries" says `sluice run --rounds 6` starts git exactly as many times
as `--rounds 1`. The code now does that. The test asserts five more starts
and five more fetches, so the test, not the code, is wrong. It still fails
main's gate.

### 6. Two hosts never land

Two `peers.rs` tests fail at `:468` with `never landed: testing`. The
`land` helper's 90 seconds of `sluice step main` and `release` on each host
pass with the combination still under test:

- `a_host_runs_a_branch_only_where_its_policy_lists_the_hosts_key`
- `the_other_host_runs_the_other_branch`

Each step now runs only where the host process's task holds the driver
lock (task 4.12), and a combination's hold over the socket is answered once
(task 3.5). The cause was not traced.

### 7. The keeper's cores

`instance.rs`'s `bound::the_keepers_cores_set_each_runs_share` fails at
`:774`. Under `[runs] max = 2` and `cores = 2`, the run was given 9 cores,
where one is asked:

```
assertion `left == right` failed: [(1, 9)]
  left: [9]
 right: [1]
```

The test submits first, which starts a host process under `max = 2` with no
`cores`, and only then writes `cores = 2`. A host process that passes on
its own tick may start the combination's run before the file changes. That
run keeps the settings it started under (core.54). This reading was not
confirmed. Either the test or the take-up of `[runs]` has to change. The
test predates the change (`d1b1121e`, on the bolt), but the change rewrote
`instance.rs` and moved every run's start into the host process.

## Smaller departures

These were confirmed in the code. None would block the change alone.

- **A look writes no reader's document.** On a repository with no task
  running, a one-shot overview's reading is a look. A look writes no
  document, so the overview and the home view render the records as the
  host holds them. Design D16 says so; the requirement text names only the
  reader's document. The output carries the same data.
- **The host process notes every repository it is asked about.** It does
  so without reading whether the repository has sluice
  (`host_process/mod.rs`, `note`). Such a repository leaves the index only
  when a host process starts.
- **The claim loop's moment with no part.** In
  `crates/sluice-core/src/step.rs:3546-3590`, `update` runs its closure
  again on a refused write, and the second run gets `part.take() == None`.
  For a moment a run can be written as starting with no part. The next
  `perform_starts` opens it again.
- **A watch's test contradicts its spec.**
  `a_watch_reads_no_followed_request_itself` (`pull_requests.rs:6680`)
  asserts no GitHub read while a row is due. The host process's pass reads
  a due request.
- **`"tick": 3` once only a wait follows.** A look's read marks are kept
  only in the host process's memory. The second half of "The tick is
  named", where only a wait follows, has no test.
- **Stale words about the intake mark and the hand-over.** They remain in:
  - `lib.rs:955`, `:970` and `:5142`
  - `pulls.rs:332`, `:1980` and `:3183`
  - the empty `impl OnGitHub {}` at `pull_requests.rs:7227`

## Requirements and scenarios

There are 16 delta specs with 124 requirements, 31 of them removed, and 192
scenarios. They were read against the code and its tests. The tools-tier
results come from the five suites above.

| Spec | Verdict |
|---|---|
| `host-process` | Met. A step beside an older `sluice run` is refused, naming it. Finding 6 may bear on its steps. |
| `host-driver` | Met. |
| `standing-executor` | Met. |
| `obsolete-combinations` | Met. |
| `run-signing` | Met. |
| `place-ports` | Met. `places` passes whole. |
| `run-bound` | Met in code, with the claim loop's moment. A check profile holds through the host process's socket (`RoomHandle::Socket`). Finding 7 is unresolved. |
| `settings-take-up` | Met in code. Finding 7 is unresolved. |
| `submission-wait` | Met in code: a wait's first tick is the pass its follow asked for, and a wait across hosts names where its request stands. Finding 4's two tests fail. |
| `request-intake-host` | Met: a wait follows across hosts by the branch's records at the host's pace, with its draft mark, and a request the queue holds at its head leaves `forge/follows/`. Finding 3 bears on the draft a blamed request is given. |
| `pull-request-intake` | Not met while finding 3 stands: a blamed request is left ready. |
| `remote-read-log` | Met. |
| `overview-reader` | Not met: finding 2. A watch's first read is of the pass its follow asked for. |
| `overview` | Not met: finding 2. |
| `forecast` | Met in code: every forecast reads held, a branch a current reading brings no revision of is refused as no branch, exit 2, and what is still missing is exit 3. Finding 1's test fails. |
| `reader-tick` | Met in code; finding 5's test contradicts its scenario. |

## Tasks

All 102 tasks are ticked. One does not hold as written:

- **6.15**: its named check, `remote_branch forecast`, fails one test
  (finding 1).

The other 12 failing tests are named by no task. But every task "leaves the
tree building and its named check passing". The tasks name these five
suites, and main's `#check-tools` runs all of them when the bolt lands.
