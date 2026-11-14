# pull-request-intake Specification

## Purpose

Decides which pull requests into a branch sluice submits to its queue,
and when: a pull request waits until it is out of draft and every
condition the branch's policy names holds on its head.

## Requirements

### Requirement: A branch turns pull requests on in its policy
A branch's policy SHALL accept a `[branches."<pattern>".pull_requests]`
table with `reviews`, a count of approving reviews, default 0, and
`checks`, a list of check names, default empty. The table SHALL be
refused on a branch with no `remote`. The conditions SHALL be read from
the branch's revision.

#### Scenario: Pull requests need a remote
- **WHEN** a branch's policy has a `pull_requests` table and no `remote`
- **THEN** the policy is refused with a message naming the branch and `remote`

#### Scenario: A pull request cannot loosen its own conditions
- **WHEN** a pull request changes `.config/sluice.toml` to lower `reviews`
- **THEN** its conditions are still the ones on the branch's revision

### Requirement: A pull request is submitted when it is ready
On the host that takes a branch's pull requests in, each asking of the
branch's open pull requests SHALL submit every open pull request into
the branch, from a branch of the same repository, that is out of draft
and whose head has at least `reviews` approving reviews and a passing
result for every name in `checks`. Every other open pull request SHALL
wait, and no check of sluice's SHALL run for it (136, 137).

#### Scenario: Waiting for QA
- **WHEN** a pull request is out of draft with one approving review and `checks = ["qa/approved"]`, and `qa/approved` has not passed
- **THEN** it is not submitted; when `qa/approved` passes on its head, the next asking submits it

#### Scenario: A draft waits
- **WHEN** a pull request is a draft and every condition holds
- **THEN** it is not submitted until it is marked ready for review

#### Scenario: A pull request opened by hand
- **WHEN** a person opens a pull request into the branch from a branch of the repository, out of draft, with every condition holding
- **THEN** the next asking submits it, within the host's `discovery` interval, with no `sluice submit` run for it

#### Scenario: Reviews on an older head do not count
- **WHEN** a pull request's only approving review was given on a commit before its current head
- **THEN** it does not meet `reviews = 1`

### Requirement: Pull requests from forks are never submitted
A pull request whose head is in another repository SHALL never be
submitted, whatever its conditions.

#### Scenario: A fork's pull request
- **WHEN** a pull request from a fork is out of draft and every condition holds
- **THEN** it is never submitted

### Requirement: A changed pull request leaves the queue
A submitted pull request SHALL be withdrawn when its head changes, when
it is moved back to draft, or when it is closed, merged by hand without
its head reaching the branch included. After a new head, a pull request
opened by hand SHALL wait for its conditions on that head; one a submit
opened SHALL be marked a draft and wait for its submitter's act.

#### Scenario: A new commit while queued
- **WHEN** a commit is pushed to a submitted pull request a person opened by hand
- **THEN** its submission is withdrawn, the submissions behind it are tested without it, and it is submitted again only once its conditions hold on the new head

#### Scenario: A new commit on a pull request a submit opened
- **WHEN** a commit that no submit pushed reaches a submitted pull request that `wt submit` opened, such as a fix another submission landed on its branch
- **THEN** its submission is withdrawn, sluice marks the pull request a draft, and no later round submits it, whatever its conditions

#### Scenario: Closed while queued
- **WHEN** a submitted pull request is closed
- **THEN** its submission is withdrawn

### Requirement: A pull request sent back is not resubmitted unchanged
A pull request opened by hand whose submission was blamed for a failure,
removed for a conflict, or withdrawn by the operator or a planner SHALL
be submitted again only on a new head, or after it is moved to draft and
back to ready for review on the same head.

#### Scenario: Blamed, then retried
- **WHEN** a hand-opened pull request's submission is blamed, and its author marks it draft and then ready again without new commits
- **THEN** the next round submits the same head again

#### Scenario: Blamed and left alone
- **WHEN** a hand-opened pull request's submission is blamed and nothing changes on it
- **THEN** no later round submits it again

#### Scenario: Withdrawn by the operator, opened by hand
- **WHEN** the operator runs `sluice withdraw` on the submission of a pull request a person opened by hand, and nothing changes on it
- **THEN** no later round submits it again, however long its conditions hold

#### Scenario: Withdrawn by the operator, then retried by its author
- **WHEN** the operator withdrew a hand-opened pull request's submission, and its author marks it draft and then ready again without new commits
- **THEN** the next round submits the same head again once its conditions hold

#### Scenario: Withdrawn by the operator, then a new commit
- **WHEN** the operator withdrew a hand-opened pull request's submission, and its author pushes a new commit
- **THEN** the next round submits the new head once its conditions hold

### Requirement: A pull request a submit opened comes back only on its submitter's act
A pull request a submit opened or updated is one whose description
names its agent. When its submission is withdrawn, blamed, timed out or
removed for a conflict, sluice SHALL mark it a draft. No round SHALL
submit it again until a submit marks it ready for review, or a person
does on GitHub, after the submission left. That holds on any head,
the one sent back included.

#### Scenario: Refused, then its branch moves
- **WHEN** a pull request `wt submit` opened is removed for a conflict, and another submission lands a fix on its branch so its head moves
- **THEN** it is a draft, no round submits it, and nothing lands in its agent's name

#### Scenario: Blamed, then submitted again on the same head
- **WHEN** a pull request `wt submit` opened is blamed, and its agent runs `wt submit` again with no new commits
- **THEN** the next round submits the same head and its checks run again

#### Scenario: Timed out, then submitted again on the same head
- **WHEN** a pull request `wt submit` opened is blamed because the branch's time limit ended its run, and its agent runs `wt submit` again with no new commits
- **THEN** the next round submits the same head and its checks run again

#### Scenario: Marked ready by a person
- **WHEN** a pull request `wt submit` opened was sent back and marked a draft, and a person marks it ready for review on GitHub
- **THEN** the next round submits it once its conditions hold

#### Scenario: Withdrawn by the operator
- **WHEN** the operator runs `sluice withdraw` on the submission of a pull request `wt submit` opened
- **THEN** sluice marks the pull request a draft and no later round submits it until a submit or a person marks it ready

#### Scenario: Held when it cannot be marked a draft
- **WHEN** a pull request `wt submit` opened is sent back and GitHub refuses to mark it a draft
- **THEN** no round submits it, its `sluice` check says it waits for the next submit, and each later round tries the draft again

### Requirement: A pull request sluice cannot read holds no other
When a round cannot fetch an open pull request's head, or cannot read
its reviews, checks or timeline, draft or ready, it SHALL NOT submit it
that round, and SHALL go on to the branch's other pull requests,
submitting, forecasting and posting each as it would have been. It
SHALL read that pull request again each round until it can be read or
it is closed, and SHALL say what failed, naming `pull/<n>`, as a
standing intake failure is said (128).

#### Scenario: GitHub serves no head for one of two pull requests
- **WHEN** two pull requests are open into `main`, out of draft, every condition holding, and GitHub serves no head for the first, whose branch is gone, and `sluice step main` runs
- **THEN** its warning names the first and the failed fetch, and the second is submitted, its combination formed and its state posted

#### Scenario: Read again until GitHub serves its head
- **WHEN** a pull request's head could not be fetched for ten passes, and later its branch is pushed again so GitHub serves its head
- **THEN** each `sluice step` until then names it in a warning, main's log holds one `read-refused` line marked `intake` for it, and the first round after submits it

#### Scenario: Closed, it is no longer read
- **WHEN** a pull request whose head could not be fetched is closed
- **THEN** no later round names it

#### Scenario: A draft whose head cannot be fetched
- **WHEN** a draft's head cannot be fetched and another pull request is ready with every condition holding, and `sluice step main` runs
- **THEN** its warning names the draft, and the other pull request is submitted

#### Scenario: Reviews that cannot be read
- **WHEN** the policy names `reviews = 1`, GitHub answers an error for one pull request's reviews, another pull request has its review, and `sluice step main` runs
- **THEN** its warning names the first and the failed read, the first is not submitted, and the second is submitted

### Requirement: A failure that is no one pull request's still ends intake
A failure that is no single pull request's, such as listing the
branch's pull requests or reading its records, SHALL end that branch's
intake for the round, said as a standing intake failure is said, and
the round SHALL still land what is queued.

#### Scenario: GitHub's list of pull requests fails
- **WHEN** GitHub answers an error for the list of `main`'s open pull requests, and `sluice step main` runs
- **THEN** its warning says intake of `main` failed, no pull request is submitted or withdrawn, and a submission already queued is still tested and landed

### Requirement: A group's pull requests are taken together

Intake SHALL take the pull requests a group's marks bind only all
together, in one pass and one write, in the group's order, once every
one of them is open, out of draft and meets its conditions on the head
it was marked at. Until then each SHALL wait, its state naming the pull
requests of its group it waits for. The group SHALL bind a pull request
only until it is first submitted at that head.

#### Scenario: Two of three are ready

- **WHEN** a group submit of three was refused for want of reviews, leaving three drafts marked as one group, a reviewer approves the first two, and a person marks all three ready for review
- **THEN** none is submitted, and each of the first two says it waits for the third; once the third is approved, the next round submits all three in the group's order and the first combination formed holds all three

#### Scenario: A round between the opens

- **WHEN** `sluice run` is running for `main` while three pull requests marked as one group are opened by hand, their conditions all holding
- **THEN** no round submits the first before the third is open, and the first combination formed holds all three

#### Scenario: One of the group is closed

- **WHEN** one pull request of a group is closed before the group is submitted, and a person has marked the others ready for review
- **THEN** the others are not submitted and say they wait for it, until a new submit names them again

### Requirement: A new head on one of a submitted group withdraws that one alone

Once a group's pull requests are submitted, by intake or by the act
itself, a new head on one SHALL withdraw that submission alone; one a
submit opened SHALL be marked a draft and wait for its submitter's act,
as any pull request a submit opened does, and the others SHALL stay
queued (120, 125).

#### Scenario: Each its own after it is taken

- **WHEN** a group's three pull requests were submitted together and a commit no submit made is pushed to the second
- **THEN** only the second's submission is withdrawn and the second is marked a draft; the first and third are tested without it and land, and no round submits the second until its submitter submits it again or a person marks it ready for review

### Requirement: A pull request whose head the branch holds is not withdrawn
A submitted pull request that leaves the open pull requests because the
branch, as that pass fetched it, holds its head SHALL NOT be withdrawn:
its work landed, by whichever landing took it, and the round lands its
submission as that landing (122, 5).

#### Scenario: Landed by another queue while queued
- **WHEN** a submitted pull request is under test, and another queue of the branch, a second copy of the repository, lands its head, so GitHub calls it merged
- **THEN** its submission is not withdrawn: it is landed as that queue's landing commit, its run ended, nothing more lands of it, and `sluice wait` on it exits 0

### Requirement: A pull request table on records each host keeps refuses its branch alone
A branch's `pull_requests` table SHALL refuse that branch alone, naming
`[signers]`, in a policy that names no `[signers]`, whose branches'
records each host keeps; the policy's other branches SHALL be read as
before (56, 137, S64).

#### Scenario: Pull requests need records on the remote
- **WHEN** main's policy has a `pull_requests` table and `remote = "origin"`, names no `[signers]`, and gives `bolt/*` a table too
- **THEN** a `sluice submit main` exits 2 naming main and `[signers]`, pushes and opens nothing, and a submission to `bolt/x` is tested and lands

#### Scenario: Status shows the refusal
- **WHEN** main is so refused
- **THEN** `sluice status main` prints the refusal, and `sluice overview` names main with it

### Requirement: A standing intake failure is said once for each cause and host
A cause of an intake failure, what intake asked and the words it was
refused with, SHALL be said by the host process once while it stands on
that host, at the pass that logs it on the branch, and never again
until it has ended, whether the process restarted or not, which is read
from what the records seam keeps and from no process's memory. A
`sluice step` SHALL print every warning its round's intake met, each
time (128; rules core.11, core.14, core.28).

#### Scenario: Forty passes refused alike
- **WHEN** GitHub refuses main's list of pull requests with the same words for forty passes of the host process, which restarts once among them
- **THEN** the host process's log says it once, and main's log holds one `read-refused` line marked `intake` for it

#### Scenario: A step beside them
- **WHEN** that refusal stands and `sluice step main` runs twice
- **THEN** each step prints the warning on stderr, and main's log gains no line for it

#### Scenario: A submit's own intake
- **WHEN** that refusal stands and `sluice submit main --no-wait` runs on the host that keeps main
- **THEN** the submit prints the warning on its stderr, and main's log gains no line for it
