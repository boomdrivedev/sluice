## Why

An agent sent its work as pull request 791 to switchboard-kit's `main`
and was given no way to wait on it. Another host takes `main`'s pull
requests in, so `sluice submit main --no-wait --json` answered with the
pull request alone and no submission number (137). The agent ran
`sluice wait main`, whose document at its time limit carried the other
host's `queue`, `"submission": 554`. It then waited on 554 by number.
On its own host, 554 named another agent's submission, wldn-design's
`design/enrolment-holds`, and the agent took that as its own outcome
(signal 2026-11-19-boomdrive-ops-1282975b/01; rule core.48).

A submission number names an entry only in the records that hold it
(26; record 0114), and before sluice takes a pull request in there is
none. The one name the work keeps on every host, from the submit to the
landing, is its pull request's number. Nothing sluice prints gives a
wait by it.

## What Changes

- **A wait names a pull request.** `sluice wait <branch>
  --pull-request <n>` follows pull request `<n>` into `<branch>`, from
  any folder of the repository and on any host. It follows the request
  as `sluice wait <branch>` follows a worktree's own and ends with the
  request's own outcome. It never ends with another submission's.
- **A pull request's answer says how to wait on it.** Every answer a
  submit or `--status` gives for a pull request names that wait: under
  `--json` as `wait`, and in words on a line of its own. That holds
  whether the answer carries a submission or not, from a worktree, by
  name or in a group act, and whichever host takes the request in.
- **An undecided wait says how to go on.** A wait or a waiting submit
  that runs out of time or is stopped while its pull request is still
  to be taken in, or is held on another host, names the same wait. In
  words, `still open: #<n>; sluice wait <branch> --pull-request <n>`
  replaces `sluice wait <branch>`, which reads the folder it runs in.
- **The docs say it.** The using-sluice skill, its submit reference,
  the commands reference and the pull requests guide hand `wait` to the
  background loop. They say that `queue.submission` is a number on the
  host that takes the request in, never one to wait on here.
- **Unchanged.** `sluice wait <id>` and `sluice wait <branch>` follow
  what they follow today. A submission's record, the document a wait
  prints once this host holds the request's submission, gains no field.
  A wait hands over only what is owed to the folder it runs in (51;
  rule core.52).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `submission-wait`: a wait follows a pull request named by its number,
  refuses one it cannot follow, and an undecided pull request's
  document and words name that wait.
- `pull-request-submit`: a pull request's answer, `--no-wait` and
  `--status`, names the wait that follows it.
- `group-submit`: under pull requests, each request's item and line
  names the wait that follows it.

## Out of scope

- **Two hosts numbering one branch apart.** On switchboard-kit's `main`,
  554 named one submission on the host that took 791 in and another on
  the host that submitted it. One branch's queue has one home (rule
  core.64), and a branch whose records each host keeps takes no
  requests in (137). How that deployment came to number `main` twice is
  ops' to read on those hosts, not a change here. The wait this change
  adds names the request, so it holds however the numbers fall.
- **A wait by number on another's submission.** `sluice wait <n>` still
  follows the submission `<n>` names, whoever's it is: the operator and
  a planner wait on any. Whether such a wait should name whose
  submission it follows is a change of its own.

## Impact

- `crates/sluice/src/lib.rs`: `Command::Wait` takes `--pull-request`;
  `PullMode::Wait` carries the request's number where one is given;
  `submit_pull` and `submit_pull_group` add `wait` to each pull request
  document and line; the `PullRequests` interface gains a read of
  whether pull request `<n>` is into a branch, answered by `()`,
  `Unreadable` in `tests/telemetry_logs.rs` and the GitHub profile.
- `crates/sluice/src/profiles/github/pulls.rs`: that read.
- `crates/sluice/tests/profiles/pull_requests.rs`: the scenarios.
- `plugins/sluice/skills/using-sluice/SKILL.md`,
  `plugins/sluice/skills/using-sluice/references/submit.md`,
  `site/src/content/docs/reference/commands.md`,
  `site/src/content/docs/guides/pull-requests.md`.
- `docs/adr/`: a record of the decision; `docs/architecture/core.md`:
  rule core.48's details cite it.
- Scripts that read these documents: `wait` is a new field beside the
  fields they read, and none is renamed or removed.
