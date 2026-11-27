## Why

On deneb on 2026-11-26, agents submitted work and then ran
`sluice wait <id>`, and sluice refused the wait. The number was on `main`
and also on an old bolt: boomdrive-cloud #37, boomdrive #40 and #44, and
sluice #162. A number that two branches hold names neither alone, so a
lookup by it is refused (26, record 0114). Each agent then ran the wait
again with `--branch main`. boomdrive's runner got the same refusal every
4 seconds until it named the branch.

sluice knew the branch each time. The submit's answer had the branch, but
it did not give the command that names it. The worktree that asked had
made the submission, but the lookup did not use that.

## What Changes

- A submit's answer gives the command that follows its submission from
  any folder, `sluice wait <id> --branch <branch>`. Under `--json` it is
  the field `wait`. In words, under `--no-wait`, it is the line
  `sluice wait <id> --branch <branch> follows it`.
- A number that more than one branch holds can name the submission that
  the worktree made. `wait`, `status`, `amend` and `withdraw` take that
  submission where the worktree made the number on one of those branches
  only. The worktree is the folder that the command runs in.
- From any other folder, the number is refused as before, and each
  branch is named. Where a branch cannot be read, the number is refused,
  because that branch can hold the number from the same worktree.

## Capabilities

### Modified Capabilities

- `number-lookup`: a worktree's own submission of a shared number is the
  one that a command takes, and a submit's answer gives its wait.

## Impact

- `crates/sluice/src/lib.rs`: `submitted_here` and `lookup_here` find the
  worktree's own submission. `wait`, `status`, `amend` and `withdraw` use
  them. `submit` gives `wait` in its answer.
- `crates/sluice/src/host_process/reads.rs` and `changes.rs`: `sluice
  host` runs a status, an amend and a withdrawal from the folder that
  its client sent. Each request already holds that folder, so the
  protocol does not change.
- `docs/adr/0114-...`: the record is amended for this day.
- The claim `a-number-two-branches-hold-names-neither-alone` in Boom: a
  new version is proposed with the worktree's own submission as a case
  that does not break it.
- `plugins/sluice/skills/using-sluice`: the skill gives the submit's
  `wait` and the worktree's own submission.
