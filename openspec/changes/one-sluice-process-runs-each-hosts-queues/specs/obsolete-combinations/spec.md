## ADDED Requirements

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

## REMOVED Requirements

### Requirement: A combination holding a submission out of the queue is obsolete
**Reason**: It named a hand-over of the branch's pull requests to another host among the withdrawals, and no hand-over is made (137; record 0093).
**Migration**: "A combination holding a withdrawn submission is obsolete" holds every other withdrawal as before.
