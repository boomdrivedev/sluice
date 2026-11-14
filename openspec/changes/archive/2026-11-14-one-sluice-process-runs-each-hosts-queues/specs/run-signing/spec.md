## ADDED Requirements

### Requirement: A branch that lands or is removed keeps every result it recorded

No landing, removal of a branch or release of a place SHALL remove a
result from the results log or a signed run record from a signer's ref.
A result belongs to its key and its system, not to a branch, and a
signer's records are only added to (69, 109; C.6; record 0093 as amended
2026-11-13).

#### Scenario: A removed bolt's pass counts for main

- **WHEN** a bolt's combination passed `cli:test` under its key, the bolt
  landed onto `main` and was removed, and `main`'s next combination's
  tree gives `cli:test` the same key
- **THEN** `main`'s gate counts that pass as a replay, and the results
  log and the signer's ref still hold it
  (`a_removed_branchs_results_still_count` in
  `crates/sluice-conformance/tests/verdicts.rs`)
