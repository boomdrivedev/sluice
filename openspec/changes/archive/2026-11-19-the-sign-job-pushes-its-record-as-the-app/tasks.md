# Tasks

Work in this change's place on its branch, cut from main. Run suites
through sluice (`sluice exec main -- cargo test ...`); a quick
`cargo check`, `cargo clippy` or one unit test may run directly. Commit
by pathspec, Conventional Commits, scoped to the part of the tree,
citing clauses by number and rules by id.

## 1. The decision recorded

- [x] 1.1 Amend record 0019 with the choice of 2026-11-18, and add both failures to rule core.29's details, as the proposal's What Changes says. True when `openspec validate --strict` passes on the specs and this change.

## 2. git sends no stored header

- [x] 2.1 In `crates/sluice/src/profiles/pushed_refs.rs`, have `git_as` empty the headers git's configuration holds for the speaker's address, on the call's command line alone. True when `sluice exec main -- cargo test -p sluice --features tools --test pushed_refs a_header_gits_configuration_holds_is_never_sent_as_the_identity` passes, and fails with the setting taken out, and `--test pull_requests`, whose tests rewrite the forge's address to a copy on the host, passes.

## 3. A refusal names what git sent

- [x] 3.1 In `crates/sluice/src/identity.rs`, have `sluice credential get` mark the file `SLUICE_CREDENTIAL_ASKED` names as it hands git the credentials; in `pushed_refs.rs`, make that file for each try as the identity, and read a refusal on a try it is unmarked as the remote's answer, naming what git sent of its own and never the identity's token. True when `sluice exec main -- cargo test -p sluice --features tools --test pushed_refs a_credential_git_sends_of_its_own_is_named_and_never_blamed_on_the_identity` and `cargo test -p sluice --lib a_credential_git_sends_of_its_own_is_named_by_where_it_is` pass, the first failing with every try read as asked.
