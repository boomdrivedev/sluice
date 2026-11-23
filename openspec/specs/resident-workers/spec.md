# resident-workers Specification

## Purpose
Each worker of sluice is a child of `sluice host`, in a process group of its own, and ends when its owner ends. A hand-over passes the runs on, and a stop ends them.

## Requirements

### Requirement: Each worker is a child of sluice host in a process group of its own

Each process that `sluice host` starts to do its work SHALL be a worker. A worker SHALL be a child of `sluice host` and SHALL start in a process group of its own. A combination's checks SHALL be a worker. A release command, a git command, an equip step and a credential command that it starts SHALL each be a worker too. `sluice host` SHALL record each worker's process group in the host's state before the worker does its work. (`workers-end-with-their-owner@292e869158d6`)

#### Scenario: A combination's checks

- **WHEN** `sluice host` runs a combination's checks
- **THEN** the checks' parent chain reaches `sluice host`, their process group is not the group of `sluice host`, and the host's state names that group

### Requirement: A worker ends when sluice host ends

When `sluice host` ends for a reason other than a hand-over, it SHALL end each worker's process group, the stop's grace given first.

Before: a run continued after the host process that started it ended. Now: a run ends with the host process that owns it, unless a hand-over passes it on. (`sluices-runs-end-with-its-host-process@e814b716f3b1`, `workers-end-with-their-owner@292e869158d6`, `nothing-detached@f807c7f5fcea`)

#### Scenario: Stopped while a combination is under test

- **WHEN** `sluice host` runs a combination's checks and receives `SIGTERM`
- **THEN** when it has exited no process of the checks runs

### Requirement: A run that ended with its owner has no outcome

A run that ended because its owner ended SHALL have no outcome. It SHALL blame no one, and its combination SHALL be open again for the next host process. It SHALL count toward the runs in a row as a run whose claim lapsed does. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

#### Scenario: The run after a stop

- **WHEN** `sluice host` was stopped while a combination's checks ran, and the next host process starts
- **THEN** the run has no outcome, no submission is blamed, and the next host process tests the combination again

### Requirement: The next host process ends what a killed host process left

A `sluice host` that starts SHALL read the process groups that the host's state names. It SHALL end each group that still runs and that no live host process owns, before it claims a run or starts a worker. Each run of those groups SHALL have no outcome. (`workers-end-with-their-owner@292e869158d6`, `nothing-detached@f807c7f5fcea`)

#### Scenario: Killed with SIGKILL

- **WHEN** `sluice host` runs a combination's checks and receives `SIGKILL`, and a command then starts the next `sluice host`
- **THEN** the next host process ended the checks' group before it started a worker, and the run has no outcome

### Requirement: A hand-over passes the runs to the next host process in place

A host process that hands over SHALL start the next host process in its own place, with the same pid. Each worker SHALL continue to run as a child of that pid, and the next host process SHALL own it.

Before: a host process that was no daemon started a detached successor and ended, and its runs continued with no owner. Now: each host process starts its successor in place. (`sluices-runs-end-with-its-host-process@e814b716f3b1`, `workers-end-with-their-owner@292e869158d6`)

#### Scenario: An upgrade while a combination is under test

- **WHEN** a host process of one version runs a combination's checks, and a command of a newer version asks it to hand over
- **THEN** a host process of the newer version answers with the same pid, and the checks continue as its children
- **AND** their result lands the combination with no check run again

#### Scenario: A stop after a hand-over

- **WHEN** a host process took a run in a hand-over, and it then receives `SIGTERM`
- **THEN** the run's process group has ended when the host process exits
