## MODIFIED Requirements

### Requirement: A press only reads

Pressing any row of the pane SHALL run nothing but reads: `sluice status
<id> --branch <branch> --json` for a blamed submission's failed tests,
`<branch>` the branch whose RECENT row was pressed, and `tail` for a
log's last lines. Rows drawn from the overview or the forecast SHALL run
nothing at all, and no press SHALL withdraw or release anything.

Before: no press set a result aside, a command sluice had. Now: sluice
has no such command (the change `sluice-host-makes-each-change`, design
D4).

#### Scenario: Every row pressed

- **WHEN** the user presses each combination, CHECKING row, conflict,
  forecast conflict, blame and `log of` row in turn
- **THEN** the only commands run are `sluice status` and `tail`

#### Scenario: A blame whose number another branch holds

- **WHEN** #9 was blamed on `main`, `bolt/a` holds a #9 too, and the
  user presses `main`'s blamed row
- **THEN** the board runs `sluice status 9 --branch main --json` and
  opens `main`'s #9's failed tests
