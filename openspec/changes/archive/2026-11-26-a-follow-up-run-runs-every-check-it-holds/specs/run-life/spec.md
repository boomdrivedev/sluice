## ADDED Requirements

### Requirement: A follow-up run runs every check it holds

A follow-up run SHALL run each check that it holds. It SHALL NOT narrow
those checks again to what its combination changed since the branch's
revision. The run that it follows chose them, and the branch can move
under the combination after that choice (17, 36, 70, record 0122). A run of the gate's checks SHALL still run only the checks that
its combination affects.

#### Scenario: The work ahead lands before the follow-up starts

- **WHEN** `main + #1 + #2` replays `a:test` unproven, #1 changed `a/`
  and #2 changed only `c/`, and #1 lands on its own combination before
  the follow-up run that holds `a:test` starts
- **THEN** the follow-up runs `a:test` with no replay, and once it
  passes, #2 lands
  (`a_follow_up_run_runs_its_checks_after_the_work_ahead_lands` in
  `crates/sluice-conformance/tests/run_life.rs`)
