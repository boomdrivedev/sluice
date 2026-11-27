## MODIFIED Requirements

### Requirement: A host reads a pull request it follows once in a quarter of the interval
Every process on a host SHALL read a followed pull request from GitHub
at most once in each quarter of `[forge] discovery`. It SHALL read
sluice's check on the request's head no more often. The quarter starts at the last read by any
process on that host. Until then it SHALL use what that read kept. What
a read kept SHALL stand only while the request's entry here is as it was
at that read. Every such read SHALL be conditional on the answer the host keeps
for its address (136, rule core.32).

#### Scenario: Two waits and a status
- **WHEN** host `a` follows `#12`, which host `b` takes in, and `discovery` is sixty seconds
- **AND** for one minute, two waits on `#12` and a `sluice status main` every ten seconds run on `a`
- **THEN** `a` read `#12` and its check from GitHub no more than four times each in all

#### Scenario: Closed after a submit read it
- **WHEN** a submit read `#12` open, and `#12` closes before the new head on it lands
- **AND** intake withdraws the submission of that head, and a wait follows the withdrawn work within a quarter of `discovery`
- **THEN** the wait reads `#12` from GitHub again, exits 4, and names `#12` closed in `carried_by`
  (`a_wait_exits_withdrawn_when_the_request_carrying_its_work_closes` in
  `crates/sluice/tests/profiles/pull_requests.rs`)
