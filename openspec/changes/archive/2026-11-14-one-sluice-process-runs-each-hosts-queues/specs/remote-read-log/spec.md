## ADDED Requirements

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
