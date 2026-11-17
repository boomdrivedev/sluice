---
title: Parallel and batch
description: How many combinations sluice tests at once, when it tests only the longest, and which to choose for a fast or a slow test suite.
sidebar:
  order: 2
---

Three settings decide how fast a queue drains and how much testing it
does: a branch's `parallel` and `batch`, and the host's `[runs] max`.
This page shows what each one does over time, why a queue tests fewer at
once after a failure, and which settings suit the test suite you have.

## Combinations

sluice never tests a submission on its own. Say A, B and C are queued on
`main`, in that order. sluice forms three combinations: `main + A`,
`main + A + B` and `main + A + B + C`. Each is what `main` would hold if
everything up to that submission landed. A submission lands only after a
combination holding it and everything ahead of it has passed.

`batch` decides which of those combinations are tested. `parallel`
decides how many at once.

## Parallel: every combination (`batch = false`)

This is the default. sluice tests each combination as soon as it is
formed, up to `parallel` at once. It bets that the submissions ahead
will pass: `main + A + B` starts before `main + A` has finished.

![Four submissions a minute apart on a 20-minute gate. With batch false, four runs at once land all four by minute 23. With batch true, main plus A runs alone, B, C and D wait, then run together as one batch and land at minute 40.](../../../../../docs/diagrams/modes.svg)

When everything passes, each submission lands about one test run after
it was submitted. The cost is runs: four submissions are four runs at
the same time, each in its own checkout and, if the tests start
containers or an emulator, with its own copy of them.

## Batch: the longest first (`batch = true`)

sluice tests only the longest combination, and starts nothing else on
the branch while that run is under test. Submissions that arrive
meanwhile wait. When the run ends, whatever queued up is tested together
as the next batch.

No timer collects a batch. A batch is whatever queued while the last run
was under test: on a quiet branch that is one submission, which starts
at once; on a busy branch batches grow by themselves.

If a batch passes, every submission in it lands together, each with its
own merge commit naming the same verdict.

Under batch, `parallel` is the largest batch. With `parallel = 4`,
sluice tests at most the first four queued submissions together, and
the rest wait for the next batch.

## Work no check reads lands at once

A submission whose changes no required check reads, such as a fix to a
guide, never waits behind code under test. sluice asks the task runner
which required checks the change reads: against `main` as it is, and
against each combination already formed, as that combination would land
now. The answer may be none every time, and the change may merge
without conflict onto all of them and the work of every submission
ahead of it. Then the next round lands it beside the queue, ahead of
everything queued, with nothing run. It takes no checkout and no room in `parallel` or the
host's bound.

The combinations under test keep their runs and their results. Say
`main + A` and `main + A + B` are under test when the guide fix lands.
They aren't tested again: what they lack is a file none of their
checks reads. When they pass, A and B land on top of the guide fix,
each landing holding the tree that was tested plus the guide.

A change that touches anything a required check reads takes the queue
whole, even when most of it is docs. So does a change to
`.config/sluice.toml`, to the task runner's own configuration or to the
tools the checks run with. So does any change to a branch that sluice
didn't last land, since nothing has tested what is on it. A runner that
can't say what a change reads, such as `runner = "command"`, lands
nothing this way.

When a combination's own changes make one of its checks read the
change, or the change conflicts with a submission ahead of it, it takes
the queue and says which submission it waits behind:

```console
$ sluice status 15
#15 queued · waits behind #13: its checks read this change
```

Of several worktrees submitted as one act, the leading ones whose
changes no check reads land this way, in the act's order; from the
first that a check reads, the rest take the queue.

## When something fails

![A, B and C queued together, a 20-minute gate, B breaks the build. With batch false, B is blamed at minute 20, the run with B in it is thrown away, and C lands at minute 40. With batch true, the batch fails at minute 20, the two shorter combinations run to find the cause, B is blamed at minute 40 and C lands at minute 60.](../../../../../docs/diagrams/failure.svg)

With `batch = false`, a failure names its submission at once:
`main + A + B` failed and `main + A` passed, so B broke the build. Every
combination with B in it is thrown away and formed again without B.

With `batch = true`, a failed batch doesn't say which submission broke
it. sluice then tests the shorter combinations it skipped, as many at
once as the branch allows, and blames the newest submission of the
first one that failed. That costs one more run before anyone is told,
and the rest of the batch waits for it.

Either way, only the blamed submission's agent is told, and the others
land without it.

## Fewer at once after a failure

`parallel` is a ceiling, not a fixed number. A branch starts out testing
`parallel` combinations at once. Each blamed submission halves that
number, and each combination that passes adds one back. It never falls
below one or rises above `parallel`.

![A branch with parallel 4 tests 4 at once, then 2 and then 1 after two failures, and climbs back to 4 one pass at a time. Beside it, main, a feature branch and an agent's sluice exec take the host's 4 runs, so a second feature branch waits.](../../../../../docs/diagrams/at-once.svg)

With `batch = false`, everything stacked on a failing combination is
thrown away. When failures come in a streak, because the branch is
broken, a test is flaky or the host is short of memory, testing many at
once throws away most of those runs. Testing fewer wastes less, and each
pass raises the number again. With `batch = true` the same number is the
size of the batch, so after failures the batches are smaller too.

`sluice status` shows where it stands:

```console
$ sluice status main
main at c575d3a · testing 1 at once (up to 4)
```

No setting turns this off. A branch stuck at one is a branch whose
submissions keep failing. If unrelated submissions all fail the same
check, look for the cause in the tests or on the host. `sluice status
<id>` names each failed check's log.

## The host's bound

`parallel` is per branch. The host's `[runs] max`, in
`~/.config/sluice/config.toml`, caps every branch of every repository on
the host together, along with the runs agents start in their own
worktrees with `sluice exec` and previews' commands. Every sluice on the
host reads one count of the runs going there, so three repositories
under `max = 3` run three at once, not nine. With no `max`, the bound is
a quarter of the logical CPUs sluice may use, and never less than one,
and every repository on the host shares it. A branch with room under its
own number still waits while the host's bound is full, whichever
repository fills it. `reserved` keeps some of the bound for submissions, so agents'
own runs can't take all of it
([configuration](/sluice/reference/configuration/#runs)).

sluice reads nothing of how busy the host is. What runs beside it, the
agents' sessions among it, is yours to leave room for when you set
`max`. On a host that gives sluice all its cores, as a Mac does,
`[runs] cores` keeps the bound's runs to fewer cores than the host has,
and leaves the rest for them.

If each run starts containers, an emulator or a database, set `max` to
how many of those the host holds in memory at once, whatever its count
of cores. Two runs that each need 6 GB on a 16 GB host is `max = 2`.

Runs at once also have to stay out of each other's way. sluice gives
each run its own checkout and its own ports (`[checkout] ports`).
Anything else the tests write, such as an output directory, a lock file
or a port they choose themselves, has to live inside the checkout. Tests
that share one break as soon as two runs overlap. The sign is unrelated
submissions failing the same check, and each of those failures halves
how many the branch tests at once.

## Systems multiply runs

A branch that names [systems](/sluice/reference/configuration/#systems-what-a-test-runs-on)
tests each combination once per system: a pass on Linux says nothing
of macOS. Each combination is one run per system its required tests
need, and each run holds only the tests that need that system.

```toml
[branches.main]
required = ["#check-fast", "#integration", "cli:build"]
systems = ["linux-x64"]
parallel = 4

[branches.main.systems_for]
"cli:build" = ["linux-x64", "linux-arm64", "macos-arm64"]
```

Here each combination is three runs: one on `linux-x64` with all three
tests, and one each on `linux-arm64` and `macos-arm64` with `cli:build`
alone. With `parallel = 4` and four submissions queued, that is twelve
runs, of which eight are single builds. Had the branch said
`systems = ["linux-x64", "linux-arm64", "macos-arm64"]` instead, all
twelve would run every test. Name a system for the tests that need it,
in `systems_for`, and leave the rest to one.

`parallel` counts combinations, never runs, and a combination passes
only once every run of it has. A host runs only the system it is, up to
its own [`[runs] max`](/sluice/reference/configuration/#runs). Runs for
another system wait for a host of that system, and a run nobody takes fails
the combination when `timeout` passes. `batch = true` keeps the count
down: one combination at a time is one run per system.

## Choosing

Start from how long the gate takes and how often submissions arrive.

| the branch | settings | why |
|---|---|---|
| a gate of a few minutes | `batch = false`, `parallel = 2` to `4` | runs are cheap; each submission lands about one run after it was submitted, and a failure names its submission at once |
| a gate of 15 minutes or more, and submissions that arrive while a run is under test | `batch = true`, `parallel = 4` to `8` | one run lands a whole batch, so the host does a fraction of the testing |
| runs that each start a large stack | `batch = true`, and the host's `max` set by memory | `batch = false` needs a stack for every combination at once |
| failures are common | `batch = false` | a failed batch costs a second run before anyone is told |
| a slow gate with few submissions | either | with nothing queued behind it, a submission is tested alone in both modes |
| every run on a CodeBuild fleet | `batch = false`, `parallel` at the fleet's maximum instances | the fleet runs as many runs at once as it holds instances, so each combination is tested as it forms, and none waits for room ([Running checks on a pool](/sluice/guides/pools/#test-side-by-side)) |

The two diagrams above are the trade in numbers. On a 20-minute gate
with four submissions a minute apart, `batch = false` runs four at once
and lands all four by minute 23; `batch = true` runs two, one at a time,
and lands them by minute 40. When B breaks the build, `batch = false`
tells B at minute 20 and `batch = true` at minute 40. Choose `batch =
true` when runs are scarce, because the host holds few stacks or runs
cost money, and a later landing is acceptable. Choose `batch = false`
when the host has room for several runs and each submission should land
as soon as it can.

Set `timeout` to about twice the gate's usual time. A run that hangs
then fails in minutes, not after the default hour. A timeout counts as a
failure, so it halves how many are tested at once too.

### Parallel runs and batches together

On one branch, `batch = true` tests one combination at a time, so one
branch can't have both. Several branches can: each branch has its own
queue, and all of them share the host's bound. A repository whose agents
land on feature branches (`feature/*` here) and that lands each one on `main` tests every
feature branch's queue at once.

A common split is `batch = false` on `feature/*`, with a short gate of the
fast checks, and `batch = true` on `main`, with the full gate:

```toml
[branches."feature/*"]
required = ["#check-fast"]
parallel = 3

[branches.main]
required = ["#check-fast", "#integration"]
parallel = 4
batch = true
hold = "5m"
timeout = "45m"
```

`hold` keeps a submission that passed from landing alone while a longer
combination holding it is under test, so submissions arriving a few
seconds apart land in one move of `main` and start one deployment
([`hold`](/sluice/reference/configuration/#hold)).

## A queue that tests one at a time

When `sluice status` says `testing 1 at once` and submissions wait,
check these in order:

1. **`batch = true` on the branch.** One run at a time is what batch
   does.
2. **Recent failures.** Each one halved the number, and it grows back by
   one for each pass.
3. **The host's bound.** `[runs] max` is full, perhaps with agents'
   `sluice exec` runs.
4. **One submission at a time.** If each submission arrives after the
   last one landed, there is only ever one combination to test.

Every key is in the [configuration reference](/sluice/reference/configuration/).
