## MODIFIED Requirements

### Requirement: A forecast with no current reader has the host process read

`sluice host` SHALL form each forecast of `sluice submit --check`. Where its reading is not current or lacks a revision that the forecast needs, it SHALL read first. Where it still lacks the revision, it SHALL read afresh once. Then it SHALL say what is missing, and exit 3. The command SHALL fetch nothing.

Before: the command formed the forecast. Now: `sluice host` forms it. (`reads-go-through-the-resident@69134e5825d7`)

#### Scenario: No reader on the host

- **WHEN** no `sluice host` runs and `main` is on a remote
- **THEN** `sluice submit main --check` starts `sluice host`, which forms the forecast from its first reading of `main` and its records
- **AND** the command fetches nothing

#### Scenario: Another host's submission

- **WHEN** `#9` was submitted to `main` from another host, its work is not on this host, and `sluice host` has no current reading
- **THEN** `sluice submit main --check` forecasts against `#9`, whose work the reading of `sluice host` fetched, and does not fail

#### Scenario: A reader whose read lacks the branch

- **WHEN** `sluice host` keeps its tick and its last reading names no revision of `main`, which is on a remote
- **THEN** `sluice submit main --check` forms on `main` and its records as `sluice host` holds them after its next reading, and the command fetches nothing

#### Scenario: Work still missing after the reading afresh

- **WHEN** the host lacks the head of `#4`, queued on `main`, and the reading afresh of `sluice host` does not bring it
- **THEN** `sluice submit main --check` says it could not read what a forecast of `main` forms from, and names the work queued on `main`
- **AND** it exits 3, and the command fetched nothing

#### Scenario: The reader could not reach the remote

- **WHEN** the reading of `sluice host`, within its tick, could not ask `main`'s remote and read `main`'s records as last fetched
- **THEN** a forecast of `main` forms on what that reading holds
- **AND** it says on standard error, in the remote's words, that it could not ask the remote
