---
number: 132
title: A project's own moon.yml affects that project's checks
status: accepted
date: 2026-11-24
decision-makers:
- the user, through the orchestrator, on 2026-11-24
- the construct stage of change/the-gate-runs-in-five-minutes
---

# A project's own moon.yml affects that project's checks

Status: accepted. The user chose on 2026-11-24 to land the change `the-gate-runs-in-five-minutes` with its gate reading only the projects a submission affects. The form below is the change's design, D1.

Deciders: the user, through the orchestrator of the sluice places, and the construct stage of `change/the-gate-runs-in-five-minutes`.

Sources: the statement in boomdrivedev/blueprints, `design/sluice/requirements.md`, clauses 15, 36 and 140; record 0131; the user's words of 2026-11-24, "why is the sluice test suite 30 minutes"; the landings #146 and #147 of 2026-11-23, each of which changed one crate's `moon.yml` and so ran all 31 of the gate's tasks, the herdr plugin's two suites of 8.5 minutes each among them.

## Context and problem statement

The moon runner narrows a run to the required checks its changed files affect (36). Where a changed file was moon's configuration, `.moon/` or any `moon.yml`, it ran every required check. The reading that lands work beside the chain (140) used a wider list, `own_setting`: moon's configuration and the toolchain's files.

A project's `moon.yml` defines that project's tasks: their commands, inputs and dependencies. moon reads such a file as affecting a task only where the task names it as an input, so sluice read every change to one as a change to the whole graph.

## Considered options

1. **A project's own `moon.yml` affects that project's tasks and what depends on them** (chosen). The settings every check reads, `.moon/`, the toolchain's files and the branch's policy, still run every required check. One list, `own_setting`, serves the run and the reading beside the chain.
2. **Every `moon.yml` runs every check.** Safe, and every change to a task's settings runs the whole gate.
3. **Option 1 without the branch's policy.** The policy is read from the branch's revision (15), so a change to it never decides its own gate; it is kept in the list, since a change that moves a check between tiers is worth every check.

## Decision outcome

A changed `<dir>/moon.yml` marks every task of the project whose source is `<dir>`, and every task that depends on one of them at any depth, as affected. The projects are read by `moon query projects`, asked only where a `moon.yml` changed. A change to `.moon/`, to a toolchain file `own_setting` names or to `.config/sluice.toml` runs every required check, in a combination's run, a worktree's run and the reading beside the chain alike.

Before: a change to any `moon.yml` ran every required check of the branch, whatever project it belonged to. The toolchain's files ran every check beside the chain but not in a run.

Now: a change to a project's `moon.yml` runs that project's checks and the checks that depend on them. A change to `.moon/`, the toolchain's files or the branch's policy runs every check, in every reading.

### Consequences

- A task of another project changes its result through a project's settings only by depending on one of its tasks, which the reading follows. moon reads the graph from the place, which holds the new `moon.yml`.
- A `moon.yml` of a project no required check names or depends on lands beside the chain.
- A change to the toolchain's files now runs every check in a run too. Nearly every task names `devenv.nix` and `devenv.lock` as inputs already, so little changes for this repository.
