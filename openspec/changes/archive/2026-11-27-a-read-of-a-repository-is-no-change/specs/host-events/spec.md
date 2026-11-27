## ADDED Requirements

### Requirement: A read of a repository's files starts no reading

An event that only says that a process opened, read, or closed a file after a read changes nothing. It SHALL NOT start a reading of a repository. The host process's own reading opens a repository's refs and records, and on Linux each open is an event. Read as a change, each such event starts another reading with no end.

#### Scenario: The host process reads a repository on Linux

- **WHEN** the host process holds a repository on Linux, and its reading opens the repository's refs and records
- **THEN** the opens start no other reading, and the host process takes up a change to its settings (`the_host_process_keeps_its_repositories_across_a_new_binding` in `crates/sluice/tests/host_process.rs`, and `a_read_is_no_change` in `crates/sluice/src/host_process/watch.rs`)
