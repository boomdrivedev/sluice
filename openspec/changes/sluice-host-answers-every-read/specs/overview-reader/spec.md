## MODIFIED Requirements

### Requirement: A follower wait reads its own record once its submission leaves the queue

When the reader's document no longer holds a wait's submission among the submissions in play, the wait SHALL ask `sluice host` for that submission's record once. It SHALL report its outcome from the answer, as `submission-wait` states: landed, failed, conflict or withdrawn.

Before: the wait read the record itself. Now: `sluice host` answers it. (`reads-go-through-the-resident@69134e5825d7`)

#### Scenario: Landed while followed

- **WHEN** a wait on `#7` follows `sluice host`, and `#7` lands, so the next document no longer holds it among the queue's submissions
- **THEN** the wait prints `passed · landed as <rev>`, the landing's revision as `sluice host` answers submission 7's record, and exits 0
- **AND** the wait read no file of the records

#### Scenario: Withdrawn while followed

- **WHEN** a wait on `#7` follows `sluice host`, and `#7` is withdrawn
- **THEN** the wait exits 4, as a wait that read the records did, and read no file of the records

### Requirement: A one-shot read waits for a current reading only so long

A one-shot read whose `sluice host` has no current reading of its repository SHALL wait for one within the round's retry bound and a tick. Past that, `sluice host` SHALL answer from the state that it last kept, fetching nothing, and SHALL say on standard error when it last read the repository.

Before: past the bound, the command read the records itself. Now: `sluice host` answers. (`reads-go-through-the-resident@69134e5825d7`)

#### Scenario: A host process held in a slow look

- **WHEN** the repository's remote does not answer, so the reading of it by `sluice host` does not end, and `sluice status main` runs
- **THEN** it prints `main`'s queue as `sluice host` last kept it, says when `sluice host` last read the repository, and exits 0
- **AND** the status ran no git and read no file of the records

### Requirement: A one-shot read is answered from the host process's last reading

`sluice status` in each form, `sluice overview` without `--watch` and the home view SHALL be answered by `sluice host` from a current reading that it keeps, started where none answers. No read command SHALL fetch, read the records or the results, or write the reader's document.

Before: the command read the records and the results itself. Now: `sluice host` answers. (`reads-go-through-the-resident@69134e5825d7`)

#### Scenario: A one-shot overview beside a current reading

- **WHEN** `sluice host` read the repository within its tick, `main` is on a remote, and `sluice overview --json` runs
- **THEN** it prints the reader's document of that reading, the remote is asked nothing, and the command read no record and no result

#### Scenario: A status with no host process

- **WHEN** no `sluice host` runs on a bare install, `main` is on a remote with `#4` queued, and `sluice status main --json` runs
- **THEN** a `sluice host` is running, `#4` is shown as its first reading found it, and the status itself fetched nothing and read no record

#### Scenario: A status during a long round

- **WHEN** a pass of the repository by `sluice host` started to land a combination a minute ago, and `sluice status main` runs
- **THEN** `sluice host` answers at once from the state that it keeps, with nothing fetched
