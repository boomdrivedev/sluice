# Tasks

Work in this change's place on its branch, cut from main. Run suites
through sluice (`sluice exec main -- cargo test ...`); a quick
`cargo check`, `cargo clippy` or one unit test may run directly. Commit
by pathspec, Conventional Commits, scoped to the part of the tree,
citing clauses by number and rules by id.

## 1. The decision recorded

- [x] 1.1 Amend record 0119 with the choice of 2026-11-18, and add routing's reading read for the pools to rule core.27's details, as the proposal's What Changes says. True when `openspec validate --strict` passes on the specs and this change.

## 2. forge check reads the pool from the policy read whole

- [x] 2.1 In `crates/sluice/src/profiles/mod.rs`, have `forge_findings` read the pools through `whole_policy`, the policy read whole at the branch's revision as the host holds it, or at main's for a branch not yet made, naming a policy it cannot read whole as a check that failed. True when `sluice exec main -- cargo test -p sluice --features tools --test forge forge_check_reads_the_pool_the_policy_names` passes, and fails with routing's reading put back.
