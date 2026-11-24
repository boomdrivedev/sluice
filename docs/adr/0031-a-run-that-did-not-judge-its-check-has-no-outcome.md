---
number: 31
title: A run that did not judge its check has no outcome
status: accepted
date: 2026-10-19
decision-makers:
- Chuck Swanberg
---

# A run that did not judge its check has no outcome

Deciders: the operator, after a host's failure made `main`'s keys unpassable and agents stopped running `sluice exec` on sluice, 2026-10-19.
Sources: could-not-run-1 in agentplot/blueprints (114 to 116, amended 84; unratified); clauses 16, 19, 38, 60, 69; rule core.7; the OpenSpec change `could-not-run`.

## Context and problem statement

On 2026-10-17 a `sluice exec main` recorded fails for `cli:e2e` and `cli:tools` while the Mac was failing. Eight minutes later the gate passed both on the same tree. The submission still failed: its keys held a pass and a fail, and `main` reads a flaky key as failed (19). Every result stays (rule core.7), so the same inputs could never pass again, and an agent concluded that sluice must not run its own checks.

The check seam had two endings, pass and fail. A check whose host lacked what it needs, such as a container engine that is down or a launch that stalls past a test's own timeout, ended in a fail recorded under its key, the same as a broken test. sluice cannot know what a repository's tests need: one needs a container engine, another a database, a GPU or a licence server. How does a run that never judged its check stay off the key?

## Considered options

1. Probe the host in the executor: ping the container engine's socket, time a launch.
2. Retry a failed check once before recording it.
3. Let the run say it did not judge the check, in ways that need no knowledge of the repository: a program that could not start; an exit the policy's `[tests] not_run` names; a host that fails the policy's `[tests] ready` after the run. Such a check has no outcome (60) and is recorded under no key. For what nobody declared, the operator sets a recorded result aside.

## Decision outcome

Option 3.

Option 1 names a tool in sluice's code, and only the tools of the host in front of it. It does not catch the next repository's need.

Option 2 records both runs (16), so a host fail followed by a pass is still a flaky key, and the poison stays.

The statement already has the ending option 3 needs: a run with no outcome is never read as a pass or a fail, and the check runs again (60). The executor reports only what it observed (38). The repository says what its tests need, in its policy, read from the branch's current revision. The test says, by its exit, that it could not run, following the convention of test harnesses that keep "skipped" and "hard error" apart from "failed".

### Consequences

- Run-level shape:
  - `RunReport` gains `not_run`, beside `tasks` and `missing`.
  - `results.jsonl` holds only outcomes, so an older reader of it is unchanged.
  - A set-aside is kept in `set-aside.jsonl` beside it, never in it.
- Readiness and the bound:
  - Readiness runs before each run of the tests, and refuses the run while it fails (84, amended).
  - It runs again after a run only when a test did not pass, so a passing run pays nothing.
  - A combination whose tests could not run three runs in a row is blamed, with each test named and why (115). A check that always says it could not run cannot hold the queue forever.
- Readiness is run-wide. With the container engine down, a real failure in a test that never uses it also goes unjudged, and runs again later. That costs one run.
- Set-aside:
  - Setting a result aside is the operator's ruling (116). The Claude Code plugin's guard refuses it from an agent.
  - In a deployment across hosts it is to be signed as a run record is, which waits on those records being built.
- sluice's own policy declares `ready = ["docker", "info"]`.
- `sluice exec` is again how agents run sluice's own suites, sharing the host's bound.

## Amendment of 2026-11-18: a run whose claim lapsed counts toward the runs in a row

Decider: the design agent, from this record's own bound and clauses 60, 77, 88 and 115, on the question of the fix `forge-check-reads-the-pool`.

On 0.97.1, at `boomdrivedev/sluice-actions-trial`, a pool's job ended with no record of its run (`pool/forge/1`): the run job started, and the program it ran could not start on the runner. Its claim lapsed (88, 77), the run had no outcome (60), and its checks were dispatched again as `pool/forge/3`, `4` and `5`, with no end until the submission was withdrawn. Only 114's causes counted toward the runs in a row (115), and the time limit (66) counts from each run's own start, so nothing bounded it. Each of those dispatches spends the pool's minutes.

**Decision.** A run whose claim lapsed, whether its standing executor stopped before reporting or its pool lost, cancelled or timed out its job before the run's record was written, counts toward the runs in a row whose checks could not run, as a run with no outcome for 114's causes does (115). Its checks still run again until that count is reached, and nothing of the lapsed run is recorded under a key. At the count the combination reads as not passed, and the culprit's session is told each check and why: for a pool's job, the job and where its log is kept (155). A run whose place could not be made still counts toward nothing, as core.58 says, since the host saw it never began.

**Options not taken.**

- *Count the time limit over a run and its follow-ups.* A job lost in seconds would be dispatched dozens of times inside one limit, and an honest run with follow-ups would be cut short.
- *A bound for pools alone.* A standing host that dies on the same run each time loops the same way, and the two are one case (77).
- *Treat a lost job as a run that never began (core.58).* sluice cannot see why a pool lost a job. core.58's case is one the host saw before anything ran, and any other host may take it.

## Amendment, 2026-11-23: no result is set aside

Decider: the user, on 2026-11-23.

Sources: the statement's clauses 19, 116, S43 and S44 (as amended 2026-11-23, boomdrivedev/blueprints 9de7d63); rules core.7 and core.37; records 0043 and 0072; the change `sluice-host-makes-each-change`, design D4.

**Before:** for a host failure that nobody declared, the operator set the recorded result aside (option 3, 116). The set-aside was kept in `set-aside.jsonl` beside `results.jsonl`, or under `refs/sluice/set-aside` on a remote, and no branch read the result after that. The Claude Code plugin's guard refused it from an agent.

**Now:** no command sets a result aside, and nothing writes or reads a set-aside. Every result a branch's records hold is read under its key. A fail the host caused, and not the code, is a fault of sluice. sluice is fixed at that cause, so that such a run records no outcome, as this record does for a check a run did not judge, record 0043 for a check its time limit ended and record 0072 for a check that failed on what its executor refused.

- A set-aside an earlier release wrote stays in the records, and nothing reads it. A key whose fail it held, and that also holds a pass, reads flaky until a real fix changes what the check reads.
- `sluice set-aside` is no command, and exits 64 as any command sluice does not know.
- The guard has no set-aside to refuse.
