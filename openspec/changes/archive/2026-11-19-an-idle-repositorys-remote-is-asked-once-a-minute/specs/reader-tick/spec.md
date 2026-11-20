## RENAMED Requirements

- FROM: `### Requirement: A remote's records are asked for once a tick`
- TO: `### Requirement: A remote's records are asked for in one fetch`

## MODIFIED Requirements

### Requirement: An idle tick starts no process

In a repository where nothing is in play, no submission queued or under
test, no place to release and no preview to drop, a pass of the host
process after its first, at its own tick or a follower's ask, SHALL start
no process, no git, credential helper or other command, but the one fetch
a remote that keeps records is asked by where the pass asks it (see "A
remote's records are asked for once a minute where nothing is in play")
(129, 135). A follower SHALL start none of its own.

#### Scenario: sluice run in a set-up repository with nothing queued

- **WHEN** a repository's `main` holds `.config/sluice.toml`, its records
  hold one landed submission and nothing queued, and git on the path
  counts every start
- **THEN** `sluice run --rounds 6` starts git exactly as many times as
  `sluice run --rounds 1`, the host process's starts counted with its own

#### Scenario: A watch of a set-up repository with nothing queued

- **WHEN** the same repository is watched by `sluice overview --json
  --watch`, which the host process reads for, with git on the path
  counting every start
- **THEN** once the watch has printed its first line, git starts no more
  while it goes on watching

#### Scenario: A branch on a remote with its records on the host

- **WHEN** `main` lands on `origin`, its records are in the host's own
  files, nothing is queued, and git counts every start
- **THEN** `sluice run --rounds 6` starts git exactly as many times as
  `sluice run --rounds 1`, and fetches nothing after its first pass

#### Scenario: A branch that takes pull requests between discoveries

- **WHEN** `main` takes pull requests, none is open, the host's discovery
  interval is a minute, and `sluice run --rounds 6` passes within it
- **THEN** it starts git exactly as many times as `sluice run --rounds 1`
  and asks GitHub nothing after its first pass

#### Scenario: Held worktrees are named without git

- **WHEN** a worktree has `main` checked out and a watch follows the host
  process, with git counting every start
- **THEN** each line the watch prints names that worktree under
  `sessions` as before, and git starts no more after its first line

### Requirement: A remote's records are asked for in one fetch

Where branches' records are on a remote, each ask of that remote, by a
pass of the host process or its look, SHALL ask it once for the records
of every branch on it, and in the overview the hosts' signs of life on
it, in one fetch under the retry policy (rule core.29), and SHALL read
each branch's records from what it brought, inside sluice, never
fetching once per branch or once per read. A round still fetches its own
branch's records before it decides anything (remote-records).

#### Scenario: Two branches' records on one remote

- **WHEN** `main` and `release` keep their records on `origin`, nothing is
  queued, and git counts every start
- **THEN** each ask of `origin` after the host process's first starts git
  once, for one fetch from `origin`, whether a pass a command asks for
  or a watch's makes it

### Requirement: A refused ask leaves each branch read as last fetched

When a tick's fetch of a remote's records is refused, every branch on that
remote SHALL be read as last fetched and said to be so, in the remote's
words, as a read that could not refresh is today, and the next ask SHALL
ask again: the next pass while work is in play, and within the minute
where nothing is (80, 129, 133).

#### Scenario: The remote refuses the tick's ask

- **WHEN** `main` and `release` keep their records on `origin` and the
  tick's fetch is refused
- **THEN** the watch's line shows both branches as last fetched, each
  saying it could not refresh, in the remote's words, and the next ask
  asks again

## ADDED Requirements

### Requirement: A remote's records are asked for once a minute where nothing is in play

Where nothing is in play in a repository, the host process SHALL ask a
remote that keeps its branches' records once a minute at most, by its
look or a pass of the repository's task, a watched one's among them, and
SHALL read every branch on it between as the last ask fetched it; while
work is in play every pass SHALL ask. A one-shot read between asks SHALL
be answered from a reading that asks nothing and keeps the last ask's
refusal (129 as amended 2026-11-19).

#### Scenario: An idle repository held open

- **WHEN** a host process lives on for an agent's held room, and
  `main`'s records are on `origin` with nothing in play
- **THEN** it asks `origin` once as it starts, and not again within the
  minute, however many times it goes through its index
  (`an_idle_repositorys_remote_is_asked_once_a_minute` in
  `crates/sluice/tests/host_process.rs`)

#### Scenario: Work comes into play and goes

- **WHEN** a watched repository's pass finds work in play, and later a
  pass finds none
- **THEN** each pass after one that found work asks, and the passes after
  the first that found none ask nothing within the minute
  (`a_tasks_pass_asks_where_work_is_in_play_or_a_watch_opens` in
  `crates/sluice/src/host_process/tasks.rs`)

### Requirement: A change or a watch's opening has the remote asked at once

A command of sluice's that tells the host process of a change, a submit
among them, a watch's opening and a forecast's second ask SHALL have the
next reading of the repository ask its remote at once, whatever is in
play (129 as amended 2026-11-19).

#### Scenario: A watch opens within the minute

- **WHEN** a watch opens on a repository with nothing in play, whose
  `origin` the host process asked within the minute
- **THEN** its first pass asks `origin` at once, and its passes after, a
  second apart, ask nothing within the minute
  (`an_idle_repositorys_remote_is_asked_once_a_minute` in
  `crates/sluice/tests/host_process.rs`)

#### Scenario: A read between asks, a change and the minute's end

- **WHEN** a one-shot read past its tick, a command's change, a
  forecast's second ask and the minute's end follow an idle look's ask
- **THEN** the read is answered from a look that asks nothing, with the
  last ask's refusal, and the change, the forecast's ask and the look at
  the minute's end each ask
  (`an_idle_repositorys_remote_is_asked_once_a_minute` in
  `crates/sluice/src/host_process/tasks.rs`)
