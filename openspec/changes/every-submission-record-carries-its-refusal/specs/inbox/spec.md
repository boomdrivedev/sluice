# Spec Delta

## MODIFIED Requirements

### Requirement: The JSON document is a contract that only gains fields

Each element of `outcomes` SHALL hold `id`, `branch`, `state`
(`landed`, `failed`, `conflict` or `refused`), `notice`, `delivery` and
`from`; a landed one `landed_as`; a failed one `result` (`failed`, or
`timed out`) and `failed`, each with its `task` and, where kept, its
`log`; a conflicting one `conflict` with its `paths` and its `with`; one
whose landing the forge refused `refused` with the forge's `words` and
its `at` (152; record 0124). From this change on, a later sluice SHALL
keep these names and meanings, and MAY add fields; `delivered` and
`delivered_to` are gone.

#### Scenario: The JSON document lists the records

- **WHEN** `sluice inbox --json` hands over submission 12's landing on
  `main` as `abc1234`
- **THEN** it prints one document whose `outcomes` holds one element
  with `id` 12, `branch` `main`, `state` `landed`, `landed_as`
  `abc1234`, a `delivery` whose `key` is `12:landed` and whose `handed`
  ends with a hand-over by `inbox`, a `notice` naming `#12` and
  `landed`, and no `delivered`

#### Scenario: A conflict names its paths

- **WHEN** `sluice inbox --json` hands over submission 14's conflict
  with submission 11 in `src/lib.rs`
- **THEN** its element holds `state` `conflict`, `conflict.paths`
  holding `src/lib.rs`, and `conflict.with` holding submission 11

#### Scenario: A landing the forge refused gives its words

- **WHEN** `sluice inbox --json` hands over the refusal of submission
  1's landing on `main`
- **THEN** its element holds `state` `refused`, `refused` with the
  forge's `words` and its `at`, a `notice` naming `#1` and those words,
  and a `delivery` whose `key` is `1:refused`
  (`a_landing_the_forge_refused_is_told_with_its_words` in
  `crates/sluice/tests/cli.rs`)
