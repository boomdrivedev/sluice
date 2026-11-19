---
number: 130
title: A pull request's wait is named by its number
status: accepted
date: 2026-11-19
decision-makers:
- the design stage of change/a-wait-follows-its-own-pull-request
- the code stage of change/a-wait-follows-its-own-pull-request
---

# A pull request's wait is named by its number

Status: accepted. The statement fixes no handle for a pull request's wait, and no clause changes. 137 has a wait show a request's entry there by its number, and C.5 has a submit that does not wait answer with the entry, or with the request where there is none. Both hold as written. Under rule core.48, the choices below are the change's own, its design's D1 and D2, each cheap to change.

Deciders: the design stage of the change `a-wait-follows-its-own-pull-request`, on the bolt `bolt/sluice-checks-and-waits-on-an-agents-own-work`, from the signal below; its code stage, which built it.

Sources: the statement, `design/sluice/requirements.md`, clauses 26, 51, 124, 125 and 137 and C.5's pull request row; records 0100 and 0114; rule core.48; the signal `2026-11-19-boomdrive-ops-1282975b/01`; the change `a-wait-follows-its-own-pull-request`, its proposal and design D1 to D7; `crates/sluice/src/lib.rs` (`submit_pull`, `submit_pull_group`, `Command::Wait`) as the bolt holds it at 13bfe4474.

## Context and problem statement

An agent sent its work as pull request 791 to switchboard-kit's `main`. Another host takes `main`'s pull requests in, so `sluice submit main --no-wait --json` answered with the pull request alone and no submission number (137, C.5). The agent ran `sluice wait main`. At its time limit, the wait's document carried the other host's `queue`, `"submission": 554`. The agent then waited on 554 by number. On its own host, 554 named another agent's submission, wldn-design's `design/enrolment-holds`, and the agent took that submission's end as its own.

The wait in the worktree had followed the right work: sluice's check on 791's head, posted by the host that took it in, named submission 554 on that host and its landing. The number came from another host's records. A submission's number names an entry only in the records that hold it (26; record 0114), and before sluice takes a pull request in there is none. Nothing sluice printed for a pull request said how to wait on it but `sluice wait <branch>`, which reads the folder it runs in.

What does a pull request's answer name as the wait that follows it, the same on every host and from every folder?

## Considered options

**The handle a pull request's wait is named by.**

1. **The pull request's number, `sluice wait <branch> --pull-request <n>`** (chosen).
2. The submission's number.
3. `sluice wait <branch>`, run in the worktree that opened the request.
4. A target spelled `#<n>` or `pull/<n>`.

**How a wait by pull request follows it, and to whom it hands the outcome.**

- a. **`sluice wait <branch>`'s own loop, given the number; the folder the wait runs in decides the hand-over** (chosen).
- b. The same loop, handing the outcome over to the worktree that opened the request, wherever the wait runs.

## Decision outcome

1. **The pull request's number is the handle (option 1).** The forge gives the number when the request opens. It is the same on every host and in every folder, and it names one request for the life of the work (record 0100 D4). A request's wait is `sluice wait <branch> --pull-request <n>`, one function spells it, and every answer sluice gives for a request still open and undecided names it: the `--no-wait` answers, with a submission and without; `--status`'s document and words; the document and the `still open: #<n>; …` line of a wait out of time or stopped; each request of a group act; and the command a wait that could not read names to run again. Under `--json` it is `wait`, the command as one string, as an error document's `next` holds commands. In words it is a line of its own, `wait: <command>`.
   - Not taken: option 2. No submission exists until the request is taken in. Across hosts, the number is the other host's records' (26; record 0114), and in the signal's case it named another's work here.
   - Not taken: option 3. It follows whatever request the folder it runs in opened. A submission by name and a group act have no one folder (124, 125). The signal's agent ran exactly this, and at its time limit had nothing to go on.
   - Not taken: option 4. In a shell, `#` begins a comment, so `sluice wait #791` runs `sluice wait`. `pull/791` is a branch name git allows, so the target would name either. A flag says what it names.
2. **A wait by pull request is `sluice wait <branch>`'s loop, given the number (option a).** `sluice wait <branch> --pull-request <n>` reads once whether the forge holds pull request `<n>` into the branch, through the pull request interface (rule core.2), and then follows it as `sluice wait <branch>` follows the worktree's own. It takes as its own the first submission of the request it reads, waits on that one once the request is taken in as another (record 0100 D4), and follows the forge and sluice's check where another host takes the request in (137). Its outcomes, exits and documents are that loop's. A branch that takes no pull requests, or no such request into the branch, is not found, exit `1`. A submission's number given with `--pull-request` is a command line sluice cannot read, exit `64`.
   - The folder still decides the hand-over. A wait in the request's own worktree hands its outcome over as `sluice wait <branch>` does there. A wait anywhere else hands nothing over, since nothing of the request is owed to that folder (51; rules core.50, core.52).
   - Not taken: option b. A wait records a hand-over only to the reader it runs for. The request's own reader collects the outcome with `sluice inbox` (51).
3. **A submission's record gains no field.** Once this host holds the request's submission, a wait prints that submission's record, whose `id` is this host's and whose `sluice wait <id>` follows it. A wait that follows a request for submission `<id>` names `sluice wait <id>` to run again.

### Consequences

- Good: an agent hands its background wait the command the answer names, the same on every host and from every folder, and never another host's number.
- Good: a wait by pull request ends with the request's own outcome, never another submission's (rule core.48).
- Bad: a wait by pull request from another folder hands nothing over. Its outcome stays owed to the request's own reader, which `sluice inbox` in its worktree collects, and the round's notice reaches its session (51).
- Bad: one more read of the forge per wait by pull request, conditional and kept for the host's other processes (136; rule core.32).
- Neutral: scripts that compare a document whole see `wait` beside the fields they read. None is renamed or removed.
- Rule core.48 holds the decision.
