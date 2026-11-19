---
number: 130
title: A worktree's checks read its own changes and replay what sluice passed
status: accepted
date: 2026-11-19
decision-makers:
- the brief of the change a-worktrees-checks-run-only-what-its-own-changes-affect
- the construct stage of change/a-worktrees-checks-run-only-what-its-own-changes-affect
---

# A worktree's checks read its own changes and replay what sluice passed

Status: accepted. The brief decides what a worktree's run reads and replays, behind the user's constraint that a change reaches a host in minutes. How it does so is the construct stage's choice, each part cheap to change.

Deciders: the brief of the change `a-worktrees-checks-run-only-what-its-own-changes-affect`, on the bolt `bolt/sluice-checks-and-waits-on-an-agents-own-work`, and the construct stage of `change/a-worktrees-checks-run-only-what-its-own-changes-affect`, which chose the form below (the change's design, D1 to D7).

Sources: the statement in boomdrivedev/blueprints, `design/sluice/requirements.md`, clauses 16, 36, 70, 108 and 138, and session-runs-1's clauses 92 and 93; records 0004, 0015 and 0035; signal 2026-11-19-boomdrive-ops-b31633c8/01; ops' root-cause analysis of 2026-11-18, cause 5: an agent's `sluice exec <base>` took 6 to 35 minutes where the queue's run of the same change took 3 to 4, and one fix's agent waited about 123 minutes on such runs while the queue ran its 13 tasks in 3.7 minutes (#283). moon 2.6.1, probed on 2026-11-19 in a scratch workspace: `moon query tasks --affected` reads the changed files from its standard input as `{"files": [...]}`, reads from the merge base given `MOON_BASE` and `MOON_HEAD`, and reads nothing a worktree holds beyond its head given a range.

## Context and problem statement

A worktree's `sluice exec <branch>` ran every affected check with moon's `--force`. A check sluice had passed on the same files ran again, with every task it depends on. A combination's run lets moon replay, and reads each replay against the passes the branch's gate counts (108). No record decided the `--force`: the code said a session run's record must be observed (16), and clause 16 says only that a replay is never recorded as a run, which `record_session` already keeps.

The run took its affected checks against the branch's revision. sluice asked whether moon's configuration changed between that revision and the worktree's head, a diff. In a worktree cut before the branch last changed a `moon.yml` it found a change, and ran every required check. moon was handed a revision range, which holds none of a worktree's uncommitted edits.

## Considered options

### Where a worktree's own changes start

1. **The merge base of its head and the branch's revision** (chosen). The branch seam answers it beside `contains`: `local_refs` by `git merge-base`, `None` where the two share no commit, and every other profile hands it on. `Sluice::session_since` gives it, and a run of the checks takes it as its `since`. With `None` every required check runs (70), and the run says so.
2. **The branch's revision, relying on moon's own merge base.** sluice's own check of moon's configuration still diffs the two revisions and finds the branch's `moon.yml` changes, and a range still holds none of the worktree's uncommitted paths.
3. **The moon profile finds the merge base in the place.** The run's opening line needs it before the run, and the core and the profile would each hold a reading of one fact.

### Whether a worktree's run replays

1. **It replays, and its replays are read as a combination's** (chosen). A run of the checks asks for `replay: true`, so moon runs without `--force`. `finish_session` reads the report through `verdict::read` against the branch's results for the keys it replayed, under the branch's rules, on the host's own system, with nothing given to prove. A replay left unproven is carried to `sluice exec`. A proven replay whose key holds a counted fail too reads as the branch's `flaky` says. Nothing of the reading is recorded or decides anything (rule core.18), and the replays are never recorded as runs (16; rule core.9).
2. **A bar of the worktree's own: any pass sluice recorded, counted or not.** The passes the gate counts are what the combination of the head will replay. Where a host signs its clean worktrees' results, a worktree's own recorded pass already proves its replay.
3. **Narrow the affected reading and keep `--force`.** A check sluice already passed on the same files would still run.

### What becomes of a replay sluice holds no pass for

1. **It runs again in the same `sluice exec`** (chosen). exec says `replayed with no result sluice recorded: <task>, …; running them again` and starts a second session run in the worktree: those tasks, every one run, no replay, nothing read beyond its head. It asks the host process for room as the first run did (rule core.49), and is read and recorded on its own under clause 92's conditions. The summary, `--json`, the exit and the worktree's last result are of the two runs joined. A run again replays nothing, so it never runs again itself.
2. **Hold the first run's room for the second.** A run counts toward the bound for as long as it runs (89), and rule core.49 hands out room in turn.
3. **Leave the unproven replays not run and exit 1.** The agent's next `sluice exec` would meet the same replay.

## Decision outcome

A worktree's run of the checks reads the same checks the queue's combination of its head would read, from the same proof of a replay. Its own changes are the files that differ between the merge base of its head and the branch's revision and the worktree as it stands, its uncommitted and untracked files among them, handed to moon on its standard input. It replays what a pass the gate counts proves, and runs again, in the same command, a replay nothing proves.

### Consequences

- A worktree that never commits records nothing (92), so each of its replays runs again, taking no longer than the forced run did, and the words say why.
- A host whose worktrees share no moon cache replays only what a worktree's own earlier runs left; the host's `[[caches]]` decide the sharing, as for combinations.
- The run again waits for room a second time, only where moon's cache held a pass sluice never recorded.
- Rule core.9 holds the decision: a worktree's run answered from the runner's cache where a pass the gate counts is recorded for the key is not ruled out, and a worktree's replay told as passed with no such pass is.
