## RENAMED Requirements

- FROM: `### Requirement: A wait runs to the submission's outcome`
- TO: `### Requirement: A wait follows the host process to the submission's outcome`

## MODIFIED Requirements

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

## REMOVED Requirements

### Requirement: A wait beside a live driver runs no round

**Reason**: Every wait follows the host process and runs no round, so
there is no wait that runs rounds beside a driver to tell apart (135;
rule core.31).
**Migration**: "A wait follows the host process to the submission's
outcome".
