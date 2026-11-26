## Context

`Sluice::start_run` gives each run `since`, the branch's revision, where
the candidate is on that revision and the revision has a verdict (36,
70). The moon profile then narrows the run's checks to those that the
changes since that revision affect. A run of the gate names the gate's
tag, and a task that the reading leaves out is not in the run. The
verdict asks the report for no tag, so the task has no line and decides
nothing.

A follow-up run (record 0122, design D7) names its checks. The verdict
asks the report for each named check, and a check that the report does
not name has no outcome (`verdict::NO_OUTCOME`). That opens a new
follow-up with the same checks.

A candidate moves to a new revision when the entries ahead of it land
exactly as sluice tested them (17). Its `on` is then the branch's new revision, and
the changes since that revision are only the work of the entries still
in it.

## Decisions

### D1. A follow-up run reads no changes

`start_run` gives `since` only to a run of the gate: a first run, or a
follow-up that holds every check. A follow-up that holds checks of its
own gets `None`, and the profile runs each check that it holds.

The checks of a follow-up are the checks that the run it follows left
undecided. That run ran them, so its own reading chose them. A second
reading can only remove checks, and a check that it removes stays with
no outcome.

The other way: read the changes again and let a check that the reading
removes pass, because the branch's verdict covers it. That needs a new
outcome for a check that the report does not name. The verdict must
then tell a check that the reading removed from a check that the run
lost. One run of the check costs less, and it decides the check.

### D2. A run of the gate keeps its reading

A first run holds the gate's checks, and a follow-up that holds every
check gets them too. They keep `since`, so they run only the checks that
their combination affects, as before.

## Risks

- A follow-up can run a check that the branch's verdict already covers.
  It runs once, and sluice records its pass under its key.
