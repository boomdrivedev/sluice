## MODIFIED Requirements

### Requirement: Each host shows it is alive and how much room it has
Each host running sluice for a repository whose branch's records are on
its remote SHALL rewrite `refs/sluice/hosts/<host>` on that remote at
the cadence of its sign-of-life bound, with the checks running on the
host, every repository's, and the room the host's bound has left,
whether or not it leads any branch. Only that host SHALL write its ref.

#### Scenario: A host's sign of life
- **WHEN** `sluice run` is running on host A with a bound of 4 and one run under test
- **THEN** `refs/sluice/hosts/A` is rewritten at least once per sign-of-life bound and says one run held and room for three

#### Scenario: Another repository's runs leave less room
- **WHEN** host A's bound is 4, one run of `main` is under test, and one run of another repository on host A is running
- **THEN** `refs/sluice/hosts/A` says two runs held and room for two

#### Scenario: The lead shows it is alive
- **WHEN** host A leads `main` and holds a claim on one of its runs
- **THEN** `refs/sluice/hosts/A` is rewritten at least once per
  sign-of-life bound, as every other host's is
  (`a_lead_shows_its_sign_of_life_as_any_claim_holder` in
  `crates/sluice-conformance/tests/run_life.rs`)

### Requirement: A claim lapses with its host's sign of life
A claim SHALL lapse once a reader has seen its host's sign of life
unchanged for the sign-of-life bound, by the reader's own observation
from when it first read that sign of life, never for the length of its
run, never because the lead changed, and never because the reader took
the lead. A pool's job's claim SHALL lapse once its job has ended with
no record of its run. A lapsed claim's run SHALL end with no outcome,
its checks held by a follow-up run, and a result reported under a claim
its host no longer holds SHALL NOT be written. The claim holder's report
SHALL be written whichever instance leads (60, 73, 77, 88).

#### Scenario: A host stops mid-run
- **WHEN** host A claimed a run of `main` and is switched off
- **THEN** after the sign-of-life bound the claim lapses, the run ends with no outcome, and another host claims its follow-up run

#### Scenario: A long run keeps its claim
- **WHEN** a run takes 40 minutes and its host keeps rewriting its sign of life
- **THEN** the claim holds until the run reports, within the branch's `timeout` counted from the run's own start

#### Scenario: A late report
- **WHEN** host A returns after its claim lapsed and tries to report the run's result
- **THEN** the result is not written to the branch's state, and the combination's outcome is the one its follow-up run reports

#### Scenario: A new lead and a live claim
- **WHEN** host B takes the lead of `main` while host A, showing its sign of life, holds a claim on a running run
- **THEN** A's claim holds, and A's report is written as the run's end
  (`a_change_of_lead_keeps_a_live_claim` in
  `crates/sluice-conformance/tests/run_life.rs`)
