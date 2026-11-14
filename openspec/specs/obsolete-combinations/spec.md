# obsolete-combinations Specification

## Purpose
What becomes of a combination whose submission left the queue while it
was under test: its run ended, its checkout released, and no verdict
read from it, at the host's next read of the branch (25, 32, 81; rules
core.10, core.16, core.23).

## Requirements

### Requirement: An obsolete combination's run is ended and its checkout released
For an obsolete combination, the host SHALL end its own run of it with
everything the run started, and SHALL release the checkout it made for
it, or hold the checkout named with what failed (123), before it starts
or claims another run. It SHALL end no run and release no checkout of
another host's (rules core.10, core.16, core.23).

#### Scenario: Its run still going
- **WHEN** `#5` is withdrawn while its run is still going on this host
- **THEN** at the host's next round the run's processes are gone and its checkout is released, before any other run starts

#### Scenario: Its release fails
- **WHEN** the release of the obsolete combination's checkout fails
- **THEN** the checkout is held, named with what failed, and tried again as any failed release is (123)

### Requirement: No verdict is read from an obsolete combination
sluice SHALL read no verdict from an obsolete combination's run, whether
it had finished or not: it SHALL write no result line for the
combination, record no check of the run, blame no submission and tell no
session anything of it (25, 81).

#### Scenario: Its run had finished
- **WHEN** `#5`'s run passed every check and `#5` was withdrawn before any round read it
- **THEN** the next round writes no result for `#5`'s combination, records no check of that run, and lands nothing of `#5`

#### Scenario: Its run had failed
- **WHEN** `#5`'s run failed a check and `#5` was withdrawn before any round read it
- **THEN** no submission is blamed and no session is told of a failure

### Requirement: A combination holding a withdrawn submission is obsolete
A combination under test that holds a submission no longer in the queue,
whoever withdrew it (a session, a planner, the operator, a resubmission,
or intake for a pull request closed, back in draft or on a new head),
SHALL be obsolete. The host's next round of the branch SHALL drop it
before it reads any run of any combination (5, 21, 67).

#### Scenario: Withdrawn while under test
- **WHEN** `#5` is under test on the host and its session withdraws it
- **THEN** the host's next round drops `#5`'s combination before reading any run, and the branch's records hold no combination with `#5` in it

#### Scenario: Closed while under test
- **WHEN** the host keeps main and is testing `pull/12`'s combination, and `#12` is closed
- **THEN** the round whose intake withdraws `pull/12` drops its combination before reading any run, and main's records hold no combination with it
