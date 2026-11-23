## MODIFIED Requirements

### Requirement: A change no binding takes up in place goes to a fresh host process

A change can move a table that no binding takes up in place, `[telemetry]` alone of today's. The host process SHALL then bind the new settings, send what its telemetry holds, and let each task finish its pass. It SHALL then start afresh in its place, with the same pid and its workers. The new image SHALL read the file itself.

Before: a successor of another pid started detached. Now: the process starts afresh in place. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

#### Scenario: Which tables hand over

- **WHEN** a change moves `[telemetry]`, alone or with other tables, or moves `[runs]` and `[forge]` alone
- **THEN** the first starts the host process afresh, and the second is taken up in place

#### Scenario: The host process sends to the collector it is changed to

- **WHEN** the host's settings say `[runs] max = 2` and `[telemetry]` with `send = true` and one collector
- **AND** the host process runs with two submissions queued on `bolt/a`
- **AND** the file is changed to name another collector while the first combination's run is going
- **THEN** the first collector is sent traces and metrics, the second is sent traces, and both submissions land
- **AND** the host process that answers has the same pid with another `since`
- **AND** each combination's run started once

### Requirement: Work in play never waits for a command across the host process's hand-over

The host process SHALL take the host's start lock before it lets its own lock go. It SHALL hold the start lock until the image started afresh in its place answers. So a command during the start afresh reaches the new image, and work in play never waits for another command.

Before: the successor had another pid. Now: it is the same pid. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

#### Scenario: A submit during the hand-over

- **WHEN** the host process is starting afresh for a changed `[telemetry]`, and a submit runs on the host meanwhile
- **THEN** the submit reaches the image started afresh, exactly one host process runs, and the submission lands with no other command run

#### Scenario: A submit during a daemon's start afresh

- **WHEN** `sluice daemon` is starting afresh in place for a changed `[telemetry]`, and a submit runs on the host meanwhile
- **THEN** the submit reaches the daemon started afresh, exactly one host process runs, and the submission lands

### Requirement: A successor that does not start leaves the host process as it was

The settings that a start afresh is for may not bind, or the start afresh may fail. The host process SHALL then keep its repositories, its workers and the settings that it holds. It SHALL take its lock and socket back, go on passing, and say once in its log why. It SHALL never end for it.

Before: a successor that did not answer was a process of another pid. Now: a failed start afresh returns to the image that asked it. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

#### Scenario: Settings it would hand over for that do not bind

- **WHEN** the host's settings say `[runs] max = 2`, and the host process runs with two submissions queued on `bolt/a`
- **AND** the file is changed in `[runs]`, `[forge]` and `[telemetry]`, with a `[forge]` key that holds no private key, while the first combination's run is going
- **THEN** both submissions land, and the same host process image runs throughout
- **AND** its log says once that it keeps the settings that it holds, naming the key

#### Scenario: A successor that does not answer

- **WHEN** the host process starts afresh, and its executable is gone
- **THEN** it takes its lock back, and its socket answers with its own pid and `since`
- **AND** its runs continue, and its log says once that the start afresh failed
