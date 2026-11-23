## MODIFIED Requirements

### Requirement: A watch that finds a reader follows its document

A `sluice overview --json --watch` SHALL follow its repository's document on a subscription to `sluice host`, and SHALL read no records, policy or checkouts itself. It SHALL print the document at its start and at each change in a part other than its time, each as one line.

Before: a watch read a file under `readers/`. Now: it gets the document on a subscription. (`reads-go-through-the-resident@69134e5825d7`)

#### Scenario: A follower prints what the reader wrote

- **WHEN** the document of `sluice host` holds submission `#7` testing on `main`, which the repository's records do not hold, and `sluice overview --json --watch` runs in the repository
- **THEN** its first line is that document, with `#7` testing, and no line that it prints is read from the records

#### Scenario: A follower prints each change once

- **WHEN** a watch follows `sluice host`, whose document changes from `#7` testing to `#7` landed, and then changes only in its time
- **THEN** the watch prints two lines, the second with `#7` landed

### Requirement: A reader serves its own host

`sluice host` SHALL read each repository that it holds apart, as its common git directory names it, and only on its own host. It SHALL keep each repository's document in its own process, and SHALL send it only on its own socket. A watch or a wait of a repository SHALL follow only that repository's document.

Before: each repository's lock and document were files in the host's state. Now: the document is in `sluice host`, and no file holds it.

#### Scenario: Two repositories

- **WHEN** `sluice overview --json --watch` runs in one repository, and a second watch runs in another repository on the host
- **THEN** one `sluice host` reads both, and each watch prints its own repository's queues

#### Scenario: Two worktrees of one repository

- **WHEN** a watch runs in one worktree of a repository, and another watch runs in another worktree of the same repository
- **THEN** both follow the one document that `sluice host` keeps for the repository

### Requirement: A watch ends only when asked, when its repository is gone, or where it has no sluice

A `sluice overview --json --watch` SHALL end only when it is stopped or the reader of its output closed it. It SHALL also end when its repository's git directory is gone, or its repository has no sluice. A failed read of the queues, a document that `sluice host` could not send, or a `sluice host` that stopped SHALL NOT end it. A watch whose repository is gone SHALL end as a failing overview does, with its error document on stdout and exit 3.

#### Scenario: The reader's document cannot be written

- **WHEN** a watch follows `sluice host`, and its subscription closes before `sluice host` sends one document
- **THEN** the watch subscribes again at its next tick, prints the overview, and is still running

#### Scenario: The repository is removed

- **WHEN** a watch follows `sluice host`, and its repository's git directory is removed
- **THEN** the watch prints the error document with the read's words, and exits 3

#### Scenario: Stopped while a failure stands

- **WHEN** a watch followed an error document for ten ticks, and it is stopped with Ctrl-C
- **THEN** it ends as a stopped watch does, with exit 130

### Requirement: A reader marks each read and names its tick

`sluice host` SHALL keep, with each repository's document, when each pass or look of the repository begins and ends, whether or not the document changed. It SHALL NOT make the document again where it did not change. The document SHALL name its tick, the pause between reads, in whole seconds as `tick`: one second while a watch follows the repository, and three otherwise.

Before: the marks were files in the host's state. Now: `sluice host` keeps them in its process.

#### Scenario: A read that changes nothing

- **WHEN** a watch follows `sluice host`, and three of its reads find the queues as they were
- **THEN** the document's content is as it was after the first read, and `sluice host` keeps the end of the third read as its last

#### Scenario: The tick is named

- **WHEN** `sluice overview --json --watch` follows `sluice host`
- **THEN** the document holds `"tick": 1`, and once the watch ended and only `sluice wait` follows, it holds `"tick": 3`

### Requirement: The host process is each repository's reader

`sluice host` SHALL be the reader of each repository that it holds. A watch or a wait SHALL follow its repository on a subscription, starting `sluice host` where none answers, and SHALL read no queue itself. While anything follows the repository, `sluice host` SHALL make the whole document of `sluice overview --json` at each change in a part other than its time.

Before: a follower read a file that `sluice host` wrote. Now: it gets the document on its subscription.

#### Scenario: A watch with no reader

- **WHEN** no process of sluice runs on a host, `sluice overview --json --watch` runs in a repository, and a submission is then queued
- **THEN** `sluice host` runs, and the watch prints the overview at its start and then the overview with the submission queued, each a whole `sluice/overview@1` document
- **AND** the watch read no records itself

#### Scenario: A wait with no reader

- **WHEN** no process of sluice runs on a host, and `sluice wait 4` runs in a repository
- **THEN** the document that its subscription gets holds the whole overview, each branch, checkout and preview of the repository, and not only submission 4's branch

### Requirement: A host process that stops is started again within a tick

Where `sluice host` ends by Ctrl-C, a kill or a crash, each watch and wait that follows it SHALL reach it again at its own next tick. It SHALL start `sluice host` where none answers. Until the new `sluice host` sends a document, each follower SHALL keep the last document that it got. It SHALL read no records itself.

Before: a follower read the last document file that the ended `sluice host` wrote. Now: it keeps the last document that its subscription got.

#### Scenario: The host process is killed

- **WHEN** a watch follows `sluice host`, and `sluice host` is killed
- **THEN** within one tick of the watch a `sluice host` runs again
- **AND** the next document that the watch gets is the one that `sluice host` read from the records

#### Scenario: The host process is killed under a wait

- **WHEN** `sluice wait 4` follows `sluice host`, and `sluice host` is killed before submission 4 is decided
- **THEN** within one tick of the wait a `sluice host` runs again, and the wait reports submission 4's outcome once that `sluice host` decides it

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

## ADDED Requirements

### Requirement: A repository held again is read afresh

When `sluice host` holds a repository again after it let the repository go, it SHALL read the repository before it sends a document of it. It SHALL send no finding of an earlier holding, such as an overview with `"set_up": false`.

#### Scenario: sluice set up after a watch ended

- **WHEN** a watch ended on the overview with `"set_up": false`, sluice is then set up so that `main` holds `.config/sluice.toml`, and a watch runs again
- **THEN** it prints the overview with no `set_up` field, and is still running

#### Scenario: A watch that starts during the first reading

- **WHEN** `sluice host` holds a repository again after a watch ended on `"set_up": false`, and a watch starts before the first reading ends
- **THEN** the watch prints nothing until that reading ends, and does not end

## REMOVED Requirements

### Requirement: No other reader's lease is taken

**Reason**: No reader outside `sluice host` reads a repository, and no reader has a lease. A watch, a wait and `sluice run` follow `sluice host` on a subscription.

**Migration**: A watch or a wait of an earlier release gets an error from `sluice host` that names the sluice to run.

### Requirement: An older reader whose lease is taken follows

**Reason**: `sluice host` takes no lease, and no older reader is followed.

**Migration**: A watch or a wait of an earlier release gets an error from `sluice host` that names the sluice to run.

### Requirement: A reader that takes the lock passes over an earlier reader's finding

**Reason**: No reader's document outlives `sluice host`. The requirement "A repository held again is read afresh" keeps what a watch sees.

**Migration**: None.

### Requirement: The host process holding the lease is the one to read

**Reason**: No older reader reads a repository beside `sluice host`.

**Migration**: A watch or a wait of an earlier release gets an error from `sluice host` that names the sluice to run.

### Requirement: The host process names the version of sluice it is

**Reason**: No file names a reader's version, since no follower reads a reader's files. `hello` names the version of `sluice host`.

**Migration**: None.

### Requirement: The host process takes an older reader's lease

**Reason**: `sluice host` reads each repository that it holds, and follows no older reader.

**Migration**: A watch or a wait of an earlier release gets an error from `sluice host` that names the sluice to run.
