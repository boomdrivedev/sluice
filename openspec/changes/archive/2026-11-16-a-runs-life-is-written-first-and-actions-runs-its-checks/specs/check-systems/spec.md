## MODIFIED Requirements

### Requirement: A run is taken only on its system
A run SHALL be claimed only by a host whose system it is and whose
signer may claim that system, or started on a pool that has a runner for
it. A run that no live host or pool can take SHALL stay open, and once
the combination's `timeout` has passed since the run opened it SHALL end
with no outcome: the combination is not passed and nobody is blamed
(66, 87; run-life).

#### Scenario: Nobody runs the system
- **WHEN** a run needs `macos-arm64` and no host or pool lists it
- **THEN** the run stays open, and when the branch's `timeout` passes it ends with no outcome, `cli:build` on `macos-arm64` named as having had no host that may take it live, the combination not passed and no submission blamed
