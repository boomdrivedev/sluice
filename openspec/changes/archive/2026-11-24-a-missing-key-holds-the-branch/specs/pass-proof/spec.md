## MODIFIED Requirements

### Requirement: Only a pass with no proof is read so

A manifest the combination's tree does not bear out, a test that
failed, a replay and a task a check was failed by SHALL be read as
before, whether their manifests were kept or not (108, 139, 149, C.6).

#### Scenario: A key the tree does not bear out

- **WHEN** a run on `main`, whose state is on `origin`, passes
  `a:test` under a key whose manifest does not hash to it
- **THEN** `a:test` is read as no pass under its key, as before, and
  #1 is blamed for it
  (`a_key_the_tree_does_not_bear_out_is_still_failed` in
  `crates/sluice-conformance/tests/unkept_proof.rs`)

#### Scenario: A failed test whose manifest is gone

- **WHEN** a moon run fails `gamma:test` and its manifest is not in the
  store
- **THEN** the check profile names `gamma:test` neither proven nor among
  the passes with no proof
  (`a_failed_task_whose_manifest_is_gone_is_not_unkept` in
  `crates/sluice/tests/profiles/moon.rs`)

### Requirement: A key the tree does not bear out is blamed with why

Where a check's key is not borne out by the combination's tree for any
reason but a missing manifest or a file the tree does not hold, the
submission blamed for it SHALL name the check failed under that key
with the check profile's words for why, as `unsigned` in its record. A
waiting submit and `sluice wait` SHALL say, under the failed line,
`<task>: the combination's tree does not bear its key out: <why>`
(C.6, 20).

#### Scenario: A forged key

- **WHEN** a signed run passes `a:test` under a key whose manifest does
  not hash to it
- **THEN** the submission is blamed on `a:test` under its key, and its
  record says why under `unsigned`, and the branch holds on nothing
  (`a_key_the_tree_does_not_bear_out_is_blamed_with_why` in
  `crates/sluice-conformance/tests/unkept_proof.rs`;
  `a_forged_key_holds_nothing` in
  `crates/sluice-conformance/tests/key_hold.rs`)

#### Scenario: The words

- **WHEN** `a:test` is blamed with the why `a:test: the manifest under
  its key does not hash to it`
- **THEN** the line reads `a:test: the combination's tree does not bear
  its key out: a:test: the manifest under its key does not hash to it`
  (`a_key_not_borne_out_is_said_with_why` in
  `crates/sluice/src/words.rs`)

#### Scenario: The check profile says why

- **WHEN** a moon run passes `alpha:test` under a key whose manifest
  was changed after the run
- **THEN** `proven` leaves `alpha:test` out and answers it among the
  refused with its key, the words `check_key` gave and no file
  (`proven_drops_a_key_the_tree_does_not_bear_out` in
  `crates/sluice/tests/profiles/moon.rs`)
