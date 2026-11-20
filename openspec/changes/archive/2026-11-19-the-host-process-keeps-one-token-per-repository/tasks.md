# Tasks

Work in this change's place on its branch, cut from main. Run suites
through sluice (`sluice exec main -- cargo test ...`); a quick
`cargo check`, `cargo clippy` or one unit test may run directly. Commit
by pathspec, Conventional Commits, scoped to the part of the tree,
citing clauses by number and rules by id.

## 1. The decision recorded

- [x] 1.1 Amend record 0108 with boomdrive-design's ruling of 2026-11-19 and the code stage's choices, and add a host process binding that holds tokens of its own to rule core.29's details. True when `openspec validate --strict` passes on the specs and this change.

## 2. One token for each repository

- [x] 2.1 In `crates/sluice/src/profiles/github/app.rs`, hold an app's tokens in `Tokens`, a store shared by clones, keyed by the app's id and the repository, each token got under its repository's own lock, and `App::holding` to share one. True when `sluice exec main -- cargo test -p sluice --test forge apps_holding_one_store_get_a_repositorys_token_once` passes.
- [x] 2.2 Give a renewal the token GitHub refused, through `App::issue` and `Identity::renew`, and take the token held where it is another. True when `sluice exec main -- cargo test -p sluice --test forge a_token_got_afresh_is_the_one_another_refused_takes` passes, and `--test pushed_refs` passes.
- [x] 2.3 Make one store as the host process starts and hand it to every binding it makes through `Bound::open_holding`. True when `sluice exec main -- cargo test -p sluice --test pull_requests the_host_process_buys_a_repositorys_token_once_across_its_looks` passes, and fails with the look's binding given a store of its own.
