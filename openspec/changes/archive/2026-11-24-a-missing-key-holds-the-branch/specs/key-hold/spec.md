## Purpose

A branch held on a check whose key names a file the combination's tree does not hold: no submission is blamed and nothing is recorded under the key. The held submissions stay queued, work not yet tried is tested, and the hold ends at the first revision of the branch that changes the check's inputs.

## ADDED Requirements

### Requirement: A key naming a file the tree does not hold blames no one and holds the branch

Where a check's key names a file the combination's tree does not hold,
nothing SHALL be recorded under the key and no submission SHALL be
blamed. The branch SHALL hold on the check, the file named, its records
keeping the hold as `key_held` and its log one `key_held` line. The
combination's submissions SHALL stay queued, waiting, and SHALL NOT be
run again while it holds (C.6, 17, 20).

#### Scenario: A file the tree does not hold

- **WHEN** a signed run of #1 reports `a:test` under a key that names
  `a/settings`, which the tree does not hold
- **THEN** #1 is waiting and blamed for nothing, no result is recorded
  under `a:test`, and the branch holds on `a:test` and `a/settings`
  with #1, logged once
  (`a_key_naming_a_file_the_tree_lacks_blames_no_one_and_holds_the_branch`
  in `crates/sluice-conformance/tests/key_hold.rs`)

#### Scenario: Rounds while it holds

- **WHEN** five rounds run while the branch holds on #1
- **THEN** no run starts and the hold is logged once
  (`a_held_entry_is_not_run_again_while_the_branch_holds` in
  `crates/sluice-conformance/tests/key_hold.rs`)

#### Scenario: The check profile names the file

- **WHEN** a moon run's checkout holds an input other than the tree does
- **THEN** `proven` refuses the key with the file and the words
  `alpha:test: alpha/notes.md is not what the tree holds`
  (`proven_names_the_file_a_key_names_that_the_tree_does_not_hold` in
  `crates/sluice/tests/profiles/moon.rs`;
  `a_file_the_tree_does_not_hold_is_named` in
  `crates/sluice/src/profiles/moon.rs`)

### Requirement: The hold ends at a revision that changes the check's inputs

The hold SHALL end, logged once as `key_hold_ended`, at the first
revision of the branch whose change from the revision the hold was read
on the check's affected reading says the check reads, or where the
reading fails. A revision that does not change the check's inputs SHALL
leave it standing. Once it ends, its submissions SHALL be formed again
on the branch (10, 140).

#### Scenario: A move that does not change its inputs

- **WHEN** the branch moves by a change to `c/lib`, which `a:test` does
  not read
- **THEN** the hold stands and nothing is formed
  (`a_move_that_does_not_change_the_checks_inputs_keeps_the_hold` in
  `crates/sluice-conformance/tests/key_hold.rs`)

#### Scenario: A revision that changes its inputs

- **WHEN** the branch moves to a revision that adds `a/settings`
- **THEN** the hold ends at that revision, and #1 is tested and lands
  (`a_revision_that_changes_the_checks_inputs_takes_the_entries_again`
  in `crates/sluice-conformance/tests/key_hold.rs`)

### Requirement: Work not yet tried is tested while the branch holds

While the branch holds, a submission that was in no combination refused
so SHALL be formed and tested as any is, so work that changes the
check's inputs lands and ends the hold. One refused alike SHALL join the
hold, blamed for nothing (6, 10).

#### Scenario: Work that changes its inputs

- **WHEN** #2, which adds `a/settings`, is submitted while the branch
  holds on #1
- **THEN** #2 is tested and lands, the hold ends, and #1 lands after it
  (`an_entry_that_changes_the_checks_inputs_lands_and_ends_the_hold` in
  `crates/sluice-conformance/tests/key_hold.rs`)

#### Scenario: Work refused alike

- **WHEN** #2, which changes `b/lib`, is submitted while the branch
  holds on #1
- **THEN** #2 is tested once, blamed for nothing, and joins the hold
  (`an_entry_refused_alike_joins_the_hold` in
  `crates/sluice-conformance/tests/key_hold.rs`)

### Requirement: A held submission stays its submitter's

A submission the hold holds SHALL be withdrawn as any queued one is,
leaving the hold; the hold SHALL end, logged with no revision, once no
submission it holds is left (5, 67).

#### Scenario: Its entries are their submitters'

- **WHEN** the operator withdraws #1, the one submission the hold holds
- **THEN** #1 is withdrawn and the hold ends
  (`a_held_entry_is_withdrawn_and_the_hold_ends_with_its_last` in
  `crates/sluice-conformance/tests/key_hold.rs`)

### Requirement: The branch's status says the hold

`sluice status <branch>` SHALL say a hold with the check, the file, the
submissions it holds and that they are tested again once a revision of
the branch changes the check's inputs, and its `--json` SHALL give the
hold as `key_held` (26).

#### Scenario: The branch's status

- **WHEN** `main` holds on `root:format`, whose key names
  `.claude/settings.json`, with #5 and #6
- **THEN** its status says `held: root:format's key names
  .claude/settings.json, which the tree of main does not hold, so nobody
  is blamed and #5 and #6 wait; they are tested again once a revision of
  main changes root:format's inputs`, with the check profile's words
  (`a_hold_on_a_key_is_said_with_its_check_file_and_entries` in
  `crates/sluice/src/words.rs`;
  `a_key_naming_a_file_the_tree_lacks_blames_no_one_and_holds_the_branch`
  in `crates/sluice-conformance/tests/key_hold.rs`)
