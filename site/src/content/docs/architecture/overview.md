---
title: How sluice is built
description: A small core that knows git, and the seams it reaches everything else through.
sidebar:
  order: 1
---

A small core that knows git, and five seams it reaches everything else
through: where a combination is checked out, how its checks run and what
holds them, how the branch moves, how agents submit, and where the state
lives. Each seam is bound by a profile, so one core works with moon or a
plain command, a sandbox or none, local refs or a forge, files on one
host or refs on a shared remote.

## The statement

The design is a statement of invariants in
[agentplot/blueprints](https://github.com/agentplot/blueprints),
`design/sluice/requirements.md`: what is always true of sluice and what
it never does, as numbered clauses, written so that any model satisfying
them is sluice. This repository cites its clauses by number, keeps the
[rules](/sluice/architecture/rules/) a change must respect, and records
each decision the statement left open as an
[ADR](/sluice/decisions/0001/). A clause that no longer holds is changed
in the statement first, never worked around in code.

## The core

The core forms combinations, keeps the chain and how many are tested at
once, records results, blames failures and lands, using git alone. It is
a step function over the state ([ADR 0005](/sluice/decisions/0005/)):
a round reads a branch's state and the seams' answers, decides the next
actions, performs them, writes the state back, and holds nothing between
rounds. Every effect is written to the state before it is performed, so
a round that crashes at any point is finished correctly by the next one.

A round takes the branch's lock, so two rounds never act on one branch
at once. sluice's own process on the host runs them, one task for each
repository with work: the first command that needs the host's queues
moved starts it, and it ends once none has work
([ADR 0093](/sluice/decisions/0093/)). `sluice step` asks it for one
round of a branch, and a waiting `sluice submit` or `sluice wait`
follows it. Nothing behaves differently because of what asked for the
round.

## The seams

| seam | what the core asks of it | profiles |
|---|---|---|
| **the checkout** | a clean checkout of exactly one combination, unique and stable for the life of its run, released when the result is read | a detached git worktree ([ADR 0013](/sluice/decisions/0013/)); a container, planned for a box that runs no agents ([executors and systems](/sluice/guides/executors-and-systems/)) |
| **the check** | which tasks a combination affects; a run of them in a checkout, each task with its key, outcome and whether it was replayed; and the run held to the limits the policy states, by an executor that reports what it enforced ([ADR 0012](/sluice/decisions/0012/)) | moon, or one command; executors `open`, `sandbox` (Seatbelt on macOS; bubblewrap and Landlock on Linux, planned) and `container` |
| **the branch** | its current revision; a merge tree of the branch and a head; a move from an expected revision to a new one, refused if the branch is elsewhere; protection against every other writer | local refs, with a `reference-transaction` hook; pushed refs on a forge, with the forge's protection and sluice's identity the only bypass |
| **the driver** | submit, withdraw, amend, state, and telling the agent the outcome | `sluice submit` from any shell; `wt submit` through worktrunk; a tool's own merge stage calling the same |
| **the state** | a branch's state as last written, with its version; a write from that version, refused when the version moved ([ADR 0008](/sluice/decisions/0008/)) | local files under the git directory; refs on the shared remote, for peers; a server's files, projected to the remote |

The core names no tool: `moon`, `wt`, `devenv` and the rest appear only
in profiles, and a profile implements its seam's interface and nothing
of the core.

## What sluice never does

- **It starts no service and binds no port**
  ([ADR 0010](/sluice/decisions/0010/)). The checks it runs are one
  process group, ended with their checkout or their time limit. Whatever
  the tests need is brought up and torn down by the test command, as
  every CI system leaves it to the job.
- **It runs no check of its own and changes nothing a gate requires.**
  The policy is the repository's, read from the branch's current
  revision, never from a submission.
- **It holds nothing a reader needs in a running process.** Every record
  is readable with no sluice process running, with plain tools, from
  every host that submits to its branch.
- **It never moves a branch past what passed,** and never from a
  revision other than the one the combination was tested on.
- **It takes no word.** Whether work may be submitted at all, approval
  included, is decided before it reaches sluice.
