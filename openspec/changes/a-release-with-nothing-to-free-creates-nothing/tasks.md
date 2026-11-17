# Tasks

Work in this change's place on its branch, cut from main. Run suites
through sluice (`sluice exec main -- cargo test ...`); a quick
`cargo check`, `cargo clippy` or one unit test may run directly. Commit
by pathspec, Conventional Commits, scoped to the part of the tree,
citing clauses by number and rules by id.

## 1. The decision recorded

- [x] 1.1 Amend record 0120 with D8 and D9, and add the case to rule core.10's details, as the proposal's What Changes says. True when `openspec validate --strict` passes on the specs and this change.

## 2. A release with nothing to free makes nothing

- [x] 2.1 In `crates/sluice/src/profiles/checks.rs`, add `names_release`; in `crates/sluice/src/profiles/places.rs`, add `ReleaseNamed`, take it beside the builder in `Places::with_release`, and have `release_unheld` read it before the records lock, a record or a scratch; bind both in `crates/sluice/src/profiles/mod.rs`. True when `sluice exec main -- cargo test -p sluice --features tools --test exec an_unheld_release_with_nothing_to_free_makes_nothing` passes, and fails with the reading taken out.
