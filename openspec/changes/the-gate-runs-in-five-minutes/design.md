# Design

## Context

See proposal.md, Why. The code as it stands:

- `Moon::plan` (`crates/sluice/src/profiles/moon.rs`) narrows a run to
  the required checks its changed files affect, by `moon query tasks
  --affected --downstream deep`. A combination asks for it when its
  base has a verdict (`step.rs`, `since`); a worktree's run always
  asks. When any changed file starts with `.moon/` or is a `moon.yml`,
  `plan` runs every required check.
- `Moon::reads`, the reading that lands work beside the chain (140),
  uses another list, `own_setting`: `.moon/`, any `moon.yml`, and the
  toolchain's files (`.prototools`, `devenv.nix`, `devenv.lock`,
  `flake.lock` and others). `plan` does not read the toolchain's
  files; nearly every task names `devenv.nix` and `devenv.lock` as
  inputs, so moon reads them as affecting those tasks.
- The herdr plugin's suite, `plugins/herdr/tests/run.sh`, is one shell
  script of 155 tests run one after another. `fresh` starts each test
  with `reap`, which ends the last test's watchers and sleeps 1 second.
  `within` checks its condition once a second. `settled` waits for 3
  seconds in which the stand-ins log nothing. Five tests sleep 6, 8 or
  10 seconds to count idle ticks; the tick there is 1 second
  (`SLUICE_HERDR_TICK=1`). `dash.sh` runs the same script under dash.
- `cli:tools` is one nextest run of 19 test binaries. nextest runs
  each test in a process of its own, 18 at once on deneb, its number of
  cores. Nothing serializes the tests. The `pull_requests` tests drive
  `sluice step`, `wt submit` and `sluice wait` as processes against a
  stand-in GitHub, in loops: `step_until` steps, then sleeps 200
  milliseconds, until a state holds. Their check script sleeps 5
  seconds for a head that holds a file `slow`, which 28 of them use.

## Measurements

On deneb, 2026-11-23, at loads of 18 to 54, each task alone through
`~/.local/state/boomdrive-unstick/gate.sh main -- moon run <task>
--force`, then the two suites again with each test timed inside the
run. Raw logs are in the session's scratch directory, not kept.

| Task | Wall | Tests |
|---|---|---|
| `plugin-herdr:test-sh` | 7.4 min | 155, one after another, 473 s |
| `plugin-herdr:test-dash` | 7.5 min | the same |
| `cli:tools` | 1.5 min to build, 9.3 min of tests | 735, 8,613 s of test time |
| `cli:test` | 1.9 min | 914, 76 s |

Before each task started, `sluice exec` waited 1 to 7 minutes for room
under the host's bound.

### The herdr suite

Of 155 tests, 79 take under 2 seconds, 52 take 2 to 4, 14 take 4 to
8, and 10 take 8 or more. The slowest:

| Test | Time | Cause |
|---|---|---|
| sidebar: a shown token is set again once a minute | 17.9 s | counts 4 ticks of 1 s, then waits on `within` |
| watch: idle ticks run nothing | 13.6 s | `settled` (3 s still), then `sleep 6` |
| sidebar: landed, then gone | 12.7 s | ticks of 1 s, `within` once a second |
| watch: the last pane leaves | 11.1 s | `ends_within`, once a second |
| sidebar: two servers forecast once | 11.1 s | `settled` twice |
| sidebar: named just after it opens | 10.8 s | ticks of 1 s |
| watch: ten repositories, four processes | 9.2 s | watchers started and ended, waited once a second |

Every test pays `reap`'s 1 second: 155 seconds of the 473. The rest
is waits that check once a second, `settled`'s 3 seconds, and ticks
of 1 second. Little is work: the stand-ins answer at once.

### `cli:tools`

| Binary | Test time | Tests |
|---|---|---|
| `pull_requests` | 5,970 s | 167 |
| `peers` | 643 s | 14 |
| `remote_branch` | 462 s | 38 |
| `worktrunk` | 433 s | 77 |
| `instance` | 288 s | 13 |
| the other 14 | 817 s | 426 |

125 tests take more than 60 seconds. The slowest:

| Test | In the suite | Alone | Cause |
|---|---|---|---|
| `pull_requests s56_a_refused_request_waits_for_its_submitter` | 231 s | 157 s | three scenarios in one test; a chain of rounds, each a `sluice step` process with git against a bare remote; a `slow` head's 5 s check |
| `pull_requests one_of_a_group_on_a_new_head_is_withdrawn_alone` | 143 s | | rounds until each of a group's states |
| `pull_requests a_host_that_stopped_passes_main_on_after_its_lease` | 115 s | | rounds until a lease ends |
| `peers a_signed_record_is_verified_once` | 106 s | 73 s | two hosts' rounds until a landing, polled each 200 ms |
| `pull_requests run_again_marks_a_draft_ready` | 81 s | 51 s | failed at 106 s in the first pass, under load |

A test alone takes about two thirds of its time in the suite. Its
time is mostly its chain of processes, each started, each reading
git; then the 5-second checks and the 200-millisecond polls.

With `--test-threads 48` in place of 18, the suite took 524 seconds
in place of 556. Each test took about twice as long, seven failed at
their backstops, and the run used 464 seconds of user CPU and 889 of
system CPU, as `time` counts the processes it waits for. The tests do
not wait on idle time that more threads would fill: they wait on
process starts, git and the file system, which the host's other work
shares. Without the `pull_requests` tests, the rest of `cli:tools` is
2,643 seconds of test time, 31% of the whole.

## Decisions

### D1. A project's own `moon.yml` affects that project's checks

**Recommended.** `own_setting` becomes the one rule for `plan` and
`reads`. It names what every check reads: `.moon/`, the toolchain's
files it names today, and the branch's policy, `.config/sluice.toml`.
A changed file it names runs every required check. A changed
`<dir>/moon.yml` of a project marks every task of that project
affected, and moon's downstream reading adds what depends on them. The
root's `moon.yml` is the root project's, read the same way; `root:format`
reads every file and so runs on any change anyway.

A project's `moon.yml` defines its tasks, their inputs and their
dependencies on other projects. A task of another project can change
its result only through a dependency on one of those tasks, which the
downstream reading follows. moon reads the graph from the place, which
holds the new `moon.yml`. So the narrower reading names every check
the change can change the result of (36).

The branch's policy is read from the branch's revision (15), so a
change to it does not decide its own gate. It is still named here: a
change that moves a check to another tier is held to every check.

Options:

- **A (recommended):** as above. #146 and #147 run the `cli` and
  root tasks, not the herdr suites: 15 minutes of task time less.
- **B:** keep every `moon.yml` as a setting every check reads. Safe
  and simple; every change to a task's settings runs the whole gate.
- **C:** A, and leave the policy out of `own_setting`. The policy does
  not decide its own gate (15), so nothing requires it.

### D2. The herdr suite waits on events

**Recommended.** In `run.sh`:

- `reap` waits for each watcher it ends to be gone, checking every
  tenth of a second up to 10 seconds, and does not sleep.
- `within` and `ends_within` check every tenth of a second. Their
  bounds stay in seconds.
- `settled` waits for the watcher's own sign that its pass is done. The
  watcher writes its tick count to `$S/ticked` at the end of each pass
  under `SLUICE_HERDR_TEST`; `settled` waits for the count to move by
  two. Where a test asks for the stand-ins to stand still, it keeps a
  stillness of half a second, checked every tenth.
- The tests that count idle ticks run with `SLUICE_HERDR_TICK=0.2`.
  `watch.sh` hands the tick to `sleep` alone, which takes fractions on
  macOS, GNU and busybox. `sleep 6` becomes a wait for 5 ticks by the
  tick count.

Expected: 473 s to about 120 s, one after another.

### D3. The herdr suite runs in four groups at once

**Recommended.** `run.sh` takes `--group <n>/<of>` and runs the tests
whose number modulo `<of>` is `<n>`, each group in a scratch folder of
its own. `tests/groups.sh` starts four groups at once and fails where
any fails. Each test already has its own folder, server sockets and
leases; `reap` and `watching` read only their own run's scratch. The
`USR1` count a test reads is its own process's.

Expected: about 120 s to 30 to 40 s.

Options:

- **A (recommended):** four groups.
- **B:** no groups; D2 alone, about 2 minutes a shell.

### D4. Both shells stay in the gate

**Recommended.** After D2 and D3 each suite takes under a minute, and
moon runs `test-sh` and `test-dash` at once. The second shell costs no
wall time. No check is dropped.

Option, for the user: run dash only (the stricter shell), and `sh`
only where the plugin's `bin/` or `lib/` changed. It saves CPU, not
wall time, and catches less: a change to the tests alone would not be
run under the shell herdr uses on macOS.

### D5. `cli:tools` runs as much at once as waits allow

**Recommended**, in this order:

1. **Each scenario is a test of its own.** A test that runs scenarios
   one after another, as `s56` does three, becomes one test for each,
   with the fixture made for each. No test of `cli:tools` runs past
   60 seconds alone on a quiet host.
2. **The polls are 50 milliseconds.** `step_until`, `peers`'s
   `until_landed` and their kind step, then wait 50 milliseconds, not
   200.
3. **The `slow` check waits on its event.** The check script waits for
   a file the test writes when it has seen the run start, with the 5
   seconds as its bound, not a fixed sleep.
4. **nextest keeps one test for each core.** More threads made each
   test slower and failed seven (Measurements).

`run_again_marks_a_draft_ready`, which failed once under load, waits
on the state it needs in place of a count of rounds.

### D6. A round in a test starts fewer processes

**Recommended, measured first.** Points 1 to 3 of D5 move time, but
the work stays: about 8,600 seconds of test time, which the host's
cores share. Task 4.5 counts the processes one `sluice step` of the
`pull_requests` fixture starts, the `git` calls among them, and where
its time goes, and cuts what the test does not need: a second
process per command where one serves, a fetch the fixture's remote
cannot have moved. The gain is not known until it is measured.

### D7. Where the pull request scenarios run

**A decision for the user.** The `pull_requests` scenarios are 69% of
`cli:tools`'s test time. They test the forge's intake and its waits
against a stand-in GitHub.

- **A (recommended until D6 is measured):** they stay in every gate
  `cli:tools` runs in. Nothing is caught later.
- **B:** they become their own task, `cli:forge`, tagged `check-tools`,
  whose inputs are the forge's code, `crates/sluice/src/profiles/github/`,
  the core's, `crates/sluice-core/`, the seams and the test file. A
  change elsewhere in the `sluice` crate, the host process or a
  command's output, would not run them, and a fault there in a pull
  request's path would be caught at the next change to the forge's
  code. It brings a crate change's gate under 5 minutes.

### D8. A budget for a slow test

**Option, not recommended in this change.** `.config/nextest.toml`
marks a test slow at 30 seconds and ends it at 120. A test that grows
past it fails, so the gate cannot drift back. Under a host load of
50, a test that passes in 40 seconds alone can take 80; a budget that
fails it would make the gate fail for the host, not the code. The
user decides.

### D9. moon's results are kept for a replay

**Investigate, in this change.** Under `sluice exec main -- moon run`,
moon said `Failed to package task outputs into archive` and `Failed to
store blobs`: the sandbox refused `<git common dir>/moon-outputs` and
`moon-home/cache/shared`. The queue's landings replay 0 or 1 task
each. Task 5.1 finds whether a combination's run is refused the same
way. Where it is, the run is given moon's store as rule core.35
already asks, and a combination tested again after a culprit leaves
replays what passed. No estimate below counts on it.

## Expected gate time

On deneb, under the load measured, from the start of the run, not
counting the wait for room:

| Submission changes | Today | After |
|---|---|---|
| Specs, docs or records alone | about 1 min | about 1 min |
| The herdr plugin alone | 9 min | about 2 min (D2, D3) |
| A crate's `moon.yml` and its code | 9 to 10 min, every task | as a crate change (D1) |
| A crate of sluice, D5 alone | 10 to 11 min | 8 to 9 min |
| A crate of sluice, D5 and D6 | 10 to 11 min | not known until task 4.5 |
| A crate of sluice, D5 and D7 B | 10 to 11 min | about 4.5 min |

## Risks

- **D1 misses a check.** A task that reads a project's settings
  through something moon's graph does not show. Mitigated: the test
  of D1 holds that a dependent project's task runs.
- **D3's groups share something not yet seen**, such as a port or a
  process list read across the host. Mitigated: the suite fails where
  a test leaves a process (`tests/leaks.sh`), and each group is run
  alone in its task first.
- **Faster polls cost CPU on a loaded host.** A poll of 50 ms is one
  `sluice status` read or file read; the steps themselves cost more.
