## ADDED Requirements

### Requirement: A wait's step is at the wait's time where sluice host passes only when asked

Each step that a wait asks of `sluice host` SHALL carry the wait's time. Where `sluice host` passes only when asked, it SHALL run the step at that time. It runs a pass at the time of the command that asks it in the same way. Elsewhere it SHALL run the step at its own time.

#### Scenario: A wait that skips its pauses

- **WHEN** `sluice host` passes only when asked, and it read a pull request before a wait began
- **AND** the wait follows that request for one minute of its clock, and skips its pauses
- **THEN** `sluice host` reads the request from GitHub one to four times (`a_wait_reads_its_pull_request_once_in_a_quarter_of_the_interval` in `crates/sluice/tests/profiles/pull_requests.rs`)
