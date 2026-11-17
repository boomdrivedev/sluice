# inbox Specification

## Purpose
How a session that starts, after a restart or on another host, or a
caller's place that never waited, collects every outcome pending for its
own submissions by one command, `sluice inbox`, each then marked
delivered so it is delivered once (51, 135; rule core.31; ADRs 0068,
0070; flume 133, flume C.6).

## Requirements

### Requirement: sluice inbox collects a worktree's pending outcomes

`sluice inbox`, run in a worktree, SHALL hand over every outcome on any
branch owed or expired to a reader the worktree reads as: its branch,
for a submission it made, whose recorded work is its branch and which
names no `from.head`; or `pull/<n>`, for a pull request's opened from
its branch, where that branch is one sluice lands on nowhere. An
outcome its reader has confirmed, and a submission withdrawn, still
queued or under test, SHALL NOT be handed over (51; weir 105).

#### Scenario: A restarted session collects a landing it missed

- **WHEN** submission 12, submitted from worktree branch `unit-a` with
  no multiplexer, lands on `main`, and a session then runs `sluice inbox`
  in that worktree
- **THEN** the command hands over submission 12's landing on `main`

#### Scenario: A session that stopped before it acted collects again

- **WHEN** the round pushed submission 12's landing into `unit-a`'s pane,
  the session stopped before it confirmed it, and its next session runs
  `sluice inbox` in that worktree
- **THEN** the command hands over submission 12's landing (S76)

#### Scenario: Another worktree's outcomes are not delivered

- **WHEN** submission 13 from worktree branch `unit-b` is blamed, and
  `sluice inbox` runs in the worktree on branch `unit-a`
- **THEN** the command hands over nothing of submission 13

#### Scenario: Outcomes on several branches are collected together

- **WHEN** worktree branch `unit-a` is owed a landing on `main` and a
  conflict on `bolt/x`
- **THEN** one run of `sluice inbox` in that worktree hands over both

#### Scenario: A failure resubmitted is not delivered

- **WHEN** submission 14 from `unit-a` was blamed, and `unit-a` submitted
  again, which withdrew 14
- **THEN** `sluice inbox` in that worktree hands over nothing of 14

#### Scenario: A submission still under test is not delivered

- **WHEN** submission 15 from `unit-a` is under test
- **THEN** `sluice inbox` in that worktree hands over nothing of 15

#### Scenario: A pull request from the worktree's branch is collected

- **WHEN** worktree branch `unit-a`, which no `[branches]` table names,
  pushed `unit-a` and opened a pull request into `main`, whose
  submission 16 is blamed and still owed to the request because its
  post failed, and `sluice inbox` runs in that worktree
- **THEN** the command hands over submission 16's failure

### Requirement: An outcome is delivered in a wait's words

Without `--json`, each outcome SHALL be printed as the notice the round
pushes into a pane for it, naming the submission, its branch and its
outcome, followed by what `sluice wait <id>` prints for it, then, unless
the call confirms on receipt, one line naming `sluice confirm` and the
outcome's key, which for an expired outcome first says it expired and
when; outcomes apart by a blank line, and nothing at all when nothing
is owed.

#### Scenario: A blame is delivered with its failed checks and logs

- **WHEN** submission 16 from `unit-a` was blamed for `cli:test`, and
  `sluice inbox` runs in that worktree
- **THEN** the output opens with a line naming `#16`, `main`, `failed`
  and `cli:test`, followed by the lines `sluice wait 16` prints for it,
  its check's log among them, and ends with a line naming `sluice
  confirm 16:failed`

#### Scenario: An expired outcome says so

- **WHEN** submission 13's blame, owed to `unit-a`, expired at
  `2026-11-15T10:02:00Z`, and `sluice inbox` runs in that worktree
- **THEN** its last line says it expired at `2026-11-15T10:02:00Z` and
  names `sluice confirm 13:failed`

### Requirement: With --json, the inbox prints one document of records

With `--json`, `sluice inbox` SHALL print one document holding
`outcomes`, a list, `[]` when nothing is owed, and `unread` only when a
branch could not be read. Each element SHALL be the submission's record
as `sluice wait <id> --json` prints it, with its `delivery` as this run
leaves it, and `notice`, the one line.

#### Scenario: Nothing pending

- **WHEN** `sluice inbox --json` runs where nothing is owed
- **THEN** it prints a document whose `outcomes` is `[]`, holds no
  `unread`, and exits 0

### Requirement: The JSON document is a contract that only gains fields

Each element of `outcomes` SHALL hold `id`, `branch`, `state`
(`landed`, `failed`, `conflict` or `refused`), `notice`, `delivery` and
`from`; a landed one `landed_as`; a failed one `result` (`failed`, or
`timed out`) and `failed`, each with its `task` and, where kept, its
`log`; a conflicting one `conflict` with its `paths` and its `with`; a
refused one `refused` (152). A later sluice SHALL keep these names and
meanings, and MAY add fields; `delivered` and `delivered_to` are gone.

#### Scenario: The JSON document lists the records

- **WHEN** `sluice inbox --json` hands over submission 12's landing on
  `main` as `abc1234`
- **THEN** it prints one document whose `outcomes` holds one element
  with `id` 12, `branch` `main`, `state` `landed`, `landed_as`
  `abc1234`, a `delivery` whose `key` is `12:landed` and whose `handed`
  ends with a hand-over by `inbox`, a `notice` naming `#12` and
  `landed`, and no `delivered`

#### Scenario: A conflict names its paths

- **WHEN** `sluice inbox --json` hands over submission 14's conflict
  with submission 11 in `src/lib.rs`
- **THEN** its element holds `state` `conflict`, `conflict.paths`
  holding `src/lib.rs`, and `conflict.with` holding submission 11

#### Scenario: A landing the forge refused gives its words

- **WHEN** `sluice inbox --json` hands over the refusal of submission
  1's landing on `main`
- **THEN** its element holds `state` `refused`, `refused` with the
  forge's `words` and its `at`, a `notice` naming `#1` and those words,
  and a `delivery` whose `key` is `1:refused`
  (`a_landing_the_forge_refused_is_told_with_its_words` in
  `crates/sluice/tests/cli.rs`)

### Requirement: The command and the round never both deliver one outcome

While it reads, prints and records a branch's outcomes, the command
SHALL hold that branch's lock, so the round SHALL NOT push a notice for
an outcome the command handed over. A command stopped between printing
and recording SHALL leave the outcome owed with no hand-over by it, to
be handed over again under the same key.

#### Scenario: The round does not push what the command delivered

- **WHEN** submission 12 landed in a herdr pane whose push was refused,
  and `sluice inbox` then hands it over
- **THEN** no later round prompts any pane for submission 12, and its
  `delivery` holds one hand-over, by `inbox`

### Requirement: A branch that cannot be read stops no other

When a branch's records cannot be read, the command SHALL still deliver
the outcomes of every branch it could read, SHALL say on stderr which
branch it could not read in the reader's own words, and SHALL exit 3.
Under `--json`, the document SHALL also hold `unread`, each with the
branch and those words.

#### Scenario: One unreadable branch

- **WHEN** worktree `unit-a` has a pending landing on `main`, and the
  records of `bolt/x` cannot be read
- **THEN** `sluice inbox` delivers the landing on `main`, says on stderr
  that `bolt/x` could not be read and why, and exits 3

### Requirement: A worktree on no branch has nothing pending

A worktree on no branch has submitted nothing, so `sluice inbox` run
there SHALL print nothing (`outcomes` `[]` under `--json`) and exit 0.

#### Scenario: A detached worktree collects

- **WHEN** `sluice inbox` runs in a worktree on no branch
- **THEN** it prints nothing and exits 0

### Requirement: A place submitted with no session collects its outcome by inbox

A submission made by `sluice submit <branch> --no-wait --json` with no
multiplexer's variables in its environment SHALL record no session;
`sluice inbox --json` run in the submitting worktree SHALL then hand
the outcome over each time it runs until its reader confirms it, and
`sluice inbox --confirm --json` SHALL hand it over and confirm it once.

#### Scenario: A caller that never waits collects a failure

- **WHEN** a caller runs `sluice submit main --no-wait --json` in
  worktree `/repo/places/unit-a` with no herdr variables, then `sluice
  step main` until submission 21 is blamed for `cli:test`, then `sluice
  inbox --json` in that worktree twice, then `sluice inbox --confirm
  --json`, then `sluice inbox --json`
- **THEN** `main`'s log holds one `pending` line for 21 and no further
  attempt; each of the first two prints one element with `id` 21,
  `state` `failed`, `failed` holding `cli:test`, a `notice` naming
  `#21`, and `delivery.state` `owed`; the third prints 21 with
  `delivery.confirmed` set; the fourth prints `outcomes` `[]`

#### Scenario: A caller that never waits collects a landing

- **WHEN** the same caller's submission 22 lands on `main` as `def5678`
  by `sluice step main`, and `sluice inbox --json` runs in its worktree
- **THEN** it prints one element with `id` 22, `state` `landed` and
  `landed_as` `def5678`

### Requirement: The round pushes nothing for a submission with no session

The round that decides a submission that recorded no session SHALL push
nothing, log one `pending` line, and try no more. `sluice step
<branch>` SHALL run one round of the branch's queue, its outbox
included.

#### Scenario: A failure with no session is pushed nowhere

- **WHEN** a caller runs `sluice submit main --no-wait --json` with no
  herdr variables, then `sluice step main` until submission 21 is
  blamed for `cli:test`
- **THEN** `main`'s log holds one `pending` line for 21 and no further
  attempt

### Requirement: No worktree collects a submission by name or a request from a branch sluice lands on

`sluice inbox` SHALL NOT deliver, in any worktree, a submission by name,
whose `from.head` and `from.worktree` are both the branch it named
(124), the worktree on that branch among them; nor a pull request's
opened from a branch sluice lands on, since no worktree pushed that
branch (1, 124).

#### Scenario: A submission by name is no worktree's

- **WHEN** submission 17, `bolt/x` submitted to `main` by name, landed,
  and `sluice inbox` runs in a worktree that has checked out `bolt/x`
- **THEN** the command delivers nothing of submission 17

#### Scenario: A pull request from a branch sluice lands on is no worktree's

- **WHEN** `bolt/x` is a branch sluice lands on, submission 18 is a pull
  request's from `bolt/x` into `main`, blamed and not delivered, and
  `sluice inbox` runs in a worktree that has checked out `bolt/x`
- **THEN** the command delivers nothing of submission 18

### Requirement: A folder with no files checked out collects as a worktree on its branch

`sluice inbox` run in a folder with no files checked out, a bare
repository or a git directory, SHALL collect what the worktree on the
branch the folder's `HEAD` names would collect, and nothing else,
reading neither the folder's files nor `HEAD`'s commit. It SHALL say
nothing on stderr because no files are checked out, and exit 0 where
every branch was read. Where `HEAD` names no branch it SHALL collect
nothing (2, 3, 51; record 0099).

#### Scenario: The git directory collects nothing of a worktree's

- **WHEN** submission 13 from worktree branch `unit-a` was blamed and is
  owed, and `sluice inbox --json` runs in the repository's git
  directory, whose `HEAD` names `main`
- **THEN** it prints a document whose `outcomes` is `[]`, says nothing on
  stderr and exits 0, and 13 stays owed with no hand-over

#### Scenario: A git directory collects its branch's own

- **WHEN** submission 13 from worktree branch `unit-a` was blamed and is
  owed, and `sluice inbox --json` runs with `SLUICE_AGENT` `agent-a` and
  no multiplexer in `unit-a`'s own git directory, which has no files
  checked out
- **THEN** its `outcomes` holds 13, handed over by `inbox`, and `main`'s
  log holds a `handed` line for 13 whose `to` is `agent-a`

#### Scenario: A HEAD whose branch has no commit

- **WHEN** `sluice inbox --json` runs in a folder with no files checked
  out whose `HEAD` names a branch with no commit, and nothing is owed
  for that branch
- **THEN** it prints a document whose `outcomes` is `[]`, says nothing on
  stderr and exits 0

### Requirement: Each outcome collected is handed over and stays owed

Each outcome the command prints SHALL be recorded as handed over by
`inbox` to its reader, once its words are written, and SHALL stay owed:
the command SHALL hand it over again each time it runs until its reader
confirms it (51; weir 102, 103, 105).

#### Scenario: Collecting twice hands over twice

- **WHEN** `sluice inbox` handed over submission 12's landing, nobody
  confirmed it, and `sluice inbox` runs again in the same worktree
- **THEN** the second run prints 12's landing again, and 12's `delivery`
  holds two hand-overs by `inbox`

#### Scenario: A confirmed outcome is collected no more

- **WHEN** `sluice confirm 12:landed` ran in the worktree, and `sluice
  inbox` then runs there
- **THEN** it prints nothing and exits 0

### Requirement: --confirm confirms what the command hands over

`sluice inbox --confirm` SHALL confirm each outcome it hands over, for
its reader, in the same write as its hand-over, and print no confirm
line, for a caller that keeps what it is given for readers of its own
(weir 103).

#### Scenario: A caller that keeps its outcomes

- **WHEN** submission 21 from `unit-a` is blamed, and `sluice inbox
  --confirm --json` runs in that worktree, then `sluice inbox --json`
- **THEN** the first gives 21 with `delivery.state` `done` and a
  `confirmed` time, and the second prints `outcomes` `[]`

### Requirement: --new hands over only what no way has handed over

`sluice inbox --new` SHALL hand over only the outcomes owed to the
worktree's readers that no way has handed to their reader yet, so a
harness that collects on each new outcome hands over no notice the round
pushed and no outcome an earlier collect handed over.

#### Scenario: A notice already pushed

- **WHEN** the round pushed submission 13's notice into the session's
  pane, and `sluice inbox --new` runs in its worktree
- **THEN** it prints nothing and records nothing

#### Scenario: A new outcome with an older one owed

- **WHEN** `unit-a` is owed submission 12's landing, handed over by an
  earlier `sluice inbox`, and submission 14's blame, handed over by
  nothing, and `sluice inbox --new` runs there
- **THEN** it prints 14's blame alone
