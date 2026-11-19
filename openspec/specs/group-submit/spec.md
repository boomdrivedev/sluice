# group-submit Specification

## Purpose
Lets a submitter hand sluice several worktrees' heads as one act, in an
order, so that all of them are queued or none is, and no combination is
formed on any of them until every one is queued (125).

## Requirements

### Requirement: Several worktrees are submitted as one act

`sluice submit <branch>` given `--worktree` more than once SHALL submit
each named worktree's head to `<branch>`, in the order given, as one
act. One `--worktree`, or none, SHALL submit as `sluice submit` does
today.

#### Scenario: Three worktrees in order

- **WHEN** `sluice submit main --worktree ../b1 --worktree ../b2 --worktree ../b3 --no-wait` runs and each worktree holds only its commits
- **THEN** three submissions to `main` are queued, numbered in the order given, and the output names each one with its worktree

#### Scenario: One worktree

- **WHEN** `sluice submit main --worktree ../b1` runs
- **THEN** it submits and answers exactly as `sluice submit main` run in `../b1` does

### Requirement: All are queued or none is

When any named worktree would be refused (changes beyond its commits, no
branch, named twice, or an `--after` the branch does not hold), the act
SHALL queue nothing, push nothing, name every refused worktree with
why, and exit 2.

#### Scenario: One worktree holds uncommitted changes

- **WHEN** `sluice submit main --worktree ../b1 --worktree ../b2` runs and `../b2` holds an uncommitted file
- **THEN** no submission is queued for either, the output names `../b2` and the file, and it exits 2

#### Scenario: A worktree named twice

- **WHEN** `sluice submit main --worktree ../b1 --worktree ../b1` runs
- **THEN** nothing is queued and the output says `../b1` is named twice

### Requirement: No combination is formed until every one is queued

The submissions of one act SHALL become visible to every reader of the
branch's state at once: no round, waiting submit or `sluice wait`, on
this host or another, SHALL ever read some of them queued and not the
rest, and so none forms a combination on any of them alone.

#### Scenario: A round running beside the act

- **WHEN** `sluice run` is running for `main` while three worktrees are submitted as one act
- **THEN** the first combination formed on any of them holds all three, in the order given, and the records hold no combination of the first alone

#### Scenario: S59, one candidate for the set

- **WHEN** three approved bolts are submitted as one act to a branch with `batch = true`, deepest first
- **THEN** the next round forms one combination of all three and tests it once; it passes, and the three land in one move, in the order given

### Requirement: Each lands after the one before

Each submission of an act SHALL be ordered after the one named before
it by its order of submission. `--after` and `--priority` SHALL apply to
every submission of the act alike. No submission of the act SHALL name
another of the act as one it lands after.

#### Scenario: Waiting on an earlier submission

- **WHEN** `sluice submit main --after 40 --worktree ../b1 --worktree ../b2` runs and submission 40 is still queued
- **THEN** both wait for 40, and once 40 is ahead of them they come in the order `../b1`, `../b2`

#### Scenario: Another submission with higher priority

- **WHEN** a later submission with higher priority is queued before the group is in any combination under test
- **THEN** it may come ahead of the group's submissions, which keep their order among themselves

### Requirement: Each is thereafter its own submission

Once queued, each submission of an act SHALL be blamed, set aside for a
conflict, withdrawn, told its outcome and waited on alone, as any
submission is.

#### Scenario: S59, the middle one conflicts

- **WHEN** three heads are submitted as one act and the second conflicts with the first as the combinations form
- **THEN** the second alone is set aside as a conflict, naming the first and the files contested, and the first and third land

#### Scenario: The middle one is blamed

- **WHEN** three heads are submitted as one act and the combination of all three fails on the second's work
- **THEN** the second alone is blamed, and the first and third are tested again without it and land

#### Scenario: One withdrawn

- **WHEN** `sluice withdraw <n>` names the first submission of an act
- **THEN** it alone is withdrawn, and the others are tested and land without it

#### Scenario: Waited on alone

- **WHEN** `sluice wait <n>` names the second submission of an act
- **THEN** it reports that submission's outcome alone and exits as a wait on any submission does

### Requirement: The answer names every submission and its outcome

A waiting act SHALL run rounds until every one of its submissions is
decided, then report each in the order given, a conflict naming what it
is contested with. It SHALL exit 0 when every one landed, otherwise
with the exit of the first in the order given that did not land, and
130 when stopped with Ctrl-C. With `--no-wait` it SHALL exit 0 once all
are queued.

#### Scenario: All land

- **WHEN** a waiting act of two submissions runs and both land
- **THEN** it prints each one's outcome in order and exits 0

#### Scenario: The second conflicts

- **WHEN** a waiting act of three submissions runs and the second is set aside as a conflict while the others land
- **THEN** it prints the first landed, the second's conflict with the files and the submission it is contested with, the third landed, and exits 2

### Requirement: The act's document

Under `--json` the act SHALL print one document on stdout however it
ends: `{"branch": <branch>, "submissions": [...]}`, one item per
worktree in the order given, each with `worktree`. With `--no-wait` an
item SHALL hold `id` and `ahead`; when waiting, it SHALL be the
submission's record as `sluice status <n> --json` prints it. A refusal
SHALL print the refused error document naming each refused worktree.

#### Scenario: No wait

- **WHEN** `sluice submit main --worktree ../b1 --worktree ../b2 --no-wait --json` runs
- **THEN** it prints one document whose `submissions` holds two items, `../b1`'s first, each with `id`, `worktree` and `ahead`

#### Scenario: Waited to the end

- **WHEN** `sluice submit main --worktree ../b1 --worktree ../b2 --json` waits until both land
- **THEN** it prints one document whose `submissions` are the two records, each with `state` `landed`

### Requirement: Several worktrees take neither --check nor --status

`sluice submit` given `--worktree` more than once with `--check` or
`--status` SHALL exit 64 and say each takes one worktree.

#### Scenario: Check over two worktrees

- **WHEN** `sluice submit main --check --worktree ../b1 --worktree ../b2` runs
- **THEN** it exits 64, saying `--check` takes one worktree, and writes nothing

### Requirement: Under pull requests the act opens every pull request

To a branch that takes pull requests, the act SHALL push each
worktree's branch and open its pull request from the branch's own
repository, or update the open one, each marked as one of the group
with its place and the group's size; a pull request the act opens SHALL
be opened as a draft. A refused worktree SHALL push nothing for any.
The act SHALL then read each pull request's conditions, the reviews and
checks the branch's policy names, on the head it pushed (125, C.5).

#### Scenario: Three pull requests

- **WHEN** `main` takes pull requests with no conditions and `sluice submit main --worktree ../b1 --worktree ../b2 --worktree ../b3 --no-wait` runs
- **THEN** the three branches are on the remote, each has an open pull request into `main`, ready for review at its head and marked 1, 2 and 3 of 3 of one group, three submissions from those pull requests are queued in the order given, and the output names each submission with its pull request and exits 0

### Requirement: Under pull requests the act queues them all when every condition holds

When every pull request meets its conditions, the act SHALL mark each
ready for review at its head as one submit does, one sluice marked a
draft when it was sent back included, and SHALL queue a submission for
each in one write, in the order given, before any round can take one of
them alone. It SHALL answer naming every submission with its pull
request, and under `--json` with `--no-wait` each item of `submissions`
SHALL hold `pull_request` beside `id`, `worktree` and `ahead`.

#### Scenario: Refused, reviewed, submitted again

- **WHEN** an act over two worktrees to `main` with `reviews = 1` was refused, every pull request needing a review, and a reviewer approves each draft at its head, and the same act runs again
- **THEN** both pull requests are ready for review and two submissions are queued in the order given, in one write

#### Scenario: A pull request sluice held as a draft

- **WHEN** one pull request of an earlier act was blamed and sluice marked it a draft, and its worktree is named again in a new act whose conditions all hold
- **THEN** the pull request is ready for review at the head pushed, and it is queued with the rest of the new act

#### Scenario: A waiting act follows each pull request

- **WHEN** the act of three runs without `--no-wait`, every condition holding, and all three land
- **THEN** it reports each one's states as it reaches them, then each one's outcome in the order given, and exits 0

### Requirement: Under pull requests the act queues none when one falls short

When any pull request falls short of its conditions, the act SHALL
queue nothing and mark none ready for review; it SHALL leave every pull
request of the act a draft at its head, SHALL name each one short of its
conditions with its worktree and what it lacks, SHALL say the others are
drafts as well, and SHALL exit 2. Under `--json` it SHALL print the
refused error document naming each such worktree.

#### Scenario: One short of a review

- **WHEN** `main` takes pull requests with `reviews = 1`, the pull requests of `../b1` and `../b3` are open at the heads their worktrees hold, each with an approving review there, `../b2`'s has none, and the act runs over the three
- **THEN** no submission is queued, none of the three is ready for review, each is a draft at its head, the output names `../b2`'s pull request and that it needs 1 review, and it exits 2

### Requirement: Under pull requests the act takes no fork's pull request

A pull request from a fork SHALL never be one of the act: the act opens
or updates only a pull request from the branch's own repository.

#### Scenario: A fork's pull request on a branch of the same name

- **WHEN** a fork has an open pull request into `main` from its own `unit/b1`, and the act names a worktree on `unit/b1`
- **THEN** the fork's pull request is not edited, marked or queued, and the act's pull request is one from the repository's `unit/b1`

### Requirement: Under pull requests a new head on one of the act holds that one alone

Once the act's submissions are queued, a new head on one of its pull
requests SHALL withdraw that pull request's submission alone, and
sluice SHALL mark that pull request a draft; the act's other
submissions SHALL stay queued, be tested without it, and land. That
pull request SHALL be submitted again only on its submitter's act, a
submit from its worktree or a person marking it ready for review, never
because its head moved (120, 125).

#### Scenario: A commit pushed to the second of three

- **WHEN** three pull requests were queued by one act, and a commit no submit made is pushed to the second's branch
- **THEN** the second's submission is withdrawn and its pull request is a draft; the first and third are not withdrawn, are tested without the second, and land; and no round submits the second again

#### Scenario: Its submitter submits it again

- **WHEN** after that the second's worktree runs `sluice submit main --no-wait`
- **THEN** its pull request is ready for review at the new head, marked one of no group, and is submitted alone once its conditions hold

### Requirement: A group takes one subject per worktree or none
`sluice submit <branch>` given `--worktree` more than once SHALL take
`--subject` once for each worktree, the first subject the first
worktree's and so on in their order, or not at all. Any other count of
subjects SHALL be refused, exit 64, before anything is pushed, opened
or queued, saying to give one `--subject` per `--worktree`, in their
order, or none (125).

#### Scenario: One subject for two worktrees
- **WHEN** an agent runs `sluice submit main --worktree ../a --worktree ../b --subject "Both"`
- **THEN** it exits 64, saying to give one `--subject` per `--worktree`, in their order, or none, and nothing is pushed, opened or queued

#### Scenario: A subject for each
- **WHEN** an agent runs `sluice submit bolt/x --no-wait --worktree ../a --worktree ../b --subject "Batch review cards" --subject "Card filters"` and `bolt/x` has no `pull_requests`
- **THEN** two submissions are queued in that order, the first keeping the subject `Batch review cards` and the second `Card filters`

#### Scenario: No subject for any
- **WHEN** an agent runs `sluice submit bolt/x --no-wait --worktree ../a --worktree ../b`
- **THEN** two submissions are queued with no subject, each named by its branch when it lands

### Requirement: Under pull requests each request is titled by its own subject
To a branch that takes pull requests, each pull request the act opens
SHALL be titled by its worktree's subject, as written, or by that
worktree's branch where the act gives no subjects; each it updates that
sluice opened SHALL take its worktree's subject as its title when the
act gives subjects; one a person opened SHALL keep its title (118, 120,
125).

#### Scenario: Two requests, two subjects
- **WHEN** `main` takes pull requests with no conditions and `sluice submit main --no-wait --worktree ../a --worktree ../b --subject "Batch review cards" --subject "Card filters"` runs, `../a` on `unit/a` and `../b` on `unit/b`
- **THEN** `unit/a`'s pull request is titled `Batch review cards` and `unit/b`'s `Card filters`

#### Scenario: Two requests, no subjects
- **WHEN** the same act runs with no `--subject`
- **THEN** `unit/a`'s pull request is titled `unit/a` and `unit/b`'s `unit/b`

### Requirement: Under pull requests each request names the wait that follows it

Under pull requests, the act's answer SHALL name, for each pull request
still open and undecided, the wait that follows it, `sluice wait
<branch> --pull-request <n>`: under `--json` as `wait` in its item of
`submissions`, beside `pull_request`, and in words on a line `wait:
sluice wait <branch> --pull-request <n>`. A waiting act that ends with
one still open SHALL say `still open: #<n>; sluice wait <branch>
--pull-request <n>` for it (125, C.5).

#### Scenario: No wait, every condition holding

- **WHEN** `main` takes pull requests with no conditions, and `sluice
  submit main --worktree ../b1 --worktree ../b2 --no-wait --json` runs,
  opening `<n1>` and `<n2>`
- **THEN** it exits 0, and each item of `submissions` holds `id`,
  `pull_request` and `wait`, `sluice wait main --pull-request <n1>` for
  the first and `sluice wait main --pull-request <n2>` for the second

#### Scenario: No wait, in words

- **WHEN** the same act runs without `--json`
- **THEN** its output holds `wait: sluice wait main --pull-request <n1>`
  and `wait: sluice wait main --pull-request <n2>`

#### Scenario: Stopped while both are open

- **WHEN** the same act runs without `--no-wait`, and is stopped with
  Ctrl-C before either pull request is decided
- **THEN** it says `still open: #<n1>; sluice wait main --pull-request
  <n1>` and `still open: #<n2>; sluice wait main --pull-request <n2>`,
  and exits 130
