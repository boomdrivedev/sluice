## RENAMED Requirements

- FROM: `### Requirement: A forecast with no current reader reads as a round does`
- TO: `### Requirement: A forecast with no current reader has the host process read`

## MODIFIED Requirements

### Requirement: A forecast with no current reader has the host process read

Where no reader keeps its tick, or the reader's last read does not hold
the branch read whole or a revision the forecast forms from, a forecast
SHALL ask the host process for a current reading, starting it where none
answers, and form on every branch as the host holds it since. Where the
host still lacks a revision it forms from, it SHALL ask for a reading
afresh once. A branch of which that reading, current and refused by no
remote, brings no revision to the host, at its own ref or a remote's,
SHALL be no branch, refused as a forecast refuses one with exit 2. What
is still missing otherwise, work queued on the branch, a combination
formed on it, or a branch where the reading was not current or a remote
refused it, SHALL be said as a read it could not make, exit 3. It SHALL
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

#### Scenario: A reader whose read lacks the branch

- **WHEN** a reader keeps its tick and its last read names no revision
  of `main`, which is on a remote
- **THEN** `sluice submit main --check` forms on `main` and its records
  as the host holds them since the host process's reading, and fetches
  nothing itself

#### Scenario: No such branch once the reading is current

- **WHEN** the host process's reading afresh is current, and the host
  holds no revision of `bolt/gone` at any ref
- **THEN** `sluice submit main --check --head bolt/gone --json` prints an
  error document whose message is `no branch bolt/gone to forecast`, and
  exits 2

#### Scenario: Work still missing after the reading afresh

- **WHEN** the host lacks the head of `#4`, queued on `main`, and the
  host process's reading afresh does not bring it
- **THEN** `sluice submit main --check` says it could not read what a
  forecast of `main` forms from, naming the work queued on `main`, and
  exits 3, having fetched nothing

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
