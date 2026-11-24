## MODIFIED Requirements

### Requirement: A way on never sends its reader elsewhere

No way-on line SHALL ask its reader to go into a worktree or to run a command from inside one. No way-on line SHALL ask its reader to pass a command to a person.

#### Scenario: Read from the main checkout

- **WHEN** a session in the repository's main checkout runs `sluice wait 12`, and #12, from `unit/a`, is blamed
- **THEN** the report names the agent working in `unit/a` as the one who fixes it and submits again, and says nothing of running a command inside `unit/a`

#### Scenario: The operator's step

- **WHEN** a blamed submission's failed check is read as flaky against a fail recorded earlier
- **THEN** the report names that fail and when it was recorded
- **AND** it names no step of the operator's and no command that sets a result aside
