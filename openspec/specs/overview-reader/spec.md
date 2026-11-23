# overview-reader Specification

## Purpose
One process on a host reads a repository's queues for everyone watching
them: the first watch or wait becomes the reader and keeps the overview
in the host's state, and every other watch and wait on the host follows
it (129; ADR 0056; rule core.25).

## Requirements

### Requirement: A watch that finds a reader follows its document

A `sluice overview --json --watch` SHALL follow its repository's document on a subscription to `sluice host`, and SHALL read no records, policy or checkouts itself. It SHALL print the document at its start and at each change in a part other than its time, each as one line.

Before: a watch read a file under `readers/`. Now: it gets the document on a subscription. (`reads-go-through-the-resident@69134e5825d7`)

#### Scenario: A follower prints what the reader wrote

- **WHEN** the document of `sluice host` holds submission `#7` testing on `main`, which the repository's records do not hold, and `sluice overview --json --watch` runs in the repository
- **THEN** its first line is that document, with `#7` testing, and no line that it prints is read from the records

#### Scenario: A follower prints each change once

- **WHEN** a watch follows `sluice host`, whose document changes from `#7` testing to `#7` landed, and then changes only in its time
- **THEN** the watch prints two lines, the second with `#7` landed

### Requirement: A follower wait reads its own record once its submission leaves the queue

When the reader's document no longer holds a wait's submission among the submissions in play, the wait SHALL ask `sluice host` for that submission's record once. It SHALL report its outcome from the answer, as `submission-wait` states: landed, failed, conflict or withdrawn.

Before: the wait read the record itself. Now: `sluice host` answers it. (`reads-go-through-the-resident@69134e5825d7`)

#### Scenario: Landed while followed

- **WHEN** a wait on `#7` follows `sluice host`, and `#7` lands, so the next document no longer holds it among the queue's submissions
- **THEN** the wait prints `passed · landed as <rev>`, the landing's revision as `sluice host` answers submission 7's record, and exits 0
- **AND** the wait read no file of the records

#### Scenario: Withdrawn while followed

- **WHEN** a wait on `#7` follows `sluice host`, and `#7` is withdrawn
- **THEN** the wait exits 4, as a wait that read the records did, and read no file of the records

### Requirement: A reader serves its own host

`sluice host` SHALL read each repository that it holds apart, as its common git directory names it, and only on its own host. It SHALL keep each repository's document in its own process, and SHALL send it only on its own socket. A watch or a wait of a repository SHALL follow only that repository's document.

Before: each repository's lock and document were files in the host's state. Now: the document is in `sluice host`, and no file holds it.

#### Scenario: Two repositories

- **WHEN** `sluice overview --json --watch` runs in one repository, and a second watch runs in another repository on the host
- **THEN** one `sluice host` reads both, and each watch prints its own repository's queues

#### Scenario: Two worktrees of one repository

- **WHEN** a watch runs in one worktree of a repository, and another watch runs in another worktree of the same repository
- **THEN** both follow the one document that `sluice host` keeps for the repository

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

A `sluice overview --json --watch` SHALL end only when it is stopped or the reader of its output closed it. It SHALL also end when its repository's git directory is gone, or its repository has no sluice. A failed read of the queues, a document that `sluice host` could not send, or a `sluice host` that stopped SHALL NOT end it. A watch whose repository is gone SHALL end as a failing overview does, with its error document on stdout and exit 3.

#### Scenario: The reader's document cannot be written

- **WHEN** a watch follows `sluice host`, and its subscription closes before `sluice host` sends one document
- **THEN** the watch subscribes again at its next tick, prints the overview, and is still running

#### Scenario: The repository is removed

- **WHEN** a watch follows `sluice host`, and its repository's git directory is removed
- **THEN** the watch prints the error document with the read's words, and exits 3

#### Scenario: Stopped while a failure stands

- **WHEN** a watch followed an error document for ten ticks, and it is stopped with Ctrl-C
- **THEN** it ends as a stopped watch does, with exit 130

### Requirement: A follower wait passes over a failure document

A `sluice wait` or waiting `sluice submit` that follows a reader whose
document is an error document SHALL read nothing from it, and nothing of
the records in its place: it SHALL say nothing new, as when there is no
whole document, until the reader writes an overview again.

#### Scenario: The reader cannot read while a wait follows

- **WHEN** `sluice wait 7` follows a reader whose document shows `#7`
  testing, and the reader's document becomes the error document
- **THEN** the wait prints no new line, reads no record of submission 7,
  and is still waiting

#### Scenario: Out of time on a failure

- **WHEN** a follower `sluice wait 7 --timeout 2s --json` last read `#7`
  testing, and the reader's document has been the error document since
- **THEN** it exits 5 and prints submission 7's record as the last
  overview it read held it, with `state` `testing`

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

### Requirement: A watch that follows ends on a document saying the repository has no sluice

A watch that follows a reader SHALL print a reader's document with
`"set_up": false` as its last line and exit 0, reading nothing itself.

#### Scenario: Two watches in a repository with no sluice

- **WHEN** a watch follows a live reader, and the reader's document
  becomes the overview with `"set_up": false`
- **THEN** the follower prints that document and exits 0, and read no
  records, policy or checkouts itself

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

`sluice host` SHALL keep, with each repository's document, when each pass or look of the repository begins and ends, whether or not the document changed. It SHALL NOT make the document again where it did not change. The document SHALL name its tick, the pause between reads, in whole seconds as `tick`: one second while a watch follows the repository, and three otherwise.

Before: the marks were files in the host's state. Now: `sluice host` keeps them in its process.

#### Scenario: A read that changes nothing

- **WHEN** a watch follows `sluice host`, and three of its reads find the queues as they were
- **THEN** the document's content is as it was after the first read, and `sluice host` keeps the end of the third read as its last

#### Scenario: The tick is named

- **WHEN** `sluice overview --json --watch` follows `sluice host`
- **THEN** the document holds `"tick": 1`, and once the watch ended and only `sluice wait` follows, it holds `"tick": 3`

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

### Requirement: The host process is each repository's reader

`sluice host` SHALL be the reader of each repository that it holds. A watch or a wait SHALL follow its repository on a subscription, starting `sluice host` where none answers, and SHALL read no queue itself. While anything follows the repository, `sluice host` SHALL make the whole document of `sluice overview --json` at each change in a part other than its time.

Before: a follower read a file that `sluice host` wrote. Now: it gets the document on its subscription.

#### Scenario: A watch with no reader

- **WHEN** no process of sluice runs on a host, `sluice overview --json --watch` runs in a repository, and a submission is then queued
- **THEN** `sluice host` runs, and the watch prints the overview at its start and then the overview with the submission queued, each a whole `sluice/overview@1` document
- **AND** the watch read no records itself

#### Scenario: A wait with no reader

- **WHEN** no process of sluice runs on a host, and `sluice wait 4` runs in a repository
- **THEN** the document that its subscription gets holds the whole overview, each branch, checkout and preview of the repository, and not only submission 4's branch

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

Where `sluice host` ends by Ctrl-C, a kill or a crash, each watch and wait that follows it SHALL reach it again at its own next tick. It SHALL start `sluice host` where none answers. Until the new `sluice host` sends a document, each follower SHALL keep the last document that it got. It SHALL read no records itself.

Before: a follower read the last document file that the ended `sluice host` wrote. Now: it keeps the last document that its subscription got.

#### Scenario: The host process is killed

- **WHEN** a watch follows `sluice host`, and `sluice host` is killed
- **THEN** within one tick of the watch a `sluice host` runs again
- **AND** the next document that the watch gets is the one that `sluice host` read from the records

#### Scenario: The host process is killed under a wait

- **WHEN** `sluice wait 4` follows `sluice host`, and `sluice host` is killed before submission 4 is decided
- **THEN** within one tick of the wait a `sluice host` runs again, and the wait reports submission 4's outcome once that `sluice host` decides it

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

A one-shot read whose `sluice host` has no current reading of its repository SHALL wait for one within the round's retry bound and a tick. Past that, `sluice host` SHALL answer from the state that it last kept, fetching nothing, and SHALL say on standard error when it last read the repository.

Before: past the bound, the command read the records itself. Now: `sluice host` answers. (`reads-go-through-the-resident@69134e5825d7`)

#### Scenario: A host process held in a slow look

- **WHEN** the repository's remote does not answer, so the reading of it by `sluice host` does not end, and `sluice status main` runs
- **THEN** it prints `main`'s queue as `sluice host` last kept it, says when `sluice host` last read the repository, and exits 0
- **AND** the status ran no git and read no file of the records

### Requirement: A one-shot read is answered from the host process's last reading

`sluice status` in each form, `sluice overview` without `--watch` and the home view SHALL be answered by `sluice host` from a current reading that it keeps, started where none answers. No read command SHALL fetch, read the records or the results, or write the reader's document.

Before: the command read the records and the results itself. Now: `sluice host` answers. (`reads-go-through-the-resident@69134e5825d7`)

#### Scenario: A one-shot overview beside a current reading

- **WHEN** `sluice host` read the repository within its tick, `main` is on a remote, and `sluice overview --json` runs
- **THEN** it prints the reader's document of that reading, the remote is asked nothing, and the command read no record and no result

#### Scenario: A status with no host process

- **WHEN** no `sluice host` runs on a bare install, `main` is on a remote with `#4` queued, and `sluice status main --json` runs
- **THEN** a `sluice host` is running, `#4` is shown as its first reading found it, and the status itself fetched nothing and read no record

#### Scenario: A status during a long round

- **WHEN** a pass of the repository by `sluice host` started to land a combination a minute ago, and `sluice status main` runs
- **THEN** `sluice host` answers at once from the state that it keeps, with nothing fetched

### Requirement: A one-shot read reads what a command changed

A reading the host process began before a command of sluice's on the
host last changed the repository's queues or branches SHALL NOT be
current for a one-shot read. Where the reading a one-shot read is
answered with was made by the host process's look, which writes no
reader's document, the overview and the home view SHALL render the
records as the host holds them (129; rule core.11).

#### Scenario: A one-shot overview after a branch is removed

- **WHEN** the host process read the repository within its tick, and
  `sluice branch delete bolt/x` then removes `bolt/x`, whose records
  stay, and `sluice overview` runs
- **THEN** it shows `bolt/x` gone, as `sluice status bolt/x` says it is,
  never at its last revision as the reading before the removal had it

### Requirement: A repository held again is read afresh

When `sluice host` holds a repository again after it let the repository go, it SHALL read the repository before it sends a document of it. It SHALL send no finding of an earlier holding, such as an overview with `"set_up": false`.

#### Scenario: sluice set up after a watch ended

- **WHEN** a watch ended on the overview with `"set_up": false`, sluice is then set up so that `main` holds `.config/sluice.toml`, and a watch runs again
- **THEN** it prints the overview with no `set_up` field, and is still running

#### Scenario: A watch that starts during the first reading

- **WHEN** `sluice host` holds a repository again after a watch ended on `"set_up": false`, and a watch starts before the first reading ends
- **THEN** the watch prints nothing until that reading ends, and does not end
