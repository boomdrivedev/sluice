## ADDED Requirements

### Requirement: A project's own settings affect that project's checks

The moon runner SHALL read a changed `moon.yml` of a project as
affecting every task of that project and what depends on them, and no
other task (36). It SHALL read a change to `.moon/`, to the
toolchain's files or to the branch's policy, `.config/sluice.toml`, as
affecting every required check. One reading SHALL serve a
combination's run and the reading that lands work beside the chain
(140).

#### Scenario: A project's moon.yml

- **WHEN** a combination changes `b/moon.yml` alone, and `c:test`
  depends on `b:test`
- **THEN** the run runs `b:test` and `c:test`, and does not run `a:test`

#### Scenario: The workspace's settings

- **WHEN** a combination changes `.moon/workspace.yml`
- **THEN** the run runs every required check

#### Scenario: The branch's policy

- **WHEN** a combination changes `.config/sluice.toml`
- **THEN** the run runs every required check, and the work does not
  land beside the chain

#### Scenario: Work beside the chain

- **WHEN** a submission changes `docs/moon.yml`, a project whose tasks
  no required check names or depends on
- **THEN** it lands beside the chain with nothing run
