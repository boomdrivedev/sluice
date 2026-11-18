---
title: Setting up sluice
description: What a developer or an agent sees when sluice is added to a repository on one machine.
sidebar:
  order: 1
---

What a developer or an agent sees when sluice is added to a repository on
one machine. The repository runs its tests with moon, and its agents each
work in a git worktree of their own. Everything here runs today on one
machine; sluice lands its own `main` this way. If the agents work in
worktrunk, read this page first and then
[worktrunk integration](/sluice/guides/worktrunk/).

## What sluice does, in one paragraph

Agents finish work in their own worktrees and **submit** it to a target
branch instead of merging it. sluice queues the submissions, tests each
one **combined with everything queued ahead of it**, exactly as it would
land, and merges it only if that combination passes. When a combination
fails, it works out which one submission broke it and tells only that
agent. No agent ever merges, and the target branch only ever moves to
code that passed.

## 1. Install

```sh
nix profile install github:agentplot/sluice
```

That installs `sluice`, and beside it `wt-submit`, which matters only to
a team on [worktrunk](/sluice/guides/worktrunk/). (`cargo install --git
https://github.com/agentplot/sluice sluice` does the same without nix.)

## 2. The config: `.config/sluice.toml`

One file, checked in. sluice reads it from the target branch, not from
the submission, so a submission cannot loosen the rules it is tested
under.

```toml
[tests]
runner = "moon"
toolchain = ["devenv", "print-dev-env"]

[executor]
kind = "sandbox"
network = "localhost"

[landing]
subject = "chore(land): #{id} {from}"

[branches."feature/*"]
required = ["#suite"]
parallel = 4
batch = false
flaky = "fail"
timeout = "20m"
trust = ["open", "sandbox", "container", "cache"]

[branches.main]
required = ["#suite", "#live"]
parallel = 2
batch = true
trust = ["confined", "sandbox"]
```

Every table and key is in the
[configuration reference](/sluice/reference/configuration/). The parts
that matter most:

**`[tests]`** says how tests run: with moon, so only the tasks a
combination affects run and unchanged ones replay their earlier result,
or as one command. Which tasks must pass is not here: it is set per
branch, because that is the point. A branch like `feature/*` here lands
small changes many times a day and wants a fast gate; `main` lands whole
features and can demand more.

**`[checkout]`** says where a combination is tested: in a clean checkout
of its own, apart from every agent's worktree, with nothing running in it
but the tests. A checkout starts from its revision alone. For a warm
build, the host's config declares the caches every run on the host
shares, a compile server's or moon's, as
[`[[caches]]`](/sluice/reference/configuration/#caches). A cache only
spares work: it never decides a result.

**`[executor]`** says what the tests may touch: nothing held back, or a
sandbox that may write only its checkout and scratch directory and reach
only the network the branch allows. Every result records which executor
ran it and what it held.

**`[branches."<pattern>"]`** is the rule per target branch: which tasks
must pass, how many combinations are tested at once, whether every
combination or only the longest is tested, how a flaky test reads, the
time limit, and which results the branch trusts.

### Services the tests need

sluice starts no services. If the tests need a database or a dev server,
the test command brings it up and tears it down, the way every CI system
and merge queue leaves it to the job: a moon task that depends on a
server task (moon 2.6's `wait` and `cleanup` dependencies), `devenv test`
with its processes and per-instance ports, testcontainers, Playwright's
`webServer`, a pytest fixture. Ports come from the tool: a free port
picked at start, or port 0. Such a port is the host's to hand any
process next, so two checkouts running side by side can collide on it.
A stack that may run beside another names its ports in
[`[checkout] ports`](/sluice/reference/configuration/#checkout-where-a-combination-is-tested)
and reads each from `SLUICE_PORT_<NAME>`. sluice gives every checkout
ports no other on the host holds, the same for its whole life and free
when the tests receive them.

What sluice does owe the tests, and nothing more:

- a clean checkout of exactly the combination under test;
- `SLUICE_CHECKOUT` and `SLUICE_TMP` in the environment: where the
  checkout is, and a scratch directory of its own;
- a time limit per branch: a run that hits it is recorded as timed out,
  and a test still running then leaves no result under its key;
- the test command run in its own process group, which sluice kills when
  the command exits, times out, or the checkout is removed, so a server
  left running can never break the next combination and get the wrong
  submission blamed.

Anything that escapes the process group (a docker container, a detached
daemon) is the repository's to clean up, through moon's `cleanup`
dependencies, devenv, or testcontainers' reaper.

An agent running the same tests while it works should run them with
`sluice exec <branch>` in its worktree rather than on its own. Several
agents on one host then share the host's bound with the submissions'
runs, and each worktree gets ports no other holds. A stack the tests
bring up stays between runs and goes with `sluice release`, or with the
next run or round on the host once the worktree is removed. A run in
a clean worktree is recorded for the submission that follows, which
replays it when the policy declares the cache, as one combination
replays another's. See
[commands](/sluice/reference/commands/#sluice-exec).

The same stack serves a person who wants to try what a branch landed:
`sluice preview <branch> <revision>` brings it up in a checkout of its
own, at addresses the policy's `[preview]` table names, and keeps it up
until it is released. See [previews](/sluice/guides/previews/).

## 3. What changes for the agents

Agents stop merging. Whatever step merged a finished branch into its
target, a `git merge` in a script, a tool's merge command, a hook around
either, becomes one command, run in the agent's worktree with its work
committed:

```sh
sluice submit feature/concurrent-units
```

A hook that ran the suite before merging goes: sluice runs the suite on
the exact combination that will land, which is what that hook was
approximating. A check after landing goes too, since nothing lands
untested. Agents still run their tests while they work; they never
merge.

A team on worktrunk makes the same change, from `wt merge` to
`wt submit`; [worktrunk integration](/sluice/guides/worktrunk/) has the
details.

## 4. Branch protection

Every branch with a `[branches]` section is protected: only sluice moves
it. `sluice protect` (and every `submit`, `step` and `run`) installs a
git `reference-transaction` hook that refuses any other update and logs
the attempt:

```console
$ git merge touches        # on feature/concurrent-units
error: feature/concurrent-units is protected by sluice; submit the change to sluice instead
```

The hook calls the installed `sluice`, never one built from a submission,
so no submission changes the guard that judges it.

On a forge, the same is the forge's branch protection, with sluice's
identity the only one allowed to push.

## 5. An agent submits

Whatever submits work to sluice and gets the outcome back is a
**driver**: it is the front door agents and tools use, and it decides
nothing about the queue. `sluice submit` from any shell is the plain
driver, and the one here. A tool's own merge stage, or worktrunk's
`wt submit`, calls the same `sluice submit` underneath.

```console
$ sluice submit feature/concurrent-units
submission 12 to feature/concurrent-units · nothing queued ahead
testing feature/concurrent-units + #12 · 3s of 15m
testing feature/concurrent-units + #12 · 32/33 tasks · running tests:t-cards 1m31s · 2m13s of 15m
testing feature/concurrent-units + #12 · 2 tasks to run, 31 replayed
passed · landed as b7d2e94
```

`sluice submit` waits for the outcome and exits with it: 0 landed, 1
failed, 2 conflict, 4 withdrawn. While it waits, it says how far its
combination has got. `--no-wait` returns at once, and `sluice wait 12`
waits for it later, running the rounds as a waiting submit does; an
agent whose tool calls are capped runs `sluice wait 12 --timeout 9m
--json` again for as long as it exits 5. `sluice status 12` reads it
without waiting. The agent's worktree and branch stay as they were:
removing them after a landing is a driver's business, and the plain
driver leaves it to the agent.

The agent in `batch-review` submitted two minutes later, so its
combination includes #12:

```console
$ sluice submit feature/concurrent-units
submission 13 to feature/concurrent-units · 1 queued ahead (#12)
testing feature/concurrent-units + #12 + #13 · 4 tasks to run, 29 replayed
failed: tests:t-batch-cards
  on feature/concurrent-units + #12 + #13 (e20a6f9)
  #12 passed on its own, so the failure is from this submission
  log of tests:t-batch-cards: .git/sluice/.logs/feature/concurrent-units/c7/tests:t-batch-cards.log
the agent working in batch-review fixes it there and submits again
```

A conflict is found before any test runs:

```console
$ sluice submit feature/concurrent-units
submission 14 to feature/concurrent-units · 1 queued ahead (#12)
conflict: plugin/lib/plan.py
  conflicts with #12 (agent-1, touches)
the agent working in forecast merges or rebases onto feature/concurrent-units there and submits again
```

A change no required check reads lands at once, even while #12 and #13
are under test, and they land after it without being tested again:

```console
$ sluice submit feature/concurrent-units
submission 15 to feature/concurrent-units · 2 queued ahead (#12, #13)
landed as 4e1a07c beside the queue: no required check reads its changes
```

Its landing commit says it passed with nothing run. It names the result
that covers the revision it landed on, by the revision that result
landed as and its own words.

```console
$ git log -1 --format=%B 4e1a07c
chore(land): #15 guide

Sluice-Submission: 15
Sluice-From: agent-4 (guide)
Sluice-Result: passed (0 run, 0 replayed) beside the queue: no required check reads its changes; covered by the result at c41e8f2d7a09b3e5f1c6284d0a9e7b3c5f18d2a6, passed (3 run, 28 replayed)
```

And before submitting, an agent can ask whether its work would conflict:

```console
$ sluice submit --check feature/concurrent-units
would conflict: plugin/lib/plan.py with #12 (agent-1, touches)
```

## 6. What keeps the queue moving

`sluice submit` queues its head and tells sluice's own process on the
host, starting it if none is running. That process keeps the queue
moving: every few seconds it runs one round of sluice's work for each
branch with work, in every repository on the host, and ends once none
has any. A round is: read the queue, start tests if there is room, read
finished tests, blame a failure, land what passed, write the queue back.
A waiting submit, or `sluice wait`, only follows it: ten agents waiting
on one branch are ten readers of one queue, not ten rounds.

Nothing needs to be kept running. One command asks for a round by hand:

```sh
sluice step feature/concurrent-units   # one round, for one branch, then exit
```

`sluice step` is for a tool that already has a loop of its own; the
round it asks for is the one sluice's process runs.

## 7. Where sluice keeps its state

In the repository's git directory, never committed and never pushed,
readable with plain tools whether or not sluice is running:

```
.git/sluice/feature/concurrent-units/
  version              57: bumped on every write, so two rounds never both write
  submissions/14.json  a submission still in the queue
  .finished/12.json    one that landed or was withdrawn, moved out of the way
  log.jsonl            every submission, test run, result, blame and landing, in order
.git/sluice/.logs/feature/concurrent-units/<candidate>/<task>.log
                       what each failed test printed, for the last 20 failed combinations
.git/sluice/results.jsonl
                       every test result of the repository, with its executor
```

The files, the submission record and the log's events are in the
[state reference](/sluice/reference/state/).

## 8. What a landing looks like in history

One merge commit per submission: first parent the branch, second parent
the agent's head, so the agent's commits stay in history and git counts
its branch as merged. What was tested rides along as trailers, which
outlive the state files:

```console
$ git log -1 feature/concurrent-units
commit b7d2e94f03
Merge: c41e8f2 3f9c2e1
Author: sluice <sluice@localhost>

    chore(land): #12 touches

    Sluice-Submission: 12
    Sluice-From: agent-1 (touches)
    Sluice-Tested: 9ab3d07c11
    Sluice-Result: passed (2 run, 31 replayed)
```

## 9. Reading and changing the queue

```console
$ sluice status feature/concurrent-units
feature/concurrent-units at b7d2e94 · 1 in the queue · testing 2 at once (up to 4)
  #14 forecast  testing   agent-3  forecast   feature/concurrent-units + #14
2 no longer in the queue: sluice status feature/concurrent-units --all
```

It shows the submissions still in the queue, each by its number and its
subject, or the branch it came from where its submit gave none. `--all`
adds every one the branch has had, landed, failed and withdrawn. `--json` lists every one,
with the queue's count as `depth`, for any other tool.

A queued submission's order can be changed until it is under test:

```sh
sluice amend 14 --after 12      # #14 lands only after #12
sluice amend 14 --priority 1    # ahead of priority-0 submissions not yet under test
```

Submitting again from the same worktree withdraws that worktree's earlier
submission.
