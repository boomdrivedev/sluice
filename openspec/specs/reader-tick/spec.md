# reader-tick Specification

## Purpose
What one tick of a reader, a pass of `sluice run` or a tick of the overview
watch that reads for its host, reads and starts, so that a reader with
nothing to do costs its host nothing a person can measure (129, 135).

## Requirements

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

### Requirement: An idle tick reads only what tells it whether anything changed

A tick in a repository where nothing is in play SHALL read nothing but the
records' list of branches, each branch's records and each branch's
revision, as the host's own files hold them, and sluice's own state of the
host's places, previews and runs. It SHALL read a branch's records whole
only once their version moved since it last read them, or a write left
part done is found (26, 129).

#### Scenario: Records read again once they moved

- **WHEN** `sluice run` passes three times over an idle `main`, then a
  submission is written to `main`'s records by another process, then it
  passes again
- **THEN** it reads `main`'s records whole at its first pass and at the
  pass after the submission, and only their version between, and the
  submission is queued at that pass as it is today

#### Scenario: A submission written mid-tick is not lost

- **WHEN** a write to `main`'s records is half done, its pending write on
  disk, while a tick reads them
- **THEN** the tick reads the records as that write leaves them, as a read
  of them always has (51)

### Requirement: A tick fetches a branch only to act on it

A tick SHALL read a branch on a remote at its revision as last fetched,
and SHALL fetch it only to run a round of it, or for intake once the
branch's discovery is due (136) (121, 129).

#### Scenario: A branch on a remote moved there and nothing in play

- **WHEN** `main` lands on `origin`, nothing is queued, and another host
  lands on `origin`'s `main`
- **THEN** a watch shows `main` at its revision as last fetched, and the
  next submission's round fetches `main` before it forms anything (121)

### Requirement: A branch's policy is read once per revision

A reader SHALL read a branch's policy at a revision once, and SHALL read
it again only once the branch's revision has changed; every round, idle
check, intake and overview of a pass that reads the policy at a revision
already read SHALL take what was read then, its refusal included. A
policy read at no revision a branch still names MAY be forgotten (56,
129).

#### Scenario: Policy read once while the revision stands

- **WHEN** `sluice run --rounds 6` passes over an idle `main` whose
  revision stands
- **THEN** `main`'s policy is read from git once, at its first pass

#### Scenario: Policy read again once the branch moves

- **WHEN** `main` moves to a revision whose policy sets `parallel = 4` while
  a watch is running
- **THEN** the watch's next line shows `main` testing up to 4 at once

#### Scenario: A refused policy stays refused at its revision

- **WHEN** `main`'s revision holds a policy sluice refuses, and `sluice
  run` passes over it three times
- **THEN** each pass says `main` passed by in the same words as today, the
  words said once as today, and the policy is read from git once

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

### Requirement: A refused ask leaves each branch read as last fetched

When a tick's fetch of a remote's records is refused, every branch on that
remote SHALL be read as last fetched and said to be so, in the remote's
words, as a read that could not refresh is today, and the next tick SHALL
ask again (80, 133).

#### Scenario: The remote refuses the tick's ask

- **WHEN** `main` and `release` keep their records on `origin` and the
  tick's fetch is refused
- **THEN** the watch's line shows both branches as last fetched, each
  saying it could not refresh, in the remote's words, and the next tick
  asks again
