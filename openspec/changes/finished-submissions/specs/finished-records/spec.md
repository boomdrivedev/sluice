## Purpose

How a branch's records let go of what has finished, so a read costs the
branch's queue and not its history: a landed or withdrawn submission
moves whole to the finished records, named by a line of an index, and
the log is kept in parts (61, 130 to 132).

## ADDED Requirements

### Requirement: A finished submission moves out of its branch's live state
A submission that has landed or been withdrawn SHALL move from
`submissions/<id>.json` to `.finished/<id>.json`, unchanged, in the
write that tells its outcome, once no combination or landing names it;
a submission one still names SHALL move on a later write. The same
write SHALL add the submission's id, state and worktree as one line of
`finished.jsonl`, and SHALL make the move whole or not at all. A blamed
or conflicting submission SHALL stay in `submissions/` (130, 53, 63).

#### Scenario: A landed submission moves
- **WHEN** submission 1 from `unit/a` lands on `main`
- **THEN** `main`'s state holds `.finished/1.json` and no `submissions/1.json`, and `finished.jsonl` holds `{"id":1,"state":"landed","worktree":"unit/a"}`

#### Scenario: A blamed submission stays
- **WHEN** submission 2 is blamed for a failed check
- **THEN** `submissions/2.json` stays where it is and `finished.jsonl` has no line for 2

#### Scenario: A write cut short moves nothing by half
- **WHEN** sluice is stopped partway through the write that moves submission 1
- **THEN** the next read finds submission 1 either in `submissions/` alone or in `.finished/` with its line in `finished.jsonl`, never in both or neither

### Requirement: A branch's own reads cost its queue
A round, `sluice overview`, `sluice` with no command and `sluice status
<branch>` SHALL read only the branch's live state: `branch.json`,
`submissions/`, `finished.jsonl` and the log the live state carries,
never `.finished/`. `sluice status <branch>`, as text or `--json`,
SHALL list the queue alone and say how many submissions are no longer
in it (61, 130).

#### Scenario: Status lists the queue
- **WHEN** `main` has had two submissions land and has none queued, and the operator runs `sluice status main`
- **THEN** it lists no submission and says `2 no longer in the queue: sluice status main --all`

#### Scenario: Status as JSON lists the queue
- **WHEN** `main` has one submission queued and two landed, and the operator runs `sluice status main --json`
- **THEN** its submissions are the queued one alone

### Requirement: A finished submission stays readable and counted
`sluice status <id>` of a finished submission SHALL read its one file
in `.finished/`, and `sluice status <branch> --all`, as text or
`--json`, SHALL read every finished submission beside the queue, with
no sluice process running. A finished submission SHALL count as landed
for an `--after` that names it and toward its worktree's attempts, and
`sluice amend` and `sluice withdraw` SHALL refuse it as they did before
it moved (131, 26, 49).

#### Scenario: A landed submission read by its number
- **WHEN** submission 1 has landed and moved, and the operator runs `sluice status 1 --json`
- **THEN** its state is `landed`

#### Scenario: After a finished submission
- **WHEN** submission 1 has landed and moved, and `unit/b` submits with `--after 1`
- **THEN** the submission is taken as coming after a landed one and lands

#### Scenario: Attempts count the finished
- **WHEN** submission 1 from `unit/a` has landed and moved, and `unit/a` submits again
- **THEN** the new submission's `attempt` is 2

#### Scenario: A finished submission past changing
- **WHEN** submission 1 has landed and moved, and the operator runs `sluice amend 1 --priority 2`
- **THEN** it is refused with `#1 is landed, past changing: sluice status 1`

### Requirement: Older state moves on its branch's next write
A branch whose state was written before finished submissions moved
SHALL have each of its landed or withdrawn submissions that nothing
names moved, as any other, by the branch's next write, with no step
run by hand (130).

#### Scenario: A branch from before the move
- **WHEN** `main`'s state holds landed submissions under `submissions/` and no `finished.jsonl`, and a new submission is queued
- **THEN** after that write the landed ones are under `.finished/`, each with its line in `finished.jsonl`

### Requirement: The log is kept in parts
`log.jsonl` SHALL be the log's current part. A write that would append
its 1,001st line SHALL first close it as `log/<n>.jsonl`, numbered from
1 oldest first, and start a new `log.jsonl`, in the same write and
under the branch's lock as every write. No line SHALL change in a move
(132, 53, 63; core.14).

#### Scenario: The 1,001st line starts a new part
- **WHEN** `main`'s `log.jsonl` holds 1,000 lines and a round writes three more
- **THEN** `log/1.jsonl` holds those 1,000 lines unchanged and `log.jsonl` holds the three

#### Scenario: Parts are numbered oldest first
- **WHEN** `main` already has `log/1.jsonl` and its `log.jsonl` is closed again
- **THEN** the closed part is `log/2.jsonl`

### Requirement: The live state carries the recent log and history reads every part
The branch's live state SHALL carry `log.jsonl` and the newest closed
part only, so `sluice overview`'s recent lines are always in it. A
reader that searches the whole log for a submission's events, the pull
request driver's removal time, a report's run counts, or the time a
combination was under test, SHALL read every part in order (61, 132).

#### Scenario: Recent lines just after a part closes
- **WHEN** `main`'s `log.jsonl` has just been closed into `log/3.jsonl` and holds two lines
- **THEN** `sluice overview` shows the branch's ten most recent lines, eight of them from `log/3.jsonl`

#### Scenario: A report reads runs from an older part
- **WHEN** a submission's runs were logged in `log/2.jsonl` and it lands after `log/3.jsonl` closed
- **THEN** its report counts every one of those runs
