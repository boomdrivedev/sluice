## ADDED Requirements

### Requirement: A read gives one version of a branch's records whole

A read of a branch's records in the host's own files SHALL give one
version of the records whole, also while a round writes them. It SHALL
NOT fail because a write moved a file during the read. It SHALL NOT give
files of two versions (131, core.41).

#### Scenario: A status while a round lands

- **WHEN** a round lands a submission on `main` and moves it to the
  finished records while `sluice status main --json --all` reads them
- **THEN** the status gives `main`'s submissions at one version, the
  landed one once, and exits 0
  (`a_read_beside_writes_is_one_version_whole` in
  `crates/sluice/tests/local_files.rs`)
