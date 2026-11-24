## MODIFIED Requirements

### Requirement: A change to the runner's own settings runs every check

Where a worktree's own changes include a setting every check reads,
`.moon/`, the toolchain's files or the branch's policy, `sluice exec`
SHALL run every required check. A project's own `moon.yml` SHALL
affect that project's checks and what depends on them, as a
combination's run reads it. So SHALL a worktree whose head shares no
history with the branch run every check (70). Under the script runner
every check SHALL run (C.2).

#### Scenario: Its own moon.yml

- **WHEN** a worktree's own commit changes `b/moon.yml`
- **THEN** `sluice exec bolt/x` runs `b:test` and does not run `a:test`

#### Scenario: The toolchain

- **WHEN** a worktree's own commit changes `devenv.nix`
- **THEN** `sluice exec bolt/x` runs `a:test` and `b:test`

#### Scenario: No history in common

- **WHEN** a worktree's head shares no commit with `bolt/x`
- **THEN** `sluice exec bolt/x` says `running every required check: the
  worktree's head shares no history with bolt/x` and runs `a:test` and
  `b:test`
