# refused-writes Specification

## Purpose
How a write sluice's branch protection refused is kept once per cause
and host in the branch's state, counted, shown by `sluice status` and
`sluice overview` until a day passes with none, and ended in the log, so
an operator and a tool that watches the host read it from sluice (124,
148).

## Requirements

### Requirement: A refused write's cause is its command on its host

A refused write's cause SHALL be the branch's ref, the host whose ref
was tried, and the command that tried it, with every whole word of the
command that is 40 or 64 hexadecimal digits read as `<revision>`.
Nothing else of the command SHALL change. Two refused writes SHALL be
one cause exactly when all three are the same (148; record 0082).

#### Scenario: A timer's update-ref with new revisions each run

- **WHEN** the protection on `build-1` refuses `git -C /home/me/app
  update-ref refs/heads/main 8079db3… 10e4791…`, each in full, and later
  the same command with two other full revisions
- **THEN** both are one cause, whose command is `git -C /home/me/app
  update-ref refs/heads/main <revision> <revision>`
  (`the_same_command_with_other_revisions_is_one_cause` in
  `crates/sluice-conformance/tests/refused_writes.rs`)

#### Scenario: An abbreviated revision is kept as written

- **WHEN** the command is `git branch -f main 8079db3`
- **THEN** its cause's command is `git branch -f main 8079db3`
  (`command_of` unit tests in `crates/sluice-core/src/refused_writes.rs`)

#### Scenario: Two hosts

- **WHEN** the same command is refused on `build-1` and on `build-2`,
  whose state is the same branch's on its remote
- **THEN** they are two causes, one per host
  (`two_hosts_are_two_causes`)

### Requirement: A cause's first refusal is logged, and the rest counted

The first refusal of a cause that does not stand SHALL be logged as
`protected`, as branch protection records it. While a cause stands, each
later refusal of it SHALL add no line to the log. It SHALL add one to the
cause's `times`, set its `last` to when it was refused, and set its
`from` and `to` to that write's (148; rule core.47).

#### Scenario: Three refusals of one command

- **WHEN** one command is refused three times on `build-1` within an
  hour
- **THEN** the branch's log has one `protected` line for it, and its
  cause has `times` 3, `first` the first refusal's time, `last` the
  third's, and the third's `from` and `to`
  (`one_line_for_a_cause_refused_three_times`)

### Requirement: The count is kept in the branch's state

Each standing cause SHALL be kept in the branch's `branch.json`, under
`refused_writes`, with `host`, `ref`, `command`, `times`, `first`,
`last`, `from` and `to`, `to` being `null` for a deletion. Nothing SHALL
count a cause from the log's lines. A part of the log closing, or the
log's lines being anywhere but the live state, SHALL change no cause and
nothing status or the overview shows of it (131, 132; rule core.41).

#### Scenario: The log moved aside

- **WHEN** a command has been refused twice on `bolt/x` and its
  `log.jsonl` is then moved to `log/1.jsonl`, as a part's close moves it
- **THEN** `sluice status bolt/x --json` still gives its cause with
  `times` 2 (`a_refused_writes_count_is_in_the_state_not_the_log` in
  `crates/sluice/tests/protect.rs`)

#### Scenario: Records written before

- **WHEN** a branch's `branch.json` has no `refused_writes` and its log
  holds `protected` lines with no `host`
- **THEN** it reads with no cause standing, and the lines read as before
  (`records_written_before_refused_writes_still_read`)

### Requirement: A cause stands until a day passes with none of its writes refused

A cause SHALL stand while less than a day has passed since its `last`.
A cause that no longer stands SHALL be shown nowhere. The first write
of the branch's state that changes something after its day SHALL take
it out and log `protected-ended` with its `host`, `ref`, `command`,
`times`, `first` and `last`, at its `last` and a day. A write of it
refused later SHALL open it anew, logged as its first was (148).

#### Scenario: Refused again within the day

- **WHEN** a cause was last refused 23 hours ago and is refused again
- **THEN** no line is added and its `times` grows by one
  (`a_cause_refused_within_its_day_still_stands`)

#### Scenario: A fixed caller

- **WHEN** a cause's last refusal was at 16:01:20 on 2026-11-02 and
  nothing of it is refused after
- **THEN** from 16:01:20 on 2026-11-03 `sluice status` and `sluice
  overview` show it no more, and the branch's next write logs
  `protected-ended` at `2026-11-03T16:01:20Z` with its `times`, `first`
  and `last` (`a_fixed_caller_ends_a_day_after_its_last`)

#### Scenario: Refused after its end

- **WHEN** that command is refused again on 2026-11-04
- **THEN** the log gains a new `protected` line and its cause has
  `times` 1 (`a_cause_refused_after_its_end_opens_anew`)

### Requirement: At most twenty causes stand per host and branch

A host SHALL keep at most twenty causes standing on one branch. A
twenty-first SHALL end at once the cause of that host whose `last` is
oldest, its `protected-ended` line logged at that time (148; record
0082).

#### Scenario: Twenty-one commits on main

- **WHEN** twenty-one `git commit` commands, each with its own message,
  are refused on `main` on one host
- **THEN** twenty causes stand, and the log has twenty-one `protected`
  lines and one `protected-ended`, for the first
  (`a_twenty_first_cause_ends_the_oldest`)

### Requirement: Status shows each standing cause above the queue

`sluice status <branch>` SHALL say, above the queue, that sluice refused
writes to the branch in the last day by commands that are not sluice's.
Then, for each standing cause, newest `last` first, it SHALL give the
command's first line, cut after 100 characters with `…`, and under it
the host, `times`, `first`, `last` and the last write's revisions, or
`deleting it` for a deletion (148; rule core.47).

#### Scenario: The timer's cause in status

- **WHEN** `main` names `remote = "origin"` and its cause on `build-1`
  has `times` 2024, `first` 2026-10-30T05:50:23Z, `last`
  2026-11-02T16:01:20Z, `from` `10e4791` and `to` `8079db3`
- **THEN** `sluice status main` says `sluice refused writes to main in
  the last day, by commands that are not sluice's:`, then `git -C
  /home/me/app update-ref refs/heads/main <revision> <revision>`, then
  `on build-1: 2024 times, first 2026-10-30T05:50:23Z, last
  2026-11-02T16:01:20Z, from 10e4791 to 8079db3`, then `sluice sets each
  host's main to each landing itself: stop what runs these`; and
  `--json` carries `refused_writes` with that cause
  (`status_shows_the_writes_its_protection_refused` in
  `crates/sluice/tests/cli.rs`)

#### Scenario: None standing

- **WHEN** no cause stands on `main`
- **THEN** status says nothing of refused writes, and `--json` carries
  `"refused_writes": []` (`status_says_nothing_of_no_refused_writes`)

### Requirement: Status says what to do and carries the causes in `--json`

After the causes, status SHALL say what to do: on a branch whose policy
names a remote, `sluice sets each host's <branch> to each landing
itself: stop what runs these`; on any other, `only sluice moves
<branch>: submit the change to sluice instead`. `sluice status <branch>
--json` SHALL carry `refused_writes`, each standing cause whole, newest
`last` first, and `[]` when none stand (124, 148).

#### Scenario: A branch kept in this host's repository

- **WHEN** `bolt/x` names no remote and a cause stands on it
- **THEN** status's last line of it says `only sluice moves bolt/x:
  submit the change to sluice instead`
  (`status_tells_a_local_branchs_refused_writes_what_to_do`)

#### Scenario: A deletion

- **WHEN** a standing cause's last refused write was `git branch -D
  bolt/x` at `10e4791`
- **THEN** status says `from 10e4791, deleting it`, and `--json` gives
  its `to` as `null` (`status_names_a_refused_deletion`)

### Requirement: The overview shows them under the branch's line

`sluice overview` SHALL give each branch with standing causes, under its
line, a line per cause, newest `last` first and at most three: `sluice
refused <times> writes on <host>, last <last>: <command>`, the command
cut as status cuts it. Where more stand, a last line SHALL say `and <n>
more: sluice status <branch>` (26, 148).

#### Scenario: Five causes standing

- **WHEN** five causes stand on `main`
- **THEN** the overview shows the three refused last, then `and 2 more:
  sluice status main` (`the_overview_shows_three_and_how_many_more`)

### Requirement: The overview's document carries them

`sluice overview --json` SHALL give each branch `refused_writes`, as
status lists them, read from the live state alone, so every host's
reader, watch and follower gives them, and `[]` where none stand (26,
129, 131).

#### Scenario: Five causes in the document

- **WHEN** five causes stand on `main`
- **THEN** `sluice overview --json` lists all five under `main`'s
  `refused_writes` (`the_overviews_document_carries_every_cause`)

#### Scenario: A follower

- **WHEN** `sluice run` reads the queue on the host and a watch follows
  its document
- **THEN** the watch's document gives `main` the same `refused_writes`
  (`a_followers_overview_carries_refused_writes` in
  `crates/sluice/tests/overview_watch.rs`)

### Requirement: Nothing is decided from a refused write

No round, gate, landing or submission SHALL read `refused_writes`, and
a standing cause SHALL make no branch in play for `sluice run` (rule
core.14).

#### Scenario: A round beside a standing cause

- **WHEN** a cause stands on `main` and a submission is queued
- **THEN** the round forms, tests and lands as it would with none
  (`a_round_beside_a_standing_cause_is_unchanged`)

### Requirement: A tool that watches the host reads them from sluice

The site's commands page and the setup skill SHALL tell an operator, and
a tool that watches the host, to read refused writes from `sluice
overview --json`'s `branches[].refused_writes`, or `sluice status
<branch> --json`'s `refused_writes`, and never from `log.jsonl` or
`branch.json` (148; the user, 2026-11-05). What the pages say is kept by
their writers and reviewers; no task reads them (principle 16; core.21).

#### Scenario: The pages say where

- **WHEN** an operator reads the overview's fields on the commands page
  and the traps of the remote branches profile
- **THEN** each names `refused_writes` and says not to read the log for
  them
