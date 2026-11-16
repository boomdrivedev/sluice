# run-signing Specification

## Purpose
Lets a branch's gate know who made a result when every host writes the
state: a result counts only when a signer the policy names has signed
it, and what it claims about how it ran counts only when that signer may
claim it (78, 79, 108, 27, 28, 29, 69).

## Requirements

### Requirement: The policy names signers and what each may claim
The policy SHALL accept `[signers.<name>]` tables, each with exactly one
identity, `ssh` (an allowed-signers line, or the path of a file of them
in the repository) or `github` (a workflow path and ref), and the
claims it may make: `confined`, default false, and `systems`, default
every system. A signer with no identity or two SHALL be refused.

#### Scenario: Two identities
- **WHEN** `[signers.box]` has both `ssh` and `github`
- **THEN** the policy is refused and the message names `signers.box`

#### Scenario: A file of session keys
- **WHEN** `[signers.sessions]` has `ssh = ".config/sluice.sessions"`
- **THEN** a result signed by any key listed in that file, read from the branch's revision, is signed by `sessions`

### Requirement: A remote branch's gate trusts signers by name
On a branch whose state is on the remote, `trust` SHALL list signer
names and `confined = true` SHALL ask for confined results. A name that
is no signer, or an executor kind, in that `trust` SHALL be refused with
a message naming it. A remote branch with no `trust` SHALL trust no
signer (27, 28).

#### Scenario: An executor kind on a remote branch
- **WHEN** `[branches.main]` has `trust = ["sandbox"]` and `main`'s state is on the remote
- **THEN** the policy is refused, and the message names `sandbox` and says `trust` lists signers

#### Scenario: No trust named
- **WHEN** a remote branch's table has no `trust`
- **THEN** no result counts for its gate, and `sluice status` says its gate trusts no signer

### Requirement: A result counts only when a trusted signer signed it
A result SHALL count for a gate only when its record is signed by a
signer the gate trusts and the signature verifies from the records and
the policy alone. A record with no signature, or one by a signer the
gate does not trust, SHALL never satisfy the gate, whatever its outcome
(78, 28). The signature SHALL verify with a key the policy of the
gate's own branch lists for that signer; a key another branch's policy
lists for a signer of the same name SHALL count for nothing on it.

#### Scenario: Signed by a key the policy does not name
- **WHEN** a result claiming a confined pass is signed by a key `main`'s policy does not list
- **THEN** `main` reads the check as missing until a result its signers signed settles it (S18)

#### Scenario: A record's executor field alone
- **WHEN** an unsigned record on the remote says its executor was a confined container
- **THEN** it counts for no gate

#### Scenario: A key only another branch lists
- **WHEN** `release` trusts `box`, `release`'s policy lists key B for `box`, `main`'s lists key A, and a pass for `release`'s candidate is signed as `box` with key A
- **THEN** `release` reads the check as missing until a pass signed with key B settles it, while the same pass counts for `main`

#### Scenario: A branch in the host's own files
- **WHEN** `scratch` keeps its records in the host's own files, its policy at its revision names no signers, and `main` on origin names `[signers.box]`
- **THEN** none of box's signed runs on origin is read for `scratch`'s gate

### Requirement: A claim counts only when its signer may make it
A result's executor kind, whether it was confined, and its system SHALL
be claims in its signed record. A result SHALL be read as confined only
when its record claims it and its signer has `confined = true`, and as
on a system only when its signer's `systems` include it. sluice SHALL
never infer confinement (79, 29).

#### Scenario: An unconfined signer claims confinement
- **WHEN** a result signed by `sessions`, whose table has `confined = false`, claims it was confined
- **THEN** a branch with `confined = true` reads it as not confined, and a bolt whose `trust` lists `sessions` reads it as it is, unconfined

### Requirement: Run records are signed statements
Every result recorded on a branch whose state is on the remote SHALL be
an in-toto statement in a DSSE envelope: one subject per check, its key and
system, its contents the outcome, the claims, the combination, the
claim it ran under and the time. An `ssh` signer SHALL sign with its
SSH key. A record whose signature sluice cannot verify, a `github`
signer's included until sluice verifies keyless signatures, SHALL count
for nothing.

#### Scenario: A box's result
- **WHEN** the `box` host records a pass of `cli:build` on `linux-x64`
- **THEN** `refs/sluice/runs/box` holds a DSSE envelope over an in-toto statement with a subject for that key on `linux-x64`, signed by the box's SSH key

### Requirement: A host signs with the key its own config names
A host SHALL sign its results as the signer and with the key its config
names under `[signing]`, as `signer` and `key`. A host with no
`[signing]` table SHALL record no result on a branch whose state is on
the remote, and say so. The key SHALL be refused when anyone but its
owner may read it.

#### Scenario: A host with no signing key
- **WHEN** a host with no `[signing]` table finishes a run for `main`, whose state is on the remote
- **THEN** no result is recorded, and the run's output says the host has no signing key and names `[signing]`

### Requirement: One host with its own files signs nothing
On a branch whose state is in the host's own files, the results that
host's sluice starts SHALL count unsigned, as that host's, and `trust`
SHALL keep listing executor kinds as today (78).

#### Scenario: A local branch
- **WHEN** a branch has no `remote` and `trust = ["sandbox"]`
- **THEN** a sandboxed run's results count for its gate with no signature

### Requirement: A replay counts only through a signed pass
A result the task runner answered from its cache SHALL count only when
a signer the gate trusts has a pass recorded for the same key on the
same system, or, on a branch in the host's own files, when that host
recorded one. `cache` in `trust` SHALL be ignored with a warning naming
this rule (108).

#### Scenario: A replay with no signed pass
- **WHEN** the task runner replays `#check-fast` from its cache and no signer `main` trusts has a pass for that key on that system
- **THEN** the replay counts for nothing and the check runs (S42)

#### Scenario: A replay with one
- **WHEN** the `box` signer recorded a pass for the key on `linux-x64`, and `main` trusts `box` and needs `linux-x64`
- **THEN** a replay of that key on `linux-x64` counts

#### Scenario: An old policy
- **WHEN** a policy has `trust = ["open", "cache"]`
- **THEN** it is read as `trust = ["open"]`, and `sluice status` warns that `cache` no longer counts

### Requirement: A landing reads its verdict from signed results
Before a landing is pushed, the lead SHALL read the combination's result
again from the signed run records, and SHALL land it only when that
reading passes; a verdict written in the branch's state alone SHALL
never land a submission.

#### Scenario: A forged verdict in the state
- **WHEN** a host writes a passing verdict for a combination into `main`'s state with no signed results behind it
- **THEN** the lead reads the combination's checks as missing and does not land it

### Requirement: A shared branch's run is taken only by a host whose signature its gate would count
On a branch whose records are on its remote, a run SHALL be taken only by
a host signing as a signer the branch's policy names, with a key that
policy lists for it at the revision the round read; the name alone SHALL
take nothing. A run no such host takes SHALL stay open until the time
limit reads its candidate not passed (87, 66). A result signed with an
unlisted key SHALL be missing, never a fail, and its run open again
(28, S18). The records seam, never the core, SHALL read the key.

#### Scenario: A host whose key the branch does not list takes no run
- **WHEN** host a signs as a signer `main`'s gate trusts, with a key `main`'s policy does not list, and a submission waits on `main`
- **THEN** host a's rounds leave the run open and unclaimed, and host b, whose key `main` lists, claims it and the submission lands

#### Scenario: A run no listed host takes is not passed at the time limit
- **WHEN** the only live host signs with a key `main`'s policy does not list, and `main`'s time limit passes with the run still open
- **THEN** the candidate reads as not passed, and no run of it was started

#### Scenario: A result signed with a key the branch does not list is missing (S18)
- **WHEN** host a runs `main`'s candidate, `main`'s policy stops listing host a's key before the result is read, and the run reports
- **THEN** its checks are missing, not failed: the submission is not failed, the run is open again, the log records the checks as uncounted for host a, and once host b's rounds run it the submission lands

### Requirement: A replay no counted pass proves is logged with its key and why
Where a run replays checks that no counted pass proves, the branch's
log SHALL record, beside the combination and its tasks, each replay's
task, key and why no counted pass proves it, as `keys` on the
`unproven` line. A line written before `keys` SHALL read with none
(108, 19, 78, 79, 83; rule core.41).

#### Scenario: Each why
- **WHEN** a run's replays are read against results that hold no run of
  the first key, a run of the second on another system alone, a run of
  the third by an executor or signer the branch does not count, a run
  of the fourth by a trusted executor held to other limits, and a
  counted fail of the fifth
- **THEN** each replay is unproven, its why in turn `no run of its key
  is recorded`, `its key ran only on another system`, `no result of its
  key is signed by a signer the branch counts` on a branch whose
  records are on its remote or `no result of its key is by a run the
  branch trusts` on one in a host's own files, `no result of its key
  was held to this run's limits`, and `no result the branch counts for
  its key is a pass`
  (`an_unproven_replay_says_why_by_its_key` in
  `crates/sluice-core/src/verdict.rs`)

#### Scenario: The unproven line
- **WHEN** a run on a branch held to a container engine replays three
  checks whose only passes were recorded by runs held to none
- **THEN** the log's `unproven` line names the three tasks and, under
  `keys`, each task with its key and `no result of its key was held to
  this run's limits`
  (`a_replay_no_trusted_pass_proves_runs_again_with_no_replay` in
  `crates/sluice-conformance/tests/verdicts.rs`)

#### Scenario: An older line
- **WHEN** the log holds an `unproven` line written with no `keys`
- **THEN** it reads with none, and is written again without them
  (`an_unproven_line_and_an_unsigned_check_read_back` in
  `crates/sluice-core/src/records.rs`)

### Requirement: A branch that lands or is removed keeps every result it recorded

No landing, removal of a branch or release of a place SHALL remove a
result from the results log or a signed run record from a signer's ref.
A result belongs to its key and its system, not to a branch, and a
signer's records are only added to (69, 109; C.6; record 0093 as amended
2026-11-13).

#### Scenario: A removed bolt's pass counts for main

- **WHEN** a bolt's combination passed `cli:test` under its key, the bolt
  landed onto `main` and was removed, and `main`'s next combination's
  tree gives `cli:test` the same key
- **THEN** `main`'s gate counts that pass as a replay, and the results
  log and the signer's ref still hold it
  (`a_removed_branchs_results_still_count` in
  `crates/sluice-conformance/tests/verdicts.rs`)
