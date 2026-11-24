## Why

The user, on 2026-11-24: "why is the sluice test suite 30 minutes. for
fuck sake. what do we get from that. i feel like 5 minutes is slow."
And: "the whole point of sluice is to be a merge queue that helps get
work merged FASTER, but all it does is slow us down."

sluice's own gate on `main` took 9 minutes of wall time and 30.6
minutes of task time for #147, on deneb at a load near 47. Each slow
task was then run alone, through `sluice exec main`, on 2026-11-23
(design, Measurements):

| Task | Alone | What makes it slow |
|---|---|---|
| `plugin-herdr:test-sh` | 7.4 min | 155 tests, one after another; a fixed wait in each |
| `plugin-herdr:test-dash` | 7.5 min | the same 155 tests under dash |
| `cli:tools` | 9.4 min, then a failure | 1.5 min to build; 735 tests, 168 of them `pull_requests` scenarios that take 79% of the test time |
| `cli:test` | 1.9 min | 1.3 min of tests, the rest build |

Four causes:

1. **A change to any `moon.yml` runs every check.** #146 and #147 each
   changed one crate's `moon.yml`, so their gates ran both herdr
   suites, 15 minutes of task time that no file of theirs affected.
   Where no `moon.yml` changes, a combination already runs only what
   it affects: #143, an archive of three changes, ran 3 tasks.
2. **The herdr plugin's tests wait on the clock.** Each test starts
   with a 1-second sleep. Each wait checks once a second. A wait for
   the plugin to go quiet needs 3 seconds with no change. Five tests
   sleep 6 to 10 seconds to count idle ticks of 1 second. The suite
   runs its 155 tests one after another.
3. **A few `cli:tools` tests are long chains.** One test runs three
   scenarios in a row and takes 182 seconds on a loaded host. Ten
   tests take more than 80 seconds. The task cannot end before its
   longest test.
4. **Almost nothing is replayed.** The last five landings replayed 0
   or 1 task each.

## What Changes

- **A project's own `moon.yml` affects that project's checks alone.**
  It affects the required checks of its project and of what depends on
  it. The settings every check reads still run every required check:
  `.moon/`, the toolchain files that `own_setting` names, and the
  branch's policy, `.config/sluice.toml`. One rule serves a
  combination's run, a worktree's run and the reading that lands work
  beside the chain (140).
- **Each test of the herdr plugin waits on its event.** A wait checks
  every tenth of a second. A test starts once the last test's watchers
  have ended, with no fixed sleep. The tests run in four groups at
  once, each group in folders of its own.
- **The herdr plugin's tests stay under both shells.** With the waits
  on events and in groups, each suite takes about a minute and a half. moon runs the two tasks
  at once, so the second shell adds no wall time.
- **A tools test steps without waiting on the clock.** A wait for a
  state polls at 50 milliseconds, not 200. `s56`, three scenarios in
  one test, becomes three tests.
- **Why replays are few is written down.** A combination's run keeps
  moon's manifests in the shared store; its archives are out of its
  reach. No estimate counts on a replay.
- **A round in a test starts fewer processes.** The tools tests'
  time is process starts, git and the file system, shared with the
  host's other work; more threads made them slower. One `sluice step`
  of the fixture is measured, and what it does not need is cut.

No check is dropped. Every test still runs where its files change.
Design D7 gives the user one option that would drop some: running the
pull request scenarios, 69% of `cli:tools`, only where the forge's or
the core's code changes.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `moon-runner`: a combination's run reads a project's `moon.yml` as
  affecting that project's checks alone.
- `exec-checks`: a worktree's run reads a project's `moon.yml` the same
  way.
- `repository-gate`: the herdr plugin's tests wait on events and run
  in groups; no test of the gate waits on the clock where an event
  tells it.

## Out of scope

- **The host's bound.** In each measured run, `sluice exec` waited 1
  to 7 minutes for room before it started. That is the queue's time,
  not the gate's. It waits on a change of its own.
- **A test budget that fails a slow test.** Design D8 gives the
  option; the user decides.
- **Other repositories' gates.** Flume and Boomdrive run their own.
  This change gives them the rule for a project's `moon.yml`, since
  they use sluice's moon runner.

## Impact

- `crates/sluice/src/profiles/moon.rs`: `own_setting`, `Moon::plan`,
  `Moon::reads`.
- `crates/sluice/tests/profiles/moon.rs`, `crates/sluice/tests/exec.rs`:
  the scenarios of a project's `moon.yml`.
- `plugins/herdr/tests/run.sh`, `plugins/herdr/tests/dash.sh`,
  `plugins/herdr/bin/watch.sh`: the waits, the tick, the groups.
- `crates/sluice/tests/profiles/pull_requests.rs`,
  `crates/sluice/tests/peers.rs`, `crates/sluice/tests/instance.rs`:
  the split scenarios and the polls.
- `devenv.nix`, `crates/sluice/src/profiles/checks.rs`: where moon
  keeps its archives and its shared store under the sandbox.
- Records and rules: a new record in `docs/adr/` for the `moon.yml`
  reading.
- Sources: clauses 15, 16, 36, 70, 108, 140, C.2; rules core.9, core.21,
  core.35; `a-leaked-child-fails-its-test@9a82a5147ba6`.

## Touches

```
crates/sluice/src/profiles/moon.rs
crates/sluice/src/profiles/checks.rs
crates/sluice/tests/profiles/moon.rs
crates/sluice/tests/exec.rs
crates/sluice/tests/profiles/pull_requests.rs
crates/sluice/tests/peers.rs
crates/sluice/tests/instance.rs
plugins/herdr/tests/run.sh
plugins/herdr/tests/dash.sh
plugins/herdr/bin/watch.sh
devenv.nix
docs/adr/**
openspec/changes/the-gate-runs-in-five-minutes/**
```
