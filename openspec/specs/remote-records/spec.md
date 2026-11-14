# remote-records Specification

## Purpose
Keeps a branch's state on its remote, under `refs/sluice/`, so every host
that submits to the branch reads and writes the same state and none of
them has to be running for the others to go on (80,
82, 109; C.6 remote refs).

## Requirements

### Requirement: A policy with signers keeps a remote branch's state on the remote
A branch whose policy names a `remote`, in a policy with a `[signers]`
table, SHALL keep its state on that remote under `refs/sluice/`. A
branch with no `remote`, or in a policy with no `[signers]`, SHALL keep
its state in the host's own files exactly as today.

#### Scenario: Signers move the state to the remote
- **WHEN** a policy has `[signers.box]` and `[branches.main] remote = "origin"`, and a submission is made to `main`
- **THEN** origin holds `refs/sluice/state/main`, and the host's own state files for `main` are not written

#### Scenario: No signers, no change
- **WHEN** a policy has no `[signers]` table and `main` names `remote = "origin"`
- **THEN** `main`'s state is kept in the host's own files, and nothing under `refs/sluice/` is pushed

### Requirement: The branch's state is one ref written only from the version read
A branch's state SHALL be one commit under `refs/sluice/state/<branch>`,
its tree the branch's state documents and its id the version. Every
write SHALL push a new commit leased on the commit read, and a write
whose lease fails SHALL be refused and reported as written since
(62, 63).

#### Scenario: Two hosts write from one version
- **WHEN** two hosts read `refs/sluice/state/main` at the same commit and both push a write
- **THEN** exactly one push is accepted, and the other host reads its write as refused and reads the state again

### Requirement: Each signer's runs have a ref of their own
Every signer's run records SHALL be kept under `refs/sluice/runs/<signer>`
and only added to. A write there SHALL never be refused because another
signer wrote, and SHALL never wait on the branch's state (109).

#### Scenario: Two signers record at once
- **WHEN** the `box` signer and the `forge` signer each record a result at the same moment
- **THEN** both are kept, one under `refs/sluice/runs/box` and one under `refs/sluice/runs/forge`, and neither write moved `refs/sluice/state/main`

#### Scenario: Two hosts of one signer record at once
- **WHEN** two session hosts that both sign as `sessions` record results at the same moment
- **THEN** both results are kept under `refs/sluice/runs/sessions`, and no earlier result is lost

### Requirement: Submitted heads and open combinations are fetchable by every host
From the moment a submission is written until it leaves sluice, its head
SHALL be on the remote under `refs/sluice/heads/<oid>`. A combination
SHALL be open to claims only while it is on the remote under
`refs/sluice/combinations/<branch>/<id>` (82). Each
SHALL be removed from the remote once nothing needs it.

#### Scenario: A head another host can test
- **WHEN** a session on host A submits to `main`
- **THEN** host B can fetch the submission's head from `refs/sluice/heads/<oid>` before any combination holding it is tested

#### Scenario: Gone once landed
- **WHEN** a submission lands and no combination holds it
- **THEN** its `refs/sluice/heads/<oid>` and every combination ref that held it are gone from the remote

### Requirement: A failed result's output is kept on the remote
The output kept of a failed combination's tasks SHALL be pushed under
`refs/sluice/logs/<branch>/<run>`, and the blamed submission's state
SHALL name where to read it from any host. Only the last 20 failed
combinations of a branch SHALL keep theirs.

#### Scenario: Reading a failure from another host
- **WHEN** a combination fails on host B and its submitter is on host A
- **THEN** `sluice status` on host A shows the failed tasks and reads their output from `refs/sluice/logs/main/<run>`

### Requirement: The state is readable from any host while another is unreachable
`sluice status`, `sluice overview` and every read of a submission SHALL
read the branch's state from the remote, as it was last written, with
no other host running (80). When the remote cannot
be reached, a read SHALL show the state last fetched and say it could
not be refreshed.

#### Scenario: The lead's host is off
- **WHEN** the host that last advanced `main` is switched off
- **THEN** `sluice status main` on any other host shows every submission's state as last written

#### Scenario: The remote is unreachable
- **WHEN** the remote cannot be reached
- **THEN** `sluice status main` shows the state last fetched and says it could not refresh it

### Requirement: Moving a branch's state to the remote keeps its queue
When a branch's state first moves to the remote, the first host to lead
it SHALL push its own queued submissions as the starting state. Every
other host SHALL submit its own queued submissions again under new ids,
and `sluice status` with an old id SHALL name the new one.

#### Scenario: Two hosts each had a queue
- **WHEN** hosts A and B each had submissions queued on `main` in their own files, and a landing adds `[signers]` to the policy
- **THEN** every submission of both hosts is queued in `refs/sluice/state/main`, and `sluice status` on host B with an old id names its new id

### Requirement: A round that read the state last fetched advances nothing
When a round's read of a branch's state on its remote could not be
made and was answered from the state last fetched, the round SHALL
form, start and land nothing, and SHALL fail saying the read failed.
Every other read SHALL still show the state last fetched and say it
could not refresh it (80, 133).

#### Scenario: A round with the state's fetch refused
- **WHEN** `main`'s state is on `origin` and a round cannot fetch `refs/sluice/state/main`, a fetch made earlier being kept on the host
- **THEN** the round forms, starts and lands nothing and fails saying the read failed, while `sluice status main` shows the state last fetched and says it could not refresh it

### Requirement: The signers a branch's records count are read from its revision on the remote
Where a branch's records are on its remote, the signers whose runs count,
and the keys their runs are verified by, SHALL be read from the policy,
whole, at the branch's revision on that remote, fetched, and when the
remote can't be reached, as last fetched. They SHALL NOT be read from the
host's own copy of the branch (27, 124). While its remote holds no
revision of the branch and the host no tracking ref of it, no signer's
runs SHALL count for it.

#### Scenario: A host whose own copy is refused counts the remote's signers
- **WHEN** two hosts share origin, `main`'s policy on origin names `[signers.box]` with box's key and `remote = "origin"`, and the second host's own `main` holds that policy with a `[[checkout.cache]]` table added
- **THEN** on the second host `sluice status main` reads `main`'s state from origin, and a round there counts the runs box signed and lands the submission they pass

#### Scenario: A branch its remote does not hold yet
- **WHEN** `release`'s policy routes it to origin, origin holds no `release` and the host no tracking ref of it, and the host's own `release` names `[signers.box]`
- **THEN** no run counts for `release` until origin holds it, and the host's own `release` is not read for its signers

### Requirement: A branch's runs are verified with its own policy's signers alone
The runs read for one branch's gate SHALL be verified with the signers
and keys its own policy names, never with those another branch on the
same remote names, whichever branch a process read first (78, 27). When
the branch's revision on its remote moves, its signers and keys SHALL be
read again at the new revision before any run is verified for it, and
while that revision stands they SHALL NOT be read again (56).

#### Scenario: Another branch's key for the same signer
- **WHEN** `main` and `release` keep their records on origin, `main`'s policy on origin lists key A for `[signers.box]` and `release`'s lists key B, and one process reads `main` first and then `release`
- **THEN** a pass signed with key A counts for `main` and is missing for `release`, and a pass signed with key B counts for `release` and is missing for `main`; read in the other order, the same

#### Scenario: A signer only the second branch names
- **WHEN** `main`'s policy on origin names `[signers.box]` alone, `release`'s names `[signers.box]` and `[signers.ci]` and trusts `ci`, and one process reads `main` first
- **THEN** a pass `ci` signed for `release`'s candidate counts for `release`

#### Scenario: A key added while a driver runs
- **WHEN** `sluice run` is driving `main`, whose policy on origin lists key A for `box`, and a landing on origin adds key B to `box`
- **THEN** once that driver has fetched `main`'s new revision, a pass signed with key B counts for `main`, with no restart, and the driver reads `main`'s policy for its signers no more often than `main`'s revision moves
