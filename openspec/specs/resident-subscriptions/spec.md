# resident-subscriptions Specification

## Purpose
A client holds one subscription to `sluice host` for many folders, as a watch, a wait or `sluice run`. `sluice host` makes one read in flight for each thing that it reads, however many asks come.

## Requirements

### Requirement: A subscription serves each folder that its client names

`sluice host` SHALL serve a subscription: a connection that stays open and gets each folder's newest overview. The client SHALL add or remove a folder on the same connection. `sluice host` SHALL send a folder's overview when the folder is added, and after that only when the overview changes in a part other than its time. (`flume-has-a-resident@49876bc5c73c`)

#### Scenario: Two folders on one connection

- **WHEN** a client subscribes to two worktrees of two repositories, and a submission lands in the second repository
- **THEN** the client gets each folder's overview once at the start, and then one new overview for the second folder only

#### Scenario: A folder added later

- **WHEN** a subscription serves one folder, and the client adds a second folder on the same connection
- **THEN** the client gets the second folder's overview, and the client opened no second connection

#### Scenario: A folder removed

- **WHEN** a subscription serves two folders, the client removes the first, and a submission then lands in the first folder's repository
- **THEN** the client gets no overview for the first folder

### Requirement: A subscription follows each folder as a watch, a wait or sluice run

A client SHALL name, for each folder that it adds, how it follows the folder: as a watch, a wait or `sluice run`. A watch is the default. While a watch follows a repository with work in play, `sluice host` SHALL read it each second. A folder that `sluice run` follows SHALL get each pass's report, in order, and no overview. Each follower SHALL keep `sluice host` running.

#### Scenario: A watch's folder is read each second

- **WHEN** a subscription follows a folder as a watch, and a submission of its repository is under test
- **THEN** each overview that the client gets holds `"tick": 1`

#### Scenario: sluice run prints the passes of its subscription

- **WHEN** `sluice run --rounds 1` runs in a repository whose `main` has one submission queued
- **THEN** it prints the report of the pass that its subscription got, and exits 0
- **AND** it sent no request but `hello` and `subscribe`, and `pass` where `sluice host` passes only when asked

### Requirement: A slow client gets only the newest overview

Where a client of a subscription reads slower than a folder's overview changes, `sluice host` SHALL send only the newest overview of that folder. The newest overview SHALL replace the older ones. A client SHALL get a folder's overviews in the order of the state that each read, also where the reads end in another order. (`one-read-in-flight@e99acda774d0`)

#### Scenario: A client that reads nothing for ten seconds

- **WHEN** a client reads nothing for ten seconds while three passes each change a folder's overview
- **THEN** the next overview that the client reads for that folder is the newest, and the two older ones never reach it

#### Scenario: A read that ends after a newer one

- **WHEN** three passes each read a folder's overview, and the read of the first pass ends after the reads of the other two
- **THEN** the overview of the first pass does not reach a client after the newer ones
- **AND** a client gets the overviews in the order of the passes (`a_document_read_that_ends_late_is_not_sent_after_a_newer_one` in `crates/sluice/src/host_process/tasks.rs`)

### Requirement: A folder with no sluice leaves the subscription

Where a folder's repository is gone or has no sluice, `sluice host` SHALL send the overview that says so. It SHALL then remove that folder from the subscription. It SHALL keep serving the other folders. A repository is gone when its git folder holds no `HEAD` file or no `objects` folder. This SHALL hold also when a round writes its records at that path after the git folder went (129). It SHALL hold also when the scan of the index finds the repository gone before the repository's own read does.

#### Scenario: A repository removed

- **WHEN** a subscription serves two folders, and the first folder's repository is removed
- **THEN** the client gets an overview for the first folder that says it is gone, and later overviews for the second folder

#### Scenario: A repository removed as a round writes in it

- **WHEN** a subscription serves a folder whose repository has work in play
- **AND** the repository's git folder goes, and a round writes its records at the git folder's path after it went
- **THEN** the client gets an overview for that folder that says it is gone (`a_repository_gone_as_a_round_writes_in_it_is_gone` in `crates/sluice/tests/host_process.rs`)

#### Scenario: The scan of the index finds the repository gone first

- **WHEN** a subscription serves a folder, and its repository's git folder goes
- **AND** the scan of the index finds the repository gone, and takes it out of the index, before the repository's own read does
- **THEN** the client gets an overview for that folder that says it is gone

### Requirement: The watch command serves the folders named on its input

`sluice overview --json --watch --stdin` SHALL open one subscription. A line `add <folder>` on its input SHALL add the folder. A line `remove <folder>` SHALL remove it. Each line of its output SHALL be one folder's overview, with `folder` in it. It SHALL end when its input closes and it serves no folder, or when it is stopped. (`flume-has-a-resident@49876bc5c73c`)

#### Scenario: Two folders from the input

- **WHEN** `sluice overview --json --watch --stdin` reads `add <a>` and `add <b>`, for worktrees of two repositories
- **THEN** it prints one line for `<a>` and one for `<b>`, each with its `folder`, and it holds one connection to `sluice host`

#### Scenario: A watch of one folder

- **WHEN** `sluice overview --json --watch` runs with no `--stdin`
- **THEN** each line that it prints is the overview as a watch printed it before this change, with no `folder`

### Requirement: Many asks of one read make one read in flight

For each repository, `sluice host` SHALL have at most one read in flight of each kind. The kinds are a folder's resolution, a reading of the repository, and each read with the same request. An ask that comes while its read is in flight SHALL join that read. An ask that comes after a command told of a change SHALL make one more read due after it. (`one-read-in-flight@e99acda774d0`)

#### Scenario: Ten statuses at one time

- **WHEN** `sluice host` read the repository within its tick, and ten `sluice status main --json` run at one time
- **THEN** each prints the same queue, and `sluice host` read `main`'s queue one time for them

#### Scenario: Twenty statuses during a reading

- **WHEN** a reading of the repository is in flight, a command told of a change, and twenty `sluice status main` run within the reading's time
- **THEN** one more reading follows the first, and no third reading starts for those statuses

#### Scenario: A subscription and a status of one repository

- **WHEN** a subscription serves a worktree, and `sluice status main` runs in another worktree of the same repository while a reading is in flight
- **THEN** the subscription and the status share that reading

### Requirement: A pass's report comes before the document read after it

`sluice host` SHALL send a folder's report of a pass before any document
that it read after that pass. So a client that reads a document has the
report of each pass before it. A wait that ends on a document says what
each of those passes released (122, 123).

#### Scenario: A release fails after a landing

- **WHEN** a round lands a submission, the release of its checkout then
  fails, and a waiting submit follows the submission
- **THEN** the submit prints the failed release and the landing, and
  exits 0
  (`a_release_failing_after_the_landing_leaves_the_submit_landed` in
  `crates/sluice/tests/landed.rs`)

#### Scenario: A report and a document ready at one time

- **WHEN** a pass's report and the document read after it are both
  ready to send
- **THEN** the client gets the report first
  (`a_report_goes_before_the_document_read_after_it` in
  `crates/sluice/src/host_process/subscription.rs`)
