## MODIFIED Requirements

### Requirement: The host process reads each result once and answers by key

The host process SHALL read each result and each signed run record once for its life, and verify each signature once. It SHALL answer a gate, a status, a verdict and a session run by the keys and combinations that they ask for. A command SHALL get results only from the host process. Where none answers, the command SHALL exit 3.

Before: a command that could not ask the host process read the results itself. Now: it exits 3. (`reads-go-through-the-resident@69134e5825d7`)

#### Scenario: A signed record verified once

- **WHEN** a branch's records are on its remote with forty signed run records, and the host process passes the repository ten times
- **AND** `sluice status <n>` runs three times for a submission under test
- **THEN** each of the forty records was verified once, and no command verified any

#### Scenario: A status reads what it asks for

- **WHEN** the repository's results log holds results of two hundred combinations
- **AND** `sluice status <n> --json` runs for a submission whose combination has three results
- **THEN** the status shows the three, and the command read no line of the results log itself

#### Scenario: A result recorded after the index was read

- **WHEN** the host process read the results log, and a run then records a pass under a key
- **THEN** the next gate that asks for that key reads the pass
- **AND** the host process read the log from the point where it last read it

#### Scenario: Results with no host process that answers

- **WHEN** a `sluice exec` needs the results of a key, and no host process answers it
- **THEN** it says that sluice's host process did not answer, reads no line of the results log, and exits 3
