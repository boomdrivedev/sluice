---
number: 123
title: A host's runs are shared among its repositories, and one waiting is never left without a run
status: accepted
date: 2026-11-13
decision-makers:
- agentplot-design
---

# A host's runs are shared among its repositories, and one waiting is never left without a run

Status: accepted, agentplot-design's. The statement's 71 and 135 and record 0093 decide it, and it is cheap to change.

Sources: wldn-ops' reading of chuck-sluice-alpha on 2026-11-13, relayed by afterthought-planner and agentplot-planner: switchboard-kit's submissions held all three of the box's runs with four more queued, while willdan-apps-portal's proven bolt waited hours for a run from 01:08Z; the statement in boomdrivedev/blueprints, `design/sluice/requirements.md`, clauses 13, 66, 71, 87 and 135; records 0093, 0119 and 0122.

## Context and problem statement

A host's bound on runs is one for the host (71), held by the one host process (135, record 0093). Nothing says which repository's run takes a part of the bound when one frees. A repository whose queue always has a run ready takes each part as it frees, so another repository with one submission waiting can wait for hours, though a single run would land it.

## Considered options

1. **A freed part goes to the repository with the fewest runs under test that has a run ready** (chosen).
2. **A fixed share for each repository, set in the host's settings.** Every new repository needs a setting, and a share an idle repository holds sits unused.
3. **A run under test is stopped to make room.** Its work is thrown away and its checks start again.
4. **First come, first served across the host.** A busy repository starves the others.

## Decision outcome

- **Fewest running first.** Whenever a part of the host's bound frees, the host process gives it to a repository with a run ready that has the fewest runs under test on the host; among those, the repository whose ready run has waited longest goes first. Within a repository, its own order of runs stands (13).
- **None goes without while it waits.** A repository with a run ready and none under test takes the next part that frees, ahead of every repository holding one, so it waits at most until one run ends.
- **Nothing is stopped.** A run under test keeps its part until it ends; the share applies only when a part frees.
- **A pool's runs are outside it.** A run on a pool takes no part of the host's bound (record 0119), so a repository on a pool neither takes a part nor waits for one.
- **A run waiting for its share is waiting for room.** It reads as waiting for room since when, under no limit (66, 87; record 0122).

### Consequences

- A repository with one submission waiting gets a run within about one run's length, whatever another repository has queued.
- When several repositories have runs ready, the bound divides about evenly among them, so a busy repository's queue moves more slowly while others wait.
- A busy branch's window may grow past what the host gives its repository; the extra candidates wait for room as any run does.

## Amendment, 2026-11-13: what the build of the share chose

Status: accepted on 2026-11-13 as the choices of the change `one-sluice-process-runs-each-hosts-queues`, its design D13 and its code stage, each within the decision above and cheap to change.

Deciders: the design and the code stage of the change `one-sluice-process-runs-each-hosts-queues`.

Sources: the statement's clauses 71 and 135; record 0093 and its build amendment; the change's design D12 and D13; `crates/sluice/src/host_process/room.rs` and `crates/sluice/src/host_process/tasks.rs`.

### Decisions

1. **A repository's runs under test are its runs of every kind.** The share counts each run holding a part of the bound, a combination's, a session's and a preview's alike, read from the host's registry of running checks. *Not taken:* counting a repository's combinations alone, which lets a repository whose agents hold the bound with `sluice exec` take candidates' parts as though idle.
2. **A part is held only for the repository the share puts first.** Among the repositories with a combination waiting, asked within thirty seconds, and the one asking, the room ranks them by their runs under test, then by how long their oldest waiting combination has waited; a repository that asks with no wait recorded waits from that ask. Where more runs are free than one, as many repositories are first as runs are free, so a free run is never left idle for a repository whose round has stopped asking. Any other ask is refused and its wait recorded or renewed. *Not taken:* a part held for whichever repository asks first, which lets a busier repository's round take every part that frees.
3. **The repository a freed part is owed to is woken.** When a part is given back, or the host process's look finds the registry counting fewer runs than before, the room tells the task of the repository the share puts first to pass at once, as a `serve` has it, started where none runs; under passes when asked, nothing passes by itself. *Not taken:* waiting for that repository's next tick, three seconds in which a busier repository asking first is refused and an idle one may find the part taken again.
