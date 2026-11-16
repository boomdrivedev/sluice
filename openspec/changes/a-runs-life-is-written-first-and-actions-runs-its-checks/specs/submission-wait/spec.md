## MODIFIED Requirements

### Requirement: The exits tell the outcomes apart

A waiting `sluice submit` and `sluice wait` SHALL exit 0 when the
submission landed, 1 when it failed or its pull request was closed, 2
on a conflict, 4 when it was withdrawn and no work of it goes on, 5 when
`--timeout` ran out, 6 when its landing is held because the forge
refused it, saying `the forge refused its landing:` with the forge's
words, and 130 when stopped with Ctrl-C. A submission that does not
exist SHALL exit 1 with the not-found error document under `--json`.

#### Scenario: Withdrawn while waited on

- **WHEN** submission `<n>` is withdrawn by the operator while `sluice
  wait <n>` waits, and no later submission and no pull request carries
  its head
- **THEN** it exits 4, and a waiting `sluice submit` withdrawn the same
  way exits 4

#### Scenario: Blamed

- **WHEN** submission `<n>` is blamed while `sluice wait <n>` waits
- **THEN** it exits 1 and names the failed tasks as a waiting submit
  does

#### Scenario: No such submission

- **WHEN** `sluice wait 9999 --json` runs and there is no submission
  9999
- **THEN** it prints `{"error":{"kind":"not_found",...}}` and exits 1

#### Scenario: A landing the forge refused

- **WHEN** the forge refuses submission `<n>`'s landing while `sluice
  wait <n>` waits
- **THEN** it exits 6 at once, printing `the forge refused its landing:`
  with the forge's words, and its document is the submission's record
  with the landing's refusal, `refused` holding the forge's `words` and
  when (`at`); a waiting `sluice submit` exits 6 the same way
  (`a_wait_on_a_landing_the_forge_refused_exits_6` in
  `crates/sluice/tests/remote_branch.rs`;
  `a_landing_the_forge_refused_is_told_with_its_words` in
  `crates/sluice/tests/cli.rs`)
