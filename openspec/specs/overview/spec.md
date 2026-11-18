# overview Specification

## Purpose
Every branch's queue at a glance, read from the state alone, for a
person, a pane or a status line; and the worktree each submission came
from.

## Requirements

### Requirement: A branch names the remote it lands on

In `sluice overview --json`, each branch SHALL have `remote`, the name of
the git remote its policy names for it, as the policy writes it and
never the remote's URL. A branch whose policy names no remote SHALL have
no `remote` field.

#### Scenario: A branch on origin

- **WHEN** `main`'s policy says `remote = "origin"` and `main` has state
- **THEN** `sluice overview --json` gives `main` a `remote` of `origin`, and no field of the document holds origin's URL

#### Scenario: A branch kept in this host's repository

- **WHEN** a branch's policy names no remote and the branch has state
- **THEN** `sluice overview --json` gives the branch no `remote` field

### Requirement: A branch names where its recent submissions came from

In `sluice overview --json`, each branch SHALL have `recent_from`, an
object mapping each submission its `recent` lines name, a landing's
`ids`, a blame's `id` and a conflict's `id` and `with`, to the worktree
it was submitted from, as its `from.worktree` says, whether the
submission is still live or finished. `recent` SHALL stay exactly as the
log wrote it.

#### Scenario: A landing, a blame and a conflict

- **WHEN** #1 from `unit/a` landed, #2 from `unit/b` was blamed, and #3 from `pull/41` conflicted with #1
- **THEN** the branch's `recent_from` is `{"1": "unit/a", "2": "unit/b", "3": "pull/41"}`, and its `recent` lines are as the log wrote them

#### Scenario: A landed submission moved to the finished records

- **WHEN** #1 landed and has moved to the finished records, and a recent line names it
- **THEN** `recent_from` still names #1's worktree

#### Scenario: No recent lines

- **WHEN** a branch has a submission queued and nothing landed, blamed or conflicting
- **THEN** its `recent_from` is `{}`

### Requirement: A flaky failure says when and where each run was

A check read as failed for being flaky (19) SHALL give, under `flaky`,
`at` and `host`: when this run's result for it was recorded and the host
that ran it; and under `flaky.other`, `host`: the host of the run it
contradicts. They SHALL come from the results recorded before the
verdict was read, and show in the overview's combinations and in
`sluice status <id> --json`.

#### Scenario: Failed on one host, passed on another

- **WHEN** a check failed for a submission when run on host `a`, and passed for the same inputs on host `b`, and the branch reads a flaky check as failed
- **THEN** the second submission's failed check has `flaky.at`, the time its pass was recorded, `flaky.host` `b`, and `flaky.other.host` `a`, in `sluice status <id> --json` and in the overview while its combination is listed

### Requirement: Records written before these fields still read

A flaky failure, or a result it points at, recorded before these fields
existed SHALL read as it did, with the missing fields left out, and
SHALL never fail a read of the overview, a status or a round.

#### Scenario: An old flaky failure

- **WHEN** a submission's state holds a flaky failure with no `at` or `host`, and an `other` with no `host`
- **THEN** `sluice status <id> --json` and `sluice overview --json` read it, give the fields they have, and leave the missing ones out

### Requirement: A branch names the subjects of its recent submissions

In `sluice overview --json`, each branch SHALL have `recent_subjects`,
an object mapping each submission its `recent` lines name, as
`recent_from` counts them, to the subject its submit gave, as written,
whether the submission is still live or finished. A submission whose
submit gave none, or whose finished line was written before subjects
were kept, SHALL have no key, and a reader names it by `recent_from`.
`recent` and `recent_from` SHALL stay as they are.

#### Scenario: A landing with a subject and one without

- **WHEN** #1 from `unit/a`, given `Batch review cards`, and #2 from `unit/b`, given none, landed together
- **THEN** the branch's `recent_subjects` is `{"1": "Batch review cards"}` and its `recent_from` is `{"1": "unit/a", "2": "unit/b"}`

#### Scenario: A landed submission moved to the finished records

- **WHEN** #1, given `Batch review cards`, landed and has moved to the finished records, and a recent line names it
- **THEN** `recent_subjects` still gives #1 as `Batch review cards`

#### Scenario: Finished before subjects were kept

- **WHEN** a recent line names #1, whose line in the finished index has no subject
- **THEN** `recent_subjects` has no key for #1, and `recent_from` names its worktree

#### Scenario: No recent lines

- **WHEN** a branch has a submission queued and nothing landed, blamed or conflicting
- **THEN** its `recent_subjects` is `{}`

### Requirement: A branch names the revision its read found

`sluice overview --json` SHALL give each branch it read `revision`: the
branch's revision as that read found it, the remote's for a branch on a
remote. It SHALL leave `revision` out where the read could not ask the
branch's remote or the remote holds no such branch. `at`, where the
records last saw the branch, SHALL stay as it is (64).

#### Scenario: A push the records have not seen
- **WHEN** `main` is on a remote, nothing is queued, and a commit is pushed to the remote's `main` by hand
- **THEN** the next overview gives `main` `revision` as that commit and `at` as the revision the records last saw

#### Scenario: A remote out of reach
- **WHEN** the overview reads `main`'s records as last fetched because its remote cannot be reached
- **THEN** `main` has no `revision`

### Requirement: A combination names the revision it was formed on

`sluice overview --json` SHALL give each combination `on`, the branch's
revision it was formed on, beside `revision`, the combination's own
(10, 64).

#### Scenario: A combination under test
- **WHEN** `#1` and `#2` are under test as one combination formed on `main` at `R`
- **THEN** that combination has `"on": "R"` and `"submissions": [1, 2]`

### Requirement: A branch names the landed submissions its queue waits behind

`sluice overview --json` SHALL give each branch `after_landed`: the
submissions that a queued submission's `after` names and that have
landed, in number order, or `[]` (7, 64).

#### Scenario: After a landed one
- **WHEN** `#5` is queued with `--after 3`, and `#3` has landed
- **THEN** the branch has `"after_landed": [3]`

### Requirement: A branch lists the pull requests another host takes in that this host follows
In `sluice overview`, a branch another host keeps, with pull requests
this host follows whose submissions the branch's queue does not hold at
their heads, SHALL have `taken_in_by` and `pull_requests`, as `sluice
status` gives them, and the same rows in its words. A one-shot overview
and the reader's document SHALL carry them as the host last kept them,
the host process reading each when due at the host's one bound (129,
137; rules core.25, core.31).

#### Scenario: A one-shot overview
- **WHEN** host `b` keeps main and `#13`, which host `a` follows, waits for a review, and `sluice overview --json` runs on `a`
- **THEN** `main` has `taken_in_by` `{"host":"b"}` and a `pull_requests` row for `#13` with its `queue` and a `read_at`

#### Scenario: A watch reads none of them itself
- **WHEN** `sluice overview --json --watch` follows host `a`'s host process for a minute while `a` follows `#13`, with `discovery` at sixty seconds
- **THEN** its documents carry `#13`'s row, the watch read nothing from GitHub, and the host process read `#13` and its check no more than four times each

### Requirement: A removed branch is named gone

`sluice overview --json` SHALL keep every branch with records in
`branches[]`, and SHALL give `"gone": true` to a branch whose records it
read without refusal and of which the host holds no revision, leaving
`gone` out otherwise (26, 105, 129; record 0091).

#### Scenario: A branch made and removed
- **WHEN** `bolt/x` was made here and removed, every submission to it made in another copy of the repository
- **THEN** `sluice overview --json` names `bolt/x` in `branches` with `"gone": true`

#### Scenario: A branch that is there
- **WHEN** `main` has records and is there
- **THEN** `sluice overview --json` gives `main` no `gone` field

### Requirement: The text shows a gone branch with nothing to watch apart

`sluice overview` SHALL name each gone branch with nothing queued, under
test or recent, no hold, no held checkout, no refused write and no read
refused on one line after the branches, `gone, nothing queued or recent:
<branch>, ...`, and give it no line of its own (129; record 0091).

#### Scenario: Nothing to watch
- **WHEN** `bolt/x` was made here and removed with nothing submitted to it here
- **THEN** `sluice overview` prints `gone, nothing queued or recent: bolt/x` and no line `bolt/x at`

### Requirement: The text heads a gone branch with something to watch as gone

`sluice overview` SHALL show a gone branch with something to watch as
any branch, its first line `<branch> is gone`, followed by
`, last at <revision>` where its records saw one, in place of the
queue's counts (record 0091).

#### Scenario: A removed branch with its landings
- **WHEN** `bolt/x` landed `#1` at `R` and was removed into `main`
- **THEN** `sluice overview` shows `bolt/x is gone, last at R` with `#1`'s landing under it
