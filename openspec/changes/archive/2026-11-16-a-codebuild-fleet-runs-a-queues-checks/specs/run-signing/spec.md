## MODIFIED Requirements

### Requirement: The policy names signers and what each may claim
The policy SHALL accept `[signers.<name>]` tables, each with exactly one
identity, `ssh` (an allowed-signers line, or the path of a file of them
in the repository), `github` (a workflow path and ref) or `kms` (a key
in AWS KMS, by its key ARN and its public half), and the claims it may
make: `confined`, default false, and `systems`, default every system. A
signer with no identity or two SHALL be refused.

#### Scenario: Two identities
- **WHEN** `[signers.box]` has both `ssh` and `github`
- **THEN** the policy is refused and the message names `signers.box`

#### Scenario: A file of session keys
- **WHEN** `[signers.sessions]` has `ssh = ".config/sluice.sessions"`
- **THEN** a result signed by any key listed in that file, read from the branch's revision, is signed by `sessions`

#### Scenario: A key in KMS
- **WHEN** `[signers.codebuild]` has `kms = { key = "arn:aws:kms:us-east-1:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab", public = "<a P-256 key's SubjectPublicKeyInfo, base64>" }`
- **THEN** the policy reads `codebuild` as a signer whose records verify by that public half
  (`a_kms_signer_names_its_key_and_public_half` in
  `crates/sluice-core/src/policy.rs`)

#### Scenario: A signer that may claim no system
- **WHEN** `[signers.lead]` has `systems = []` and `main` trusts it
- **THEN** a host signing as `lead` claims no run of `main`, whatever the run's system, one naming none included, and a result it signed counts for nothing
  (`a_signer_that_may_claim_no_system_claims_nothing` in
  `crates/sluice-core/src/policy.rs`)

#### Scenario: A KMS signer by an alias or with no public half
- **WHEN** `[signers.codebuild] kms` names an alias's ARN, a bare key id, no `public`, or a public half that is no P-256 key
- **THEN** the policy is refused, naming `signers.codebuild` and the field
  (`a_kms_signer_needs_a_key_arn_and_a_p256_public_half` in
  `crates/sluice-core/src/policy.rs`)

### Requirement: Run records are signed statements
Every result recorded on a branch whose state is on the remote SHALL be
an in-toto statement in a DSSE envelope: one subject per check, its key and
system, its contents the outcome, the claims, the combination, the
claim it ran under and the time. An `ssh` signer SHALL sign with its
SSH key, a `github` signer keylessly, as a sigstore bundle holding the
envelope, and a `kms` signer with its key in KMS. A record whose
signature sluice cannot verify SHALL count for nothing.

#### Scenario: A box's result
- **WHEN** the `box` host records a pass of `cli:build` on `linux-x64`
- **THEN** `refs/sluice/runs/box` holds a DSSE envelope over an in-toto statement with a subject for that key on `linux-x64`, signed by the box's SSH key

#### Scenario: A forge job's result
- **WHEN** the pool's sign job records a pass of `cli:build` on `macos-arm64`
- **THEN** `refs/sluice/runs/forge` holds a sigstore bundle whose envelope is the same statement, signed with the sign job's keyless identity
  (`a_keyless_bundle_holds_the_runs_statement` in
  `crates/sluice/src/profiles/github/keyless.rs`)

#### Scenario: A CodeBuild sign build's result
- **WHEN** the CodeBuild pool's sign build records a pass of `cli:build` on `linux-x64`
- **THEN** `refs/sluice/runs/codebuild` holds a DSSE envelope over the same statement, its signature the KMS key's and its `keyid` the key's ARN
  (`a_kms_envelope_holds_the_runs_statement` in
  `crates/sluice/src/signing/kms.rs`)

### Requirement: A shared branch's run is taken only by a host whose signature its gate would count
On a branch whose records are on its remote, a run SHALL be taken only by
a host signing as a signer its policy names, with a key the policy
lists for it at the revision the round read, or by a pool signing as a
`github` or `kms` signer it names; the name alone SHALL take nothing. A
run none takes SHALL end at the time limit with no outcome, its
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

### Requirement: A key-service signer signs as timber's does

A `kms` signer's record SHALL be signed by an `ECC_NIST_P256` key made in
KMS with the usage `SIGN_VERIFY`, by `ECDSA_SHA_256` over the SHA-256 of
the envelope's pre-authentication encoding sent as a digest, the
signature KMS's DER bytes and the envelope's `keyid` the key's ARN, as
timber's key-service signer signs its claims (timber C.7, timber record
0017; 78, 141).

#### Scenario: What the sign build asks of KMS

- **WHEN** the sign build signs a run's statement with the key
  `arn:aws:kms:us-east-1:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab`
- **THEN** it sends KMS one `Sign` with `ECDSA_SHA_256`, `DIGEST` and
  the 32 bytes of the encoding's SHA-256, and the envelope carries the
  answer's signature under that ARN
  (`a_kms_sign_sends_the_encodings_digest` in
  `crates/sluice/src/signing/kms.rs`)

### Requirement: Every host verifies a KMS record from the records alone

Every host that reads a `kms` signer's record SHALL verify it with the
public half `[signers.<name>] kms` lists at the branch's revision,
normalizing the signature's S low first, calling no service, and SHALL
count it only where its `keyid` is the listed key's ARN and the half
verifies it. A record that does not verify SHALL count for nothing, and
the reading process's warning that leaves it out SHALL say why (78, 79).

#### Scenario: A record by the listed key

- **WHEN** `refs/sluice/runs/codebuild` holds an envelope the listed key
  signed, with a high S, and the host reaches no AWS service
- **THEN** its pass reads as signed by `codebuild`
  (`a_kms_record_verifies_by_the_listed_half_alone` in
  `crates/sluice/tests/remote_refs.rs`)

#### Scenario: A host signing as a KMS signer

- **WHEN** a host's `[signing]` names `codebuild`, a `kms` signer, with
  an SSH key
- **THEN** the branch's policy lists none of its keys: it is not
  accepted, leads nothing and claims nothing
  (`a_kms_record_is_never_a_hosts` in
  `crates/sluice/tests/remote_refs.rs`)

#### Scenario: A record by another key

- **WHEN** an envelope under the listed ARN was signed by another P-256
  key
- **THEN** it counts for nothing, its check reads as missing, never
  failed, and the warning names the signer and the key
  (`a_kms_record_by_another_key_counts_for_nothing` in
  `crates/sluice/tests/remote_refs.rs`)
