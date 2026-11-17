# Submitting

Full reference: <https://agentplot.github.io/sluice/reference/commands/>.

## Flags

| flag | effect |
|---|---|
| `--check` | Only forecast: would the head conflict with the branch and everything queued? Writes nothing. Prints `no conflict with <branch> or anything queued` or `would conflict: <paths> with #<id> (<agent>, <worktree>)`. |
| `--no-wait` | Return once queued (or once the pull request is open). |
| `--status` | Where this worktree's last submission, or its pull request, stands. |
| `--after 2,5` | Land only after those submissions. |
| `--priority N` | Higher goes first, ahead of lower-priority submissions not yet under test; the default is 0. |
| `--subject <text>` | One line saying what lands: the title of the pull request the submit opens, or of one it opened before, and the name of its landing commit. With several worktrees, once for each, in their order, or not at all. |
| `--worktree <path>` | Submit from another worktree than the current directory. Given more than once, every worktree's head as one act, in that order: see below. |
| `--json` | One JSON document: the submission record when it finishes; `{id,branch,ahead}` with `--no-wait`, and `{id,branch,ahead,pull_request}` for a pull request sluice queued at once; `{"conflict":null}` or `{"conflict":{paths,with}}` with `--check`; `{pull_request,branch,state}` for a pull request still waiting for its conditions, with its `head` and `draft` under `--status`, and `taken_in_by`, `{host}` naming the host that keeps the branch, and `queue` where another host keeps the branch and takes its pull requests in; under `--status`, with `sends`, the revision a submit from the worktree would send now; `{branch,submissions}` for several worktrees. |
| `--timeout 9m` | `wt submit` only: submit, then wait at most that long, exit `5` when it runs out; run again, follow the submission already made. See "Who moves the queue". |

## Refused: a required task kept out of CI

```
refused: main requires root:integration (.config/sluice.toml line 59), which the task runner in this worktree keeps out of CI, so it is never run and main cannot pass while it is required
the agent working in integration lets it run in CI there, or drops it from main's policy in the same submission that lets it run, and submits again
```

The branch requires a task the task runner, in your worktree, never runs
in CI (moon's `runInCI: false` or `skip`), so no combination can pass.
Nothing was queued, and on a pull-request branch nothing was pushed. A task
required through a tag names it: `requires app:serve through #check-fast`.
The way on is a commit in your worktree that lets the task run in CI, or
that drops it from the policy together with such a commit; the submission
that does is taken. Submitting again unchanged is refused again. When the
runner can't be asked (its toolchain fails), the submission is taken with a
`warning:` line, `warnings` under `--json --no-wait`, and the run reads the
task missing as before.

## What a waiting submit prints

```
submission 12 to feature/x · 1 queued ahead (#11)
testing feature/x + #11 + #12 · 3s of 15m
testing feature/x + #11 + #12 · 30/33 tasks · running tests:t-cards 1m10s, tests:e2e 40s · 1m52s of 15m
testing feature/x + #11 + #12 · 4 tasks to run, 29 replayed
passed · landed as b7d2e94
```

While it waits it prints a line each time its combination's tasks move:
how many have ended of how many, up to three running with how long each
has run, and the combination's time against its limit. `sluice status
<branch>` and `sluice status <id>` show the same line.

`N tasks to run, M replayed`: with moon, only affected tasks run, and a task
whose inputs did not change may replay its earlier result, when the branch
trusts `"cache"`.

A change no required check reads, such as docs, lands at once without
waiting for the combinations under test, which keep their results and
land after it untested again:

```text
landed as 4e1a07c beside the queue: no required check reads its changes
```

Under `--json` its record has `"beside": true`. When a combination's
checks would read it, or it conflicts with a submission ahead, it takes
the queue and says `waits behind #13: its checks read this change` or
`conflicts with #13's work`; its record has `"beside": {"behind": 13,
"why": "read"}`. A change to `.config/sluice.toml` or to the runner's
own settings always takes the queue.

## Several worktrees as one act

Submit a set as one act when its heads must be queued together, such as
approved feature branches landing on `main`. Submitted one by one, a round between
the submits can test the first alone, and land it before the rest are
queued.

```sh
sluice submit main --worktree ../feature-a --worktree ../feature-b --worktree ../feature-c
sluice submit main --worktree ../feature-a --worktree ../feature-b --no-wait --json
# {"branch":"main","submissions":[{"id":41,"worktree":"../feature-a","ahead":[]},{"id":42,"worktree":"../feature-b","ahead":[41]}]}
```

- All or none: a refused worktree (uncommitted changes, named twice) is
  named with why, nothing is queued, exit `2`.
- In order: each is queued after the one before; `--after` and
  `--priority` apply to every one alike.
- Then each is its own submission. One that fails or conflicts is sent
  back alone and the others land; wait on each with `sluice wait <id>
  --timeout 9m --json`, as on any submission.
- A waiting act reports each one's outcome in order and exits `0` when
  all landed, otherwise with the exit of the first that did not. Under
  `--json`, one document: `{branch,submissions}`, each item that
  submission's record plus its `worktree`.
- `--check` and `--status` take one worktree (exit `64` with more).
- `--subject` is given once per worktree, in their order, or not at all
  (exit `64` otherwise); each names its own submission.

## Who moves the queue

A round is one pass of sluice's work on a branch: read the queue, start
tests if there is room, read finished tests, blame a failure, land what
passed. sluice's own host process runs them, for every repository on the
host: a submit, waiting or not, tells it of the submission, starting it
where none runs, and it ends once no queue has work. Nothing is installed
or kept running for it.

- A waiting `sluice submit` / `wt submit` follows the host process to the
  outcome, and runs no round.
- `sluice wait <id>` does the same for a submission made with
  `--no-wait`; `--timeout 9m` returns after that long, exit `5`, with the
  submission's record under `--json`, for a caller whose calls are capped.
  It removes no worktree. A round the host process could not run is said
  once on stderr, and the wait goes on.
- `wt submit <branch> --timeout 9m --json` does the same under worktrunk,
  from submit to landing: run in the background again while it exits `5`,
  it submits once, as `--no-wait` does, its answer on stderr, then follows
  that submission as `sluice wait <branch>` does, with the same output,
  document and exits. Run again where the worktree's submission stands
  (queued, under test, passed or landed, of the revision `sends` names, a
  submit from the worktree would send now) or its pull request is open and
  ready there, it submits nothing and follows it. Once it lands, it
  releases and removes the worktree, at once when it had already landed;
  `--no-remove` keeps it. `wt submit <branch> --no-wait` run again where
  one stands answers with it, as `--status` prints it, submits nothing and
  removes nothing. Only a new head submits again: where one stands,
  `--after` or `--priority` exits `64` and names `sluice amend <id>`.
  `--timeout` with `--no-wait`, `--check` or `--status` exits `64` and
  submits nothing. A new commit, or a submission blamed, in conflict or
  withdrawn, is submitted again.
- `sluice step <branch>`: one round, asked of the host process, then
  exit. On a branch with pull requests, it takes the branch's open pull
  requests in first, those opened by hand among them.

## Branches on GitHub

With `remote = "origin"` on the branch, every round fetches the branch
first, and a landing is a push that only succeeds if the branch has not
moved (always a fast-forward of what it tested on). "landed" is reported only once GitHub accepted the push. A refused
push is logged as `refused` and the next round tests again on the new tip.

## Pull requests

With `[branches.main.pull_requests]`:

```
$ wt submit main --subject "Batch review cards"
pushed batch-review · opened #41: Batch review cards
waiting: needs 1 review, qa/approved
  forecast: merges without conflict behind #38, #40
queued behind #38, #40
testing main + #38 + #40 + #41
passed · landed as b7d2e94
```

- Its title is the `--subject`, or your branch's name with none. A later
  `--subject` retitles a pull request sluice opened, never one a person
  opened; a submit with none leaves the title as it is.
- The pull request waits while it is a draft or misses a review or a check
  the policy names, then joins the queue.
- A submit of new commits replaces its submission in the queue
  (`withdrew its submission 12`), and the new commits wait for their
  conditions.
- Sent back (blamed, timed out, in conflict, withdrawn), or moved by
  commits no submit pushed, it is a draft until your next submit, whose
  line ends `marked ready for review`; the check reads
  `waiting: the next submit`. The same commits submitted again are
  tested again.
- sluice posts a `sluice` check on its latest commit with the state, and a
  comment when it sends one back.
- A submit following a pull request exits `1` when it is closed without
  landing.
- Never press GitHub's merge button on such a branch.

## Landing commits

One merge commit per submission: first parent the branch, second parent your
head. Its first line names the work, `chore(land): #12 Batch review cards` by
default: the submit's `--subject`, the title of the pull request it was taken
from, or your branch with neither. Trailers record who submitted it and what
was tested:

```
Sluice-Submission: 12
Sluice-From: agent-1 (touches)
Sluice-Tested: 9ab3d07c11
Sluice-Result: passed (2 run, 31 replayed)
```

A change that landed beside the queue names no combination, since none
was run. Its result is its own, passed with nothing run, followed by the
result that covers the revision it landed on, by the revision of the
branch that result landed as and its own words:

```
Sluice-Submission: 15
Sluice-From: agent-4 (guide)
Sluice-Result: passed (0 run, 0 replayed) beside the queue: no required check reads its changes; covered by the result at b7d2e94f031c5a8e6d2f40b9a7c3e1d58f6a2b47, passed (2 run, 31 replayed)
```

A tool that reads a branch's head as passed reads the first word of
`Sluice-Result`, which is `passed` on every landing.

The plain `sluice submit` leaves your worktree and branch in place, and
so does `sluice wait`; `wt submit` removes them on landing (`--no-remove`
keeps them), whether it waited from the start, under `--timeout` in
rounds, or was run again to follow a `--no-wait` submission. A
`--no-wait` run removes nothing. Never remove a landed worktree by hand:
run `wt submit <branch>` again in it.
