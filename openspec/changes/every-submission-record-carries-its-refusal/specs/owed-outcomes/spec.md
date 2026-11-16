# Spec Delta

## MODIFIED Requirements

### Requirement: A submission with no reader gives no delivery

A submission by name, a withdrawn one and one not decided SHALL give no
`delivery` in any of those documents, but for one whose landing the
forge refused, which gives its refusal's (124, 152; rule core.50;
record 0124 as amended 2026-11-16).

#### Scenario: A queued submission

- **WHEN** submission 15 is queued on `main`, and `sluice status 15
  --json` runs
- **THEN** its document holds no `delivery`

#### Scenario: A landing the forge refused

- **WHEN** the forge refused submission 1's landing on `main`, and
  `sluice status 1 --json` runs
- **THEN** its `delivery` holds `key` `1:refused`
  (`a_landing_the_forge_refused_is_told_with_its_words` in
  `crates/sluice/tests/cli.rs`)
