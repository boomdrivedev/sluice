---
number: 110
title: A sluice under sluice's own fetch fetches nothing, and the protection judges with nothing fetched
status: accepted
date: 2026-11-10
decision-makers:
- the code stage of fix/main--a-fetch-and-its-hook-share-no-lock
---

# A sluice under sluice's own fetch fetches nothing, and the protection judges with nothing fetched

Status: accepted. The code stage of `fix/main--a-fetch-and-its-hook-share-no-lock` decided it under the statement as written (124, 133, 134, S64) and rules core.28 and core.29, which it keeps. It is cheap to change later.

Deciders: the code stage of `fix/main--a-fetch-and-its-hook-share-no-lock`.
Sources: `agentplot/blueprints:design/sluice/requirements.md` clauses 124, 133 and 134 and scenario S64 at 60c25ef; rules core.28, core.29; records 0059, 0086; signal 2026-11-10-agentplot-ops-635d7c76/01 (deneb, 2026-11-10, sluice 0.73.1 and 0.78.1); the branch-protection spec's requirement "A write is judged by the branch's own value on the host"; `crates/sluice/src/profiles/pushed_refs.rs` `call` and `fetching`, `crates/sluice/src/protect.rs` `judge`, and `crates/sluice-core/src/ops.rs` `needing` as main held them at 69bc9934.

## Context and problem statement

On deneb on 2026-11-10, in agentplot/sluice, `sluice submit main --no-wait --json` took pull request #235 in. Its fetch of the request's head held the repository's fetch lock, `sluice-fetch.lock` (record 0086), while its git ran `git fetch ... +refs/pull/235/head:refs/sluice/origin/pull/235`. That fetch's maintenance ran `git pack-refs --all --prune --auto`. The packing pruned each branch's loose file, and each pruning ran the `reference-transaction` hook, `sluice hook reference-transaction prepared`.

For a deletion of a protected branch, the hook asked which submissions still need the branch before it asked whether the packed refs hold it. That question read the branch's records, which are kept on the remote, so it fetched them. The fetch waited on the fetch lock, which the hook's own ancestor held. git waited for the hook, and the submit waited for git, for more than forty minutes. Every sluice command that reads the repository queued behind them, the runner's acts among them.

The statement already rules this out. Clause 124 lets git's own packing through, since it leaves every protected ref at the value it had. S64 has every fetch start that packing and the protection let it through. The protection reads only what guarding needs. And the branch-protection spec says it fetches nothing to judge a write. The hook broke all of these by reading the branch's state with a fetch to judge a deletion. Beyond this one path, nothing stopped any sluice that a fetch's git runs from waiting on the lock its ancestor holds. How does no sluice process wait on a lock its own ancestor holds?

## Considered options

1. **The protection judges with nothing fetched, and a sluice under sluice's own fetch fetches nothing** (chosen). The hook lets git's packing through before it reads any record. It reads what still needs a branch from the branch's state as this host last fetched it. Only recording a refusal reaches the remote.

   A fetch that holds the repository's fetch lock names the lock's path to its git, in `SLUICE_FETCH_HELD`. Every process that git runs inherits it, the hook among them. A sluice process whose environment names this repository's fetch lock, while that lock is held, takes no lock and runs no git for a fetch. It reads the refs as they stand, as after a race lost to another fetch (record 0086). The fetch above it brings what it would have.
2. Bound the wait for the lock alone, by a try's limit. The deadlock would end only at the limit, with the hook's read failed or the outer git stopped. Every fetch the deadlock catches would wait that long, and a refusal would be logged that the remote never made.
3. Run sluice's fetches with git's maintenance off, `-c maintenance.auto=false`. That removes the one path seen. But it moves the repository's upkeep onto other gits, and any other hook path under a fetch would still deadlock.
4. Find the lock's holder among the process's ancestors, by a process id kept in the lock file and a walk up the process tree. The walk differs between macOS and Linux. An flock names no holder, and a process id outlives no restart.

## Decision outcome

Option 1. The environment names the lock only for the git of a try that holds it, and only a process under that git inherits it. A process that finds the variable with the lock free, as a detached maintenance can after its fetch ended, takes the lock as any fetch does. A process that finds it with the lock held by another fetch than its ancestor's fetches nothing that once. Its refs read as that fetch leaves them, as after a race lost (record 0086).

### Consequences

- A fetch's packing runs the hook, and the hook passes with no record read and nothing fetched.
- A deletion of a protected branch is judged from the branch's state as this host last fetched it. Another host's submission made since the last fetch here does not hold a deletion of this host's copy. The remote's branch is not this host's to delete, and the next round fetches it.
- A refusal the hook records under sluice's own fetch is written from the branch's state as last fetched. Where the remote moved since, the leased write is refused and the hook reports it, as any refusal it could not record is reported.
- Rule core.29 names the case in its details.

## Amendment, 2026-11-24: sluice host's git carries its name, and the hook asks sluice host

Status: accepted. The user approved the change `the-git-hook-asks-sluice-host` on 2026-11-23.

Deciders: the user, on 2026-11-23.

Sources: `residents-do-the-network-and-git-work@a2d68cf95ffe`, `reads-go-through-the-resident@69134e5825d7`; record 0093 as amended 2026-11-24; the change `the-git-hook-asks-sluice-host`, design D1 and D3.

### Decisions

1. **A transaction of sluice host's own git passes by its name.** Before: a fetch of sluice's own whose packing ran the hook passed because the hook read the refs as that fetch left them, with no lock and nothing fetched. Now: each git command of `sluice host` carries `SLUICE_HOST_PROCESS`, the pid of `sluice host` and its start time, and the hook lets its transaction go with no ask where that pid runs and started at that time. So a fetch of `sluice host` whose packing runs the hook waits on nothing, and the hook asks `sluice host` nothing about a write that `sluice host` decided. A value whose pid does not run, or started at another time, names no `sluice host`, and the hook asks.
2. **sluice host judges every other transaction with nothing fetched.** Before: the hook judged in its own process. Now: `sluice host` judges what the hook sends, as option 1 judged it: git's packing is let through before any record is read, and what still needs a branch is read from its state as this host last fetched it. It writes each refusal under the branch's lock.
