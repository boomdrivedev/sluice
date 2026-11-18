# Spec Delta

## MODIFIED Requirements

### Requirement: The document is the submission's record

Under `--json`, `sluice wait` and a waiting `sluice submit` SHALL print
one document on stdout however they end, decided, out of time or
stopped with Ctrl-C: the submission's record, as `sluice status <n>
--json` prints it. Its `state` SHALL be one of `queued`, `waiting`,
`testing`, `passed`, `landing`, `landed`, `failed`, `conflict`,
`withdrawn` and `refused` (152), and under `sluice/submission@1` none of
these SHALL be renamed or removed and none added.

#### Scenario: Stopped with Ctrl-C

- **WHEN** `sluice wait <n> --json` is stopped with Ctrl-C while the
  submission is queued
- **THEN** stdout holds one document, the record with `state`
  `queued`, and it exits 130

#### Scenario: A landing the forge refused gives its notice

- **WHEN** the forge refuses submission 1's landing, and `sluice wait 1
  --json` runs
- **THEN** it exits 6, its document holding `state` `refused`, `refused`
  with the forge's `words` and its `at`, and a `notice` naming `#1` and
  those words (`a_landing_the_forge_refused_is_told_with_its_words` in
  `crates/sluice/tests/cli.rs`)
