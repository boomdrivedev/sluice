# host-process Specification

## Purpose
sluice's one host process for each user on a host: how a command reaches
it or starts it, how it moves every repository's queues apart and holds
the host's bound, how it finds its repositories again, when it ends, and
how a newer sluice takes over, with nothing installed and nothing lost
when it stops, and how every read of a queue follows it (135, 129, 71,
150; rules core.31, core.25, core.49; record 0093).

## Requirements

### Requirement: A command that needs the queues moved reaches the host process or starts it

A command of sluice's that needs its host's queues moved, a submit
among them, SHALL reach sluice's host process, and SHALL start it where
none answers. Nothing SHALL be installed for this, and no policy or
configuration key SHALL be needed (135; rule core.31).

#### Scenario: A withdrawal with no host process running

- **WHEN** a submission's combination is under test, no host process
  runs, and `sluice withdraw` takes the submission out
- **THEN** a host process is running, and its next pass ends the run and
  releases the combination's checkout

### Requirement: A submit writes its submission and tells the host process

A submit, waiting or not, SHALL write its submission and then tell the
host process of it. A submit with `--no-wait` SHALL then return at once,
and a waiting one SHALL then follow the host process as `sluice wait`
does (135).

#### Scenario: A submit with no host process running

- **WHEN** no sluice process runs on a host, and an agent runs `sluice
  submit main --no-wait` in a repository whose `main` holds a policy
- **THEN** the submit exits 0 with the submission's number, a sluice host
  process is running, and that process lands the submission with no
  other command run

#### Scenario: Ten submits at once

- **WHEN** ten worktrees of one repository each run `sluice submit main
  --no-wait` at the same moment, with no host process running
- **THEN** each exits 0 with its own number, and exactly one host process
  runs on the host

### Requirement: One host process for each user on a host

A host SHALL run at most one sluice host process for each user, whatever
repositories that user's commands run in. A second one started while the
first lives SHALL end at once and move nothing. Several users on a host
SHALL each have their own, each with its own bound (135).

#### Scenario: Two repositories, one process

- **WHEN** a submission is queued in repository A and another in
  repository B, by the same user, each with `--no-wait`
- **THEN** one host process runs, and it lands both

#### Scenario: A second host process

- **WHEN** a host process runs and `sluice host` is started by hand
- **THEN** the second exits at once, and the first goes on

### Requirement: Each repository's queues are moved apart

The host process SHALL move every repository it holds, each in a task of
its own, so a branch it cannot read, a refused policy or a slow fetch in
one repository holds no round of another (26). A fault in one
repository's task SHALL end only that task. The task SHALL be started
again at the next tick, and the fault SHALL be written to the host
process's log with the repository named.

#### Scenario: One repository's policy is refused

- **WHEN** repository A's `main` holds a policy sluice refuses, and a
  submission is queued on repository B's `main`
- **THEN** B's submission lands, and the overview of A shows the refusal
  in its words

### Requirement: The host process's look at an idle repository holds no command

The host process's look at a repository it holds no task for SHALL hold
neither a command's connection nor another repository's look while that
repository's remote is slow to answer, and SHALL pass a spent allowance
by rather than wait it out (129, 134; rule core.29).

#### Scenario: An idle repository's remote does not answer

- **WHEN** the host process looks at idle repository A, whose records
  are on a remote that does not answer, and a command in repository B
  reaches the host process
- **THEN** B's command is answered at once, and A is given a task only
  once its look ends and finds work in play

### Requirement: A pass reads a branch while a combination is under test

At each of its passes of a repository, the host process SHALL run a
round for every branch whose records hold a submission queued, a
combination under test or a checkout waiting to be released, even when no
submission of it is queued, so a run is read and an obsolete combination
dropped at its next pass. Whether a branch is in play SHALL be read from
its records alone, and a branch with nothing in play SHALL cost the pass
its records' read alone (129; rule core.31).

#### Scenario: The queue emptied while a run went on

- **WHEN** the one submission on `main` is withdrawn while its
  combination is under test, and nothing else is queued
- **THEN** the host process's next pass runs a round for `main`, which
  ends the run and releases its checkout

#### Scenario: Nothing under test and nothing queued

- **WHEN** `main`'s records hold no submission in the queue, no
  combination under test and no checkout to release
- **THEN** the host process's pass runs no round for `main` and reads no
  revision or file of it

### Requirement: sluice step is one round of the host process

`sluice step <branch>` SHALL ask the host process for one round of the
branch, starting it where none answers, and SHALL print that round's
outcome, its releases and its hold as it prints them today, as text or
under `--json`, with the same exits, and every warning the round's
intake met on stderr. The round SHALL be the one a pass of the host
process runs for the branch, so a step and a pass behave alike (rule
core.14).

#### Scenario: A step with a submission queued

- **WHEN** `main` has one submission queued and nothing under test, and
  `sluice step main --json` runs
- **THEN** it prints one document with `"outcome": "stepped"`, `main`'s
  records hold the submission's combination, and the round ran in the
  host process

#### Scenario: A step whose intake met a request it cannot read

- **WHEN** main takes pull requests, the host keeps it, GitHub serves no
  head for `#12`, and `sluice step main --json` runs
- **THEN** it prints `sluice: warning: taking in main's pull requests:
  pull/12:` and the failed fetch on stderr, and its document on stdout

### Requirement: The host process finds its repositories again from an index

Every command of sluice's that finds a repository has sluice, records or
a policy at a revision the host holds, SHALL add the repository to an
index in the host's state where it is not there yet. A host process
SHALL find the repositories it holds in that index when it starts, and
SHALL move each that has work in play, whichever repository the command
that started it ran in. A repository whose directory is gone, or which
has no sluice, SHALL be taken out of the index (135).

#### Scenario: Started from another repository

- **WHEN** repository A has a submission queued, no host process runs,
  and `sluice submit main --no-wait` runs in repository B
- **THEN** the host process that starts lands both A's submission and
  B's

#### Scenario: A repository removed

- **WHEN** a repository in the index is deleted from disk
- **THEN** the host process takes it out of the index at its next tick,
  and runs no task for it

### Requirement: The host process ends once nothing is in play and nothing follows it

The host process SHALL end once no repository it holds has work in play
and nothing follows it. Work in play is a submission queued or under
test, a checkout to release or a preview to drop. A follower is a watch,
a wait or a `sluice run`. A run waiting for room or holding it from its
count to its start also keeps it. It SHALL also end at its next tick once
its state folder is gone. A command that meets it ending SHALL start a
new one, so none is lost between the two (135).

#### Scenario: The queue empties

- **WHEN** the one submission queued on a host lands, and nothing
  follows the host process
- **THEN** the host process has ended within two ticks of the landing

#### Scenario: A watch keeps it

- **WHEN** `sluice overview --json --watch` runs in a repository with
  nothing queued
- **THEN** the host process runs for as long as the watch does, and ends
  within two ticks of the watch's end

#### Scenario: The state folder is removed

- **WHEN** a host process runs with work in play and its state folder is
  deleted
- **THEN** it has ended within two ticks, and its socket is gone

### Requirement: A host process that stops loses nothing

The host process SHALL keep nothing that the records and the host's
state do not hold. A run SHALL outlive the host process that started
it. Any command of sluice's that needs the queues moved or reads them,
with no host process answering, SHALL start one, a `sluice status` and
the home view among them. The process that starts SHALL carry the work
on from the records, and SHALL run no check again for a combination that
already has a result (135, 129, S10).

#### Scenario: Killed while a combination is under test

- **WHEN** the host process is killed while a combination's checks run,
  and `sluice status main` runs once the checks have ended
- **THEN** a host process is running again, the combination's result is
  read from the run that outlived the first, and the submission lands
  with no check run again

### Requirement: The host process carries nothing of the command that started it

The host process SHALL start with the host's own environment and SHALL
keep none of the starting command's repository, worktree, index or
multiplexer session, nor its working folder. A run SHALL be equipped
from its repository's policy (147), never from the environment of the
command that started the host process (135).

#### Scenario: Started from inside a hook and a pane

- **WHEN** `sluice submit main --no-wait` runs with `GIT_DIR`,
  `GIT_WORK_TREE`, `HERDR_PANE_ID` and `HERDR_SOCKET_PATH` set, and
  starts the host process
- **THEN** the combination's checks run with none of those variables
  set, and the host process's working folder is `/`

### Requirement: A newer sluice's command has an older host process hand over

A command of a newer sluice that reaches an older host process SHALL
have it hand its repositories over and end, and SHALL then start its
own. The older process SHALL finish the round each task is in, start no
other, and let its locks go as it ends, and the newer command SHALL wait
for it to end for as long as it gives its tasks to finish their rounds.
A command of an older sluice that reaches a newer host process SHALL use
it. Versions SHALL be compared as release numbers.

#### Scenario: The host's sluice is upgraded

- **WHEN** a host process of sluice 0.71.0 runs, and a submit of sluice
  0.72.0 runs on the host
- **THEN** the 0.71.0 process ends, a 0.72.0 host process runs, and the
  submission lands

#### Scenario: A round that outlasts a start's wait

- **WHEN** a 0.72.0 command asks a 0.71.0 host process to hand over, and
  one of its tasks takes twelve seconds to finish its round
- **THEN** the command waits for the 0.71.0 process to end, and then
  reaches a 0.72.0 host process of its own start

### Requirement: An older sluice run keeps the repository it drives

Where a `sluice run` of a sluice from before the host process drives a
repository on the host, the host process SHALL run no round of that
repository while it lives. It SHALL follow that run's document, and
SHALL move every other repository. It SHALL take the repository's rounds
at its first tick after that run ends.

#### Scenario: A repository an older run drives

- **WHEN** an older `sluice run` holds repository A's driver lock, and a
  submission is queued in A and another in B
- **THEN** the host process lands B's and runs no round of A, and A's
  submission is landed by the older run

### Requirement: The overview names the host process and the runs waiting

`sluice overview --json` SHALL give `host_process`, the host process's
`version`, `pid` and `since`, or null where none answers, beside `host`,
the host's name. Its `room` SHALL give `waiting`, the runs waiting for
room by kind, `turn`, whose the next room a combination and a session
both want is, `queue` or `sessions`, and `repositories`, each
repository's runs under test and runs waiting. `sluice status` with no
target SHALL print the same room on its line (150, 71).

#### Scenario: A session's check waits behind a combination

- **WHEN** the bound is one, a combination's run holds it, and `sluice
  exec main` waits for room
- **THEN** `sluice overview --json` gives `room.waiting.sessions` 1 and
  `host_process.pid` the host process's pid

#### Scenario: Two repositories' runs

- **WHEN** repository A has two runs under test and repository B has one
  combination waiting for room
- **THEN** `sluice overview --json` in either repository gives
  `room.repositories` with A's two runs under test and B's one waiting

### Requirement: The host process reads each result once and answers by key

The host process SHALL read each recorded result and each signed run
record once for its life, verifying each signature once, and SHALL
answer a gate, a status, a verdict and a session run by the keys and
combinations they ask for, so no read loads or verifies the whole
history again (129, 69; record 0093 as amended 2026-11-13). A command
that reads results SHALL ask the host process for them, starting it
where none answers.

#### Scenario: A signed record verified once

- **WHEN** a branch's records are on its remote with forty signed run
  records, the host process passes the repository ten times, and `sluice
  status <n>` runs three times for a submission under test
- **THEN** each of the forty records was verified once, and no command
  verified any

#### Scenario: A status reads what it asks for

- **WHEN** the repository's results log holds results of two hundred
  combinations, and `sluice status <n> --json` runs for a submission
  whose combination has three
- **THEN** the status shows the three, and the command read no line of
  the results log itself

#### Scenario: A result recorded after the index was read

- **WHEN** the host process has read the results log, and a run then
  records a pass under a key
- **THEN** the next gate that asks for that key reads the pass, the host
  process having read the log from where it last read it
