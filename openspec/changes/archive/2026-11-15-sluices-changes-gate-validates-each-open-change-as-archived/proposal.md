## Why

Principle 16 of boomdrivedev/blueprints, as amended 2026-11-15
(3a5c19d), keeps a tool's own validation of the files it works on in the
gate: "OpenSpec's `openspec validate` of a kit's specs and its open
changes, each change validated as it would archive". `root:changes` runs
`openspec validate --changes --strict`, which passes a change that
`openspec archive` refuses, or that archives into a spec `root:specs`
then fails: archive validates the specs it rebuilds without `--strict`.
Each is found only at the archive, after the gate has passed the branch.
Four of the tree's eight open changes are such changes today. Record
0128 decides how the gate holds them.

## What Changes

- **`root:changes` validates each open change as archived.**
  `check-changes.sh` runs `openspec validate --changes --strict`, then,
  for each open change, `openspec archive` in a scratch copy of
  `openspec/` holding the tree's specs, its configuration and that
  change alone, and `openspec validate --strict` of each spec the
  archive writes. A change that fails is named with OpenSpec's words.
  Boomdrive's `check-changes.sh` (its record 0050, core.61) is the
  pattern it copies.
- **The tree's open changes archive into valid specs.** The two changes built
  on the intake mark, `a-host-hands-over-only-to-the-host-its-mark-names`
  and `a-hand-overs-withdrawal-holds-no-request`, are removed, never
  archived: rule core.33 rules out the mark. In
  `a-places-scratch-fits-a-socket`, the long requirement on a worktree's
  former scratch is three, each with its own scenarios. In
  `actions-pool`, the pool's reports from `sluice forge check` are a
  requirement of their own, and the requirement as the tree holds it
  stays. `finished-submissions`' new capability has a Purpose.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `repository-gate`: `root:changes` validates each open change as it
  would archive.

## Impact

- `moon.yml`, `check-changes.sh` (new); the open changes
  `a-places-scratch-fits-a-socket`, `actions-pool` and
  `finished-submissions`, and the two removed.
- Records and rules: record 0128; rule core.61.
- Records, policy and the host's settings of sluice's: nothing changes.
- Out of scope: whether `a-places-scratch-fits-a-socket`, `actions-pool`
  and `finished-submissions` are archived, built or dropped; each stays
  as open as it was.
