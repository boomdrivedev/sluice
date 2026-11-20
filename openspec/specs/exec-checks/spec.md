# exec-checks Specification

## Purpose
An agent's own checks (`sluice exec`) shown as load on the host: the
tasks its run keeps while it goes, the host's room against its bound,
what a finished check is worth, and when checks hold the queue back.

## Requirements

### Requirement: A check's tasks are kept while it runs

While a session run goes, `sluice exec` SHALL keep on its record in the
host's state the run's tasks, as its runner gives them, rewritten when
they change, and the command of a run of a command; a run of a command
SHALL have no tasks. The record SHALL go when the run ends, as it does
today. Keeping them SHALL write nothing to any branch's records.

#### Scenario: Tasks fill as the check goes

- **WHEN** an agent runs `sluice exec main` in a clean worktree under the
  moon runner, and one of its two tasks has ended
- **THEN** the run's record in the host's state lists both tasks, one
  `passed` and one `running`

#### Scenario: A command's run

- **WHEN** an agent runs `sluice exec main -- cargo test`
- **THEN** the record names the command `cargo test` and lists no tasks

#### Scenario: No branch's version moves

- **WHEN** a session run's tasks are rewritten three times
- **THEN** every branch's records keep their version

### Requirement: The overview shows a check on its worktree

`sluice overview --json` SHALL give each entry of `sessions[]` whose
worktree has a session run going a `run`, `{started, command?, tasks[]}`:
when it started, the command of a run of a command, and the tasks kept
for it, absent when none were. An entry with no run going SHALL have no
`run`. The text form SHALL list each such run as `checking <worktree> →
<branch>` with its count of ended tasks and when it started.

#### Scenario: A check under way

- **WHEN** a worktree of the repository is running its checks against
  `main`, three of eight tasks ended
- **THEN** its `sessions[]` entry has `run.started`, and `run.tasks` lists
  eight tasks, three of them ended

#### Scenario: A command

- **WHEN** a worktree is running `sluice exec main -- ./scripts/e2e.sh`
- **THEN** its entry's `run` names `command` and has no `tasks`

#### Scenario: The watch prints when a task ends

- **WHEN** `sluice overview --json --watch` is running and a task of a
  session run ends
- **THEN** the watch prints the document again

### Requirement: The overview gives the host's room

`sluice overview --json` SHALL give a top-level `room`, `{bound, queue,
sessions, reserve}`: the host's bound, the submissions' runs running on
the host, every repository's, the session runs on the host, and the part
of the bound kept from session runs (session-runs-1 clause 91; 71).
`sluice exec` SHALL start a session run only when `bound - reserve -
queue - sessions` is above zero, so the two read the same numbers.

#### Scenario: Room

- **WHEN** the host's bound is 3 with none reserved, `main` has 2
  combinations under test and one agent's checks are running
- **THEN** `room` is `{bound: 3, queue: 2, sessions: 1, reserve: 0}`

#### Scenario: Another repository's check

- **WHEN** an agent in another repository on the host is running its
  checks
- **THEN** `room.sessions` counts it and no entry of `sessions[]` has its
  `run`

#### Scenario: Another repository's submission

- **WHEN** another repository on the host has one combination's run
  running
- **THEN** `room.queue` counts it, and no branch of this repository shows
  it under test

#### Scenario: An agent waits on another repository's queue

- **WHEN** the bound is 2 with none reserved and two other repositories
  each have one combination's run running
- **THEN** `sluice exec` prints `waiting: the host is at its bound` and
  starts once one of them ends

### Requirement: The counts say when checks hold the queue back

The counts line of `sluice`, `sluice status <branch>` and `sluice
overview` SHALL add `· 1 slot to an agent's checks`, or `· N slots to
agents' checks`, to a branch when the host has no free run (`queue +
sessions >= bound`), at least one session run holds one, and that branch
has a submission queued while fewer of its candidates are under test
than its window; and SHALL add nothing otherwise.

#### Scenario: Held back

- **WHEN** the bound is 3, `main` tests 2 combinations of a window of 3
  with a third submission queued, and one agent's checks are running
- **THEN** `main`'s line reads `testing 2 at once (up to 3) · 1 slot to
  an agent's checks`

#### Scenario: Not held back

- **WHEN** an agent's checks are running and the host has a free run
- **THEN** the line says nothing of checks

### Requirement: Checks decide nothing

A session run's tasks, command and `room` SHALL be read by no round,
gate, verdict, blame or replay, and SHALL never be a run or a result
(16, 38; rules core.7, core.9, core.18).

#### Scenario: A task reads failed

- **WHEN** a session run's task reads `failed` while the run goes, and
  the run ends passing
- **THEN** its recorded results and `last_result` are those of the
  finished run

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
