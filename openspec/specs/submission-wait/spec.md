# submission-wait Specification

## Purpose
Waiting on a submission's outcome, with a time limit an agent's tool
call can live within, and the exits and document a wait ends with.

## Requirements

### Requirement: A wait follows the host process to the submission's outcome

`sluice wait <n>` SHALL wait until submission `<n>` is decided, landed,
failed, conflict or withdrawn, following the host process's document and
running no round itself, and SHALL then report it as a waiting submit
does, through one loop (135, 129; rule core.31). One already decided
SHALL be reported at once (51). A round the host process could not run
on the branch SHALL NOT end the wait, which SHALL say it once on stderr,
in the document's words, and go on.

#### Scenario: It lands

- **WHEN** an agent submits with `--no-wait` and runs `sluice wait <n>`
- **THEN** the host process runs the rounds until the submission lands,
  the wait runs none, and it prints `passed · landed as <rev>` and exits
  0

#### Scenario: Already decided

- **WHEN** submission `<n>` landed before `sluice wait <n>` runs
- **THEN** it exits 0 at once, and no round ran

#### Scenario: The branch cannot be read for a while

- **WHEN** `sluice wait <n>` follows the host process, and three of its
  rounds of the submission's branch fail with the same words before a
  fourth lands it
- **THEN** the wait prints those words once on stderr, then `passed ·
  landed as <rev>`, and exits 0

### Requirement: A timeout returns the submission as it stands

With `--timeout <duration>` (`90s`, `9m`, `1h30m`), `sluice wait` SHALL
take its first pass whatever that time, the host process's pass it asked
for when it began to follow (1, 135), and SHALL return once that time
has passed with the submission undecided, exit 5, and leave the
submission queued (5). Its record SHALL be as its last pass left it. A
duration it cannot read SHALL exit 64.

#### Scenario: Still testing

- **WHEN** a submission's combination is still under test and
  `sluice wait <n> --timeout 10s --json` runs
- **THEN** it exits 5 within a few seconds of the ten, printing the
  submission's record with `state` `testing`, and the submission stays
  in the queue

#### Scenario: Out of time as it starts

- **WHEN** a submission is queued with nothing under test and `sluice
  wait <n> --timeout 0s --json` runs
- **THEN** the host process's pass it asked for starts the submission's
  combination, and it exits 5, printing the submission's record with
  `state` `testing`

#### Scenario: A duration it cannot read

- **WHEN** `sluice wait 3 --timeout soon` runs
- **THEN** it exits 64

### Requirement: The exits tell the outcomes apart

A waiting `sluice submit` and `sluice wait` SHALL exit 0 when the
submission landed, 1 when it failed or its pull request was closed, 2
on a conflict, 4 when it was withdrawn and no work of it goes on, 5 when
`--timeout` ran out, and 130 when stopped with Ctrl-C. A submission that
does not exist SHALL exit 1 with the not-found error document under
`--json`.

#### Scenario: Withdrawn while waited on

- **WHEN** submission `<n>` is withdrawn by the operator while `sluice
  wait <n>` waits, and no later submission and no pull request carries
  its head
- **THEN** it exits 4, and a waiting `sluice submit` withdrawn the same
  way exits 4

#### Scenario: Blamed

- **WHEN** submission `<n>` is blamed while `sluice wait <n>` waits
- **THEN** it exits 1 and names the failed tasks as a waiting submit
  does

#### Scenario: No such submission

- **WHEN** `sluice wait 9999 --json` runs and there is no submission
  9999
- **THEN** it prints `{"error":{"kind":"not_found",...}}` and exits 1

### Requirement: A number on no branch read, while one was not, exits 3

A number found on no branch sluice could read, while some branch could
not be read, SHALL exit 3 with the internal error document under
`--json`, naming each branch that could not be read, since whether the
submission exists is not known (26).

#### Scenario: Not found among the branches that could be read

- **WHEN** `sluice wait 9999 --json` runs, no branch sluice could read
  holds submission 9999, and `bolt/b`'s state could not be read
- **THEN** it prints `{"error":{"kind":"internal",...}}` whose message
  names `bolt/b` and the words it was refused with, and exits 3

### Requirement: The document is the submission's record

Under `--json`, `sluice wait` and a waiting `sluice submit` SHALL print
one document on stdout however they end, decided, out of time or
stopped with Ctrl-C: the submission's record, as `sluice status <n>
--json` prints it. Its `state` SHALL be one of `queued`, `waiting`,
`testing`, `passed`, `landing`, `landed`, `failed`, `conflict` and
`withdrawn`, and under `sluice/submission@1` none of these SHALL be
renamed or removed and none added.

#### Scenario: Stopped with Ctrl-C

- **WHEN** `sluice wait <n> --json` is stopped with Ctrl-C while the
  submission is queued
- **THEN** stdout holds one document, the record with `state`
  `queued`, and it exits 130

### Requirement: A branch names this worktree's last submission

`sluice wait <branch>` SHALL wait for this worktree's last submission
to `<branch>`, found as `sluice submit <branch> --status` finds it,
and SHALL say `nothing submitted from <worktree> to <branch>` and exit
1 when there is none. On a branch that takes pull requests, it SHALL
follow this worktree's pull request as a waiting submit does.

#### Scenario: The worktree's own

- **WHEN** a worktree submitted #2 and then #4 to `main`, and another
  worktree submitted #3
- **THEN** `sluice wait main` in the first waits for #4

### Requirement: A wait on a pull request follows the submission sluice took

On a branch that takes pull requests, once sluice has taken this
worktree's pull request in at its current head, `sluice wait <branch>`
SHALL follow that submission as `sluice wait <id>` follows one: out of
time or stopped with Ctrl-C, under `--json` it SHALL print the
submission's record, and without `--json` it SHALL say `still waiting:
sluice wait <id>`.

#### Scenario: A pull request taken in, out of time

- **WHEN** `main` takes pull requests with no conditions, an agent's
  `wt submit main --no-wait` opened pull request `<n>` and sluice took
  it in as submission `<id>`, whose combination is still under test,
  and `sluice wait main --timeout 2s --json` runs in the agent's
  worktree
- **THEN** it exits 5 and prints the record of submission `<id>`, with
  `state` `queued` or `testing`, as `sluice wait <id> --timeout 2s
  --json` would

### Requirement: A wait on a pull request not yet taken answers with the pull request

Before sluice has taken this worktree's pull request in, `sluice wait
<branch>` SHALL answer with the pull request and what it waits for,
`pull_request`, `branch` and `state` under `--json`. A waiting `sluice
submit` that follows a pull request and is stopped with Ctrl-C SHALL
end as `sluice wait <branch>` does, before sluice takes the pull
request in and after.

#### Scenario: A pull request waiting for a review, out of time

- **WHEN** an agent's pull request waits for a review and `sluice wait
  main --timeout 2s --json` runs in the agent's worktree
- **THEN** it exits 5 and prints `pull_request`, `branch` and `state`
  `waiting: needs 1 review`

### Requirement: A landing stands whatever fails after it

A waiting `sluice submit` and `sluice wait` SHALL report the submission
as landed and exit 0 once its record says landed or its branch holds its
work (122; rule core.48), whatever the host process's round failed after
the landing. What failed SHALL be in the host process's log and the
branch's state, and SHALL never be the wait's outcome. A wait SHALL exit
3 only for a read it could not make itself.

#### Scenario: The release fails after the landing

- **WHEN** a submission's combination passes, the branch moves to it, and
  releasing the combination's checkout then fails in the host process's
  round, while a waiting `sluice submit` follows it
- **THEN** stdout ends `passed · landed as <rev>`, the host process's log
  names the failed release, and the command exits 0

#### Scenario: Telling the outcome fails after the landing

- **WHEN** a submission lands, and telling its session the outcome fails
  in the host process's round, while `sluice wait <n>` follows it
- **THEN** it reports the submission landed and exits 0

#### Scenario: Under JSON

- **WHEN** a submission lands and releasing its combination's checkout
  fails in the host process's round, while `sluice wait <n> --json`
  follows it
- **THEN** stdout holds one document, the submission's record with
  `state` `landed` and its `landed_as`, and it exits 0

#### Scenario: The branch moved and the round stopped before recording it

- **WHEN** the host process's round moves the branch to a submission's
  landing and fails writing that it landed, while a waiting `sluice
  submit` follows it
- **THEN** the command reports the submission landed as the landing's
  commit and exits 0, under `--json` its record with `state` `landed` and
  that `landed_as`, and the host process's next round records it landed

#### Scenario: A round that fails before the landing

- **WHEN** the host process's round fails before the submission's branch
  has moved, while a waiting `sluice submit` follows it
- **THEN** the submit says the failure once on stderr, goes on waiting,
  and the submission is not reported as landed

#### Scenario: A pull request's submission

- **WHEN** a waiting `sluice submit` follows a pull request whose
  submission lands, and releasing the combination's checkout then fails
  in the host process's round
- **THEN** it reports the submission landed and exits 0

### Requirement: A wait removes no worktree

`sluice wait` SHALL NOT remove, release or clean the worktree a submission came from, whatever it waits on and however that submission ends. Removing a landed worktree is its driver's: under worktrunk, `wt submit` run again in that worktree (25; rule core.16; record 0065).

#### Scenario: Landed, waited on with sluice wait

- **WHEN** a worktree that ran `sluice exec main` submits with `--no-wait`, and `sluice wait <id>` runs until `<id>` lands
- **THEN** it exits 0, the worktree is still there with its head, and `sluice status` still lists its ports

### Requirement: A wait across hosts names where its pull request stands
A `sluice wait` or waiting `sluice submit` whose pull request another
host takes in SHALL, in every `--json` document it prints for the open
pull request, `--status`'s and the one at its timeout among them, give
`taken_in_by` and `queue` as `sluice status` gives them. In its words
it SHALL say which host holds the pull request as which submission, or
that it waits to be taken in by the host that keeps the branch, each
time that changes (137).

#### Scenario: Out of time, queued on the other host
- **WHEN** host `b` keeps main and holds `#12` as submission 1406, and `sluice wait main --timeout 2s --json` runs in `#12`'s worktree on host `a`
- **THEN** it exits 5 and prints `pull_request` 12, `taken_in_by` `{"host":"b"}` and `queue` with `"submission":1406`

#### Scenario: The status a submit reads
- **WHEN** the same, and `sluice submit main --status --json` runs in that worktree
- **THEN** it prints `head`, `draft`, `taken_in_by` and `queue` beside `pull_request`, `branch` and `state`

#### Scenario: Its words
- **WHEN** `sluice wait main` follows `#12` on `a` while `b` holds it as submission 1406
- **THEN** it prints a line naming `#12`, submission 1406 and `b`, once, and again only when one of them changes

#### Scenario: Not yet taken in
- **WHEN** host `b` keeps main and `#13` needs one review, and `sluice wait main --timeout 2s --json` runs in `#13`'s worktree on `a`
- **THEN** it exits 5, its `queue` reads `"taken_in":false` and `"waits_for":["1 review"]`, and its `taken_in_by` reads `{"host":"b"}`

### Requirement: A wait reports landed a withdrawn submission whose work the branch holds

A waiting `sluice submit` and `sluice wait`, reading their submission
withdrawn while its branch holds its head, SHALL report it landed as the
commit that landed it: its record with `state` `landed`, that
`landed_as` and `landed_elsewhere`, the words `landed as <rev>:
<branch> holds its work`, and exit 0. They SHALL read the branch as a
round reads it, from its remote, before they call a closed pull request
closed without landing. The record kept SHALL stay withdrawn (122, 5,
67).

#### Scenario: Withdrawn after another queue landed it

- **WHEN** submission `<n>` of pull request `#330` was withdrawn, and
  another queue of `main` has landed `#330`'s head as `562606d`
- **THEN** `sluice wait <n> --json` prints `<n>`'s record with `state`
  `landed` and `landed_as` `562606d` and exits 0, and `sluice status
  <n>` still says withdrawn

#### Scenario: Its pull request closed as landed

- **WHEN** that pull request is closed, and `sluice wait main` runs in
  its worktree
- **THEN** it says `landed as 562606d` and exits 0, never `closed
  without landing`

#### Scenario: Closed before this host fetched the branch

- **WHEN** `main` takes pull requests, a driver of the repository is
  live on the host, this host took pull request `#12` in as `<n>`,
  another queue of `main` lands `#12`'s head as `<rev>` on the remote
  and GitHub reads `#12` closed, and no command of this host has
  fetched `main` since
- **THEN** `sluice wait main` in `#12`'s worktree says `landed as
  <rev>` and exits 0, never `closed without landing`

### Requirement: A wait follows the submission that carries a withdrawn one's work

Where a later submission from the withdrawn one's place holds its head,
a waiting `sluice submit` and `sluice wait` SHALL say `#<a> withdrawn by
<who>: #<b> carries its work, so this wait follows it` and follow
`#<b>`. They SHALL end with `#<a>`, never `#<b>`, as their outcome:
landed as the commit that landed its head once `#<b>` lands, exit 0;
withdrawn once `#<b>` is blamed, in conflict, or withdrawn with nothing
carrying it on, naming how `#<b>` ended, exit 4 (67, 122, 51).

#### Scenario: Withdrawn by its session's next submit

- **WHEN** a session submits `<a>`, then submits a commit on top of it
  as `<b>`, and `sluice wait <a> --json` runs
- **THEN** it exits 0 once `<b>` lands, printing `<a>`'s record with
  `state` `landed`, `landed_elsewhere`, `landed_as` the commit that
  landed `<b>`, and `carried_by` `{"id": <b>, "state": "landed"}`, and
  `sluice status <a>` still says withdrawn

#### Scenario: Its words

- **WHEN** the same, and `sluice wait <a>` runs without `--json`
- **THEN** it says `#<a> withdrawn by resubmission: #<b> carries its
  work, so this wait follows it`, ends `landed as <rev>: main holds its
  work, by #<b>'s landing`, and exits 0

#### Scenario: What carried its work was blamed

- **WHEN** a session submits `<a>`, then a commit on top of it as `<b>`
  that fails a check, and `sluice wait <a> --json` runs
- **THEN** it exits 4 once `<b>` is blamed, printing `<a>`'s record
  with `state` `withdrawn` and `carried_by` `{"id": <b>, "state":
  "failed"}`, and without `--json` it says `#<a> withdrawn by
  resubmission: #<b>, which carried its work, failed`

#### Scenario: Out of time while it follows

- **WHEN** `<b>` carries `<a>`'s work and is still under test, and
  `sluice wait <a> --timeout 2s --json` runs
- **THEN** it exits 5, printing `<a>`'s record with `carried_by`
  naming `<b>` and its `state` as it stands, and without `--json` it
  ends `still waiting: sluice wait <a>`

### Requirement: A wait follows the pull request that carries a withdrawn submission's work

For a pull request's submission withdrawn while the pull request is
open and no draft on a new head that holds its head, or on its own head
after this host handed it over (137), a waiting `sluice submit` and
`sluice wait` SHALL follow the pull request as `sluice wait <branch>`
does, and SHALL end with that submission as a wait that follows a later
submission ends. A withdrawal that leaves it on its own head otherwise
SHALL be reported withdrawn (120).

#### Scenario: Withdrawn by the operator, still open

- **WHEN** the operator withdraws the submission `<a>` of a pull request
  a person opened, which stays open and ready on its head, and `sluice
  wait <a>` runs
- **THEN** it exits 4 at once

#### Scenario: A pull request's new head

- **WHEN** a session's `wt submit` opened pull request `#12`, sluice took
  it in as `<a>`, the session submits a new head on it, which sluice
  takes in as `<b>`, and `sluice wait <a> --json` runs
- **THEN** it follows `#12` and exits 0 once `<b>` lands, printing
  `<a>`'s record with `state` `landed`, `landed_elsewhere` and
  `carried_by` `{"id": <b>, "state": "landed"}`

#### Scenario: Closed on its new head

- **WHEN** the same, and `#12` is closed before `<b>` lands
- **THEN** it exits 4, printing `<a>`'s record with `state` `withdrawn`
  and `carried_by` `{"pull_request": 12, "state": "closed"}`

### Requirement: A waiting submit to a branch that takes pull requests ends with the submission it made

A waiting `sluice submit` to a branch that takes pull requests, and
`sluice wait <branch>` there, SHALL take as their own the first
submission of the pull request this host's records hold as they first
read it, and once it is read SHALL wait on it as `sluice wait <id>`
does. They SHALL never end with a submission of the pull request on
another head as their own outcome (135, 67).

#### Scenario: The branch moves under its landing

- **WHEN** `sluice submit main --head bolt/x` waits, sluice took its
  pull request `#12` in as `<a>`, a commit then lands on `bolt/x` on
  GitHub, sluice takes `#12` in again as `<b>` on that head, and `<b>`
  lands
- **THEN** the submit exits 0 and ends `landed as <rev>: main holds its
  work, by #<b>'s landing`, and under `--json` it prints `<a>`'s record
  with `state` `landed` and `carried_by` `{"id": <b>, "state":
  "landed"}`

#### Scenario: Landed by another queue, in a bare clone

- **WHEN** `sluice submit main --head bolt/x` waits in the repository's
  git directory, sluice took its pull request `#12` in as `<a>`, and
  another queue of `main` lands `#12`'s head as `<rev>`
- **THEN** the submit exits 0, says `landed as <rev>` and never
  `withdrawn`, and `sluice status <a> --json` gives `state` `landed`
  with `landed_elsewhere`

### Requirement: A wait marks delivered only its own outcome

A waiting `sluice submit` and `sluice wait` that follow a later
submission or a pull request SHALL hand over only the outcome they
report as their own. The outcome of the submission they followed SHALL
stay owed, with no hand-over by them, for its own carriers, the round's
notice or `sluice inbox` (51; weir 103).

#### Scenario: The resubmission's landing stays pending

- **WHEN** a session submits `<a>`, then a commit on top of it as `<b>`,
  and `sluice wait <a>` runs in its worktree until `<b>` lands
- **THEN** `sluice status <b> --json` gives `<b>`'s `delivery` with no
  hand-over by `wait`, and `sluice inbox` in that worktree then hands
  over `<b>`'s landing

### Requirement: A wait follows its submission to its queue's new home

A wait SHALL follow its submission to where it lives: where the home it
read the submission in is closed by a move, it SHALL read the closed
home's pointer and go on with the submission in the new home, under the
number it has there. It SHALL never work out what carried the
submission, and its document SHALL be the submission's record in the
new home, naming `was` and `carried_from` where its number changed
(record 0122; 26; rule core.48).

#### Scenario: The queue moves while a wait follows

- **WHEN** `sluice wait 52` follows #52 on `bolt/x` in this host's own files, and the branch's queue moves to origin before #52 is tested
- **THEN** the wait goes on following #52 on origin, prints `passed · landed as <rev>` once origin's lead lands it, and exits 0

#### Scenario: Carried under another number

- **WHEN** `sluice wait 52 --json` follows #52 on this host, and the move carries it to origin as #61 because origin's `bolt/x` already held a #52
- **THEN** the wait follows #61, never origin's own #52, and its document is #61's record naming `was` 52 and `carried_from` this host's own files
