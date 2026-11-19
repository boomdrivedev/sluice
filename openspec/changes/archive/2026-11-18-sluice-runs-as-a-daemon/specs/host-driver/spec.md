## MODIFIED Requirements

### Requirement: sluice run says nothing needs it

`sluice run` SHALL say once on stderr that sluice starts its own host
process, that nothing needs `sluice run`, and that `sluice daemon` keeps
the host process running on a machine that chooses to, and SHALL be left
out of `sluice --help` (135, 137; rule core.31).

#### Scenario: Started by hand

- **WHEN** a person runs `sluice run` in a repository
- **THEN** stderr's first line says sluice starts its own host process,
  that nothing needs `sluice run`, and that `sluice daemon` keeps it
  running on a machine that chooses to, and `sluice --help` names no
  `run`
  (`sluice_run_says_nothing_needs_it` in `crates/sluice/tests/cli.rs`)
