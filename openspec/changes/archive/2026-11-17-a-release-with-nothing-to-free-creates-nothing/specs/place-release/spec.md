## ADDED Requirements

### Requirement: A release with nothing to free makes nothing

A release in a worktree sluice holds nothing for, under a policy that
names no release command, SHALL read only: it SHALL make no record,
lock, scratch or folder, neither under the repository's git directory
nor under the user's scratch directory, and SHALL say
`nothing to release` and exit 0 (95; record 0120 as amended
2026-11-17; rule core.10).

#### Scenario: Nothing to free, nothing made

- **WHEN** `sluice release` runs in a worktree sluice holds nothing
  for, under a policy that names no release command, the user's
  scratch directory not yet made
- **THEN** it says `nothing to release` and exits 0, the user's scratch
  directory is still not made, and nothing under the repository's
  `sluice/` changed
  (`an_unheld_release_with_nothing_to_free_makes_nothing` in
  `crates/sluice/tests/exec.rs`)
