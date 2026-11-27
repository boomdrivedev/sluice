## Context

`Sluice::finish_run` reads a finished run's report. The verdict gives
three lists of checks with no outcome:

- `report.not_run`: the checks that the run could not run (114).
- `v.unjudged`: the checks that the runner skipped for a dependency that
  reads passed (149).
- `v.none`: the checks that the report holds no outcome for (record
  0122, design D8).

The first two go through `read_not_run`. It counts the run in
`not_run_runs`, and at `NOT_RUN_BOUND` (three) it names each check
failed with why (115). The third list did not go through it. A follow-up
run held those checks, and nothing counted that run.

## Decisions

### D1. A check that the report holds nothing for counts toward the runs in a row

`finish_run` adds `v.none` to the checks that `read_not_run` reads,
where no check failed on its own result and the time limit did not end
the run. Under the bound, the run ends with no outcome for each of them,
and a follow-up run holds them. At the bound, each is named failed with
the reason `the run's report holds no outcome for it`, and the newest
submission is blamed (115).

Why a fail and not a hold for the operator? The deployment's bound
already decides each other run that leaves checks with no outcome. The claim
`a-candidate-with-no-outcome-in-runs-in-a-row-reads-not-passed` also
asks for it. A fail tells the submitter at once, and the queue moves on. A hold
stops the branch until a person acts, and the person can do nothing that
a new submission cannot do. Three runs take about one minute, so a
person who reads `sluice status` sees the fail within a minute, not
after half an hour.

The other way: count these runs on their own, apart from 114's causes.
That gives two counts for one fact, a check with no outcome, and a mix
of causes could then run six times. One count is simpler.

### D2. Beside a fail, the check keeps no outcome

Where a check failed on its own result, or the time limit ended the run,
the run already has a verdict. A check that the report holds nothing
for then keeps no outcome, and nobody is blamed for it, as before. Thus
the change applies only to the runs that decide nothing.

### D3. A group of checks with no task has no outcome

A revision with no verdict runs every required check. A report that
holds no task for it is no pass (55, 70). The run can name a group, such
as `#gate`. Where the report names no check at all, the verdict gave the
group no line. The run's end then gave it no outcome with no count.
`finish_run` now gives each such group no outcome, with the same reason,
so it counts as D1 says. Where the report names some check, that check
counts the run, and the group gets no line of its own.

### D4. A check that the tree does not bear out stays out of the count

A check whose key names a file that the combination's tree does not
hold holds the branch (the decision
`a-key-the-tree-does-not-bear-out-holds-the-branch`). The hold decides
when sluice forms the combination again, so that check does not count.

## Risks

- A runner that keeps leaving a check out of its report now blames the
  newest submission after three runs. The reason names the cause, and
  the person fixes the runner or the policy. Before, the queue held the
  branch without end.
