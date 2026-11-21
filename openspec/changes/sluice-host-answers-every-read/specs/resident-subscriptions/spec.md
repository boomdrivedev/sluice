## Purpose

A client holds one subscription to `sluice host` for many folders. `sluice host` makes one read in flight for each thing that it reads, however many asks come.

## ADDED Requirements

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

### Requirement: A slow client gets only the newest overview

Where a client of a subscription reads slower than a folder's overview changes, `sluice host` SHALL send only the newest overview of that folder. The newest overview SHALL replace the older ones. (`one-read-in-flight@e99acda774d0`)

#### Scenario: A client that reads nothing for ten seconds

- **WHEN** a client reads nothing for ten seconds while three passes each change a folder's overview
- **THEN** the next overview that the client reads for that folder is the newest, and the two older ones never reach it

### Requirement: A folder with no sluice leaves the subscription

Where a folder's repository is gone or has no sluice, `sluice host` SHALL send the overview that says so. It SHALL then remove that folder from the subscription. It SHALL keep serving the other folders.

#### Scenario: A repository removed

- **WHEN** a subscription serves two folders, and the first folder's repository is removed
- **THEN** the client gets an overview for the first folder that says it is gone, and later overviews for the second folder

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
