## MODIFIED Requirements

### Requirement: A folder with no sluice leaves the subscription

Where a folder's repository is gone or has no sluice, `sluice host` SHALL send the overview that says so. It SHALL then remove that folder from the subscription. It SHALL keep serving the other folders. A repository is gone when its git folder holds no `HEAD` file or no `objects` folder. This SHALL hold also when a round writes its records at that path after the git folder went (129).

#### Scenario: A repository removed

- **WHEN** a subscription serves two folders, and the first folder's repository is removed
- **THEN** the client gets an overview for the first folder that says it is gone, and later overviews for the second folder

#### Scenario: A repository removed as a round writes in it

- **WHEN** a subscription serves a folder whose repository has work in play
- **AND** the repository's git folder goes, and a round writes its records at the git folder's path after it went
- **THEN** the client gets an overview for that folder that says it is gone (`a_repository_gone_as_a_round_writes_in_it_is_gone` in `crates/sluice/tests/host_process.rs`)
