## MODIFIED Requirements

### Requirement: A command that needs the queues moved reaches the host process or starts it

A command of sluice's that needs its host's queues moved, a submit among them, SHALL reach sluice's host process. Where none answers, it SHALL start one, unless the host's settings say `[daemon] managed = true`. Nothing SHALL be installed for this, and no policy key SHALL be needed (135, rule core.31, `the-service-manager-starts-each-resident@d0e66e466414`).

#### Scenario: A withdrawal with no host process running

- **WHEN** a submission's combination is under test, no host process
  runs, and `sluice withdraw` takes the submission out
- **THEN** a host process is running, and its next pass ends the run and
  releases the combination's checkout

## ADDED Requirements

### Requirement: A command starts no host process where the service manager keeps the daemon

Where `[daemon] managed` is true and no host process answers, a command SHALL start no host process. It SHALL fail at once, its words naming the socket, the setting and the log. The git hook SHALL fail the same way (`clients-never-fetch-or-start-a-tool@8aa9c57f19b2`).

Before: a command started one wherever none answered. Now: it starts none there.

#### Scenario: No daemon answers where the service manager keeps it

- **WHEN** the host's settings say `[daemon] managed = true`, no host process runs, and a command reaches for one
- **THEN** the command fails at once, its words naming the socket, `[daemon] managed` and the host process's log
- **AND** no host process runs after it

#### Scenario: A daemon answers where the service manager keeps it

- **WHEN** the host's settings say `[daemon] managed = true`, and a host process answers
- **THEN** the command reaches that host process

### Requirement: A command reads [daemon] managed alone

A command SHALL read `[daemon] managed` alone of the host's settings file, and pass over every other table. A file that is not there, or that the command cannot read, SHALL give false.

#### Scenario: A settings file with other tables

- **WHEN** the host's settings hold `[runs]` and `[forge]` but no `[daemon]`, and no host process runs
- **THEN** a command starts the host process, and the host process answers

### Requirement: A command starts the host process through process-wrap

A command SHALL start the host process through process-wrap, in a session of its own. It SHALL reap the process if the process ends while the command runs (`each-worker-starts-through-process-wrap@025fae162990`).

Before: the command wrote a `setsid` of its own in the child. Now: process-wrap makes the session.

#### Scenario: A started host process is in a session of its own

- **WHEN** no host process runs, and a command starts one
- **THEN** the host process leads a session of its own, apart from the command's
