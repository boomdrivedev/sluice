## MODIFIED Requirements

### Requirement: A policy with signers keeps a remote branch's state on the remote
A branch whose queue has no home, and whose policy at its head names a
`remote` in a policy with a `[signers]` table, SHALL have its home on
that remote, its state kept under `refs/sluice/`. A branch with no
`remote`, or in a policy with no `[signers]`, SHALL have its home in the
host's own files. Once a branch has a home, the home its records name
SHALL be where its state is kept, whatever a later policy names, until a
move takes it elsewhere (queue-home; record 0122).

#### Scenario: Signers move the state to the remote
- **WHEN** a policy has `[signers.box]` and `[branches.main] remote = "origin"`, `main` has no home yet, and a submission is made to `main`
- **THEN** origin holds `refs/sluice/state/main`, and the host's own state files for `main` are not written

#### Scenario: No signers, no change
- **WHEN** a policy has no `[signers]` table and `main` names `remote = "origin"`
- **THEN** `main`'s state is kept in the host's own files, and nothing under `refs/sluice/` is pushed

## REMOVED Requirements

### Requirement: Moving a branch's state to the remote keeps its queue

**Reason**: The first host to lead pushed its own queue and every other
host submitted its queue again under new numbers, a side effect of
whichever lock first routed the branch. Record 0122 writes the move step
by step in the branch's records, each host carrying its own submissions
under their numbers (queue-home).

**Migration**: Drain each host's queues before installing the release
that carries this. Nothing reads the `sluice-remote/moved/` notes an
older sluice wrote.
