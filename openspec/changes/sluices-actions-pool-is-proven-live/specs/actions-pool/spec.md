## ADDED Requirements

### Requirement: The pool's trial is kept and every gate reads it

The records the GitHub Actions pool's trial signed, a run on each of
`linux-x64` and `macos-arm64` from a released sluice, SHALL be kept
under `crates/sluice/tests/trials/actions/` with the trial's policy, and
sluice's tests SHALL read them as every host reads a pool's signed run,
from the records alone, in every gate (78, 86, 88, 107; record 0119).

#### Scenario: The trial's run on each system

- **WHEN** the kept record of the trial's run on `linux-x64`, and the
  one on `macos-arm64`, are read with the signer the trial's policy
  names, in the repository the trial ran in
- **THEN** each verifies as `forge` against the roots sluice carries,
  every check the trial's `main` requires reads a pass on that record's
  own system under its run's claim, and the trial's `main` counts it
  (`the_trials_runs_count_for_its_gate` in
  `crates/sluice/src/profiles/remote_refs.rs`)

#### Scenario: The trial's records under another workflow

- **WHEN** the same records are read for the signer's workflow on a pull
  request's ref, or in another repository
- **THEN** each counts for nothing, naming the identity its certificate
  carried, the trial's workflow on `main`
  (`the_trials_runs_count_only_from_its_signers_workflow` in
  `crates/sluice/src/profiles/remote_refs.rs`)
