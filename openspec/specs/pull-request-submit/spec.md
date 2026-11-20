# pull-request-submit Specification

## Purpose

Lets an agent's `sluice submit` or `wt submit` to a branch with pull
requests open a pull request for its work, and follow it to its outcome.

## Requirements

### Requirement: submit opens or updates a pull request
`sluice submit <branch>` and `wt submit <branch>`, to a branch with
`pull_requests`, SHALL open a pull request into the branch from the
agent's branch, ready for review, or update the one already open from
that agent's branch and mark it ready for review at the head
submitted. They SHALL NOT submit the pull request: it waits for its
conditions like any other (1, 120, 124).

#### Scenario: First submit
- **WHEN** an agent runs `wt submit main` from a worktree on `unit/review` and `main` has `pull_requests`
- **THEN** `unit/review` is on the remote, an open pull request from it into `main` exists, and the output names the pull request and what it waits for

#### Scenario: Submit again
- **WHEN** the agent commits again and runs `wt submit main` from the same worktree
- **THEN** the same pull request now has the new head, and no second pull request is opened

#### Scenario: Submit a draft sluice made
- **WHEN** sluice marked the agent's pull request a draft after it was sent back, and the agent runs `wt submit main`
- **THEN** the pull request is ready for review at the head pushed, and the next round submits it once its conditions hold

#### Scenario: Submitted by the operator's ops
- **WHEN** a pull request an agent opened was sent back, and the operator's ops runs `sluice submit main` from a worktree on the same branch
- **THEN** the pull request is ready for review at the head pushed, names ops as its agent, and the next round submits it once its conditions hold

### Requirement: A submit pushes only a branch sluice lands on nowhere
From a worktree on a branch sluice lands on nowhere, `sluice submit
<branch>` and `wt submit <branch>`, to a branch with `pull_requests`,
SHALL push that branch to the branch's remote first, and the head
submitted is the worktree's head. A submit SHALL push only a branch
sluice lands on nowhere, and SHALL write no ref of a branch sluice
lands on, on the host or the remote (1, 120, 124).

#### Scenario: A worktree holding more than its commits
- **WHEN** the worktree holds uncommitted changes
- **THEN** the submit is refused as it is today, and nothing is pushed

### Requirement: A submit from a worktree on a branch sluice lands on pushes nothing
From a worktree that has checked out a branch sluice lands on, such as
a bolt, `sluice submit <branch>` and `wt submit <branch>`, to a branch
with `pull_requests`, SHALL push nothing: the pull request is from that
branch as the remote holds it, the head submitted is the remote's
revision of it, and where the worktree's head, the host's copy, differs
from that revision, the output SHALL say so with both revisions (1,
120, 124).

#### Scenario: A bolt's landing worktree, its copy two landings behind (S58)
- **WHEN** a worktree has checked out `bolt/x`, which sluice lands on and whose policy names `remote = "origin"`, this host's `bolt/x` stands two landings behind origin's `8079db3`, `main` has `pull_requests`, and the worktree runs `wt submit main --no-wait`
- **THEN** origin's `bolt/x` is still `8079db3`, this host's `bolt/x` is where it was, a pull request from `bolt/x` into `main` is open and ready for review at `8079db3`, and the output names the pull request and says this host's `bolt/x` is behind origin's, naming both revisions, without saying it pushed anything

#### Scenario: The next read levels the copy, and a hand cannot
- **WHEN** after that submit a tool in the worktree runs `git reset --hard 8079db3`, and then `sluice step bolt/x` runs
- **THEN** the reset is refused, naming `git reset --hard 8079db3`, the revision this host's `bolt/x` holds and `8079db3`; the step sets this host's `bolt/x` to `8079db3`

#### Scenario: Submit again from the bolt after another landing on it
- **WHEN** the bolt's pull request is open at `8079db3`, another submission lands on `bolt/x` as `5a1e2f0`, and the worktree runs `wt submit main --no-wait` again
- **THEN** no second pull request is opened, nothing is pushed, and the pull request is ready for review at `5a1e2f0`

### Requirement: A waiting submit follows its pull request
A `wt submit` or `sluice submit` that waits SHALL report each state its
pull request reaches, run sluice's rounds while it waits as it does
today, and exit with the outcome: landed, failed or conflict.

#### Scenario: Waits through review
- **WHEN** an agent's waiting submit opens a pull request that needs a review, and a reviewer approves it, and it lands
- **THEN** the submit prints waiting, then queued or testing, then landed, and exits 0

### Requirement: A submit that does not wait answers with what sluice read
A `wt submit` or `sluice submit` with `--no-wait` SHALL return once the
pull request is open and sluice has read it once, and SHALL answer with
the submission sluice took for the pull request at the head it pushed,
when sluice took it then because its conditions held; otherwise with the
pull request and what it waits for. Either answer SHALL exit 0.

#### Scenario: No wait
- **WHEN** an agent runs `wt submit main --no-wait`
- **THEN** it returns once the pull request is open, and `wt submit --status` later reports its state

#### Scenario: No wait, in words
- **WHEN** `main` takes pull requests with no conditions, and an agent runs `wt submit main --no-wait`
- **THEN** after the line `pushed unit/<name> · opened #<n>: <title>` it prints `submission <id> to main from #<n> · nothing queued ahead`, and exits 0

### Requirement: A submit that does not wait answers in one document
Under `--json`, a `--no-wait` submit's answer with a submission SHALL be
the document a `--no-wait` submit prints on any branch, `id`, `branch`
and `ahead`, with `pull_request`, the pull request's number, and `wait`,
the wait that follows it, beside them; its answer with a pull request
SHALL be `pull_request`, `branch`, `state`, the state sluice posts on
it, and `wait`.

#### Scenario: No wait, its conditions hold
- **WHEN** `main` takes pull requests with no conditions, and an agent runs `wt submit main --no-wait --json` from a worktree on `unit/ready`
- **THEN** it exits 0 and prints one document with the `id` of a submission from the pull request's checkout `pull/<n>` at the head pushed, `branch` `main`, `ahead`, `pull_request` `<n>` and `wait` `sluice wait main --pull-request <n>`; and `sluice wait <id> --json` follows that submission to `landed`

#### Scenario: No wait, waiting for a review
- **WHEN** `main` takes pull requests with `reviews = 1`, and an agent runs `wt submit main --no-wait --json`
- **THEN** it exits 0 and prints `{"pull_request": <n>, "branch": "main", "state": "waiting: needs 1 review", "wait": "sluice wait main --pull-request <n>"}`, with no `id`, and no submission of the pull request is queued

### Requirement: The agent's worktree is removed only on landing
When a pull request opened by `wt submit` lands, the worktrunk driver
SHALL remove the agent's worktree as it does for any landing; when it is
sent back, the worktree SHALL stay.

#### Scenario: Landed
- **WHEN** the pull request opened by an agent's `wt submit` lands
- **THEN** the agent's worktree is removed unless the submit was given `--no-remove`

### Requirement: A submit on a queued pull request replaces its submission
When a submit pushes a new head to a pull request whose submission is
still queued and not landing, the submit SHALL withdraw that submission
itself. The pull request SHALL then wait for its conditions on the new
head. sluice SHALL NOT mark it a draft for that change.

#### Scenario: A new commit, submitted
- **WHEN** an agent's pull request is queued, and the agent commits again and runs `wt submit main --no-wait`
- **THEN** the old submission is withdrawn, the pull request is ready for review at the new head, and the next round submits the new head once its conditions hold

### Requirement: A submit's advice says what it takes
What a submit prints for a pull request sent back SHALL name only ways
on that sluice takes. A submit of the same head SHALL be taken and
tested again, never answered with the verdict of the head's earlier
submission.

#### Scenario: Timed out, the same head submitted again
- **WHEN** a waiting `wt submit` ends with its pull request timed out and the advice "if the host was slow, submit the same head again", and the agent runs `wt submit main` with no new commits
- **THEN** the submit prints the new combination under test and its outcome, never "nothing was queued or run again"

#### Scenario: Blamed advice
- **WHEN** a waiting `wt submit` ends with its pull request blamed
- **THEN** it says to fix it in the worktree and submit again, and says nothing of marking the pull request a draft

### Requirement: A pull request's description holds its commits' messages whole
The description of a pull request `sluice submit` or `wt submit` opens
or updates SHALL hold the message of each commit between the branch it
lands on and the head submitted, whole and oldest first, a blank line
between them, followed by sluice's marks, whenever that fits GitHub's
limit of 65,536 characters (C.5 pull request).

#### Scenario: Every message fits
- **WHEN** an agent's branch holds three commits whose messages total a few hundred characters, and the agent runs `wt submit main --no-wait`
- **THEN** the pull request's description is the three messages, whole and oldest first, then the line naming the agent, and holds no line counting more commits

### Requirement: A description too long for GitHub ends with the rest's subjects
When the messages do not all fit, the description SHALL hold the first
messages, each whole, as many as fit; then a line giving the count of
the rest and the address of GitHub's compare view of the request's
branch against the branch it lands on; then each of the rest's subjects
in order; then sluice's marks. No message SHALL be cut part way, and
GitHub SHALL NOT refuse the description for its length (C.5).

#### Scenario: A bolt too long for one description
- **WHEN** an agent's branch holds three commits whose messages are 30,000 characters each, and the agent runs `wt submit main --no-wait`
- **THEN** GitHub accepts the pull request; its description holds the first two messages whole, then the line `1 more commit, each by its subject; all of them in full: https://github.com/<owner>/<repo>/compare/main...unit/<name>`, then `- ` and the third commit's subject, then the line naming the agent; and it is no longer than 65,536 characters

#### Scenario: One message longer than the limit
- **WHEN** an agent's branch holds one commit whose message is 70,000 characters, and the agent runs `wt submit main --no-wait`
- **THEN** GitHub accepts the pull request; its description holds no part of the message's body, the line `1 more commit, …` with the compare view's address, the commit's subject, and the line naming the agent

#### Scenario: More subjects than the limit holds
- **WHEN** an agent's branch holds 70 commits whose subjects are 1,000 characters each, and the agent runs `wt submit main --no-wait`
- **THEN** GitHub accepts the pull request; its description holds no message whole, the line `70 more commits, …` with the compare view's address, the first subjects in order as many as fit, each whole, then the line `and <m> more, in the compare view`, then the line naming the agent

### Requirement: A submit again writes the description for its new head
A submit that updates an open pull request SHALL write its description
as these requirements say for the head it submits, in place of the
description the request had, before that head is pushed, and SHALL leave
the description as it is when it already reads so.

#### Scenario: A new commit, submitted again
- **WHEN** an agent's pull request was opened at one commit, the agent commits again with the message `Add the second file`, and runs `wt submit main --no-wait`
- **THEN** the pull request's description holds both messages, oldest first, then the line naming the agent

#### Scenario: Submitted again at the same head
- **WHEN** an agent runs `wt submit main --no-wait` twice with no commit between
- **THEN** the second submit does not edit the pull request's description

### Requirement: A pull request's status names its head and whether it is a draft

On a branch that takes pull requests, `sluice submit <branch> --status --json`, and `wt submit <branch> --status --json`, SHALL print, for a pull request open and not yet decided, its `head`, the revision the forge holds as its head, `draft`, `true` while it is a draft, and `sends`, the revision a submit from the worktree would send now (124), beside `pull_request`, `branch` and `state`. A decided one SHALL still print its submission's record, with `sends`, where the host holds one.

#### Scenario: Waiting for a review

- **WHEN** `main` takes pull requests with `reviews = 1`, an agent's `wt submit main --no-wait` opened pull request `<n>` at `<rev>`, and `wt submit main --status --json` runs in the agent's worktree with nothing committed since
- **THEN** it exits 0 and prints `pull_request` `<n>`, `branch` `main`, `state` `waiting: needs 1 review`, `head` `<rev>`, `draft` `false` and `sends` `<rev>`

#### Scenario: A draft

- **WHEN** an agent's pull request is open as a draft at `<rev>`, and `wt submit main --status --json` runs in the agent's worktree
- **THEN** it prints `head` `<rev>` and `draft` `true`

### Requirement: A pull request a submit opens is titled by what lands
A pull request `sluice submit` or `wt submit` opens SHALL be titled by
the subject the submit gives with `--subject`, exactly as written, or,
where it gives none, by the name of the branch submitted. Its title
SHALL never be a commit's message, the first commit's or any other's
(118, C.5 pull request).

#### Scenario: A bolt submitted with no subject
- **WHEN** a worktree on `bolt/review-cards`, whose first commit is `docs(openspec): propose review-cards`, runs `wt submit main --no-wait` and `main` has `pull_requests`
- **THEN** the pull request opened is titled `bolt/review-cards`, and the output says `opened #<n>: bolt/review-cards`

#### Scenario: A session submitted with a subject
- **WHEN** an agent on `unit/review` runs `wt submit main --no-wait --subject "Batch review cards"`
- **THEN** the pull request opened is titled `Batch review cards`, and the output says `pushed unit/review · opened #<n>: Batch review cards`

#### Scenario: The description still carries the commits
- **WHEN** a submit with `--subject "Batch review cards"` opens a pull request from a branch holding two commits
- **THEN** the description holds both commits' messages, whole and oldest first, and the title is not either message's first line

### Requirement: A submit with a subject retitles a pull request sluice opened
A submit that updates an open pull request sluice opened SHALL set its
title to the subject the submit gives, when it gives one that differs
from the title. A submit that gives no subject SHALL leave the title as
it is (C.5 pull request, 120).

#### Scenario: A new head with a subject
- **WHEN** sluice opened `unit/review`'s pull request titled `unit/review`, the agent commits again and runs `wt submit main --no-wait --subject "Batch review cards"`
- **THEN** the same pull request, at the new head, is titled `Batch review cards`

#### Scenario: A new head with no subject
- **WHEN** sluice opened a pull request titled `Batch review cards`, the agent commits again and runs `wt submit main --no-wait`
- **THEN** the pull request at the new head is still titled `Batch review cards`

#### Scenario: A request opened before titles changed
- **WHEN** a pull request sluice opened is titled by its first commit, `docs(openspec): propose review`, and the agent runs `wt submit main --no-wait --subject "Batch review cards"`
- **THEN** the pull request is titled `Batch review cards`

### Requirement: A pull request a person opened keeps its title
A submit that updates a pull request a person opened SHALL NOT change
its title, whether or not the submit gives a subject; it writes the
description and marks the request ready as it does for any request
(120, C.5 pull request).

#### Scenario: A subject on a request opened by hand
- **WHEN** a person opened a pull request from `unit/review` titled `Review cards, first pass`, and an agent on `unit/review` runs `wt submit main --no-wait --subject "Batch review cards"`
- **THEN** the pull request is still titled `Review cards, first pass`, its description holds the branch's commits, and it is ready for review at the head submitted

### Requirement: A subject is one line
`--subject` SHALL take one line of text. A subject that is empty or
holds a line break, or `--subject` given with `--check` or `--status`,
SHALL be refused as a command line sluice cannot read, exit 64, before
anything is pushed or opened.

#### Scenario: Two lines
- **WHEN** an agent runs `sluice submit main --subject "$(printf 'one\ntwo')"`
- **THEN** it exits 64, saying a subject is one line, and nothing is pushed or opened

#### Scenario: An empty subject
- **WHEN** an agent runs `sluice submit main --subject ""`
- **THEN** it exits 64, saying the subject is empty, and nothing is pushed or opened

#### Scenario: With a check
- **WHEN** an agent runs `sluice submit main --check --subject "Batch review cards"`
- **THEN** it exits 64 and nothing is forecast, pushed or opened

### Requirement: A pull request's status on another host names sluice's check

On a host that does not take the branch's pull requests in (137), `sluice submit <branch> --status --json` and `wt submit <branch> --status --json` SHALL print a pull request landed or sent back with `state` `landed`, `failed` or `conflict`, the `title` and `summary` of sluice's check on its head, and its `pull_request`, `branch`, `head`, `draft` and `sends`, and SHALL exit 0, as `--status` does wherever it finds the worktree's pull request.

#### Scenario: Sent back on another host

- **WHEN** host B takes main's pull requests in, B blamed an agent's pull request `<n>` at `<rev>` and marked it a draft, and `wt submit main --status --json` runs in the agent's worktree on host A
- **THEN** it exits 0 and prints `pull_request` `<n>`, `branch` `main`, `state` `failed`, the `title` and `summary` of sluice's check on `<rev>`, `head` `<rev>`, `draft` `true` and `sends` `<rev>`

### Requirement: A submit takes again a pull request another host sent back

Where another host takes the branch's pull requests in (137), `sluice submit <branch>` and `wt submit <branch>` SHALL mark the worktree's pull request ready for review at the head pushed when sluice's check on its head before the push says it was sent back, failed or in conflict, or held for the next submit, draft or not, as a submit does on the host that takes it in (120, C.5). They SHALL NOT mark a draft, even for a moment, a pull request whose check says anything else.

#### Scenario: Sent back as a draft

- **WHEN** host B takes main's pull requests in, B blamed an agent's pull request `<n>` and marked it a draft, and the agent runs `sluice submit main --no-wait` on host A with nothing committed since
- **THEN** `<n>` is ready for review at that head, and B's next pass takes it in again

#### Scenario: Sent back, not a draft

- **WHEN** host B blamed pull request `<n>`, which a person opened from `unit/review` and which B therefore did not mark a draft, and an agent runs `sluice submit main --no-wait` on host A from a worktree on `unit/review` with nothing committed since
- **THEN** `<n>` is marked a draft and then ready for review at that head, and B's next pass takes it in again

#### Scenario: Still queued on the other host

- **WHEN** host B has pull request `<n>` queued, its `sluice` check saying `queued: next on main`, and the agent runs `sluice submit main --no-wait` on host A with nothing committed since
- **THEN** `<n>` is never marked a draft, and B's submission of it stays queued

### Requirement: A pull request's answer names the wait that follows it

Wherever a submit to a branch that takes pull requests answers for a
pull request still open and undecided, with `--no-wait` or `--status`,
from a worktree or by name (124), whichever host takes it in (137), its
answer SHALL name the wait that follows that request, `sluice wait
<branch> --pull-request <n>`: under `--json` as `wait`, and in words on
a line of its own after the answer, `wait: sluice wait <branch>
--pull-request <n>` (C.5; rule core.48).

#### Scenario: No wait, another host takes it in

- **WHEN** host `b` keeps `main` and takes its pull requests in, and an
  agent runs `sluice submit main --no-wait --json` on host `a` from a
  worktree on `unit/two-apps`, which opens pull request `<n>`
- **THEN** it exits 0 and prints `pull_request` `<n>`, `branch` `main`,
  `state`, `taken_in_by` `{"host":"b"}` and `wait` `sluice wait main
  --pull-request <n>`, with no `id`; and that wait, run in the
  repository's main checkout on `a`, follows `<n>` to the outcome `b`
  gives it

#### Scenario: No wait, in words, taken in

- **WHEN** `main` takes pull requests with no conditions, and an agent
  runs `wt submit main --no-wait` from a worktree on `unit/ready`
- **THEN** after `submission <id> to main from #<n> · nothing queued
  ahead` it prints `wait: sluice wait main --pull-request <n>`, and
  exits 0

#### Scenario: No wait, in words, waiting for a review

- **WHEN** `main` takes pull requests with `reviews = 1`, and an agent
  runs `wt submit main --no-wait` from a worktree on `unit/review`
- **THEN** after `waiting: needs 1 review` it prints `wait: sluice wait
  main --pull-request <n>`, and exits 0

#### Scenario: Its status

- **WHEN** an agent's pull request `<n>` waits for a review, and `wt
  submit main --status --json` runs in the agent's worktree
- **THEN** it prints `wait` `sluice wait main --pull-request <n>` beside
  `pull_request`, `branch`, `state`, `head`, `draft` and `sends`

#### Scenario: Submitted by name

- **WHEN** `main` takes pull requests with `reviews = 1`, and `sluice
  submit main --head bolt/x --no-wait --json` runs in the repository's
  main checkout, which opens pull request `<n>` from `bolt/x`
- **THEN** it prints `pull_request` `<n>` and `wait` `sluice wait main
  --pull-request <n>`, with no `id`
