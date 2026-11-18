## MODIFIED Requirements

### Requirement: Run records are signed statements
Every result recorded on a branch whose state is on the remote SHALL be
an in-toto statement in a DSSE envelope: one subject per check, its key and
system, its contents the outcome, the claims, the combination, the
claim it ran under and the time. An `ssh` signer SHALL sign with its
SSH key, and a `github` signer keylessly, as a sigstore bundle holding
the envelope. A record whose signature sluice cannot verify SHALL count
for nothing.

#### Scenario: A box's result
- **WHEN** the `box` host records a pass of `cli:build` on `linux-x64`
- **THEN** `refs/sluice/runs/box` holds a DSSE envelope over an in-toto statement with a subject for that key on `linux-x64`, signed by the box's SSH key

#### Scenario: A forge job's result
- **WHEN** the pool's sign job records a pass of `cli:build` on `macos-arm64`
- **THEN** `refs/sluice/runs/forge` holds a sigstore bundle whose envelope is the same statement, signed with the sign job's keyless identity
  (`a_keyless_bundle_holds_the_runs_statement` in
  `crates/sluice/src/profiles/github/keyless.rs`)

### Requirement: A shared branch's run is taken only by a host whose signature its gate would count
On a branch whose records are on its remote, a run SHALL be taken only by
a host signing as a signer its policy names, with a key the policy
lists for it at the revision the round read, or by a pool whose jobs
sign as a `github` signer it names; the name alone SHALL take nothing.
A run none takes SHALL end at the time limit with no outcome, its
candidate not passed (87, 66). A result signed with an unlisted key
SHALL be missing, never a fail (28, S18). The records seam SHALL read
the key.

#### Scenario: A host whose key the branch does not list takes no run
- **WHEN** host a signs as a signer `main`'s gate trusts, with a key `main`'s policy does not list, and a submission waits on `main`
- **THEN** host a's rounds leave the run open and unclaimed, and host b, whose key `main` lists, claims it and the submission lands

#### Scenario: A run no listed host takes is not passed at the time limit
- **WHEN** the only live host signs with a key `main`'s policy does not list, and `main`'s time limit passes with the run still open
- **THEN** the run ends with no outcome, the candidate reads as not passed, no submission is blamed, and no run of it was started

#### Scenario: A result signed with a key the branch does not list is missing (S18)
- **WHEN** host a runs `main`'s candidate, `main`'s policy stops listing host a's key before the result is read, and the run reports
- **THEN** its checks are missing, not failed: the submission is not failed, a follow-up run holds those checks, the log records the checks as uncounted for host a, and once host b's rounds run it the submission lands

## ADDED Requirements

### Requirement: A forge signer signs keylessly

A `github` signer's run record SHALL be a sigstore bundle whose envelope
is the run's statement, signed with the sign job's workload identity
through the sigstore instance GitHub uses for the repository: the
public-good instance for a public repository, GitHub's own for a
private one (78, 141).

#### Scenario: A sign job on main

- **WHEN** `sluice.yml`'s sign job on `main` signs a pass of `cli:build`
  on `linux-x64`, and `[signers.forge] github` names that workflow on
  `main`
- **THEN** every host reads the pass as signed by `forge`
  (`a_bundle_from_the_signers_workflow_counts` in
  `crates/sluice/tests/profiles/forge.rs`)

#### Scenario: A private repository's bundle

- **WHEN** the bundle comes from GitHub's own instance, with a timestamp
  and no transparency log entry
- **THEN** it verifies against GitHub's trusted root and counts as
  `forge` (`a_bundle_of_githubs_own_instance_counts` in
  `crates/sluice/tests/profiles/forge.rs`)

### Requirement: Every host verifies a keyless record from the records alone

Every host that reads a `github` signer's record SHALL verify it from the
bundle and the trusted roots sluice carries, calling no service, and
SHALL count it only when the certificate's workflow, ref, repository and
issuer match `[signers.<name>] github` at the branch's revision (78, 79).

#### Scenario: A pull request's workflow

- **WHEN** a bundle's certificate names `sluice.yml` on a pull request's
  ref
- **THEN** the record counts for nothing, and why, which the reading
  process's warning that leaves it out gives, names the identity it
  carried (`a_bundle_from_a_pull_requests_ref_counts_for_nothing` in
  `crates/sluice/tests/profiles/forge.rs`)

#### Scenario: A bundle no root sluice carries verifies

- **WHEN** a bundle's certificate chains to no trusted root sluice
  carries
- **THEN** the record counts for nothing, read as unverified with why,
  never as a pass (`a_bundle_no_carried_root_verifies_counts_for_nothing`
  in `crates/sluice/tests/profiles/forge.rs`)
