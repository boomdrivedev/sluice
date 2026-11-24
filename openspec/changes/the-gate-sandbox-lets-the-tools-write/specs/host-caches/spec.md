## ADDED Requirements

### Requirement: A sandboxed moon run may write where moon keeps its task archives

Under the sandbox, a run whose runner is moon SHALL be able to write the
folder where moon keeps the run's task archives, its checkout's
`.moon/cache/outputs`, where a link there leads. One in the checkout
SHALL add nothing to the sandbox it is given (C.2 sandbox row; record
0105; rule core.34).

#### Scenario: moon's archives through a link

- **WHEN** a sandboxed moon run's checkout links `.moon/cache/outputs`
  to a folder in the repository's git common directory
- **THEN** its sandbox opens that folder where the link resolves, and
  no folder above it
  (`the_profile_opens_moons_archives_where_their_link_leads` in
  `crates/sluice/src/profiles/checks.rs`)

#### Scenario: moon's archives in the checkout

- **WHEN** a sandboxed moon run's `.moon/cache/outputs` is a folder of
  its checkout
- **THEN** its sandbox is the one it is given with no archives
  (`the_profile_opens_moons_archives_where_their_link_leads`)
