---
title: Pull requests
description: Each submission gets a pull request on GitHub. It waits for review, QA and other checks, then joins sluice's queue and lands when its combination passes.
sidebar:
  order: 6
---

This builds on [GitHub integration](/sluice/guides/github/) and on
[protecting `main` on GitHub](/sluice/guides/github-app/): sluice reads
pull requests and posts on them as its GitHub App.
There, work went straight from an agent's worktree into sluice's queue.
Here, every piece of work also gets a pull request: a page where people
review it, where other systems post their checks, and where sluice shows
what it is doing with it.

None of this needs GitHub Actions. sluice still runs the tests on your
machine. Your repository's own CI can keep running on pull requests as
it does now.

## Keeping your team's pull requests

A team that lands its work through pull requests keeps them. sluice
opens a pull request when an agent submits, and people open theirs by
hand, as they always have. A draft waits until someone marks it ready
for review. A ready one waits for the reviews and checks your policy
names, the ones your team requires today. Then sluice tests it with
everything ahead of it and lands it with its own push, and GitHub marks
it merged.

Only who lands it changes: nobody presses merge. Developers open pull
requests, review them, push fixes and mark drafts ready as before, and
nothing else about their day changes. When Claude sets sluice up, it
asks whether your team lands on each branch through pull requests,
reads the reviews and checks GitHub requires there now, and writes them
into the policy.

sluice has no mode in which GitHub's merge button lands and sluice only
tests. What the button lands was never tested with the pull requests
that landed before it, so two pull requests that each passed alone can
break `main` together. sluice keeps `main` passing by landing each pull request
itself, in the combination it tested.

The rest of this page holds the detail: [how a pull request waits, then
joins the queue](#two-stages-waiting-then-the-queue), [the
policy](#the-policy), [how sluice notices them](#sluice-notices-them),
[what sluice posts](#what-sluice-posts) and [GitHub's
settings](#github-settings).

## Two stages: waiting, then the queue

A pull request **waits** while it is a draft, or while something your
policy asks for is missing: an approving review, or a check another
system posts, such as your QA team's `qa/approved`. When it is out of
draft and nothing is missing, it **joins the queue**. sluice tests it
combined with everything ahead of it and lands it the moment that
passes.

```mermaid
flowchart LR
    open(["opened: by wt submit, or by hand"]) --> wait
    subgraph wait ["waiting"]
        direction TB
        draft["draft: still being written"] --> ready["ready for review:<br/>reviewers, QA, CI"]
    end
    wait -- "out of draft and nothing missing" --> queue
    subgraph queue ["sluice's queue"]
        direction TB
        test["tested with everything ahead of it"] --> verdict{"passed?"}
    end
    verdict -- "yes" --> landed(["landed: sluice pushes main,<br/>GitHub marks it merged"])
    verdict -- "no: it broke the combination" --> wait
    queue -- "new commits pushed" --> wait
```

A pull request never waits inside the queue. The queue lands in order,
so one held there for a day of QA would hold up everything behind it.
Everything in the queue is ready to land as soon as it passes.

## The policy

```toml
[signers.box]
ssh = ".config/sluice.boxes"   # one line per host's key

[branches.main]
remote = "origin"
required = ["#check-fast", "#check-tools"]
trust = ["box"]                # the signers whose runs count on main

[branches.main.pull_requests]
reviews = 1                  # approving reviews on the current head
checks = ["qa/approved"]     # checks other systems post, by name
log_lines = 40               # post the end of a failed test's output
```

`required` is what sluice tests. `reviews` and `checks` are what a pull
request needs before sluice takes it. A name in `checks` is the name
GitHub shows for the check on the pull request, whoever posts it: your
CI, a deployment, a QA tool.

`log_lines` posts the last lines of a failed test's output in the pull
request's report, with anything shaped like a token or a key replaced by
`[redacted]`. Leave it out and the report says where the output is kept
on the host instead. Anyone who can read the repository reads the pull
request, and scrubbing catches the common shapes of a secret, not every
one, so turn it on only where the tests' output is safe to show.

A branch that takes pull requests names
[`[signers]`](/sluice/reference/configuration/#signersname-who-may-sign-results),
so its state is kept on its `remote` and the host that keeps it there
takes its pull requests in. A policy with no `[signers]` keeps every
branch's state on each host, where no host takes the pull requests in
for the others, so sluice refuses that branch alone and every other goes
on. `sluice submit main` exits 2, and pushes and opens nothing:

```console
$ sluice submit main
refused: main takes pull requests, which only the host that keeps it on origin takes in, and its state is each host's own: name [signers] in .config/sluice.toml so its state is kept on origin (137)
```

`sluice status main`, `sluice step main` and `sluice overview` say the
same.

With no `[branches.main.pull_requests]` table, there are no pull
requests and work goes straight into the queue, as in the previous
guide.

## sluice notices them

An agent's waiting `wt submit` follows sluice's host process, which moves
the queue. sluice starts that process itself when work arrives, and it
ends once nothing is in play and nothing follows it, with nothing to
install. A pull request opened by hand has nobody waiting on it, so the
host process notices it while it lives, or once the next command of
sluice's on the host starts it. To keep it noticing while you work, keep
a watch going, which the host process lives for:

```sh
sluice overview --json --watch
```

Each pass fetches `main` and moves the queue. Once a minute it also asks
GitHub for the open pull requests into `main` and posts their states.
It asks at once when a submit on the host opened or updated a pull
request, or when the queue changed. A pull request opened by hand, a review, or another
system's check is seen within that minute of happening; an agent's
submit sees its own pull request at once. `[forge] discovery` in the
[host's config](/sluice/reference/configuration/#forge) sets the minute.

### One host takes the pull requests in

When several hosts reach `main`, one takes its pull requests in, so no
pull request is tested on two hosts: the host that keeps `main` on its
remote, which `main`'s state names. Whichever of that host's sluice
processes asks, a round, a `sluice step` or a submit, takes them in, one
at a time. The first host to pass `main` keeps it. Another host takes it
once `main`'s state has stood unchanged for `[peers] lease`, as that host
has watched it, with work queued, as when the host that kept it stopped.

A host that doesn't keep `main` asks GitHub nothing about its pull
requests and withdraws nothing. A wait on that host follows its pull
request on GitHub, as [`sluice wait`](/sluice/reference/commands/#sluice-wait)
says. `sluice status main` and
[`sluice overview`](/sluice/reference/commands/#sluice-overview) there
list each pull request the host submitted or waited on, with the host
that keeps `main` and where the request stands in that host's queue.

An older sluice took the pull requests in by an intake mark, the ref
`refs/sluice/intake/main` on the remote. Nothing reads it now. Once every
host runs this sluice, delete it by hand:

```sh
git push origin :refs/sluice/intake/main
```

## An agent submits

```console
$ wt submit main --subject "Batch review cards"
pushed batch-review · opened #41: Batch review cards
waiting: needs 1 review, qa/approved
  forecast: merges without conflict behind #38, #40
```

The agent's branch is pushed, and the pull request is opened. Its
title is the subject, one line in the agent's words saying what lands.
With no subject, the title is the branch's name, `batch-review`,
and never a commit's message: a branch's first commit can say little
of what the branch lands. Its description holds the branch's commit
messages, each whole and oldest first, and names the agent, which makes
it the agent's pull request. Submitting again from the same worktree
updates the same pull request and marks it ready for review.

A later submit with a subject retitles a pull request sluice opened,
and one with none leaves its title as it stands. A pull request a
person opened keeps the title they gave it, whatever the subject. When
a pull request lands, its landing commit names it by its title,
`chore(land): #12 Batch review cards` under the default landing
subject, and its `Sluice-From` trailer names the agent and the pull
request.

GitHub holds 65,536 characters in a description. When a branch's
messages pass that, as a long-lived feature branch's can, the description holds as many
of them whole as fit. A count of the rest follows, with a link to
GitHub's comparison of the branch, then the subject of each of the rest:

```text
38 more commits, each by its subject; all of them in full: https://github.com/org/cards/compare/main...feature/review-cards

- Show the review card's footer
- Count the cards left to review
…
```

No message is cut part way. Each submit writes the description again
for the commits it pushes, so it follows the pull request's commits. It
replaces any text a person added there; a person's notes belong in a
comment.

### A feature branch's landing worktree submits

A worktree that has checked out a branch sluice lands on, such as a
feature branch kept on GitHub, submits that branch as GitHub holds it. Nothing is
pushed: sluice lands on the feature branch, and only sluice writes it. The pull
request is from the feature branch into `main`, at GitHub's revision of it, which
is also the head submitted:

```console
$ wt submit main
opened #52: feature/review-cards
this host's feature/review-cards is at 10e4791, behind origin's 8079db3; submitted 8079db3
waiting: needs 1 review
```

The second line appears only when the worktree's head, this host's own
`feature/review-cards`, differs from GitHub's: behind it, ahead of it, or
apart from it. It refuses nothing; a round of the feature branch sets this host's
branch to its landing. Submitting again after another landing on the
feature branch updates the same pull request and marks it ready at the new
revision.

A waiting `wt submit` reports each state as it changes and returns when
the pull request lands or is sent back. Waiting can take a day when
people review, so an agent that shouldn't wait submits with
`--no-wait`, and `wt submit --status` reads it later. When the pull
request's conditions already hold, sluice queues it then, and the
submit answers with that submission:

```console
$ wt submit main --no-wait --json
{"id":12,"branch":"main","ahead":[],"pull_request":41}
```

When they don't, the submit answers with the pull request and what it
waits for, `{"pull_request":41,"branch":"main","state":"waiting: needs
1 review"}`. An agent waiting in steps hands an `id` to `sluice wait 12
--timeout 9m --json`. With only `pull_request`, it runs `sluice wait
main --timeout 9m --json` in its worktree, which follows the pull
request and, once sluice queues it, its submission. Each exits `5`
while there is no outcome yet; run it again.

## Several pull requests as one act

`sluice submit main` given several worktrees pushes each branch and
opens or updates each pull request, in the order given, as one act. It
then reads each pull request's reviews and checks on the commit it
pushed, and takes every pull request or none of them.

When any falls short, sluice queues none of them and leaves every one a
draft at its commit, so no round takes one of them alone. It names each
one short of its conditions with what it lacks, and exits `2`:

```console
$ sluice submit main --worktree ../feature-a --worktree ../feature-b --no-wait --subject "Runner credentials" --subject "Runner cache"
pushed feature/a · opened #51: Runner credentials
pushed feature/b · opened #52: Runner cache
refused: #51 from ../feature-a needs 1 review
refused: #52 from ../feature-b needs 1 review
nothing submitted: #51, #52 are drafts at their heads
once each meets its conditions, the same submit takes them all: sluice submit main --worktree ../feature-a --worktree ../feature-b
```

Reviewers review the drafts. Once each meets its conditions, run the
same submit again. sluice marks every pull request ready for review and
queues all of them at once, in the order given, each with its
submission:

```console
$ sluice submit main --worktree ../feature-a --worktree ../feature-b --no-wait
pushed feature/a · updated #51: Runner credentials · marked ready for review
submission 61 to main from #51 · nothing queued ahead
pushed feature/b · updated #52: Runner cache · marked ready for review
submission 62 to main from #52 · 1 queued ahead (#51)
```

Under `--json` each item of `submissions` holds its `id`, `worktree`,
`ahead` and `pull_request`. When sluice refuses one of the worktrees,
one holding changes beyond its commits, it refuses the act before
anything is pushed. A pull request from a fork is never one of the act:
sluice opens or updates only the repository's own.

Each pull request's description gets a mark beside the one naming its
agent. The mark says it is one of the group, with its place in the
group, the group's size, and the commit it was opened at:

```text
<!-- sluice: group=20261023T120000Z-4f0c2a1 1/2 head=4f0c2a1… -->
```

Once queued, each is a pull request of its own in the queue, blamed, sent
back and landed alone. New commits that no submit pushed, on one of
them, take that one out of the queue, and sluice marks it a draft. The
others stay queued and land without it. It waits for its worktree to
submit again, or for a person to mark it ready for review: the mark
binds only the commit it was made at.

A person can also mark the drafts of a refused act ready for review by
hand. sluice then takes the group's pull requests only all together, in
one pass and in the group's order. It takes them once every one is open,
out of draft, and meeting its conditions on the commit it was marked at.
Until then each waits for the rest. One that is ready says which of its
group it waits for, `waiting for its group: #52`, and one that is not
says what it lacks itself. So no round tests the first alone before the
rest are in.

A group waits as long as any of it does. To free the rest when one is
closed or will not be ready, submit them again, as a new group without
that one or one by one. A submit of one worktree writes its pull
request's description without the group's mark.

## A person opens one by hand

Open a pull request into `main` from any branch of the repository, as
you always have. sluice finds it the next time it asks GitHub, within
`[forge] discovery` of your opening it, and posts its state. A draft
waits; mark it ready for review when it is. Pull requests from forks
are never taken.

## What sluice posts

sluice posts three things on a pull request, all as its app:

- a check named `sluice` on its latest commit, saying where it stands;
- once its combination has a result, a report in its conversation, one
  comment edited as the result changes, and the same report as the
  `sluice` check's details;
- a check for each required test on each system, such as
  `sluice / app:test (linux-x64)`, or `sluice / app:test` when the
  policy names no systems.

It also comments once when it sends a pull request back. The `sluice`
check reads:

| the check reads | means |
|---|---|
| `waiting: a draft` | still a draft; sluice takes it once it is marked ready for review |
| `waiting: the next submit` | an agent's pull request whose submission left the queue; sluice marked it a draft. Once the agent's next `sluice submit` from its branch on any host, or a person, marks it ready for review, sluice takes it again |
| `waiting: withdrawn by the operator` | a pull request a person opened, withdrawn with `sluice withdraw` (`by the planner` when a planner withdrew it); sluice takes it again on new commits, or once it is marked a draft and then ready for review |
| `waiting: needs 1 review, qa/approved` | out of draft, and these are missing on its latest commit; the details add where it would land now: "merges without conflict behind #38, #40" or "conflicts with #40 in plan.py" |
| `queued behind #38, #40` | in the queue, behind the pull requests it names |
| `testing main + #38 + #40 + #41` | its combination is being tested |
| `failed: app:test` | its combination failed and it is to blame; the comment names the failed tests and where their logs are |
| `conflict: plan.py` | it no longer merges with what is ahead of it; the comment names the files and the pull requests |
| `landed as b7d2e94` | on `main`; GitHub marks the pull request merged |
| `landed as 4e1a07c beside the queue` | no required check reads its changes, so it landed on `main` at once with no check run, ahead of the queue. The check and the report say so, and name the result that covers it, as its landing commit does: "Covered by the result at `b7d2e94`, passed (2 run, 31 replayed)." |

While a pull request waits or is queued, the check shows as pending on
GitHub; while it is tested, in progress; failed and conflict fail it,
and landed passes it.

The check's summary opens by naming the host that posted it. Once that
host has taken the pull request in at its latest commit, it reads
"Submission 1406 on b.", 1406 being the submission's number on host
`b`. Before then it reads "Waiting to be taken in on b.". The summary
ends with a block GitHub doesn't show, which carries the same for a
program to read:

```html
<!-- sluice:queue {"host":"b","taken_in":true,"submission":1406,"state":"queued","ahead":[{"submission":1405,"pull_request":38}],"waits_for":[]} -->
```

`state` is the submission's state on that host, and `ahead` lists the
submissions ahead of it there, in the order that host will land them,
priorities and `--after` included, each with its pull request's number,
or `null` for work submitted from a worktree. Before the host takes the
pull request in, `taken_in` is `false`, `submission` and `state` are
`null`, and `waits_for` lists what the check's title says it waits for,
such as `["1 review", "qa/approved"]`. A pull request sent back and
waiting for its submitter's next submit keeps its `submission`, its
`state` says how it was sent back, and `waits_for` is `["the next
submit"]`. sluice posts only when the state or a field of the block
changes.

sluice on another host reads where the pull request stands from this
block, not from the check's title or whether GitHub shows the run
failed. Taken in and `failed` or `conflict` is sent back, `landed` is
landed, and anything else is still open. A check an older sluice
posted has no block, so sluice reads its title and whether it failed.

The forecast runs no tests. It only checks whether the pull request
would merge on top of `main` and the queue as they are now, so a
conflict shows up while people are still reviewing.

### The report

The report is read from sluice's own records, so it reads the same
whatever runs your tests:

```text
## sluice: failed on main, #41 is to blame

Tested `a1b2c3d` = main `9f8e7d6` + #38 + #40 + #41 · 3 ran, 5 replayed

Total time: 4m 12s | Comparison time: 9m 40s | Estimated savings: 5m 28s (56% faster)

|    | Test         | Time  | Result        | Info                              |
|----|--------------|-------|---------------|-----------------------------------|
| 🟥 | `app:test`   | 2m 3s | Failed        | sandbox on box-1 · 2 attempts     |
| 🟨 | `app:e2e`    | 51s   | Flaky         | passed on the same inputs at …    |
| ⬛ | `site:check` |       | Could not run | exited 77, which not_run names …  |
| 🟩 | `app:lint`   | 18s   | Passed        | sandbox on box-1                  |
| 🟪 | `core:test`  |       | Replayed      | from a pass the gate trusts       |
```

- **The head line and the combination.** The first lines say where the
  pull request stands, and which pull requests were tested together, in
  the order they land.
- **One row per test on each system,** failed first. A row says whether
  the test ran, and where, or was replayed from a pass the branch
  trusts. It also says how long it took, how many times your task
  runner tried it, whether it is flaky, and why it could not run.
- **The runner's own figures.** The total time, the comparison time and
  the savings come from your task runner, moon today. They decide
  nothing: a test that passed on its second try is a pass, never flaky.
- **Long reports.** Past 20 rows the rest fold away, and collapsed
  sections list the executor, the changed files and, with `log_lines`,
  the failed tests' output.

While a combination is tested, the report shows each test as it starts
and ends. An ended test shows no result until the combination's results
are read.

A pull request tested together with one that is blamed sees none of
that failure. Its checks go back to queued, and its report says its
combination is formed again. Only the blamed pull request is told.

## The QA example, start to finish

```mermaid
sequenceDiagram
    participant A as agent
    participant S as sluice
    participant G as GitHub
    participant Q as QA and a reviewer
    A->>G: wt submit: push the branch, open PR 41
    S->>G: post "waiting: needs 1 review, qa/approved"
    Q->>G: deploy PR 41 to the test environment, test it
    Q->>G: post qa/approved, approve the review
    S->>G: nothing missing: PR 41 joins the queue
    Note over S: test main + PR 38 + PR 40 + PR 41
    S->>G: post "testing behind PR 38, PR 40"
    S->>G: PR 38, PR 40 land, push main with PR 41
    Note over G: PR 41's commits are in main: merged
    S->>G: post "landed"
    S-->>A: landed
```

## When things change

- **New commits on a pull request a person opened.** It leaves the
  queue and waits again. GitHub drops the old review if your repository
  dismisses stale reviews, and QA tests the new commits. sluice forms
  the combinations behind it again without it.
- **New commits on an agent's pull request.** Commits the agent's own
  submit pushes replace its submission in the queue, and wait for their
  conditions. Commits that arrive any other way, such as a fix another
  submission lands on its branch, take it out of the queue, and sluice
  marks it a draft. No round takes it again in the agent's name until
  the agent's next `sluice submit`, or a person marking it ready for
  review.
- **Sent back as the blamed submission, or for a conflict.** It stays
  open with the comment saying why. A pull request a person opened waits
  for a fix: push one and it waits for its conditions again, or mark it
  a draft and then ready to retry the same commits. An agent's is
  marked a draft, and waits for the agent's next `sluice submit`, with a
  fix or the same commits, or for a person to mark it ready for review.
  That submit marks it ready from any host, whichever host takes
  `main`'s pull requests in. A submit from the branch of one sent back
  that is no draft, such as one a person opened, marks it a draft and
  then ready for review. sluice then takes it again.
- **Withdrawn with `sluice withdraw`.** An agent's pull request is
  marked a draft, and waits the same way. One a person opened stays out
  of the queue as a blamed one does, however long its conditions hold:
  push new commits, or mark it a draft and then ready, to bring it back.
  Its `sluice` check names who withdrew it.
- **Back to draft, or closed.** It leaves the queue.
- **sluice can't read it.** sluice may fail to fetch a pull request's
  head from GitHub, or to read its reviews or checks. sluice then
  prints a warning naming the pull request and what failed, and goes on
  with the branch's other pull requests. sluice reads it again each time
  it asks GitHub until it can, or until the pull request is closed. A draft's `sluice`
  check reads `waiting: a draft` either way.
- **One of a group closed before the group is taken.** The rest wait,
  and say they wait for it, until a submit names them again.
- **Merged with GitHub's button.** Don't, on a branch sluice keeps:
  sluice has no mode for it, as [keeping your team's pull
  requests](#keeping-your-teams-pull-requests) says. With the ruleset
  from the previous guide only the bypass can, and sluice treats it as
  `main` moving under its queue, retesting everything queued.

## GitHub settings

Keep the ruleset from [the previous guide](/sluice/guides/github/). On
top of it:

- Give sluice's app Checks and Pull requests, read and write, beside
  Contents: it posts its check and its comments, and opens pull
  requests, with them.
- Leave GitHub's merge queue off; sluice is the queue.
- Don't require approvals or status checks in the ruleset, sluice's own
  checks included. Put approvals and other systems' checks in
  `[branches.main.pull_requests]` instead, so they are read from `main`
  itself and a pull request can't loosen them. sluice's checks show
  where a pull request stands; what lands is decided by the tests sluice
  runs.
- Turn on "dismiss stale pull request approvals" if new commits should
  need a new review.

## GitHub Actions as sluice's executor

This guide runs sluice's tests on your machine. Running them, and
sluice's rounds, as GitHub Actions jobs is [designed, not
built](/sluice/roadmap/). It will use pull requests exactly as this
guide does, and change only where the work runs.
