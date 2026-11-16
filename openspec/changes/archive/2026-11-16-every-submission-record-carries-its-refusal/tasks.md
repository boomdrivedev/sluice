# Tasks

Work in this change's place on its branch, cut from
`bolt/sluice-is-refactored-and-runs-on-pools`. Run suites through sluice
(`sluice exec bolt/sluice-is-refactored-and-runs-on-pools -- cargo test
...`); a quick `cargo check` or one unit test may run directly. Commit by
pathspec, Conventional Commits, scoped to the part of the tree, citing
clauses by number and rules by id.

## 1. The inbox hands a refusal over

- [x] 1.1 In `crates/sluice-core/src/line.rs`, add `Line::refusal_of`; in `crates/sluice-core/src/owed.rs`, have `owed_on` take a refused submission, its words from its outbox item or else the held landing, and `delivery_of` give a refused submission's delivery under `<id>:refused`, which `Submission::notification` in `crates/sluice-core/src/records.rs` names. True when `sluice exec bolt/sluice-is-refactored-and-runs-on-pools -- cargo test -p sluice-conformance --test landing a_held_landing_is_collected_by_its_worktrees_inbox` passes.

## 2. Every document carries the refusal

- [x] 2.1 In `crates/sluice/src/lib.rs`, have `decided_document` read a refused submission's refusal from the branch's held landing as `report` does and add `refused` and the refusal's `notice`, and build `status_json`, the status command's arm for a number, `inbox` and `report`'s document on it. True when `sluice exec bolt/sluice-is-refactored-and-runs-on-pools -- cargo test -p sluice --test cli a_landing_the_forge_refused_is_told_with_its_words` passes.

## 3. The decision recorded

- [x] 3.1 Amend record 0124 with what the build shows and tells, and rule core.5's details with it. True when `sluice exec bolt/sluice-is-refactored-and-runs-on-pools -- moon run root:adrs site:rules` passes.
