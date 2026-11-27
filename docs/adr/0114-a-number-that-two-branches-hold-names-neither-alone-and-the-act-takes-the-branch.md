---
number: 114
title: A number that two branches hold names neither alone, and the act takes the branch
status: accepted
date: 2026-11-12
decision-makers:
- agentplot-design
---

# A number that two branches hold names neither alone, and the act takes the branch

Status: accepted on 2026-11-12 by agentplot-design. It is the design agent's own choice, decided by the statement's 26 and rule core.11. It is cheap to change and holds no choice of the user's.

Sources: the statement's 26, as amended on agentplot/blueprints's branch `design/sluice-a-number-two-branches-hold-names-neither` (`94fd86b`); `crates/sluice-core/src/lib.rs` `find`, `crates/sluice-core/src/ops.rs` `lookup` and `crates/sluice/src/profiles/records.rs` `branches` and `allocate_id`, at `v0.78.1` and `a9edcd90`; wldn-ops's readings of switchboard-kit on chuck-herdr-alpha, 2026-11-12, relayed by agentplot-operator.

## Context and problem statement

A branch whose records are on its remote takes its submissions' numbers from the remote's own count, and a branch whose records are the host's takes them from the host's count. So one host can read two submissions with one number. On chuck-herdr-alpha on 2026-11-12, `sluice withdraw 26` meant `bolt/vendor-recordings-need-no-repository-deletes`'s #26, held on GitHub. It was refused because `bolt/a-class-builds-the-next-releases-head` holds a landed #26 in the host's own files. `sluice withdraw 25` failed the same way. A lookup by number reads the host's branches in name order and takes the first that holds the number. `withdraw`, `status`, `wait` and `amend` take a number alone, so nothing could reach either submission.

## Considered options

1. **A number held on more than one branch is refused, naming each branch, and the act takes the branch** (chosen).
2. **One count for every branch a host reads.** Numbers already given on both counts collide, and a remote's count is shared by every host on it, so no host's count can be made to agree with it.
3. **The first branch in name order, as now.** It acts on a submission its asker never meant, and it acts silently.

## Decision outcome

- **A number names a submission only where one branch holds it.** A lookup by number reads every branch it can (26). Where more than one holds the number, it is refused, naming each branch that does, and nothing is done.
- **The act takes the branch.** Every act that names a submission by its number takes the branch too, and with it reads that branch alone: `withdraw`, `status`, `wait` and `amend`. A number held on one branch needs no branch, as now.
- **A branch that cannot be read is said as now.** The refusal names the branches that could not be read beside those that hold the number (core.11).
- **The act's form.** The branch is `--branch <branch>`, given with a number alone. The refusal exits `2` from `status`, `amend` and `withdraw`, as their other refusals do, and `64` from `wait`, whose `2` is a conflict that `wt submit` and an agent's wait read as the work's outcome. Under `--json`, its document's `next` gives the command with `--branch` once for each branch that holds the number. A number a submission held before its branch's records moved to the remote is read the same way. These are the change `a-number-two-branches-hold-names-neither`'s own choices (its design's D4 and D5), cheap to change. The paths not taken: `64` from all four, which a script calling `withdraw` or `amend` would read apart from every other refusal of theirs, `2`; and `<branch>#<n>` in place of the number, where a flag reads the same on all four commands and leaves `wait <branch>` and `status <branch>` as they are.

### Consequences

- A session's own `wait` after its `submit` is unchanged where its number is held on one branch, which is every branch with records in one place.
- The statement's 26 is amended on agentplot/blueprints's branch above, which the user pushes.

## Amendment, 2026-11-26: the worktree's own submission names the branch, and a submit gives its wait

Status: accepted on 2026-11-26 on the brief for the place `fix--a-submission-is-named-by-its-branch`. A new version of the claim `a-number-two-branches-hold-names-neither-alone` that says it is proposed in Boom, for the user to accept.

Deciders: the brief's author, on 2026-11-26. The change `a-submission-is-named-by-its-branch` made the choices within it (its design's D1 to D3).

Sources: deneb on 2026-11-26. After each submit, agents ran `sluice wait <id>`, and sluice refused it. The number was on `main` and on an old bolt (boomdrive-cloud #37, boomdrive #40 and #44, sluice #162). Each agent ran the wait again with `--branch main`, and boomdrive's runner got the refusal every 4 seconds until it named the branch. Rules core.11, core.25 and core.50.

sluice knew the branch each time. The submit's answer named the branch but gave no command that names it. The worktree that waited made the submission.

1. **A submit's answer gives its wait.** Before: `sluice submit --no-wait --json` gave `{id,branch,ahead}`. Now: it also gives `wait`, `sluice wait <id> --branch <branch>`, which follows the submission from any folder. The answer in words under `--no-wait` gives the line `sluice wait <id> --branch <branch> follows it`. A submit of several worktrees gives both for each submission.
2. **The worktree's own submission is the one the number names.** Before: a number that more than one branch holds was refused for every command that gave no `--branch`. Now: `wait`, `status`, `amend` and `withdraw` take the branch where the worktree they run in made that number. They read the worktree's own submission by the one reading of a worktree's own submission that the inbox and a wait's delivery use (`Sluice::made`). A pull request's submission is the worktree's own where the request was opened from the worktree's branch. This holds only where the worktree made the number on one of those branches only, and every branch could be read. Anywhere else the number is refused as before, naming each branch. A branch that could not be read keeps the refusal, since it can hold the number from the same worktree (core.11). A submission by name is no worktree's (core.50).
3. **`sluice host` runs from its client's folder.** Before: `sluice host` ran a status, an amend and a withdrawal with the folder `.`, its own. Now: it runs them with the folder the request names. Every client already sends that folder, so a client of an earlier release gets the same answer.

*Not taken:* refuse the number always and fix only the submit's answer. The session context and the skill tell agents to wait with `sluice wait <id>`, and such a wait would still be refused.
