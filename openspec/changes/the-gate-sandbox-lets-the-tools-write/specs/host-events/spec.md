## MODIFIED Requirements

### Requirement: A change to a repository's files starts a reading

The host process SHALL watch the refs of each repository that it holds and sluice's records in the host's files. A change there SHALL start a reading of that repository once the events stop for the debounce time. Many events in one burst SHALL start one reading. (`tokio-streams-before-rxrust@0b9fc11cda8b`)

#### Scenario: Another process writes the records

- **WHEN** a watch follows the host process, and nothing is in play
- **AND** another process writes a submission to `main`'s records, and no command tells the host process
- **THEN** the watch prints `main` with that submission queued, before the minute after which an idle repository is read again
  (`a_records_write_reaches_a_watch_by_its_file_event` in `crates/sluice/tests/host_process.rs`)

#### Scenario: Another process writes the records, on a quiet host

- **WHEN** the same, on a host that runs no other work
- **THEN** the watch prints it within two seconds of the debounce time; `cli:speed` runs it, and no gate does
  (`a_records_write_reaches_a_watch_by_its_file_event_at_speed`)

#### Scenario: A burst of ref changes

- **WHEN** a script moves twenty refs of a repository within 50 ms
- **THEN** the host process makes one reading of that repository for the burst
