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

### Requirement: A newer sluice's command has an older host process hand over

A newer command that reaches an older host process SHALL have it hand its repositories over and end. It finishes each task's round, starts none, and lets its locks go. The command SHALL wait for it as long as it gives its tasks, then start its own. An older daemon starts afresh in place. An older command's request that the newer process does not know SHALL get an error naming the sluice to run.

Before: an older command used a newer host process. Now: it gets that error.

#### Scenario: The host's sluice is upgraded

- **WHEN** a host process of sluice 0.71.0 runs, and a submit of sluice 0.72.0 runs on the host
- **THEN** the 0.71.0 process ends, a 0.72.0 host process runs, and the submission lands

#### Scenario: A round that outlasts a start's wait

- **WHEN** a 0.72.0 command asks a 0.71.0 host process to hand over, and one of its tasks takes twelve seconds to finish its round
- **THEN** the command waits for the 0.71.0 process to end, and then reaches a 0.72.0 host process of its own start

## REMOVED Requirements

### Requirement: An older sluice run keeps the repository it drives

**Reason**: `sluice host` moves each repository that it holds, and looks for no driver's lock. A `sluice run` follows `sluice host` on a subscription.

**Migration**: A `sluice run` of an earlier release gets an error from `sluice host` that names the sluice to run. The host's unit runs `sluice daemon`, or the new `sluice run`.
