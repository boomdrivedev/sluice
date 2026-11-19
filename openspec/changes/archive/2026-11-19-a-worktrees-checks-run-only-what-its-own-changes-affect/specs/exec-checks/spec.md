## ADDED Requirements

### Requirement: A worktree's checks are those its own changes affect

`sluice exec <branch>` SHALL run the branch's required checks that the
worktree's own changes affect, with what depends on them (36). Its own
changes SHALL be the files that differ between the merge base of its
head and the branch's revision and the worktree as it stands. Every path
it holds beyond its head SHALL count among them, and so SHALL an
untracked file git does not ignore. The branch's commits since that
merge base SHALL never count as the worktree's.

#### Scenario: The branch moved on

- **WHEN** a worktree cut from `bolt/x` committed a change to `b/lib`
  alone, and `bolt/x` has since landed changes to `a/lib` and
  `a/moon.yml`
- **THEN** `sluice exec bolt/x` runs `b:test` and does not run `a:test`

#### Scenario: An edit not committed

- **WHEN** a worktree whose head is `bolt/x`'s revision holds an
  unstaged change to `a/lib`
- **THEN** `sluice exec bolt/x` runs `a:test`, does not run `b:test`,
  and says `not recorded: worktree is not clean`

#### Scenario: A file not yet tracked

- **WHEN** a worktree whose head is `bolt/x`'s revision holds an
  untracked file `b/new` that git does not ignore
- **THEN** `sluice exec bolt/x` runs `b:test` and does not run `a:test`

#### Scenario: Nothing of its own

- **WHEN** a worktree holds exactly `bolt/x`'s revision
- **THEN** `sluice exec bolt/x` runs no check and exits 0

### Requirement: A change to the runner's own settings runs every check

Where a worktree's own changes include moon's own configuration,
`.moon/` or a `moon.yml`, `sluice exec` SHALL run every required check,
as a combination's run does. So SHALL a worktree whose head shares no
history with the branch (70). Under the script runner, which reads every
check as affected, `sluice exec` SHALL run every required check, as it
does today (C.2).

#### Scenario: Its own moon.yml

- **WHEN** a worktree's own commit changes `b/moon.yml`
- **THEN** `sluice exec bolt/x` runs `a:test` and `b:test`

#### Scenario: No history in common

- **WHEN** a worktree's head shares no commit with `bolt/x`
- **THEN** `sluice exec bolt/x` says `running every required check: the
  worktree's head shares no history with bolt/x` and runs `a:test` and
  `b:test`

### Requirement: A check sluice already passed on the same files is replayed

A worktree's run of the checks SHALL let the runner answer a check from
its cache. A replay SHALL stand only where the branch's records hold a
pass its gate counts for the check's key, held to the run's limits,
grant and system, as a combination's replay is (108, 138). A key with a
counted fail too SHALL read as the branch's `flaky` says. A replay SHALL
never be recorded as a run (16; rule core.9).

#### Scenario: The same files again

- **WHEN** a clean worktree's `sluice exec bolt/x` passed `a:test` and
  `b:test` and recorded both, and it runs `sluice exec bolt/x` again
  with nothing changed
- **THEN** both checks are replayed and none runs, it says `passed: 2
  tasks (2 replayed)`, and no result is added to the records

#### Scenario: After a fix

- **WHEN** a clean worktree's run recorded a pass of `b:test` and a fail
  of `a:test`, and the worktree commits a change to `a/lib` alone
- **THEN** the next `sluice exec bolt/x` runs `a:test` and replays
  `b:test`

### Requirement: A replay sluice holds no pass for runs again

Where a worktree's run replayed a check whose key holds no pass the
branch's gate counts, `sluice exec` SHALL say `replayed with no result
sluice recorded: <task>, …; running them again`. It SHALL then run those
checks again in the worktree with no replay, waiting for room in the
host's bound as its first run did. Their results SHALL stand in place of
the replays, recorded as the first run's are (92).

#### Scenario: A pass sluice never recorded

- **WHEN** a worktree's `sluice exec bolt/x` passed `a:test` with the
  change to `a/lib` unstaged, recording nothing, and the worktree then
  commits that change as it was
- **THEN** the next `sluice exec bolt/x` says `replayed with no result
  sluice recorded: a:test; running them again`, runs `a:test`, and
  records its pass under its key

### Requirement: The output says what was read and what was replayed

`sluice exec` SHALL open with `running the checks the worktree's own
changes affect since <revision> on <branch>`, the merge base's short
revision, adding `, now at <revision>` where the branch has moved past
it. Its pass line SHALL count the replays, `passed: N tasks (K
replayed)`, and give `passed: N tasks` where none was replayed. `--json`
SHALL give `since`, the merge base's full revision or `null`, and each
task's `replayed`.

#### Scenario: A worktree behind its branch

- **WHEN** a worktree was cut from `feature/x` at `1a2b3c4`, and
  `feature/x` is now at `c41e8f2`
- **THEN** `sluice exec feature/x` opens with `running the checks the
  worktree's own changes affect since 1a2b3c4 on feature/x, now at
  c41e8f2`

#### Scenario: The document for scripts

- **WHEN** `sluice exec bolt/x --json` replays `b:test` and runs
  `a:test`
- **THEN** its document's `since` is the merge base's full revision,
  `tasks` gives `b:test` with `replayed` true and `recorded` false, and
  `a:test` with `replayed` false
