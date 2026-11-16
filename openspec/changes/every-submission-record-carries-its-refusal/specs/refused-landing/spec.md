# Spec Delta

## ADDED Requirements

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

## MODIFIED Requirements

### Requirement: A held landing is told to its submitter

A submission whose landing the forge refused SHALL be told to its
submitter as an outcome is told, under `<id>:refused`, with the forge's
words, owed until its reader confirms it (50, 51; rule core.52). While
the submission is refused, `sluice inbox` in its worktree SHALL hand the
refusal over as it hands over an outcome, its words read from the held
landing once the round has pushed its notice, and every document of the
submission SHALL give its `delivery` under that key (135; rule core.31).
Its status SHALL say `held: the forge refused its landing:` with the
forge's words.

#### Scenario: The submitter's notice

- **WHEN** #2021's landing is refused by the forge
- **THEN** a delivery under `2021:refused` is owed to #2021's work,
  carrying the forge's words, and `sluice status 2021` says `held: the
  forge refused its landing:` with them
  (`a_held_landing_is_owed_to_its_submitter` in
  `crates/sluice-conformance/tests/landing.rs`;
  `a_landing_the_forge_refused_is_told_with_its_words` in
  `crates/sluice/tests/cli.rs`)

#### Scenario: Collected by its worktree's inbox

- **WHEN** #1's landing is refused by the forge and the round has pushed
  its notice into its pane, and `sluice inbox` runs in its worktree
- **THEN** the inbox hands over #1's refusal under `1:refused` with the
  forge's words, and `sluice status 1 --json` gives its `delivery` under
  that key (`a_held_landing_is_collected_by_its_worktrees_inbox` in
  `crates/sluice-conformance/tests/landing.rs`)
