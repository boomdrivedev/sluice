# Spec Delta

## MODIFIED Requirements

### Requirement: A decided submission's document gives its delivery

`sluice status <n> --json`, `sluice wait --json`, a waiting `sluice
submit --json`, and each element of `sluice inbox --json` SHALL give a
landed, failed or conflicting submission's `delivery`, and that of the
refusal of a submission whose landing the forge refused, under
`<id>:refused` (152; record 0124): `key`, `reader`, `state` (`owed`,
`expired` or `done`), `recorded_at`, `expires_at`, `handed` with each
hand-over's `by` and `at`, `confirmed` (a time or `null`) and `known`;
one let go of as `key` and `state` `done` alone. None of them SHALL hold
`delivered` or `delivered_to`.

#### Scenario: A landing pushed and not confirmed

- **WHEN** submission 12 landed on `main` and the round pushed its
  notice into its pane, and `sluice status 12 --json` runs
- **THEN** its `delivery` holds `key` `12:landed`, `reader` `unit-a`,
  `state` `owed`, `handed` with `by` `notice`, and `confirmed` `null`,
  and the document holds no `delivered`

#### Scenario: A landing confirmed and let go of

- **WHEN** `unit-a` confirmed `12:landed`, and 12 has moved to the
  finished state
- **THEN** `sluice status 12 --json` gives `delivery` as `key`
  `12:landed` and `state` `done` alone

#### Scenario: A landing the forge refused

- **WHEN** the forge refused submission 1's landing on `main`, and
  `sluice status 1 --json` runs
- **THEN** its `delivery` holds `key` `1:refused`
  (`a_landing_the_forge_refused_is_told_with_its_words` in
  `crates/sluice/tests/cli.rs`)

### Requirement: A submission with no reader gives no delivery

A submission by name, a withdrawn one and one neither decided nor held
by the forge's refusal of its landing SHALL give no `delivery` in any of
those documents (124; rule core.50).

#### Scenario: A queued submission

- **WHEN** submission 15 is queued on `main`, and `sluice status 15
  --json` runs
- **THEN** its document holds no `delivery`
