# Design

## Context

Clause 135, as amended at boomdrivedev/blueprints 2a045b2, says every
sluice process raises its soft limit on open files to its hard limit as
it starts. Record 0126 decides the form, and rule core.59 holds it.

What exists on the bolt today:

- `crates/sluice/src/main.rs`'s `main` reads what a `sluice run` handed
  it (`HandOver::take`) first, before any thread or child starts, then
  parses its command line. Every sluice process starts there: a
  command, the host process, a run started afresh, a supervisor
  (`sluice supervise`) and the hook (`sluice hook`).
- Nothing of sluice's reads or sets a limit on open files. A run's
  process group inherits sluice's limits.
- On deneb, `launchctl limit maxfiles` answers `256 unlimited`, and
  `kern.maxfilesperproc` is 245760. macOS's `setrlimit` refuses a soft
  limit on open files above `kern.maxfilesperproc`.

## Goals / Non-Goals

**Goals:** every sluice process, and every run it starts, has a soft
limit on open files equal to its hard limit, as far as the system lets
one process open.

**Non-Goals:** a host's own setting of the limit; any other limit;
another tool of the family's limit.

## Decisions

- **D1. One call, first in `main`.** `raise_open_files()` runs right
  after `HandOver::take`, so every sluice process raises its own before
  it parses its command line, reads a file or starts anything.
- **D2. The rlimit crate.** `rlimit::increase_nofile_limit(u64::MAX)`
  sets the soft limit to the hard limit, and on macOS to no more than
  `kern.maxfilesperproc`, and leaves a soft limit already at its hard
  limit. Not taken: nix's `resource` feature with a sysctl of sluice's
  own, which is code of sluice's for what a maintained crate does.
- **D3. The runs inherit it.** Nothing restores the limit sluice
  started with for a child. Not taken: Go's way since 1.19, restoring
  it, since the moon runs are what open the most files.
- **D4. A refusal warns and goes on.** On an error sluice says
  `sluice: warning: the limit on open files stays as sluice was started
  with: <the error>` on standard error, and goes on, as it ran before
  this change. Not taken: an exit, which would stop a host that ran
  sluice under a low limit before.

## Risks / Trade-offs

- A program in a check that closes every descriptor up to its soft
  limit walks the raised limit. A check that needs a lower limit sets
  its own in its command.
- The crate, given a soft limit above `kern.maxfilesperproc` and an
  unlimited hard limit, sets the soft limit to `kern.maxfilesperproc`.
  macOS lets no process open more than that, so nothing is lost.
- On Linux, an unlimited hard limit is refused above `fs.nr_open`, and
  sluice warns and runs under the limit it was given.
