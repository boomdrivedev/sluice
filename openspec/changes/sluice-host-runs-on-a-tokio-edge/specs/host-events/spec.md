## Purpose

sluice's host process waits on events and timers, never on a poll. A change to a repository's files starts a reading of that repository, and triggers that come during a reading join one reading after it.

## ADDED Requirements

### Requirement: The host process waits on events, not on a poll

The host process SHALL answer a connection when it arrives, and SHALL learn of a signal and of a stop when they come. No wait of the host process SHALL be a loop that sleeps and looks again. (`tokio-streams-before-rxrust@0b9fc11cda8b`)

#### Scenario: An idle host process on a paused clock

- **WHEN** a host process with nothing in play runs on a paused clock, and the clock moves one minute
- **THEN** the host process woke only for the timers that fell due in that minute, and for no poll

#### Scenario: A connection to an idle host process

- **WHEN** a host process has had nothing to do for ten minutes, and a command connects
- **THEN** the host process answers `hello` with no timer of its own between the connection and the answer

### Requirement: A change to a repository's files starts a reading

The host process SHALL watch the refs of each repository that it holds and sluice's records in the host's files. A change there SHALL start a reading of that repository once the events stop for the debounce time. Many events in one burst SHALL start one reading. (`tokio-streams-before-rxrust@0b9fc11cda8b`)

#### Scenario: Another process writes the records

- **WHEN** a watch follows the host process, and nothing is in play
- **AND** another process writes a submission to `main`'s records, and no command tells the host process
- **THEN** the watch prints `main` with that submission queued within one second of the debounce time

#### Scenario: A burst of ref changes

- **WHEN** a script moves twenty refs of a repository within 50 ms
- **THEN** the host process makes one reading of that repository for the burst

### Requirement: An idle repository is read only when something calls for it

A file event, a command, a follower's opening or the minute's backstop SHALL start a reading of a repository with nothing in play. Nothing else SHALL start one.

Before: the host process looked at each due repository every three seconds. Now: an event or a command starts a reading, and the backstop reads what no event reported. (`tokio-streams-before-rxrust@0b9fc11cda8b`)

#### Scenario: Three idle repositories for a minute

- **WHEN** the host process holds three repositories with nothing in play, on a paused clock
- **AND** the clock moves one minute with no event and no command
- **THEN** each repository was read once, at the minute's backstop

### Requirement: Triggers during a reading join one reading after it

For each repository, the host process SHALL have at most one reading in flight. Triggers that come while a reading is in flight SHALL make one more reading due after it, however many they are. The newest reading SHALL replace the older one in what the host process keeps. (`tokio-streams-before-rxrust@0b9fc11cda8b`, `one-read-in-flight@e99acda774d0`)

#### Scenario: Five serves during a reading

- **WHEN** a reading of a repository is in flight, and five commands send `serve` for it before the reading ends
- **THEN** the host process makes one more reading of it after the first, and no third

### Requirement: A signal stops each host process as a hand-over does

`SIGTERM` and `SIGINT` SHALL stop a host process that a command started, as they stop a daemon. Each task SHALL finish its round. The host process SHALL then let its locks and its socket go, and exit 0.

Before: only a daemon read these signals, and a host process that a command started ended at once on them. Now: each host process stops in order on them. (`tokio-streams-before-rxrust@0b9fc11cda8b`)

#### Scenario: SIGTERM during a round

- **WHEN** a host process that a submit started is in a round of `main`, and it receives `SIGTERM`
- **THEN** it finishes the round, its socket is gone, and it exits 0

### Requirement: The host process's waits are on one clock that a test can pause

Each timer, interval, debounce and time limit of the host process SHALL be on one clock. A test SHALL be able to pause that clock and move it, and the host process SHALL act as it would after that time. (`tokio-streams-before-rxrust@0b9fc11cda8b`)

#### Scenario: The minute's backstop on a paused clock

- **WHEN** a test pauses the host process's clock and moves it 59 seconds, then one second more
- **THEN** no backstop reading starts in the first 59 seconds, and one starts after the 60th
