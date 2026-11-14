# forecast Specification

## Purpose
Whether a head, or a branch no worktree holds, would conflict if
submitted now, and what the forecast was formed from: told to whoever can
act on it, and deciding nothing (64, 65, 47a; ADR 0039).

## Requirements

### Requirement: A branch is forecast with no worktree of it

`sluice submit <branch> --check --head <from>` SHALL forecast the
revision of the branch `<from>` against `<branch>` and every submission
queued on it, as `--check` forecasts a worktree's head (64, 65, 47a),
leaving out the submissions made from `<from>` (67). It SHALL need no
worktree holding `<from>`, write no record and move no branch. A
`<from>` that does not exist SHALL be refused with exit 2.

#### Scenario: A bolt against main

- **WHEN** `bolt/y` changes `builds.md`, `#1` on `main` changes the same
  lines, and no worktree holds `bolt/y`
- **THEN** `sluice submit main --check --head bolt/y` prints
  `would conflict: builds.md with #1 (<agent>, <worktree>)` and exits 2,
  and `main`'s records keep their version

#### Scenario: Its own submission left out

- **WHEN** a submission made from `bolt/y` is queued on `main`, and
  `bolt/y` has since moved
- **THEN** the forecast of `bolt/y` is formed without that submission

#### Scenario: No such branch

- **WHEN** `sluice submit main --check --head bolt/gone --json` runs
- **THEN** it prints an error document whose message is `no branch
  bolt/gone to forecast`, and exits 2

### Requirement: A forecast says what it was formed from

`sluice submit --check --json` SHALL print, beside `conflict`, `head`
(the revision forecast), `onto` (the branch's revision it was formed on)
and `ahead` (the submissions formed ahead of it, in order). These SHALL
be added fields, and `conflict` SHALL be as before: null, or `{paths,
with}`.

#### Scenario: Clean behind one

- **WHEN** `#1` is queued on `main` and the worktree's head merges
  cleanly behind it
- **THEN** the document is `{"conflict": null, "head": "<head>", "onto":
  "<main's revision>", "ahead": [1]}`

### Requirement: A forecast takes the round's combinations

A forecast SHALL take each combination a round formed on the branch's
current revision with exactly the submissions ahead of the head up to
it, and form again only what no such combination holds. What it reports
SHALL be the same as forming every one again.

#### Scenario: Two combinations formed

- **WHEN** a round has formed the combinations of `#1` and of `#1` and
  `#2` on `main`'s revision, and a third worktree is forecast
- **THEN** the forecast forms one merge, the head on the second
  combination

### Requirement: exec forecasts before it runs

Before it waits for room, `sluice exec <branch>` SHALL forecast the
worktree's head on the branch's revision its run is held to, say `would
conflict: <paths> with <what>` when it would conflict, and run as it
would otherwise. `--json` SHALL carry it as `forecast`, in `submit
--check --json`'s shape, or null when none was formed. It SHALL never
change the run or its exit, and nothing of it SHALL be recorded (64;
session-runs-1 clause 92).

#### Scenario: The branch moved under the work

- **WHEN** `main` has landed a change to `a.txt` since the worktree's
  head changed it differently, and the agent runs `sluice exec main`
- **THEN** exec first says `would conflict: a.txt with main`, then runs
  the checks, and exits as their outcome says

#### Scenario: Nothing to say

- **WHEN** the head merges cleanly
- **THEN** exec says nothing of a forecast, and `--json` gives
  `"forecast": {"conflict": null, …}`

### Requirement: A forecast takes the branch from a live reader and fetches nothing

Where the host's reader of the repository is live and keeping its tick,
and its last read holds the branch read whole, `sluice submit <branch>
--check` SHALL form on the branch's revision and the submissions queued
on it as that read found them, and SHALL ask the branch's remote
nothing. What it prints and how it exits SHALL be what forming on that
revision and queue gives (64, 129).

#### Scenario: A remote out of reach
- **WHEN** a watch on the host reads the repository, `main` is on a remote and `#1` is queued on it, and the remote then cannot be reached
- **THEN** within the watch's tick of its last read, `sluice submit main --check` prints the forecast against `#1` and exits as it says, and nothing it runs asks the remote anything

#### Scenario: Formed from what the reader read
- **WHEN** a live reader's last read, within its tick, says `main` is at revision `R` with `#4` queued, and the host's records say otherwise
- **THEN** `sluice submit main --check --json` gives `"onto": "R"` and `"ahead": [4]`

### Requirement: A branch forecast with no worktree is taken from the reader too

`sluice submit <branch> --check --head <from>` SHALL take `<from>`'s
revision from the same read where the reader's last read holds
`<from>`, and SHALL then fetch nothing for it either. Where the read
does not hold `<from>`, its revision SHALL be read as it is today (64).

#### Scenario: A bolt against main, with nothing fetched
- **WHEN** a live reader keeping its tick last read `main` and `bolt/y`, both on a remote, and the remote then cannot be reached
- **THEN** `sluice submit main --check --head bolt/y` prints the forecast of `bolt/y`'s revision as the reader read it, and exits as it says

### Requirement: A reader keeps its tick while it reads on time

A forecast SHALL count the reader as keeping its tick only while the
host process answers and either its last read of the repository ended no
more than its tick ago, or it is reading now and its last read ended no
more than a tick before this read began. Any other reader SHALL be read
as none (64, 129).

#### Scenario: A reader that has stalled
- **WHEN** the host process answers but its last read of the repository ended five ticks ago and it began no read since
- **THEN** a forecast of `main` asks the host process for a reading and forms from it

#### Scenario: A reader in a slow read
- **WHEN** the reader's last read ended a second before its next began, and that next read has run for twenty seconds
- **THEN** a forecast takes the branch from the last read and fetches nothing

### Requirement: A forecast with no current reader has the host process read

Where no reader keeps its tick, or its last read lacks the branch or a
revision the forecast forms from, a forecast SHALL ask the host process for a current reading, starting it where none
answers, and form on every branch as the host holds it since. Where the
host still lacks a revision it forms from, it SHALL ask for a reading
afresh once, and say what is still missing as a read it could not make,
exit 3. It SHALL fetch nothing itself (64, 129; record 0093 as amended
2026-11-13).

#### Scenario: No reader on the host

- **WHEN** no host process runs and `main` is on a remote
- **THEN** `sluice submit main --check` starts the host process, forms on
  `main` and its records as that process's first reading fetched them,
  and fetches nothing itself

#### Scenario: Another host's submission

- **WHEN** `#9` was submitted to `main` from another host, its work is
  not on this host, and no reader keeps its tick
- **THEN** `sluice submit main --check` forecasts against `#9`, whose
  work the host process's reading fetched, rather than failing

#### Scenario: A reader whose read lacks the branch

- **WHEN** a reader keeps its tick and its last read names no revision
  of `main`, which is on a remote
- **THEN** `sluice submit main --check` forms on `main` and its records
  as the host holds them since the host process's reading, and fetches
  nothing itself

#### Scenario: Work still missing after the reading afresh

- **WHEN** the host lacks the head of `#4`, queued on `main`, and the
  host process's reading afresh does not bring it
- **THEN** `sluice submit main --check` says it could not read what a
  forecast of `main` forms from, naming the work queued on `main`, and
  exits 3, having fetched nothing

#### Scenario: The reader could not reach the remote

- **WHEN** the host process's reading, within its tick, could not ask
  `main`'s remote and read `main`'s records as last fetched
- **THEN** a forecast of `main` forms on what that reading holds and says
  on stderr, in the remote's words, that it could not be asked

### Requirement: A branch a current reading brings no revision of is no branch

A forecast whose reading afresh is current, refused by no remote, and
brings no revision of the branch, or of `--head`'s branch, to the host,
at its own ref or a remote's, SHALL refuse that branch as no branch,
exit 2, as a forecast refuses a branch that does not exist (64).

#### Scenario: No such branch once the reading is current

- **WHEN** the host process's reading afresh is current, and the host
  holds no revision of `bolt/gone` at any ref
- **THEN** `sluice submit main --check --head bolt/gone --json` prints an
  error document whose message is `no branch bolt/gone to forecast`, and
  exits 2

#### Scenario: A remote refused the reading

- **WHEN** the host process's reading afresh could not ask `main`'s
  remote, and the host holds no revision of `bolt/y` at any ref
- **THEN** `sluice submit main --check --head bolt/y` says it could not
  read what a forecast of `main` forms from, naming `bolt/y`, and exits 3
