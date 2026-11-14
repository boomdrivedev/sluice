## RENAMED Requirements

- FROM: `### Requirement: A reader serves one repository on its own host`
- TO: `### Requirement: A reader serves its own host`

## ADDED Requirements

### Requirement: The host process is each repository's reader

sluice's host process SHALL be the reader of every repository it holds
on the host (129, 135). A watch or a wait that finds no live reader of
its repository SHALL reach the host process, starting it where none
answers, and follow its document, reading no queue itself. Each time the
overview, all but its time, changes, the host process SHALL replace the
reader's document with the whole document `sluice overview --json`
prints, while anything follows the repository.

#### Scenario: A watch with no reader

- **WHEN** nothing of sluice's runs on a host, `sluice overview --json
  --watch` runs in a repository, and a submission is then queued
- **THEN** a host process is running, the reader's document in the host's
  state holds the overview at the start, then the overview with the
  submission queued, each a whole `sluice/overview@1` document, and the
  watch read no records itself

#### Scenario: A wait with no reader

- **WHEN** nothing of sluice's runs on a host and `sluice wait 4` runs in
  a repository
- **THEN** the reader's document, written by the host process, holds the
  whole overview, every branch, checkout and preview of the repository,
  not only submission 4's branch

### Requirement: A wait takes its submission from the host process's document

A `sluice wait`, waiting `sluice submit` or `wt submit` SHALL read its
submission's state, its combination's progress and its branch's hold
from the reader's document, not the records, and SHALL say them as
`submission-wait` states. It SHALL run no round (135; rule core.31).
Its exits, its lines and its `--json` record SHALL be as
`submission-wait` states.

#### Scenario: A follower wait says its combination's progress

- **WHEN** a live reader's document shows submission `#7`'s combination
  with 3 of 5 tasks done, and `sluice wait 7` runs
- **THEN** it prints the line a wait prints for 3 of 5 tasks done, read
  from that document

#### Scenario: A follower wait out of time

- **WHEN** a live reader's document holds submission `#7` with `state`
  `testing`, and `sluice wait 7 --timeout 2s --json` runs
- **THEN** it exits 5 and prints submission 7's record as the document
  holds it, with `state` `testing`

#### Scenario: A follower wait before the host process reaches it

- **WHEN** submission `#7` is queued and the host process has run no
  round since, and `sluice wait 7 --timeout 2s` runs
- **THEN** the wait runs no round, and exits 5 with `#7` as the host
  process's document holds it

### Requirement: A host process that stops is started again within a tick

When the host process ends, by Ctrl-C, a kill or a crash, every watch and
wait that follows it SHALL reach it again at its own next tick, starting
it where none answers. Until the new host process writes, each follower
SHALL go on following the document the last one wrote, and SHALL read
no records itself (129).

#### Scenario: The host process is killed

- **WHEN** a watch follows the host process and the host process is
  killed
- **THEN** within one tick of the watch a host process runs again, and
  the next document written is the one it read from the records

#### Scenario: The host process is killed under a wait

- **WHEN** `sluice wait 4` follows the host process and the host process
  is killed before submission 4 is decided
- **THEN** within one tick of the wait a host process runs again, and the
  wait reports submission 4's outcome once that process decides it

### Requirement: The host process holding the lease is the one to read

When the host process holds the lease of an older reader that stopped, it
SHALL be the one to become the reader, and no watch or wait SHALL take
the reader's place before it.

#### Scenario: An older reader stops

- **WHEN** a watch of a sluice from before the host process is a
  repository's reader, a host process holds its lease, and that watch is
  stopped
- **THEN** the host process is the reader at its next tick, and every
  watch and wait of the repository follows it

### Requirement: The host process names the version of sluice it is

The host process, as it becomes a repository's reader, SHALL write,
beside its lock in the host's state, the version of sluice it is, whole,
so a command that finds it live can tell which sluice reads. The
reader's document SHALL be unchanged by it.

#### Scenario: The host process becomes the reader

- **WHEN** no sluice watches a repository on the host and `sluice overview
  --json --watch` of sluice 0.72.0 runs in it
- **THEN** the host's state names 0.72.0 as the version of the
  repository's reader, and the reader's document is the overview as
  `sluice overview --json` prints it, with nothing added

### Requirement: The host process takes an older reader's lease

Where a reader of an older sluice, a watch or a wait of a release from
before the host process, holds a repository's reader lock, the host
process SHALL take that reader's lease at its next tick. It SHALL follow
that reader, reading nothing itself and running no round of the
repository, until the reader lets go, and SHALL then become the reader
at its own next tick. Versions SHALL be compared as release numbers.

#### Scenario: A host process beside an older watch

- **WHEN** a watch of sluice 0.70.0 is a repository's reader, and a
  submit of sluice 0.72.0 runs in the repository
- **THEN** the host process holds the lease within its next tick, and
  once the older reader lets go, it is the reader within its next tick,
  writes the document it read from the records, and the host's state
  names 0.72.0 as the reader's version

### Requirement: A branch the host process could not read is written unread

When the host process cannot read one branch's policy, revision or
records, it SHALL write the overview with that branch unread, its
`unread` the refusal's words, whole and unchanged, and every other branch
read (26, 133). A refusal that stands with the same words SHALL be
written once while it stands.

#### Scenario: A wait follows while another branch's records cannot be read

- **WHEN** `sluice wait 4` follows the host process and each read of
  another branch's records fails with the same words
- **THEN** the reader's document is the overview with that branch unread,
  its `unread` the failure's words, and submission 4's branch read, it is
  written once while the failure stands, and the wait goes on waiting on
  submission 4


### Requirement: A one-shot read waits for a current reading only so long

A one-shot read whose host process has no current reading of its
repository SHALL wait for the reading it asked for within the round's
retry bound and a tick. Past that, it SHALL read the records as the host
holds them, fetching nothing, and say on stderr when the host process
last read the repository (129; rule core.29).

#### Scenario: A host process held in a slow look

- **WHEN** the repository's remote does not answer, so the host
  process's look of it does not end, and `sluice status main` runs
- **THEN** it prints `main`'s queue as the host holds it, says when the
  host process last read the repository, fetches nothing itself, and
  exits 0

### Requirement: A one-shot read is answered from the host process's last reading

`sluice status` in each form, `sluice overview` without `--watch` and
the home view SHALL ask the host process for a current reading of their
repository, starting it where none answers, and SHALL then answer from
it: the overview and the home view from the reader's document, a status
from the records as the host holds them, its results from the host
process. None SHALL fetch, become a reader or write the reader's
document (129; record 0093 as amended 2026-11-13; rule core.25).

#### Scenario: A one-shot overview beside a current reading

- **WHEN** the host process read the repository within its tick, `main`
  is on a remote, and `sluice overview --json` runs
- **THEN** it prints the reader's document of that reading, asks the
  remote nothing, and reads no result

#### Scenario: A status with no host process

- **WHEN** no host process runs, `main` is on a remote with `#4`
  queued, and `sluice status main --json` runs
- **THEN** a host process is running, `#4` is shown as that process's
  first reading found it, and the status itself fetched nothing

#### Scenario: A status during a long round

- **WHEN** the host process's pass of the repository is landing a
  combination and has run for a minute, and `sluice status main` runs
- **THEN** it answers at once from the records as the host holds them,
  with nothing fetched

## MODIFIED Requirements

### Requirement: No other reader's lease is taken

The host process SHALL NOT take the lease of a reader of its own
version, of a newer one, or of one that names no version, and no watch,
`sluice wait` or waiting `sluice submit` SHALL take a lease at all: each
follows the reader as it finds it.

#### Scenario: A watch of the same version

- **WHEN** the host process of sluice 0.72.0 is a repository's reader and
  a watch of 0.72.0 runs in the repository
- **THEN** the watch follows the host process's document for as long as
  it runs, and never takes a lease

#### Scenario: A reader that names no version

- **WHEN** a live reader's lock is held and the host's state names no
  version for it, as a reader of a sluice from before versions were
  named does, and a submit of 0.72.0 runs in the repository
- **THEN** the host process follows that reader's document for as long
  as the reader runs, and runs no round of the repository meanwhile

#### Scenario: A newer wait beside an older reader

- **WHEN** a watch of sluice 0.70.0 is the reader and `sluice wait 7` of
  sluice 0.72.0 runs in the repository
- **THEN** the wait follows the reader's document, and takes no lease of
  its own

### Requirement: An older reader whose lease is taken follows

A reader of an older sluice, whether a watch, a `sluice wait` or a
waiting `sluice submit`, SHALL look at each tick, before it reads,
whether a newer process has taken its lease. When one has, it SHALL let
go of its lock and follow the reader's document from then on, reading
no records, policy or checkouts itself, as any follower does. No two
processes SHALL read the repository's queues at once.

#### Scenario: An older watch lets go

- **WHEN** a watch of sluice 0.70.0 is the reader, the host process takes
  its lease, and a submission `#7` is then queued
- **THEN** the 0.70.0 watch reads nothing after its next tick, and the
  line it prints with `#7` queued is the document the host process wrote

#### Scenario: An older wait lets go

- **WHEN** `sluice wait 7` of sluice 0.70.0 is the reader and the host
  process takes its lease
- **THEN** the wait goes on as a follower: it takes `#7`'s progress from
  the document the host process writes, and ends as it would have

#### Scenario: Only one reads during the handover

- **WHEN** the host process takes the lease of a reader of 0.70.0
- **THEN** at every moment until the 0.70.0 reader lets go, the reader's
  document is written only by the 0.70.0 reader, and after it, only by
  the host process

### Requirement: A reader serves its own host

The host process SHALL read each repository it holds apart, as its
common git directory names it, and only on its own host: each
repository's lock and document are in the host's own sluice state,
beside no other host's. A watch or wait of a repository SHALL follow
only that repository's document.

#### Scenario: Two repositories

- **WHEN** `sluice overview --json --watch` runs in one repository, and a
  second watch runs in another repository on the host
- **THEN** one host process reads both, and each watch prints its own
  repository's queues

#### Scenario: Two worktrees of one repository

- **WHEN** a watch runs in one worktree of a repository and another watch
  runs in another worktree of the same repository
- **THEN** both follow the one document the host process writes for the
  repository

### Requirement: A reader that could not read the queues writes the failure as its document

When the host process cannot list a repository's records, it SHALL
replace the reader's document with the error document a failing
`sluice overview --json` prints, `{"error":{"kind","message","next"}}`,
whose message is the failure's words, whole and unchanged (133). It
SHALL read again at its next tick, and write the overview it reads once
it can. A failure that stands with the same words SHALL be written once
while it stands.

#### Scenario: A watch's read fails and then succeeds

- **WHEN** a watch follows the host process, the repository's records
  are listed again at its second tick and the list fails with `fatal:
  unable to read 4b825dc6: Resource temporarily unavailable`, and its
  third tick lists them
- **THEN** the reader's document after the second tick is
  `{"error":{"kind":"internal","message":…,"next":[]}}` whose message
  holds `fatal: unable to read 4b825dc6: Resource temporarily
  unavailable`, and after the third tick it is the overview read then

#### Scenario: A standing failure is written once

- **WHEN** five ticks of the host process in a row fail to list a
  followed repository's records with the same words
- **THEN** the reader's document is written once for them

### Requirement: A watch prints a read it could not make and keeps watching

A `sluice overview --json --watch` whose reader's document is an error
document SHALL print it as one line, as it prints a changed overview,
and SHALL NOT end. It SHALL print a failure that stands with the same
words once, and print the overview next written, whatever it held before
the failure.

#### Scenario: A failed read under load

- **WHEN** a watch prints the overview at its start, the host process's
  second read fails, and its third reads an overview unchanged from its
  first
- **THEN** the watch prints three lines, the overview, the error document
  with the failure's words, and the overview again, and it is still
  running

#### Scenario: A follower prints the reader's failure

- **WHEN** a watch follows a reader whose document changes from an
  overview to the error document with `HTTP 503`, and then back to an
  overview
- **THEN** the watch prints three lines, the second the error document
  with `HTTP 503`, and is still running

### Requirement: A watch ends only when asked, when its repository is gone, or where it has no sluice

A `sluice overview --json --watch` SHALL end only when it is stopped, when
whoever reads its output has closed it, when its repository's git
directory no longer exists, or when its repository has no sluice. A
failed read of the queues, a failed write of the reader's document, or a
host process that stopped SHALL NOT end it. A watch whose repository is
gone SHALL end as a failing overview does, its error document on stdout
and exit 3.

#### Scenario: The reader's document cannot be written

- **WHEN** a watch follows the host process and one write of the
  reader's document fails
- **THEN** the watch still prints the overview at a later tick, once the
  document is written, and is still running

#### Scenario: The repository is removed

- **WHEN** a watch follows the host process and its repository's git
  directory is removed
- **THEN** the watch prints the error document with the read's words and
  exits 3

#### Scenario: Stopped while a failure stands

- **WHEN** a watch has followed an error document for ten ticks and it is
  stopped with Ctrl-C
- **THEN** it ends as a stopped watch does, exit 130

### Requirement: A watch ends where its repository has no sluice

When the host process finds that a followed repository's records list no
branch and that no revision the host holds has sluice's policy, it SHALL
write the overview with `"set_up": false` as the reader's document and
hold the repository no longer, and the watch SHALL print it as its last
line and exit 0. The revisions checked are every local branch and every
remote's tracking branch as last fetched, with nothing fetched (129). No
other document holds `set_up`.

#### Scenario: A repository sluice was never set up in

- **WHEN** a repository has no records and no branch of it, local or
  tracked, holds `.config/sluice.toml`, and `sluice overview --json
  --watch` runs in it
- **THEN** it prints one line, the overview with `branches` `[]` and
  `"set_up": false`, which is the reader's document, and exits 0

#### Scenario: Set up with nothing submitted

- **WHEN** a repository has no records and its `main` holds
  `.config/sluice.toml`, and a watch runs in it for five ticks
- **THEN** it prints one line, the overview with `branches` `[]` and no
  `set_up` field, and is still running

#### Scenario: The policy only on a remote's branch

- **WHEN** a repository has no records, its own `main` holds no policy,
  and its `origin/main`, as last fetched, holds `.config/sluice.toml`
- **THEN** the watch prints the overview with no `set_up` field and is
  still running, and nothing was fetched

#### Scenario: A policy in the working copy alone

- **WHEN** a repository has no records and `.config/sluice.toml` is in a
  worktree but committed on no branch
- **THEN** the watch prints the overview with `"set_up": false` and exits
  0

#### Scenario: Records with no policy

- **WHEN** a repository's records list `main` and no revision the host
  holds has the policy
- **THEN** the watch prints the overview with `main` and no `set_up`
  field, and is still running

### Requirement: A check of the revisions that fails is a read refused

When the host process cannot tell whether any revision the host holds
has a followed repository's policy, it SHALL write the error document
with the failure's words, whole and unchanged, as for a read of the
queues it could not make (133), and SHALL check again at its next tick.

#### Scenario: git fails to list the branches

- **WHEN** a watch follows the host process in a repository with no
  records, and listing its branches fails with `fatal: unable to read
  4b825dc6: Resource temporarily unavailable`
- **THEN** the watch prints the error document whose message holds those
  words, does not end, and at the host process's next tick, the branches
  listed and none holding the policy, prints the overview with
  `"set_up": false` and exits 0

### Requirement: A reader that takes the lock passes over an earlier reader's finding

The host process, as it becomes a repository's reader, SHALL remove a
reader's document with `"set_up": false` left by an earlier reader
before it reads, so that no watch that starts while it reads ends on
that earlier finding.

#### Scenario: sluice set up after a watch ended

- **WHEN** a watch ended leaving the reader's document with `"set_up":
  false`, sluice is then set up and `main` holds `.config/sluice.toml`,
  and a watch runs again
- **THEN** it prints the overview with no `set_up` field, the reader's
  document is that overview, and it is still running

#### Scenario: A watch that starts while the new reader reads

- **WHEN** an earlier reader left the document with `"set_up": false`,
  the host process has taken the lock and not yet written, and a watch
  starts
- **THEN** the watch prints nothing until the host process writes, and
  does not end

### Requirement: Whether the repository has sluice is asked again only once the host's revisions or the records' list changed

The host process SHALL ask whether any revision the host holds has a
repository's policy when it takes the repository, and again only once a
revision the host holds, a local branch's or a remote's tracking
branch's, or the records' list of branches, has changed since it last
asked. A tick that finds both as they were SHALL ask nothing of the
policy. The question SHALL start no process (129).

#### Scenario: Set up with nothing submitted, watched for many ticks

- **WHEN** a repository has no records, its `main` holds
  `.config/sluice.toml`, and a watch runs in it with git on the path
  counting every start
- **THEN** it prints the overview with `branches` `[]` and no `set_up`
  field, git starts no more once that line is printed, and the watch is
  still running

#### Scenario: The policy removed while a watch runs

- **WHEN** a watch runs in a repository with no records whose `main`
  alone holds `.config/sluice.toml`, and `main` then moves to a commit
  that removes it
- **THEN** the host process asks again at its next tick, and the watch
  prints the overview with `"set_up": false` as its last line and exits
  0

### Requirement: A reader marks each read and names its tick

The host process SHALL mark in the host's state, beside each
repository's document, when each read begins, as a pass or a look of the
repository begins, and when it ends, as that pass or look ends, whether
or not the document changed, and without rewriting the document when it
did not. The document SHALL name its tick, the pause between its reads,
in whole seconds as `tick`: one second while a watch follows the
repository, three otherwise (64, 129).

#### Scenario: A read that changes nothing
- **WHEN** a watch follows the host process and three of its reads find the queues as they were
- **THEN** the document's content is as it was after the first, and the mark of its last read's end is the time the third ended

#### Scenario: The tick is named
- **WHEN** `sluice overview --json --watch` follows the host process
- **THEN** the document holds `"tick": 1`, and once the watch has ended and only `sluice wait` follows, it holds `"tick": 3`

### Requirement: A reader's ask of a remote brings what a forecast forms from

Where the host process reads a branch's records from the branch's
remote, the same call SHALL bring the branch's revision, the work
submitted to branches on that remote, and the combinations formed on the
branch, and it SHALL make no other call to that remote for them (64,
129; rule core.29).

#### Scenario: One call for the branch
- **WHEN** the host process reads `main`, which is on a remote with `#1` queued from another host
- **THEN** each of its reads makes one call to the remote for `main`, and after it the host holds `main`'s revision on the remote and `#1`'s work

#### Scenario: A branch the remote lacks
- **WHEN** the remote holds `main`'s records but no longer holds the branch `main`
- **THEN** the host process reads `main`'s records as it does today, and its document names no revision for `main`

## REMOVED Requirements

### Requirement: A live driver holds the reader's place

**Reason**: No driver of one repository exists apart from the host
process, which is the reader (129, 135).
**Migration**: "The host process is each repository's reader".

### Requirement: The first watch or wait of a repository on a host becomes its reader

**Reason**: No watch or wait reads the queues. One that finds no reader
reaches the host process, starting it where none answers, and the host
process reads (129, 135).
**Migration**: "The host process is each repository's reader".

### Requirement: A wait that finds a reader takes its submission from the reader's document

**Reason**: Every wait follows the host process and runs no round, so a
wait that runs rounds where no driver lives is gone (135; rule core.31).
**Migration**: "A wait takes its submission from the host process's
document".

### Requirement: A reader that stops is replaced within a tick

**Reason**: The reader is the host process, which its followers start
again; no follower becomes the reader (129).
**Migration**: "A host process that stops is started again within a
tick".

### Requirement: The watch holding the lease is the one to replace a stopped reader

**Reason**: Only the host process takes a lease, and it is the one to
read once an older reader lets go (129, 135).
**Migration**: "The host process holding the lease is the one to read".

### Requirement: A reader names the version of sluice it is

**Reason**: The reader is the host process, which names its version as
it becomes a repository's reader.
**Migration**: "The host process names the version of sluice it is".

### Requirement: A newer watch takes an older reader's lease

**Reason**: A newer command has an older host process hand over, and the
host process takes the lease of a reader from before it; no watch takes
a lease (129, 135).
**Migration**: "The host process takes an older reader's lease", and
`host-process`, "A newer sluice's command has an older host process hand
over".

### Requirement: A reader that could not read one branch writes it unread

**Reason**: The reader is the host process; no wait is a reader whose
own submission's branch it reads apart.
**Migration**: "A branch the host process could not read is written
unread".

### Requirement: A one-shot overview reads the records itself

**Reason**: A one-shot overview, a status and the home view are answered
from the host process's last reading and fetch nothing of their own
(129; record 0093 as amended 2026-11-13).
**Migration**: "A one-shot read is answered from the host process's last
reading".
