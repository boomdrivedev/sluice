# Tasks

Work in this change's place on its branch, cut from main. Run suites
through sluice (`sluice exec main -- cargo test ...`); a quick
`cargo check`, `cargo clippy` or one unit test may run directly. Commit
by pathspec, Conventional Commits, scoped to the part of the tree,
citing clauses by number and rules by id.

## 1. The decision recorded

- [x] 1.1 Write record 0129 with the design agent's ruling of 2026-11-19 and the code stage's choice of git's `--follow-symlinks`, and add a link read as its own text to rule core.9's details. True when `openspec validate --strict` passes on the specs and this change.

## 2. The key check reads a link as moon does

- [x] 2.1 In `crates/sluice/src/profiles/moon.rs`, have `file_at` read `<revision>:<path>` through `git cat-file --batch --follow-symlinks -Z`, taking the content only where git answers with a blob. True when `cargo test -p sluice --lib profiles::moon::tests::tree` passes, and fails with `cat-file blob` put back.
- [x] 2.2 Prove a moon run whose task's input is a tracked link keeps its key. True when `sluice exec main -- cargo test -p sluice --features tools --test moon a_key_whose_input_is_a_link_is_proven` passes.
