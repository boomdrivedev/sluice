# `sluice exec`, `sluice equip` and `sluice release`

## Which form

| you want | run | what it does |
|---|---|---|
| a fast loop while editing | `sluice exec <branch> -- moon run <project:task>` (or `-- moon run :test --affected`, `-- npm test`) | your command with the worktree's ports, `SLUICE_TMP`, the host's bound and the branch's limits; moon's cache applies; uncommitted edits count; nothing recorded |
| the gate's verdict before submitting | `sluice exec <branch>`, committed first so it is recorded | the branch's required tasks your own changes affect since your worktree left the branch, your uncommitted and untracked files included, and their dependents (`moon query tasks --affected --downstream deep`, every required task if `.moon/` or a `moon.yml` of yours changed); a task sluice passed on the same files is replayed, and a replay sluice holds no pass for runs again; recorded when the worktree is clean |
| to know whether that saves the submission work | read the branch's `trust` | reused only on a branch that trusts `"open"` that has not moved since; otherwise feedback only |

Affected-only is the whole gate: a task's key is the task and a hash of its
inputs, so a task your change does not touch has the key it passed under
when the branch's tip landed, and the submission's gate already holds that
result. The submission computes the affected set the same way, since the
branch's verified revision. Your run lets moon answer a task from its
cache, as the submission's run does, and the replay stands only where
sluice holds a pass the branch's gate counts for its key. A replay with no
such pass runs again in the same `sluice exec`. A replay is never recorded
as a run.

## `sluice exec <branch>`

```console
$ sluice exec feature/x
running the checks the worktree's own changes affect since 1a2b3c4 on feature/x, now at c41e8f2
ports: SLUICE_PORT_DB=20001 SLUICE_PORT_WEB=20000
…what the tests print…
passed: 2 tasks (1 replayed)
recorded 1 result under its key for 8d41b6c
```

- Runs the tests your own changes affect since your worktree left the
  branch: your commits since the last commit you share with it, and every
  file you hold beyond your head, uncommitted edits and untracked files
  included. What the branch landed since is not yours. The first line
  names where you left it, and the branch's revision where it has moved
  on. A worktree with nothing of its own runs no test and exits 0. It
  reads the branch's policy, never your worktree's.
- Replays a test sluice passed on the same files, counted in the pass
  line, `passed: 2 tasks (1 replayed)`. A replay sluice holds no pass for,
  as one of a run that was not recorded, runs again in the same command:
  `replayed with no result sluice recorded: <task>, …; running them
  again`.
- Counts against the host's bound (`[runs] max` in
  `~/.config/sluice/config.toml`) while it runs and waits while it is full,
  saying `waiting: the host is at its bound`. The bound counts every
  repository's runs on the host, queues and agents alike, so another
  repository's runs can make it wait. It never takes the share
  `[runs] reserved` keeps for submissions.
- Holds the branch's executor, memory, network and time limit while it
  runs.
- Gives the worktree the same ports on every run (`SLUICE_PORT_<NAME>`) and
  a scratch directory, `SLUICE_TMP`.
- With a clean worktree, each result is recorded under its key as an `open`
  run, whatever executor held it: the worktree is the agent's, so the run is
  never confined. A later submission reuses it only on a branch that trusts
  `"open"`.
  Otherwise it says `not recorded: worktree is not clean`.

`sluice exec <branch> --release` releases the worktree once the run ends.

`sluice exec <branch> -- <command>` runs any command the same way and exits
with its code. It records nothing.

A test that could not run (it exited as `[tests] not_run` names, or it
could not be started) is listed as `could not run: <test>: <why>` and
recorded nowhere. A command after `--` is not held to `not_run`.

Under the sandbox, a test that did not pass in a run the sandbox refused
something could not run either, and is listed with what was refused:

```console
could not run: cli:tools: sluice's sandbox refused the run file-write-create /Users/me/.cache/tool/lock (by tool)
nothing was recorded for a test that could not run: once the host can run it, run again
```

sluice's sandbox stopped it, not the test's code. The cure is the
branch's policy, the host's `[[caches]]` or the test, never a set-aside,
since nothing was recorded. A command after `--` is never read so. When
sluice can't read what the sandbox refused the run, it says so after the
run's lines, and records the run as one refused nothing:

```console
sluice could not read what its sandbox refused the run: log: exited 1 before it was ready
```

A required test the task runner skipped because a task it depends on ran
and failed is failed by that task. The task is listed under `failed:`, the
test on the line beneath, and the task's fail is recorded under its key
when the worktree is clean. The test never ran: fix the task.

```console
failed: api:integration
  root:integration failed by api:integration, a task it depends on
recorded 1 result under its key for 8d41b6c
```

`--json` prints one document on stdout:
`{branch, tasks[{task,hash,outcome,recorded}], dependencies[{task,hash,outcome,recorded}], failed_by[{task,by}], missing, not_run[{task,reason}], timed_out, ports, forecast, exit}`;
the run's output goes to stderr. `dependencies` holds each task that failed
a required test, `failed_by` each test so failed with the tasks (`by`) that
failed it; neither is in `missing`, which lists only the required tests
left without a result.

Exit codes: `0` all passed, `1` a test failed, was not run, could not run
or timed out, `2` refused, `130` Ctrl-C (which ends the run).

## `sluice equip`

Every checkout, worktree and preview sluice runs anything in is equipped
first, by the branch's policy: the toolchain `[tests] toolchain` names,
then, under moon, `proto install --config-mode upwards` where the working
copy holds a `.prototools`, `moon --version` (2.6 or later) and
`moon setup`, stopping at the first that fails. `sluice exec` does it
before your checks or your command, so it needs no `sluice equip` first.

`sluice equip <branch>` does that and nothing else, in the worktree it runs
in:

```console
$ sluice equip feature/x
equipping /src/app-x for feature/x at c41e8f2
toolchain: passed
proto install: passed
moon --version: passed
moon setup: failed
  exited 1: error: no such toolchain
```

- Run it in a worktree sluice did not make, before moon runs there by
  hand: a fresh clone, or a worktree a tool made for an agent. A tool that
  makes worktrees for agents runs it in each, so each is ready as sluice's
  own checkouts are.
- From any folder of the worktree, it equips the worktree's top, where its
  `.prototools` is.
- It reads the policy at the branch's current revision. `--at <revision>`
  reads the policy there instead, whether or not that policy names the
  branch, for a worktree made on a base the branch has not landed.
- It holds nothing: no ports, no record of the worktree, no part of the
  host's bound, and it never waits for room. Nothing is left for
  `sluice release` after it.

`--json` prints one document on stdout:
`{branch, revision, steps[{step,passed,words}]}`; a refusal prints
`{error{kind,message,next}}` and no steps.

Exit codes: `0` every step passed, or there was none, `1` a step failed,
its words beneath it, `2` refused.

## Release

What a run hands off, a stack of containers above all, stays up between
runs. `sluice release` in the worktree runs the policy's `[tests] release`
command, ends every run's process group and frees the ports. The worktree
stays.

- `wt submit` releases before worktrunk removes the worktree.
- On plain git, run `sluice release` before `git worktree remove`.
- A worktree removed without release is released by the next `sluice exec`
  or round on the host.
- `sluice status` (no argument) lists every worktree sluice holds ports or
  runs for.
