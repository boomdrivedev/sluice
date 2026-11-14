## RENAMED Requirements

- FROM: `### Requirement: A forecast with no current reader reads as a round does`
- TO: `### Requirement: A forecast with no current reader has the host process read`

## MODIFIED Requirements

### Requirement: A forecast with no current reader has the host process read

Where no reader keeps its tick, a forecast SHALL ask the host process
for a current reading, starting it where none answers, and form from
it. Where that reading does not hold the branch, or a revision it forms
from is not on the host, it SHALL ask for a reading afresh once, and say
what is still missing as a read it could not make, exit 3. It SHALL
fetch nothing itself (64, 129; record 0093 as amended 2026-11-13).

#### Scenario: No reader on the host

- **WHEN** no host process runs and `main` is on a remote
- **THEN** `sluice submit main --check` starts the host process, forms on
  `main` and its records as that process's first reading fetched them,
  and fetches nothing itself

#### Scenario: Another host's submission

- **WHEN** `#9` was submitted to `main` from another host, its work is
  not on this host, and no reader keeps its tick
- **THEN** `sluice submit main --check` forecasts against `#9`, whose
  work the host process's reading fetched, rather than failing

#### Scenario: The reader could not reach the remote

- **WHEN** the host process's reading, within its tick, could not ask
  `main`'s remote and read `main`'s records as last fetched
- **THEN** a forecast of `main` forms on what that reading holds and says
  on stderr, in the remote's words, that it could not be asked

### Requirement: A reader keeps its tick while it reads on time

A forecast SHALL count the reader as keeping its tick only while the
host process answers and either its last read of the repository ended no
more than its tick ago, or it is reading now and its last read ended no
more than a tick before this read began. Any other reader SHALL be read
as none (64, 129).

#### Scenario: A reader that has stalled
- **WHEN** the host process answers but its last read of the repository ended five ticks ago and it began no read since
- **THEN** a forecast of `main` asks the host process for a reading and forms from it

#### Scenario: A reader in a slow read
- **WHEN** the reader's last read ended a second before its next began, and that next read has run for twenty seconds
- **THEN** a forecast takes the branch from the last read and fetches nothing
