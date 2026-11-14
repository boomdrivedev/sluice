# queue-home Specification

## Purpose
Where a branch's queue lives, the host's own files or a remote's refs,
and how it moves between them (record 0122; C.6; rule core.59). The home
is written in the branch's records, and every process reads it there and
routes by the policy only where no home holds the branch. A move is
written step by step: the move in the current home, then the live
submissions carried with their numbers once nothing is under test, then
the old home closed with a pointer. A branch whose signers go comes home
to each host's own files, and a submission is found wherever its home's
pointer names it.

## Requirements

### Requirement: A branch's queue has one home, written in its records

A branch's queue SHALL have one home, the host's own files or a remote's
refs, written in the branch's records as `home.json` with the revision of
the policy that chose it. The branch's lock, its numbers and its
submissions SHALL live in that home and nowhere else. A branch with no
home anywhere SHALL have one written by the first write that needs it:
on its remote where the policy at its head names signers and a remote
for it, in the host's own files otherwise (record 0122; C.6).

#### Scenario: The first submission writes the home

- **WHEN** a branch's policy at its head has `[signers.box]` and `remote = "origin"` for it, no home of it exists, and a submission is made to it
- **THEN** origin's `refs/sluice/state/<branch>` holds the submission and a `home.json` naming `origin` and the policy's revision, and the host's own files of the branch are not written

#### Scenario: A branch with no signers

- **WHEN** a branch's policy names no `[signers]` and a submission is made to it
- **THEN** the host's own files of the branch hold the submission and a `home.json` naming the host's own files

### Requirement: Every process reads the home from the records

Every process SHALL read a branch's home from the branch's records: this
host's own `home.json` of it, then the state's on each remote that may
hold it as last fetched (129), and SHALL route the branch by its policy
only where neither holds a home. A process that keeps running SHALL read
the home again at each of its passes, never keeping it for its life. Two
homes of one branch that are both open or moving SHALL refuse that
branch alone, naming both (26).

#### Scenario: A policy that names another home does not move the reading

- **WHEN** a branch's queue is open in this host's own files and a landing brings a policy naming signers and `remote = "origin"` for it
- **THEN** `sluice status <branch>`, a submit and the host process's next pass all read the branch's records in this host's own files until a move closes them

#### Scenario: A host with no records of the branch

- **WHEN** another host's queue of `bolt/x` is open on origin and this host holds no records of `bolt/x`
- **THEN** a submit to `bolt/x` on this host writes its submission on origin under origin's next number

#### Scenario: Two open homes

- **WHEN** this host's own files and origin's state of `bolt/x` both hold a `home.json` that is open
- **THEN** a round, a submit and a status of `bolt/x` are refused naming both homes, and every other branch is read as before

### Requirement: A move is written in the current home and opens no combination until the runs under test end

A round that keeps a branch's open home and reads a policy at the
branch's head naming another home SHALL write the move in the current
home, logged `moving`, and from then SHALL form and open no combination.
Combinations already under test SHALL go on, their verdicts read and
what passed landed, and submissions SHALL still be taken in. A head
whose policy names the open home again SHALL end the move, logged
`stayed` (record 0122; 58).

#### Scenario: A move waits for a combination under test

- **WHEN** a combination of `bolt/x` is under test in this host's own files, a landing brings a policy naming signers and `remote = "origin"`, and a submission is made meanwhile
- **THEN** the round logs `moving`, opens no combination for the new submission, and carries nothing while the combination is under test

#### Scenario: The policy names the open home again

- **WHEN** a move of `bolt/x` to origin is written and a landing brings back a policy naming no signers before the move carries anything
- **THEN** the next round logs `stayed`, the queue stays open in this host's own files, and combinations form again

### Requirement: A move carries every live submission with its number and state, and closes the old home with a pointer

Once no combination of a moving branch is under test and no landing is
unsettled, the round SHALL carry every live submission to the new home,
each with its number and its state, with the owed deliveries, and write
the new home's `home.json` open, naming the home it came from; then it
SHALL close the old home with a pointer to the new one. A submission
whose write meets a closed home SHALL be written in the home its pointer
names (record 0122; 62, 63).

#### Scenario: A host's queue moves to origin

- **WHEN** submissions #52 queued and #53 blamed are live on `bolt/x` in this host's own files, a move to origin is written, and no combination is under test
- **THEN** origin's state of `bolt/x` holds #52 queued and #53 blamed under those numbers, origin's next number is past 53, the host's own `home.json` of `bolt/x` is closed naming origin, and the round logs `carried` naming both numbers

#### Scenario: A submit racing the carry

- **WHEN** a submit read `bolt/x`'s home in this host's own files as moving, and the carry closed it before the submit's write
- **THEN** the submit's write is refused there and written on origin, and the submission is queued on origin

### Requirement: A move stopped halfway is finished by the next round

Each step of a move SHALL be written before the next begins, and a round
that finds a move written SHALL go on from the step it reached: carrying
where the new home holds none of it, closing where the new home names
the old one as where it came from and the old one is still moving. No
submission SHALL be carried twice (record 0122; 59).

#### Scenario: Stopped after the new home was written

- **WHEN** a round wrote `bolt/x`'s submissions and `home.json` on origin and stopped before closing this host's own home
- **THEN** the next round closes this host's own home naming origin, writes nothing more on origin, and each submission is queued once

### Requirement: A branch comes home to each host's own files

When a remote home's branch has a policy at its head naming no signers,
its lead SHALL close the remote home toward the hosts' own files once no
combination is under test, and each host SHALL carry the live
submissions it made into its own files, the lead in that round and every
other host the first time a round of its own reads the closed remote
home, writing their pointers in the remote home (record 0122; 26; C.6).

#### Scenario: Two hosts' submissions come home

- **WHEN** `bolt/x` is open on origin with #7 made on host A and #8 made on host B, a landing brings a policy naming no signers, and no combination is under test
- **THEN** after A's round and B's next round, A's own files hold #7 and B's own files hold #8, each queued under its number, and origin's `home.json` of `bolt/x` is closed naming each submission and the host that took it

### Requirement: A submission whose host never takes it home stays readable

A submission left in a closed remote home because its host has not
carried it SHALL stay readable there, and its status SHALL say it waits
for its host to take it home (26).

#### Scenario: A host that has not come back

- **WHEN** `bolt/x` came home and host C, which made #9 on origin, has not run sluice since
- **THEN** `sluice status 9 --branch bolt/x` on any host that reads origin answers #9 queued in origin's closed home, waiting for C to take it home

### Requirement: A carried submission keeps its number

A carried submission SHALL keep its number where the new home's branch
holds no submission of that number, live or finished, and the new home's
count SHALL be raised past the highest number carried (record 0122;
record 0114; rule core.41).

#### Scenario: Numbers past the remote's count

- **WHEN** this host's own queue of `bolt/x` holds #300 and moves to origin, whose next number is 40
- **THEN** origin holds #300 under that number, and origin's next submission takes a number past 300

### Requirement: A carried number the new home holds takes the next one

A number the new home's branch already holds SHALL be carried under the
new home's next number, the submission's record naming its old number
in `was` and the home it came from in `carried_from`, and the old home's
pointer naming both. A status of the old number on the host that held it
SHALL name the number it has now (record 0122; 26; rule core.41).

#### Scenario: Two hosts each held #52

- **WHEN** host A's and host B's own files each held #52 on `bolt/x`, and A's queue moved to origin first
- **THEN** B's #52 is carried to origin under origin's next number, its record names `was` 52 and `carried_from` B's own files, and `sluice status 52` on B names the number it has now

### Requirement: A closed home keeps its history

A closed home SHALL keep its finished submissions, their index and its
log, and nothing SHALL write them again but its pointer. A status of a
submission that finished before the move SHALL read it in the closed
home (rule core.41).

#### Scenario: A submission landed before the move

- **WHEN** #40 landed on `bolt/x` in this host's own files, and the branch's queue then moved to origin
- **THEN** `sluice status 40` on this host answers #40 landed, read from this host's closed home

### Requirement: No setting names how a queue runs

No key of a host's settings or of a branch's policy SHALL choose how a
branch's queue runs, which host leads it or where it executes; each SHALL
be refused as a key sluice does not know (137; record 0093 as amended
2026-11-08).

#### Scenario: A host's settings name a lead

- **WHEN** `~/.config/sluice/config.toml` has `[peers] lead = true`
- **THEN** sluice refuses the settings naming `lead`

#### Scenario: A policy names how a branch runs

- **WHEN** `.config/sluice.toml` has `[branches.main] runs = "daemon"`
- **THEN** the policy is refused naming `runs`

### Requirement: A branch's status names its home and its move

`sluice status <branch>` and the overview SHALL name the branch's home,
and while a move is written, where it goes, since when, and how many
combinations under test it waits for; the JSON form SHALL give them as
`home` (26).

#### Scenario: A move waiting

- **WHEN** `bolt/x` is moving from this host's own files to origin since 10:00 with one combination under test
- **THEN** `sluice status bolt/x` says it is moving to origin since 10:00, waiting for 1 combination under test, and `--json` gives `home` with `at` and `moving`
