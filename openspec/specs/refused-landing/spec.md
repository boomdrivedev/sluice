# refused-landing Specification

## Purpose
A landing the forge refuses: told apart from a branch found elsewhere, held with the forge's words for its submitters to withdraw or replace, told to them, and pushed again only on the operator's word (record 0124).

## Requirements

### Requirement: A landing the forge refuses is told from a branch found elsewhere

A landing's push that the remote refuses SHALL be read by reading the
branch after the refusal: where the branch is still at the revision the
landing expected, the forge refused the landing itself, whatever words
it gave; where it is elsewhere, the landing found the branch elsewhere
and the combination forms again (24); where it holds the landing, the
landing landed (122). A push refused at the ref with its lease stale
SHALL read as before (152; record 0124; rule core.5).

#### Scenario: Push protection

- **WHEN** a landing's push is answered `! [remote rejected] main ->
  main (push declined due to repository rule violations)` and `main`,
  read after it, is still at the revision the landing expected
- **THEN** the push reads as refused by the forge, with those words
  whole (`a_push_the_forge_declines_reads_as_refused_by_the_forge` in
  `crates/sluice/tests/pushed_refs.rs`)

#### Scenario: A branch moved meanwhile

- **WHEN** the same refusal comes and `main`, read after it, is at a
  revision another host landed
- **THEN** the landing reads as having found `main` elsewhere, and the
  combination forms again on the revision found
  (`a_refused_push_with_the_branch_moved_reads_as_elsewhere` in
  `crates/sluice/tests/pushed_refs.rs`)

### Requirement: A landing the forge refused is held with its words, never pushed again by itself

A landing refused by the forge SHALL be written refused in the branch's
records, with the forge's words whole, before anything else acts on the
branch. The refusal SHALL be logged once, never once per round, and
shown on the branch and on each submission the landing carries. No round
SHALL push it again, nothing SHALL be formed again for it, and the
submissions behind it SHALL wait. Nobody SHALL be blamed for it, and the
refusal SHALL decide no submission's outcome (152; record 0124).

#### Scenario: Refused every round

- **WHEN** the forge refuses #2021's landing on `bolt/x` for a test key
  it reads as a secret, and three rounds run after it
- **THEN** the landing was pushed once, `log.jsonl` holds one
  `forge_refused` line with the forge's words, and `sluice status
  bolt/x` shows the landing held with them
  (`a_landing_the_forge_refused_is_pushed_once_and_held` in
  `crates/sluice-conformance/tests/landing.rs`)

#### Scenario: The submissions behind it

- **WHEN** #2022 is queued on `bolt/x` behind #2021's held landing
- **THEN** nothing is formed for #2022 while the landing is held, and its
  status says it waits behind the landing the forge refused
  (`the_submissions_behind_a_held_landing_wait` in
  `crates/sluice-conformance/tests/landing.rs`;
  `a_landing_the_forge_refused_is_told_with_its_words` in
  `crates/sluice/tests/cli.rs`)

### Requirement: A held landing's submissions are their submitters'

A submission of a landing refused by the forge SHALL no longer be
landing. Its submitter, its planner or the operator SHALL be able to
withdraw it, and a submission from its place SHALL withdraw it as it
withdraws any earlier submission of that place (5, 67). Once any of the
landing's submissions leaves, the landing SHALL be dropped in the write
that takes it out, and those that remain SHALL go back to the queue and
be formed again on the branch (24; rule core.48).

#### Scenario: The fixed head from the same place

- **WHEN** #2021's landing is held, and its place submits #2022 with the
  test key removed
- **THEN** #2021 is withdrawn, the landing is dropped, and #2022 is
  formed and tested on `bolt/x`
  (`a_submission_from_its_place_clears_a_held_landing` in
  `crates/sluice-conformance/tests/landing.rs`)

#### Scenario: One of two leaves

- **WHEN** a held landing carries #5 and #6, and #5 is withdrawn
- **THEN** the landing is dropped and #6 is queued again and formed on
  the branch (`a_held_landing_is_dropped_once_one_of_its_submissions_leaves`
  in `crates/sluice-conformance/tests/landing.rs`)

#### Scenario: A push after a withdrawal

- **WHEN** a held landing carries #5 and #6, #5 is withdrawn, and the
  operator runs `sluice push` before the next round
- **THEN** nothing is pushed, #5 stays withdrawn, and #6 is formed on the
  branch and lands (`a_push_after_a_withdrawal_lands_nothing_withdrawn`
  and `a_push_drops_a_held_landing_one_of_whose_submissions_left` in
  `crates/sluice-conformance/tests/landing.rs`)

### Requirement: A held landing is told to its submitter

A submission whose landing the forge refused SHALL be told to its
submitter as an outcome is told, under `<id>:refused`, with the forge's
words, owed until its reader confirms it (50, 51; rule core.52). Its
status SHALL say `held: the forge refused its landing:` with the forge's
words.

#### Scenario: The submitter's notice

- **WHEN** #2021's landing is refused by the forge
- **THEN** a delivery under `2021:refused` is owed to #2021's work,
  carrying the forge's words, and `sluice status 2021` says `held: the
  forge refused its landing:` with them
  (`a_held_landing_is_owed_to_its_submitter` in
  `crates/sluice-conformance/tests/landing.rs`;
  `a_landing_the_forge_refused_is_told_with_its_words` in
  `crates/sluice/tests/cli.rs`)

### Requirement: The operator pushes a held landing again

`sluice push <branch>` SHALL push the branch's held landing again once,
on the operator's word, as the same landing: where the forge takes it,
its submissions land (122); where the forge refuses it again, it stays
held with the new words. On a branch with no held landing it SHALL say
so and push nothing, and a held landing one of whose submissions has
left the queue it SHALL drop, pushing nothing (24, 152; record 0124;
rule core.48).

#### Scenario: The rule changed

- **WHEN** #2021's landing is held, the forge's rule has changed, and the
  operator runs `sluice push bolt/x`
- **THEN** the landing is pushed once and #2021 lands as the same
  landing, with no run (`push_lands_a_held_landing_once_the_forge_takes_it`
  in `crates/sluice/tests/remote_branch.rs`)

#### Scenario: Refused again

- **WHEN** the operator runs `sluice push bolt/x` and the forge refuses
  it again
- **THEN** it exits non-zero with the forge's words, and the landing
  stays held with them
  (`push_refused_again_leaves_the_landing_held` in
  `crates/sluice/tests/remote_branch.rs`)

### Requirement: Every document of a held submission carries its refusal

Every JSON document that gives a submission whose landing the forge
refused, a status's, an inbox's or a wait's, SHALL give its record with
`refused`, the forge's `words` and when (`at`), read from the branch's
held landing, and `notice`, the refusal's words as the round pushes
them. The refusal SHALL still decide no outcome (152; record 0124 as
amended 2026-11-16).

#### Scenario: The status's, the inbox's and the wait's

- **WHEN** the forge refuses #1's landing on `main`, and #2 is queued
  behind it
- **THEN** `sluice wait 1 --json`, `sluice status 1 --json`, #1's
  element of `sluice status main --all --json` and #1's element of
  `sluice inbox --json` in its worktree each hold `state` `refused`,
  `refused` with the forge's `words` and its `at`, and a `notice`
  naming `#1` and the forge's words; `sluice status 2 --json` holds
  neither `refused` nor `notice`
  (`a_landing_the_forge_refused_is_told_with_its_words` in
  `crates/sluice/tests/cli.rs`)

### Requirement: The inbox hands a held landing's refusal over

While a submission is refused, `sluice inbox` in its worktree SHALL hand
over its refusal owed under `<id>:refused` as it hands over an outcome,
its words read from the held landing once the round has pushed its
notice, and every document of the submission SHALL give its `delivery`
under that key (50, 51, 135, 152; rules core.31, core.52; record 0124
as amended 2026-11-16).

#### Scenario: Collected by its worktree's inbox

- **WHEN** #1's landing is refused by the forge and the round has pushed
  its notice into its pane, and `sluice inbox` runs in its worktree
- **THEN** the inbox hands over #1's refusal under `1:refused` with the
  forge's words, and `sluice status 1 --json` gives its `delivery` under
  that key (`a_held_landing_is_collected_by_its_worktrees_inbox` in
  `crates/sluice-conformance/tests/landing.rs`;
  `a_landing_the_forge_refused_is_told_with_its_words` in
  `crates/sluice/tests/cli.rs`)
