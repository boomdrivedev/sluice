# remote-read-log Specification

## Purpose
How a read of a branch's remote that a round could not make, of the
branch or of its state, is written to the branch's log once per cause
and host with its end, so whoever reads the queue after can read its
stalls there (133).

## Requirements

### Requirement: A read the round could not make is logged on the branch
When a round cannot read the branch's revision, or its state, from the
branch's remote, the branch's log SHALL gain a `read-refused` line
naming the host, the remote, the ref asked for, and what git said of
the refusal, whole and unchanged. sluice's own note on the refused
credentials SHALL NOT be part of it (133).

#### Scenario: A fetch of the branch is refused
- **WHEN** a round on `build-1` fetches `main` from `origin` and git says `remote: Repository not found.` then `fatal: repository 'https://github.com/acme/app.git/' not found`
- **THEN** `main`'s `log.jsonl` gains `{"event":"read-refused","host":"build-1","remote":"origin","asked":"refs/heads/main","said":"remote: Repository not found.\nfatal: repository 'https://github.com/acme/app.git/' not found"}` with its time

#### Scenario: A fetch of the state is refused
- **WHEN** `main`'s state is on `origin` and a round cannot fetch `refs/sluice/state/main`
- **THEN** the line, once written, names `refs/sluice/state/main` as asked

#### Scenario: The note on the credentials stays out of the line
- **WHEN** the refusal stood after sluice tried a fresh credential, and sluice's error names each refused credential and its age
- **THEN** the line's `said` holds git's words alone, and the round's error still names the credentials

### Requirement: A refusal is logged once per cause and host
A host's refusal SHALL be logged once for each distinct ref asked for
and words said for as long as it stands on that host, never once per
round. A cause already logged while the refusal stands SHALL NOT be
logged again when it comes back. Another host's rounds SHALL log their
own refusals apart (133).

#### Scenario: Forty rounds refused alike
- **WHEN** forty rounds on `build-1` in a row are refused with the same words
- **THEN** the log holds one `read-refused` line for them

#### Scenario: The words change while it stands
- **WHEN** the refusal's words change from `Repository not found` to `The requested URL returned error: 503` and back again before any read succeeds
- **THEN** the log holds one line for each of the two causes

#### Scenario: Two hosts refused
- **WHEN** rounds on `build-1` and on `build-2` are both refused
- **THEN** the log holds a line for each host

### Requirement: The end of a refusal is logged with how long and how many rounds
When a round on a host whose refusal stands reads the branch and its
state again, the log SHALL gain a `read-again` line naming the host,
the remote, when the refusal began, how many seconds it stood and how
many of that host's rounds it refused. A round that never reached the
remote, its branch's lock held elsewhere, SHALL count as no round
(133).

#### Scenario: The remote answers again
- **WHEN** `build-1`'s rounds were refused from 09:16:40 for twelve rounds, and its round at 09:46:40 reads `main` from `origin`
- **THEN** `main`'s log gains `{"event":"read-again","host":"build-1","remote":"origin","since":"2026-10-26T09:16:40Z","stood":1800,"rounds":12}` with the time of that round

#### Scenario: A refusal starts again after it ended
- **WHEN** a read succeeds and a later round is refused again with the same words
- **THEN** the log gains a new `read-refused` line, and a new `read-again` when that one ends

### Requirement: The host keeps a standing refusal until it can be written
While a refusal stands, the host SHALL keep its causes, its first time
and its count of rounds in its own state, apart from the branch's
records. A line that could not be written when it happened, because
the state is on the refusing remote, SHALL be written in the host's
first write to the branch's state after, at the time it happened, and
once written SHALL NOT be written again (73, 133).

#### Scenario: The state is on the refusing remote
- **WHEN** `main`'s state is on `origin`, `build-1`'s rounds cannot fetch it from 09:16:40 to 09:46:40, and then one can
- **THEN** that round's write adds the `read-refused` line timed 09:16:40 and the `read-again` line timed 09:46:40, and the log gains nothing from the rounds between

#### Scenario: The state is on the host
- **WHEN** `main` is on `origin` with its state in the host's own files, and a round cannot fetch `main`
- **THEN** that round writes the `read-refused` line before it fails

#### Scenario: Nothing is decided from the kept refusal
- **WHEN** a host keeps a standing refusal of `main`
- **THEN** no round, gate, landing or submission reads it, and only `sluice status` and the next write of the log do

### Requirement: The round that could not read does nothing else
A round that could not read the branch or its state SHALL form, start
and land nothing, and SHALL end as it does today, its error saying why
to whoever ran it, beside the log and never in its place. Checks
already running SHALL keep running (133; record 0017).

#### Scenario: A waiting submit meets a refusal
- **WHEN** `sluice submit main` waits and the round it runs cannot fetch `main`
- **THEN** it prints why the fetch failed as it does today, and the branch's log holds the `read-refused` line too

### Requirement: Status shows this host's standing refusal
While a refusal stands on this host, `sluice status <branch>` SHALL
say above the queue that this host could not read the ref asked for
from the remote, since when, for how many rounds, and what git said,
and SHALL carry the same under `refused` in `--json`; once it has
ended, status SHALL show nothing of it (record 0061).

#### Scenario: Status during a refusal
- **WHEN** `build-1`'s rounds of `main` have been refused for twelve rounds since 09:16:40
- **THEN** `sluice status main` on `build-1` says it could not read `refs/heads/main` from `origin` since 09:16:40, twelve rounds, with git's words, and `--json` carries `refused` with `remote`, `asked`, `said`, `since` and `rounds`

#### Scenario: Status once it ends
- **WHEN** a round on `build-1` has read `main` again
- **THEN** `sluice status main` and its `--json` say nothing of a refusal

### Requirement: An intake refusal is logged once per cause and host with its end
When a host's intake of a branch's pull requests fails, the branch's log
SHALL gain a `read-refused` line with `"intake":true`, naming the host,
the branch's remote, what intake asked, `<branch>'s pull requests` or
`pull/<n>`, and the forge's words, once for each distinct cause while it
stands on that host, and a `read-again` line so marked at the first
intake on that host that met no failure. The round's own lines SHALL
carry no `intake` (133; rule core.28).

#### Scenario: A list GitHub refuses
- **WHEN** `build-1`, keeping main, asks GitHub for main's open pull requests and GitHub answers `502 Bad Gateway` for twelve passes, then the list
- **THEN** main's log holds one `read-refused` line with `"intake":true`, `"asked":"main's pull requests"` and GitHub's words, and one `read-again` with `"intake":true` and `"rounds":12`

#### Scenario: Two requests refused
- **WHEN** `build-1`'s intake cannot fetch the heads of `pull/12` and `pull/13`
- **THEN** main's log holds a `read-refused` line marked `intake` for each

#### Scenario: A round reads the branch meanwhile
- **WHEN** that refusal stands and each round reads `main` and its records from `origin`
- **THEN** main's log gains no `read-again` until intake reads the list again

### Requirement: The host keeps a standing intake refusal apart from the round's
While an intake refusal stands, the host SHALL keep its causes, its
first time and its count of intakes in its own state beside, and apart
from, its refusal of the branch's remote, and SHALL write a line not
written when it happened in its first write to the branch's state after.
No round, gate, landing or submission SHALL read it; only `sluice
status` and the next write of the log do (73, 133; rule core.14).

#### Scenario: Kept apart
- **WHEN** `build-1`'s intake of main is refused and its rounds' fetch of main is refused too
- **THEN** each refusal is kept, logged and ended on its own

### Requirement: Status shows this host's standing intake refusal
While an intake refusal stands on this host, `sluice status <branch>`
SHALL say above the queue that this host could not take in the branch's
pull requests, since when, for how many intakes, what it asked and the
forge's words, and SHALL carry the same under `intake_refused` in
`--json`; once it has ended, status SHALL show nothing of it (133).

#### Scenario: Status during an intake refusal
- **WHEN** `build-1`'s intake of main has been refused for twelve passes since 09:16:40
- **THEN** `sluice status main` on `build-1` says it could not take in main's pull requests since 09:16:40, twelve rounds, with GitHub's words, and `--json` carries `intake_refused` with `remote`, `asked`, `said`, `since` and `rounds`
