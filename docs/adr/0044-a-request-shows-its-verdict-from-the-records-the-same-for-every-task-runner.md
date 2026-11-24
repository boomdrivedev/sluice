---
number: 44
title: A request shows its verdict from the records, the same for every task runner
status: accepted
date: 2026-10-21
decision-makers:
- Chuck Swanberg
---

# A request shows its verdict from the records, the same for every task runner

Deciders: the operator, planning what a pull request shows of sluice's verdict, 2026-10-20.
Sources: `agentplot/blueprints:design/sluice/requirements.md` clauses 16, 19, 20, 26, 27, 38, 49, 53, 114 to 116, 118, 119, C.2, C.5 (many-hosts-3's addition), S50, S51; rules core.1, core.2, core.7, core.9, core.11, core.14; ADRs 0021, 0022, 0028, 0032.

## Context and problem statement

A pull request into a branch sluice lands showed one `sluice` check whose text was its state, "landed as 0cef58d", and nothing in its conversation. A team used to moon's CI sees a report on each pull request instead: `moon ci` writes a run report, and moon's run-report-action posts it as a comment, a table of tasks with their time, status and cache. sluice will support other task runners, nx and turbo among them, and runs a combination's checks on many hosts, each on the systems it holds. What does a pull request show of a verdict, who draws it, and from what?

## Considered options

1. sluice draws its own report from its records, and a task runner's profile hands back what it knows beyond the result as optional detail recorded with the result.
2. sluice hands the run to the task runner's own reporting: moon's run-report-action, and nx's and turbo's equivalents later.
3. sluice posts the task runner's report file as it is.

## Decision outcome

Option 1. The core reads a submission's report from the records: the submission, the results, the set-asides and the branch's log. A driver renders it. The report is the same form whatever runner wrote the results (53). For a pull request, the driver posts it three ways:

- one comment in its conversation, edited as the report changes;
- the `sluice` check's text;
- one check per required check per system, as many-hosts-3 asks.

A runner's detail reaches the records through the check seam:

- per task, the time it took and how often it was tried;
- per run, its time, the runner's estimate of the time it saved, and the changed files.

The detail is recorded with the result before the verdict is read (core.7), and nothing reads a verdict from it (119).

A submission tested in a combination another is blamed for shows that its combination is formed again, and no row of it. The core decides this, so no driver can show a failure that clause 20 keeps from it (118).

The moon runner also runs with moon's CI mode on (`moon exec --ci`), so a task a repository keeps out of CI with `runInCI: false` is not run, as moon's own CI would not run it. sluice still picks the targets: the branch's required checks, narrowed to those affected.

Option 2 runs only inside the forge's job service, and posts as that job's token, not as sluice's app. It also reports one run of the runner, not the combination that lands:
- its changed files are every submission's in the combination;
- its "cached" is the runner's cache, not a replay the gate trusts (core.9);
- it has no combination, no blame and no system.

Each runner would also bring its own layout. Option 3 has the same faults, and reads a file the runner overwrites at its next run, which is state outside the records (core.11).

### Consequences

- Rules core.1 and core.2 hold. The report's form and its reading are the core's and name no runner. The moon profile alone reads moon's report. The GitHub profile alone draws and posts.
- Rule core.14 holds: the report is a view over the records, never stored. A landed or blamed submission's report reads the same after the combination has left `branch.json`.
- A repository that also runs its runner's own reporting in its own CI keeps it. sluice's comment has its own marker, and neither overwrites the other.
- The end of a failed check's output is posted only when the branch's policy sets `pull_requests.log_lines`, scrubbed as telemetry scrubs it (ADR 0022). Anyone who can read the repository reads a pull request.
- `--ci` changes moon's own printed output, for example log groups on a CI provider. It does not set `CI` in the tasks' environment (checked on moon 2.6.1). sluice reads the report, not the printed output.
- The app needs Checks and Pull requests write, which ADR 0021 already named. `sluice forge check` now verifies them.
- nx and turbo profiles fill the same detail, or leave a field empty when their runner does not report it. The report leaves out what is empty.

## Amendment, 2026-11-23: the report reads no set-aside

Decider: the user, on 2026-11-23, who decided that no result is set aside.

Sources: the statement's clause 116 (as amended 2026-11-23, boomdrivedev/blueprints 9de7d63); record 0031 as amended 2026-11-23; the change `sluice-host-makes-each-change`, design D4.

**Before:** the core read a submission's report from the submission, the results, the set-asides and the branch's log, and listed a check whose result was set aside, with its reason.

**Now:** the core reads the report from the submission, the results and the branch's log. It reads no set-aside and lists none. A check whose fail an earlier release set aside, and that also holds a pass, is listed flaky, with the result it contradicts.
