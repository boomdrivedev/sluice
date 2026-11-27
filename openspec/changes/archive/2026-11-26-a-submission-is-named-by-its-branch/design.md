## Context

A branch whose records are on its remote numbers its submissions from
the remote's count. A branch in the host's own files numbers them from
the host's count. So two branches can hold one number (26, record 0114).
A lookup by number reads every branch and refuses a number that more
than one branch holds. The command must then take `--branch`.

The usual case is a session that submits, then waits. The submit knows
the branch. The worktree that waits knows which submission it made:
each record holds `from.worktree`, the branch of the worktree that made
it. `sluice wait <branch>` already finds the worktree's last submission
by that field.

`wait`, `status`, `amend` and `withdraw` run in `sluice host` (rule
core.25). The client sends its folder with each request. A wait also
sends the worktree. A status, an amend and a withdrawal ran with the
folder `.`, which is the folder of `sluice host` and not the client's.

## Decisions

### D1. The submit's answer gives its wait

The answer of `sluice submit --no-wait --json` gets the field `wait`,
`sluice wait <id> --branch <branch>`. The answer in words gets the line
`sluice wait <id> --branch <branch> follows it`, after the lines that the
answer had. A submit of several worktrees gives the same field and line
for each submission. The command names the branch always. Thus it
follows the submission from any folder, and a runner that is not in the
worktree can use it as it is. A pull request's answer has its own `wait`
already (record 0130), and it does not change.

### D2. The worktree's own submission is the one that the number names

More than one branch can hold a number that a command gives with no
branch. Then sluice reads the submission of the number on each of those
branches. It keeps the branches where `from.worktree` is the branch of
the folder that the command runs in, as the driver reads it. Where one
branch only is kept, the command acts on that branch, as if it had
`--branch` with that branch. Its lines and its `next` then name that
branch.

sluice refuses the number as before in these cases:

- The folder holds no session that the driver can read, for example a
  folder outside the repository.
- The worktree made the number on none of the branches, or on more than
  one.
- A branch could not be read. That branch can hold the number from the
  same worktree, so the worktree's submission on the other branch is not
  the only one (rule core.11).

A submission by name has no worktree (124, rule core.50), so it is never
a worktree's own submission.

The other way: refuse the number always, and fix only the submit's
answer. A session that waits with `sluice wait <id>` is then still
refused, and the session context and the skill tell agents to wait that
way.

### D3. `sluice host` runs a read or a change from its client's folder

`sluice host` runs a status, an amend and a withdrawal with the folder
of the request as the command's worktree. The request already holds the
folder, so a client of an earlier release gets the same result. The key
of a read that `sluice host` shares between clients holds the folder, so
two folders never share one answer.
