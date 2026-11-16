## Why

A process inherits its soft limit on open files from what started it,
and launchd gives what it starts 256. A sluice that launchd, or
anything launchd started, runs is held to 256 open files, and so is
every moon run it starts. A run that reaches the limit fails with "Too
many open files", and the fail reads as the candidate's. Clause 135,
as amended on boomdrivedev/blueprints's branch
`design/sluice-raises-its-open-files-limit-at-start` (2a045b2), says
every sluice process raises its soft limit to its hard limit as it
starts, so no host need raise it for sluice. Record 0126 decides how.

## What Changes

- **sluice raises its limit as it starts.** `main` raises the soft
  limit on open files to the hard limit, as far as the system lets one
  process open (macOS's `kern.maxfilesperproc`), before it parses its
  command line, reads a file or starts anything. A soft limit already
  at its hard limit is left as it is.
- **The runs inherit it.** Nothing of sluice's sets a run's limit, so
  a check, its moon and what they start have the raised limit.
- **A refusal is said.** Where the system refuses the raise, sluice
  says so on standard error and goes on under the limit it was given.

## Capabilities

### New Capabilities

- `open-files-limit`: every sluice process raises its soft limit on
  open files to its hard limit as it starts, and the runs it starts
  inherit it.

### Modified Capabilities

None.

## Impact

- `crates/sluice`: `raise_open_files` in `src/main.rs`, through the
  rlimit crate, a new workspace dependency.
- Records, policy and the host's settings: nothing changes.
- Docs: record 0126; rule core.59; the site's rules page.
- Out of scope:
  - A host's own setting of the limit, which 135 leaves to no host.
  - Any other tool of the family. None of them sets a limit on open
    files, and none starts the runs sluice starts. Each raises its own
    in a change of its own, where it needs to.
