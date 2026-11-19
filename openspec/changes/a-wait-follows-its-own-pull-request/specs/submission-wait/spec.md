## ADDED Requirements

### Requirement: A wait names a pull request by its number

`sluice wait <branch> --pull-request <n>` SHALL follow pull request
`<n>` into `<branch>` from any folder of the repository, on any host, as
`sluice wait <branch>` follows the pull request a worktree opened, with
the same outcomes, exits and documents. It SHALL take as its own the
first submission of the request it reads, and SHALL never end with
another submission as its own outcome (135, 137; rule core.48).

#### Scenario: Held on another host, out of time

- **WHEN** host `b` keeps `main` and holds pull request `#12` as
  submission 1406, and `sluice wait main --pull-request 12 --timeout 2s
  --json` runs in the repository's main checkout on host `a`
- **THEN** it exits 5 and prints `pull_request` 12, `taken_in_by`
  `{"host":"b"}`, `queue` with `"submission":1406` and `"host":"b"`,
  and `wait` `sluice wait main --pull-request 12`, with no `id`

#### Scenario: Landed on another host

- **WHEN** the same, and `b` lands `#12`, which GitHub then shows merged
- **THEN** `sluice wait main --pull-request 12 --json` on `a` prints
  `pull_request` 12 with `state` `landed`, and exits 0

#### Scenario: Taken in here, out of time

- **WHEN** `main` takes pull requests with no conditions, this host took
  `#12` in as submission `<id>` whose combination is under test, and
  `sluice wait main --pull-request 12 --timeout 2s --json` runs in the
  repository's main checkout
- **THEN** it exits 5 and prints the record of submission `<id>`, as
  `sluice wait <id> --timeout 2s --json` would

#### Scenario: Another worktree's pull request

- **WHEN** worktree `unit/a` opened `#12` and worktree `unit/b` opened
  `#13`, this host took both in, GitHub refuses every post of their
  outcomes, so neither is confirmed on its request (C.5), and `sluice
  wait main --pull-request 12` runs in `unit/b` until `#12` lands
- **THEN** it reports `#12`'s submission landed and exits 0, never
  `#13`'s, and `sluice inbox` in `unit/a` then hands over `#12`'s
  landing, which the wait handed over to no one (51; rule core.52)

### Requirement: A wait refuses a pull request it cannot follow

`--pull-request` with a submission's number as the target SHALL be a
command line sluice cannot read, exit 64. Where `<branch>` takes no pull
requests, or the forge holds no pull request `<n>` into `<branch>`,
`sluice wait <branch> --pull-request <n>` SHALL follow nothing, say `no
pull request #<n> into <branch>`, and exit 1, under `--json` with the
not-found error document.

#### Scenario: With a submission's number

- **WHEN** `sluice wait 12 --pull-request 12` runs
- **THEN** it exits 64, saying `--pull-request` names a pull request
  into a branch, as `sluice wait <branch> --pull-request <n>`

#### Scenario: Into another branch

- **WHEN** pull request `#12` is into `bolt/x`, and `sluice wait main
  --pull-request 12 --json` runs
- **THEN** it prints `{"error":{"kind":"not_found",...}}` whose message
  is `no pull request #12 into main`, and exits 1

#### Scenario: A branch that takes no pull requests

- **WHEN** `bolt/x` takes no pull requests, and `sluice wait bolt/x
  --pull-request 12` runs
- **THEN** it says `no pull request #12 into bolt/x` and exits 1

### Requirement: An undecided pull request's document names its wait

A wait or a waiting submit that runs out of time or is stopped with
Ctrl-C while this host holds no submission of its pull request SHALL
give, in the document it prints for the pull request, `wait`, `sluice
wait <branch> --pull-request <n>`, and SHALL say in words `still open:
#<n>; sluice wait <branch> --pull-request <n>`, never a wait that reads
the folder it runs in (137, C.5). A submission's record, printed once
this host holds the request's submission, SHALL gain no field.

#### Scenario: Out of time, held on another host

- **WHEN** host `b` keeps `main` and holds `#12` as submission 1406, and
  `sluice wait main --timeout 2s --json` runs in `#12`'s worktree on
  host `a`
- **THEN** it exits 5, and its document gives `wait` `sluice wait main
  --pull-request 12` beside `pull_request`, `taken_in_by` and `queue`

#### Scenario: Out of time, waiting for a review, in words

- **WHEN** an agent's pull request `#12` waits for a review, and `sluice
  wait main --timeout 2s` runs in the agent's worktree
- **THEN** it exits 5, and its last line is `still open: #12; sluice
  wait main --pull-request 12`

#### Scenario: Taken in here, out of time

- **WHEN** this host took the agent's `#12` in as submission `<id>`,
  still under test, and `sluice wait main --timeout 2s --json` runs in
  the agent's worktree
- **THEN** it exits 5 and prints `<id>`'s record with no `wait`, and
  without `--json` it says `still waiting: sluice wait <id>`

### Requirement: A wait that could not read its pull request names itself again

A wait or a waiting submit that follows pull request `<n>` and ends on a
read of the forge it could not make (134) SHALL name, as the command to
run again, `sluice wait <branch> --pull-request <n>`, or `sluice wait
<id>` where it follows the request for submission `<id>`, in its words
and as its error document's `next`, never `sluice wait <branch>`, which
follows the pull request of the folder it runs in.

#### Scenario: The forge never answers

- **WHEN** every read of `#12` fails as a forge that could not answer
  does, for as long as the retry policy tries, while `sluice wait main
  --pull-request 12 --json` follows it
- **THEN** it exits 3 and prints the internal error document whose
  `next` is `["sluice wait main --pull-request 12"]`
