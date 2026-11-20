## Context

See proposal.md, Why. What the records show of the signal's case:

- sluice's check on pull request 791's head, `b7e3345`, posted by the
  host that took it in, says `Submission 554 on chuck-sluice-alpha` and
  `landed as 1a46299`. That host's report comment on the request is
  dated 02:28Z on 2026-11-19, nine minutes before the agent's words.
  The 554 the first `sluice wait main --json` gave at its time limit,
  in `queue.submission`, was 791's own submission on the host that took
  it in. The wait followed the right work.
- The agent then ran `sluice wait 554 --branch main` on the host it
  submitted from. There 554 read as another agent's submission, and
  the agent took it for its own. The number came from another host's
  records, and nothing sluice printed said how else to wait.
- `origin`'s `refs/sluice/state/main` for switchboard-kit was last
  written on 2026-11-11 and holds submissions up to 22. Each host
  numbered `main` from records of its own. That is the deployment's
  matter (proposal, Out of scope); the design below holds however the
  numbers fall.

Where the code stands:

- `submit_pull` (`crates/sluice/src/lib.rs`) runs every pull request
  submit and wait: `PullMode::Open` for a submit, `Status`, `Wait` for
  `sluice wait <branch>`, which finds the request by the folder's branch
  (`PullRequests::find`), and `Follow` for a withdrawn submission whose
  request carries its work. Each mode then runs one loop over
  `PullRequests::follow`.
- Its documents for a request with no submission here are built in four
  places: the `--no-wait` answers, `--status`'s, the `still` closure at
  a time limit or Ctrl-C, and `submit_pull_group`'s items. Its words
  for going on are `still open: #<n>; sluice wait <branch>`, and its
  `again`, the command `could_not_read` names, is `sluice wait
  <branch>`. Both read the folder the next wait runs in.
- `report_delivering` hands over only what `delivering` finds owed to
  the reader of the folder the wait runs for (51; rule core.52).

## Goals / Non-Goals

**Goals:**

- One handle for a request's work, the same on every host and from
  every folder, that every way on sluice prints for a request names.
- A wait by that handle that is the existing loop, so its outcomes,
  exits, documents and hand-overs are those of `sluice wait <branch>`.

**Non-Goals:**

- No new state, record field or forge read beyond one conditional read
  of the request when a wait names it.
- No change to `sluice wait <id>`, to `sluice wait <branch>`'s choice of
  what it follows, or to the submission's record.
- No change to how a number is looked up (26; record 0114).

## Decisions

### D1. The pull request's number is the handle

A request's wait is named `sluice wait <branch> --pull-request <n>`.
The forge gives the number when the request opens. It is the same on
every host and in every folder, and it names one request for the life
of the work (record 0100 D4).

Alternatives:

- **The submission's number.** None exists until the request is taken
  in. Across hosts the number is the other host's records' (26; record
  0114), and in the signal's case it named another's work here.
- **`sluice wait <branch>` run in the worktree.** It follows whatever
  request the folder it runs in opened. submit-hints rules out a way on
  that sends its reader into a worktree. A submission by name and a
  group act have no one folder. And the signal's agent ran exactly
  this, and still had nothing to go on at its time limit.
- **A target spelled `#<n>` or `pull/<n>`.** In a shell, `#` begins a
  comment, so `sluice wait #791` runs `sluice wait`. `pull/791` is a
  branch name git allows, so the target would name either. A flag says
  what it names.

### D2. A wait by pull request is `sluice wait <branch>`'s loop given the number

`PullMode::Wait` carries `number: Option<u64>`. Given one,
`submit_pull` takes it in place of `PullRequests::find`. Everything
after is the loop as it stands. It takes as its own the first
submission of the request it reads, waits on that one once the request
is taken in as another (record 0100 D4), and follows GitHub and
sluice's check where another host takes the request in (137).

The folder still decides the hand-over. `delivered_to` stays the
worktree the wait runs for, `--worktree`, `.` by default. A wait in
the request's own worktree hands its outcome over as `sluice wait
<branch>` does there, and one anywhere else hands nothing over, since
nothing of the request is owed to that folder (51; rules core.50,
core.52).

Alternative: hand the outcome over to the worktree that opened the
request. A wait records a hand-over only to the reader it runs for. The
request's own reader collects the outcome with `sluice inbox` (51).

### D3. The handle is checked once, through the pull request interface

`Command::Wait` takes `--pull-request <n>` (`pull_request:
Option<u64>`). With a submission's number as the target it exits 64,
as `--branch` with a branch target does (number-lookup). Then:

- `takes_pulls(&s, &target)` false: not found, `no pull request #<n>
  into <branch>`, exit 1.
- The `PullRequests` interface gains `holds(seams, clock, branch, n) ->
  Result<bool>`: whether the forge holds pull request `<n>`, open or
  closed, into `branch`. `Pulls` answers it by one `forge.pull` read,
  conditional as every read is and kept in `seen` (136; rule core.32),
  comparing `base`. `()` answers false. `Unreadable` in
  `tests/telemetry_logs.rs` fails as its other reads do. False: the
  same not-found answer. A read the forge could not answer ends the
  wait as `could_not_read` ends any (134).

The read sits behind the interface the core already reaches the forge
through (rule core.2). A profile with no forge answers that no request
exists.

### D4. One function spells the wait

`pull_wait(branch, n) -> String` gives `sluice wait <branch>
--pull-request <n>`. These use it:

- the `--no-wait` answers, with a submission and without;
- `--status`'s document for an open request, and its words;
- the `still` closure's document and `still open: #<n>; …` line;
- `submit_pull_group`'s items and lines;
- `again` once the loop knows the number.

`again` stays `sluice wait <id>` in `PullMode::Follow`, which follows
the request for submission `<id>`. Before `find` returns a number it
stays `sluice wait <branch>`, since no request is known yet.

The words put the wait on its own line after the answer, `wait:
<command>`. That way a line that already reads one way is never
re-read. Under `--json`, `wait` is the command as one string, as an
error document's `next` holds commands. A structured `{branch,
pull_request}` would make every reader rebuild the command, and the
document already gives both fields.

### D5. The record is the submission's record, unchanged

Once this host holds the request's submission, a wait prints that
submission's record, whose `id` is this host's and whose `sluice wait
<id>` follows it. The record gains no `wait`. It is
`sluice/submission@1`'s, read by every status, inbox and wait. A
caller looping on a wait runs the same command again whichever
document it got.

### D6. The words of the docs

The using-sluice skill and its submit reference tell an agent on a
pull request branch to hand `wait` to its background wait, and to run it
again while it exits 5. They say `queue.submission` is the number on the
host that takes the request in, never one to hand to `sluice wait`
here. The commands reference and the pull requests guide say the same.
None names a host or a team's branch of ours (rule core.26).

### D7. A record, and no clause changed

The statement fixes no handle for a request's wait. 137 says a wait
shows a request's entry there by its number, and C.5 has `--no-wait`
return the entry, or the request where there is none. Both hold as
written. The decision is the repository's own. A record in `docs/adr/`
names D1 and D2 with their alternatives, citing 26, 51, 124, 125, 137,
C.5, records 0100 and 0114, and the signal. Rule core.48's details then
rule out a request's answer that names no wait, and a way on for a
request that names a wait reading the folder it runs in, citing that
record.

## Risks / Trade-offs

- [A wait by pull request from another folder hands nothing over] →
  By design (D2): the outcome stays owed to the request's own reader,
  which `sluice inbox` in its worktree collects, and the round's
  notice reaches its session (51).
- [Scripts that compare a document whole] → `wait` is one new field
  beside the rest, none renamed or removed. worktrunk's `standing`
  reads `id` and `pull_request` alone.
- [The request gets a new head after the answer] → The wait takes as
  its own the first submission of the request it reads, as `sluice wait
  <branch>` does (record 0100 D4). A submit on a new head answers with
  the same wait.
- [One more forge read per wait by pull request] → One conditional read
  at the wait's start, kept for the host's other processes. `find`
  makes a list read per `sluice wait <branch>` today (136).
- [An older sluice given `--pull-request`] → It refuses the flag as a
  command line it cannot read. The skill ships with the binary that
  reads it.
