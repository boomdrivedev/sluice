# Tasks

Work in this change's place on its branch, cut from `main`, the base
every check below runs against. Run suites through sluice, as
CLAUDE.md's Running checks has it. Commit by pathspec, Conventional
Commits, scoped to the part of the tree.

## 1. The check profile names the file

- [x] 1.1 `Refused` in `crates/sluice-seams` carries `file`, the file the key names that the tree does not hold (design D1). In `crates/sluice/src/profiles/moon.rs`, `check_key` fails with `NotHeld` for an input the tree lacks or holds with other content. True when `sluice exec main -- cargo test -p sluice --lib a_file_the_tree_does_not_hold_is_named` passes.
- [x] 1.2 `proven` passes the file on. True when `sluice exec main -- cargo test -p sluice --features tools --test moon proven_names_the_file_a_key_names_that_the_tree_does_not_hold` and `... --test moon proven_drops_a_key_the_tree_does_not_bear_out` pass.
- [x] 1.3 The fakes' `lacks` names a file a task's key names, refused where the run's tree does not hold it, and `forges` names none. True when `sluice exec main -- cargo test -p sluice-fakes a_key_naming_a_file_the_tree_lacks_is_refused_naming_it` passes.

## 2. The branch holds

- [x] 2.1 `finish_run` records nothing under such a key and leaves the check with no outcome; `judge` reads a combination the hold holds `not passed` with no follow-up; `branch.json` keeps `key_held` and the log one `key_held` line (design D2, D3). True when `sluice exec main -- cargo test -p sluice-conformance --test key_hold a_key_naming_a_file_the_tree_lacks_blames_no_one_and_holds_the_branch` and `... a_held_entry_is_not_run_again_while_the_branch_holds` pass.
- [x] 2.2 The held entries leave the chain; an entry not yet tried is tested, and one refused alike joins the hold (design D4). True when `sluice exec main -- cargo test -p sluice-conformance --test key_hold an_entry_that_changes_the_checks_inputs_lands_and_ends_the_hold` and `... an_entry_refused_alike_joins_the_hold` pass.
- [x] 2.3 Each round weighs the hold against the branch's revision by the affected reading, and ends it once no entry it holds is left (design D5). True when `sluice exec main -- cargo test -p sluice-conformance --test key_hold` passes.
- [x] 2.4 A key refused for another cause is blamed as before. True when `sluice exec main -- cargo test -p sluice-conformance --test unkept_proof` and `... --test key_hold a_forged_key_holds_nothing` pass.

## 3. The status says it

- [x] 3.1 `sluice status <branch>` says the hold and `--json` gives `key_held` (design D6). True when `sluice exec main -- cargo test -p sluice --lib a_hold_on_a_key_is_said_with_its_check_file_and_entries` passes.
