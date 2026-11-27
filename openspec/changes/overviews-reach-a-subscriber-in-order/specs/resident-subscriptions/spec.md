## MODIFIED Requirements

### Requirement: A slow client gets only the newest overview

Where a client of a subscription reads slower than a folder's overview changes, `sluice host` SHALL send only the newest overview of that folder. The newest overview SHALL replace the older ones. A client SHALL get a folder's overviews in the order of the state that each read, also where the reads end in another order. (`one-read-in-flight@e99acda774d0`)

#### Scenario: A client that reads nothing for ten seconds

- **WHEN** a client reads nothing for ten seconds while three passes each change a folder's overview
- **THEN** the next overview that the client reads for that folder is the newest, and the two older ones never reach it

#### Scenario: A read that ends after a newer one

- **WHEN** three passes each read a folder's overview, and the read of the first pass ends after the reads of the other two
- **THEN** the overview of the first pass does not reach a client after the newer ones
- **AND** a client gets the overviews in the order of the passes (`a_document_read_that_ends_late_is_not_sent_after_a_newer_one` in `crates/sluice/src/host_process/tasks.rs`)
