## Why

`sluice release` in a worktree sluice holds nothing for, under a policy
that names no release command, wrote outside the worktree before it
found it had nothing to free: it took the records lock, wrote and let go
a record under the repository's `sluice/`, and made the repository's
directory under the user's scratch directory, which it left. A caller
whose sandbox allows only its place, that place's scratch and its
declared caches was refused, and its release failed.

On 2026-11-17 on deneb, flume's tool test s107 kept a place because its
teardown's `sluice release`, sluice 0.95.1, answered `creating
/private/tmp/sluice-501/<key>: Operation not permitted`. sluice 0.93.2's
release made nothing there. Record 0120 as amended 2026-11-17 decides
it.

## What Changes

- **A release with nothing to free reads only.** The place profile asks
  whether the policy names a release command before it takes the
  records lock, writes a record or makes a scratch. Where it names none,
  the release makes nothing, says `nothing to release` and exits 0.
- **The check seam answers whether a release command is named.** Its
  reading, `checks::names_release`, is handed to the place profile by
  the binding beside the release command's builder (rule core.2).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `place-release`: a release with nothing to free makes nothing.

## Out of scope

- **A release that runs a release command.** It keeps a record and a
  scratch of its own for the command's length (record 0120, D1), and a
  caller whose sandbox refuses the user's scratch directory is refused
  there still; whether such a release's scratch may live elsewhere is a
  change of its own.
- **The statement's clause 95.** It holds as written: it says what a
  release runs, not what it writes when it runs nothing.

## Impact

- `crates/sluice/src/profiles/places.rs`: `release_unheld` reads
  whether a release command is named first; `ReleaseNamed` and
  `Places::with_release(named, builder)`.
- `crates/sluice/src/profiles/checks.rs`: `names_release`.
- `crates/sluice/src/profiles/mod.rs`: the binding hands both over.
- Tests: `crates/sluice/tests/exec.rs`
  (`an_unheld_release_with_nothing_to_free_makes_nothing`),
  `crates/sluice/tests/profiles/places.rs`.
- Records and rules: record 0120 as amended 2026-11-17, rule core.10.
- Sources: session-runs-1 clause 95; clause 35 as amended; rules
  core.2, core.10, core.30.

## Touches

```
crates/sluice/src/profiles/places.rs
crates/sluice/src/profiles/checks.rs
crates/sluice/src/profiles/mod.rs
crates/sluice/tests/exec.rs
crates/sluice/tests/profiles/places.rs
docs/adr/0120-a-worktree-sluice-holds-nothing-for-is-released-by-its-policys-release-command.md
docs/architecture/core.md
openspec/changes/a-release-with-nothing-to-free-creates-nothing/**
```
