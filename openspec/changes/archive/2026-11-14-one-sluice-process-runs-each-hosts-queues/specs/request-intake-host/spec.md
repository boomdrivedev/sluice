## MODIFIED Requirements

### Requirement: One host takes a branch's pull requests in
A branch's pull requests SHALL be taken in only where its records are on
its remote, by the host that keeps the branch there, under intake's
lock. No other host SHALL ask GitHub for the branch's open pull
requests, and no host SHALL keep, read or push an intake mark,
`refs/sluice/intake/<branch>`, or read one an older sluice left
(58, 72, 137; rule core.33).

#### Scenario: Records on the remote
- **WHEN** another host keeps main and its lease has not passed by this host's observation
- **THEN** a `sluice wait` on this host asks GitHub nothing about main's open pull requests

#### Scenario: Records on each host
- **WHEN** main's policy names no `[signers]` and gives main a `pull_requests` table, and `sluice step main` runs on host A
- **THEN** main is refused naming `[signers]`, and A asks GitHub nothing about main's open pull requests

#### Scenario: The host that keeps it takes them in
- **WHEN** host `a` keeps main, and a person opens `#12` into main with every condition holding
- **THEN** `a`'s next pass submits `#12`, and nothing is pushed to `refs/sluice/intake/main`

#### Scenario: A host that stopped
- **WHEN** host `a`, which kept main, stopped, and host `b` has seen main's records unchanged for the lease with `#12` waiting
- **THEN** `b` keeps main and takes `#12` in

#### Scenario: An older mark is read by nothing
- **WHEN** the remote holds `refs/sluice/intake/main` naming host `c`, which an older sluice pushed, and host `a` keeps main
- **THEN** `a` takes main's pull requests in, and no fetch of main brings that ref

### Requirement: A wait whose pull request another host takes in runs nothing for it
A `sluice wait` or waiting `sluice submit` whose pull request another
host takes in, the host that keeps its branch, SHALL take nothing in
and run no round for it. It SHALL follow the pull request by GitHub's
word, open, merged or closed, and sluice's check on its head, each read
conditionally and no more often than once in a quarter of the host's
`discovery` (137).

#### Scenario: Read at its pace
- **WHEN** a wait on host A follows `#12`, which host B, keeping main, takes in, for one minute with `discovery` at sixty seconds
- **THEN** it read `#12` and its check from GitHub no more than four times each, and ran no round and no intake

### Requirement: A host keeps the pull requests it follows
Where another host keeps a branch, a host SHALL keep in its state,
under `forge/follows/`, each pull request into it a submit on the host
opened, updated or marked ready, and each a wait or status on it
followed, with what it last read of each. A pull request SHALL leave
once a read finds it merged or closed and a wait or status has said so,
or once the branch's queue holds it at its head (137).

#### Scenario: Submitted here, taken in there
- **WHEN** host `b` keeps main, and `wt submit main --no-wait` on host `a` opens `#12`
- **THEN** `a`'s `forge/follows/` names `#12`, and `a` asks GitHub nothing about main's open pull requests

#### Scenario: Handed over
- **WHEN** main's records hold `a`'s submission `<id>` of `#12`, which an older sluice withdrew for a hand-over to `b`, and `sluice wait <id>` on `a` follows `#12`
- **THEN** `a`'s `forge/follows/` names `#12`

#### Scenario: Merged
- **WHEN** `a` follows `#12`, `b` lands it, and `sluice status main` on `a` reads it merged
- **THEN** that status says `#12` landed, and `a`'s `forge/follows/` names `#12` no longer

### Requirement: A wait reads a held pull request by its draft mark
Where sluice's check on the head of a pull request that a wait follows
on GitHub says it waits for the next submit, the wait SHALL read it as
sent back while the pull request is a draft, and, once a ready mark has
ended the draft, as waiting to be taken in again by the host that keeps
the branch, which the wait goes on following (120, 137, C.5).

#### Scenario: Held, then marked ready
- **WHEN** B, keeping main, blamed `#12` and marked it a draft, its check saying `waiting: the next submit`, and a submit on A marks `#12` ready for review at the same head before B's next pass
- **THEN** a wait on A reports `#12` waiting to be taken in by B, and goes on following it

### Requirement: A wait on a submission this host handed over follows its pull request
A `sluice wait <id>` whose submission an older sluice withdrew for a
hand-over SHALL follow its pull request while it is open and no draft,
never reporting the submission withdrawn (122, 137; rule core.48).

#### Scenario: A wait on a submission handed over follows its pull request
- **WHEN** main's records hold submission `<id>` of `pull/12`, withdrawn with `handed_over_to` `b`, `#12` is open and no draft, and `sluice wait <id> --timeout 2s` runs
- **THEN** it exits 5 with `#12` named as the pull request it follows

## ADDED Requirements

### Requirement: A host that does not keep the branch shows each request it follows
For each pull request it follows whose submission the branch's queue
does not hold at its head, `sluice status <branch>` on a host that does
not keep the branch SHALL show the host the branch's records name as
keeping it, as `taken_in_by` `{"host"}`, `null` where none does, and the
pull request's queue as sluice's check on its current head carries it,
read from no record of the pull request (137).

#### Scenario: Waiting for a review there
- **WHEN** host `b` keeps main and `#13`, opened from `a`, needs one review
- **THEN** `sluice status main` on `a` says `#13` waits to be taken in by `b`, needing 1 review, and `--json` gives `taken_in_by` `{"host":"b"}`

#### Scenario: No check on its head yet
- **WHEN** `b` keeps main and `a`'s submit has just pushed a new head to `#12`, on which no `sluice` check stands
- **THEN** `sluice status main --json` on `a` gives `#12` a `queue` of `null`, and its words say it waits to be taken in by `b`

#### Scenario: The keeper and the check name different hosts
- **WHEN** host `c` has just taken main from `b` after its lease, and `#13`'s check, posted by `b`, still says it waits to be taken in on `b`
- **THEN** `sluice status main --json` on `a` gives `taken_in_by` naming `c` and `#13`'s `queue` naming `b`

#### Scenario: Queued on the other host
- **WHEN** `b` keeps main and has taken `#12` in as submission 1406
- **THEN** `sluice status main` on `a` shows submission 1406 in main's queue and no row of `#12` beside it

### Requirement: A withdrawal an older sluice wrote for a hand-over holds no pull request
No submission SHALL be withdrawn for a hand-over. A withdrawal whose
record names `withdrawn.handed_over_to`, or, naming none, whose `by` is
`<host> takes <branch>'s pull requests in` as sluice 0.60.0 to 0.70.2
wrote every hand-over, SHALL hold no pull request: the host that keeps
the branch SHALL take it in again at its head, with no draft. Any other
withdrawal SHALL hold one as before (120; record 0088 as amended).

#### Scenario: Carried onto the remote
- **WHEN** main's records, moved onto its remote when its policy named `[signers]`, hold `pull/12`'s submission withdrawn with `handed_over_to` `b`, and the host that keeps main reads `#12` open and ready at that head
- **THEN** it takes `#12` in again, and `#12` is never a draft

#### Scenario: An older operator's withdrawal
- **WHEN** a submission of `pull/12`, opened by `sluice submit`, was withdrawn by the operator before withdrawals named a host
- **THEN** `pull/12` waits for its submitter's act as before

### Requirement: A pull request keeps one number from intake to landing
A pull request taken in SHALL keep the one submission it was taken in
as, and its number, until that submission lands or is withdrawn by its
own submitter's act, a new head, a draft or a close, however many hosts
follow the pull request or run sluice beside the host that keeps its
branch (120, 137).

#### Scenario: Two hosts beside the keeper
- **WHEN** host `a` keeps main and takes `#1` in as submission 358, and hosts `b` and `c` each pass main ten times while `#1`'s combination is tested
- **THEN** `#1` lands as submission 358, and main's records hold no other submission of `pull/1`

## REMOVED Requirements

### Requirement: The intake mark is kept by a leased push
**Reason**: No mark is kept (137; record 0093, "No intake mark").
**Migration**: The host that keeps a branch whose records are on its remote takes its requests in; a remote's `refs/sluice/intake/<branch>` may be deleted by hand.

### Requirement: The intake mark is taken by rule
**Reason**: No mark is taken (137; record 0093).
**Migration**: The lead's lease decides who keeps a branch (72, 73).

### Requirement: A wait lets the intake mark go
**Reason**: No wait holds a mark (135, 137).
**Migration**: None; `forge/intake/` in a host's state may be removed by hand.

### Requirement: A pull request's submission is advanced only on the host that takes it in
**Reason**: No hand-over: one host keeps a branch's records on its remote and takes its requests in, so no host withdraws its submissions for another (137; rule core.33).
**Migration**: A branch whose records each host keeps names `[signers]` to take pull requests.

### Requirement: A host that does not take the pull requests in shows each as the taking host's
**Reason**: It named the host the intake mark names and the mark's holder (137; record 0093).
**Migration**: "A host that does not keep the branch shows each request it follows" names the host the branch's records name as keeping it, as `taken_in_by` `{"host"}`, with no holder.

### Requirement: A hand-over ends the runs it made obsolete
**Reason**: No hand-over is made (137).
**Migration**: None.
