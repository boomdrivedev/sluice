## MODIFIED Requirements

### Requirement: An idle tick starts no process

In a repository where nothing is in play, no submission queued or under
test, no place to release and no preview to drop, a pass of the host
process after its first, at its own tick or a follower's ask, SHALL start
no process, no git, credential helper or other command, but the one fetch
a remote that keeps records is asked by (see "A remote's records are
asked for once a tick") (129, 135). A follower SHALL start none of its
own.

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

### Requirement: A remote's records are asked for once a tick

Where branches' records are on a remote, a pass of the host process SHALL
ask that remote once for the records of every branch on it, and in the
overview the hosts' signs of life on it, in one fetch under the retry
policy (rule core.29), and SHALL read each branch's records from what it
brought, inside sluice, never fetching once per branch or once per read.
A round still fetches its own branch's records before it decides
anything (remote-records).

#### Scenario: Two branches' records on one remote

- **WHEN** `main` and `release` keep their records on `origin`, nothing is
  queued, and git counts every start
- **THEN** each pass of the host process after its first starts git once,
  for one fetch from `origin`, whether `sluice run --rounds` or a watch
  follows it
