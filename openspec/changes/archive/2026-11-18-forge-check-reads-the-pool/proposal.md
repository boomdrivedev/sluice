## Why

`sluice forge check main` read no line of a pool on GitHub Actions that
`main`'s policy names. The GitHub Actions pool's trial ran on
2026-11-18, boomdrivedev/sluice-actions-trial on sluice 0.97.1. There
the check said seven lines, all ok, and exited 0. None was the pool's:
its signer, its workflow, the app's permission to run workflows or the
`sluice` environment.

The check read the pool from `Branches::policy_of`, routing's reading of
the policy. That reading takes only the remote each pattern names and
whether signers are named (record 0069; rule core.27). It leaves the
pools out, so no pool's check ever ran.

## What Changes

- **forge check reads the policy whole.** It reads the pools from the
  policy read whole at the branch's revision as the host holds it, with
  no fetch (56; rule core.15). A branch not yet made is read at main's,
  which it is made from.
- **A policy it cannot read whole is named.** forge check says the
  revision, the policy's own words and that its pools went unread, as a
  check that failed, and exits 1.
- **Recorded.** Record 0119 is amended 2026-11-18 with the choice, and
  rule core.27's details rule out routing's reading read for the pools.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `forge-identity`: forge check reads a pool from the policy read whole,
  and names a policy it cannot read whole.

## Out of scope

- **A lapsed pool run dispatched again with no bound.** In the same
  trial, a run whose job ended with no record of its run
  (`pool/forge/1`) was dispatched again as `pool/forge/3`, 4 and 5 until
  its submission was withdrawn. A lapsed claim's run has no outcome and
  is run again (60, 77, 88). No clause counts it toward the runs in a
  row that read a combination not passed (115). What bounds it is the
  statement's to decide first, and it waits on that decision and a
  change of its own.
- **`sluice pool check`.** It reads its branch's policy through the
  branch seam, fetching, as it did; only a CodeBuild pool is its.

## Impact

- `crates/sluice/src/profiles/mod.rs`: `forge_findings` reads the pools
  through `whole_policy`.
- Tests: `crates/sluice/tests/profiles/forge.rs`
  (`forge_check_reads_the_pool_the_policy_names`; `OnGitHub::with_policy`
  and `POOL_POLICY`).
- Records and rules: record 0119 as amended 2026-11-18; rule core.27.
- Sources: clause 56; records 0069 and 0119; rules core.15, core.27.

## Touches

```
crates/sluice/src/profiles/mod.rs
crates/sluice/tests/profiles/forge.rs
docs/adr/0119-a-queues-runs-go-to-a-pool-and-a-codebuild-fleet-runs-as-many-at-once-as-it-holds.md
docs/architecture/core.md
openspec/changes/forge-check-reads-the-pool/**
```
