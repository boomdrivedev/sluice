# Design

## Context

The statement's clauses 88, 107, 111 and 113, Part C's launched
executor and C.8, and scenario S40 are what this change builds, on top
of the `peers` change. See proposal.md for why.

What `peers` leaves in place:

- Records on the remote under `refs/sluice/`, the lead and the lease,
  claims and signs of life, SSH signers verified from the policy
  (ADRs 0037, 0032, 0033).
- The check seam's launched side (`launch`, `job_alive`, `wake`) and
  its GitHub profile, `github/actions.rs`: a run is a
  `workflow_dispatch` of the pool's workflow titled `sluice <claim>`,
  alive while such a run is queued or running (ADR 0034).
- `[signers.<name>] github` and `[pools.<name>]` parsed and checked by
  the policy; a `github` signer's records count for nothing, since
  nothing verifies them yet.
- `moon::check_key` and the check seam's `proven`: a key is kept only
  when the combination's tree bears it out (`peers` D9).

## Goals / Non-Goals

**Goals:**

- A submission lands with every host of its submitter off, on a
  repository whose only executor is the pool.
- A check never shares a job with the identity that signs its result.
- A private repository's names never reach a public log.

**Non-Goals:**

- A pool on another forge.
- Running sluice's own gate on the pool; adopting it is the operator's.

## Decisions

### D1. Keyless signatures and their verification

- **GitHub signers** sign keylessly: the signing job calls
  `actions/attest` with the statement's subjects and predicate, which
  signs through the sigstore instance GitHub uses for the repository:
  the public-good instance for a public repository, GitHub's own for a
  private one, so a private repository's names never reach a public log.
  The bundle is stored as the run's file. sluice verifies both kinds of
  bundle, in the GitHub profile:
  - a public-good bundle, which carries one transparency-log entry, with
    the `sigstore` crate against the public-good trusted root;
  - a bundle of GitHub's own instance, which carries no log entry and an
    RFC 3161 timestamp in its place, by sluice's own checks: the
    certificate chains to GitHub's Fulcio root, the timestamp is signed
    by GitHub's timestamp authority and falls within the certificate's
    validity, and the certificate's key signed the envelope's
    pre-authentication encoding. `sigstore` 0.14 refuses a bundle with
    no log entry, so it cannot take this one.
  Both trusted roots are fetched by TUF and cached on the host. Either
  way, the certificate's workflow identity (`<repo>/<workflow>@<ref>`)
  and issuer are checked against `[signers.<name>] github`.
- *Alternatives:* sluice signing keylessly itself with sigstore-rs. It
  speaks only the public-good instance and would publish a private
  repository's workflow names. Public repositories alone, or forge jobs
  signing with an SSH key from the `sluice` environment: each leaves
  private repositories without keyless signing. `gh attestation verify`:
  every verifying host would need `gh`. GPG signatures: a second key
  system for operators who already have SSH keys.

Verification lives in the GitHub profile and is handed to the records
profile through the binding, as the pool is; the core still sees only
`signer` and the claims (core.1).

### D2. The workflows and `sluice job`

`sluice init actions` writes two workflows from templates kept in
`crates/sluice/src/profiles/github/workflows/`:

- `sluice.yml`, on `workflow_dispatch`, `schedule` (every 15 minutes,
  the backstop of 111) and `workflow_run` of `sluice-pr.yml`. A **round**
  job takes a branch and runs rounds until the branch is idle or until
  50 minutes, short of the job's limit (the lease of `peers` D3). A **run** job has
  `permissions: contents: read` and no environment: it fetches the
  combination's ref, runs the checks under the task runner, and uploads
  the run's report as an artifact. A **sign** job, `needs: run`, runs no
  code of the combination: it keeps the keys `proven` keeps, signs, and pushes
  `runs/forge`. Only the round and sign jobs use the `sluice`
  environment, which holds the app's key and admits only the signer's
  `ref`.
- `sluice-pr.yml`, on `pull_request`: a run job per system for the
  head, with no secrets and no `id-token`. Its report is signed by
  `sluice.yml`'s `workflow_run` sign job, whose identity is the
  workflow on `main`, so a pull request that edits either file signs as
  nothing the policy names (113).

The jobs call `sluice job`, which the workflows are written around:

- `sluice job round --branch <b>` leads the branch under one name for
  every round job of the pool, `pool/<pool>/round`, so a scheduled job
  goes on from the last one with no lease to wait out; the workflow's
  `concurrency` keeps one per branch at a time. It runs rounds until the
  branch has nothing to do, or 50 minutes have gone.
- `sluice job run --claim <c> --branch <b>` makes the claimed run's
  checkout from the combination's ref, runs its checks, and writes the
  run's report and moon's manifests for its keys to a directory the job
  uploads. It records nothing and holds no key.
- `sluice job sign --claim <c> --branch <b> --from <dir>` keeps only the
  keys the combination's tree bears out (`proven`) and writes the run's
  statement; `actions/attest` signs it; `sluice job record` adds the
  bundle to `refs/sluice/runs/<signer>` and reports the run's outcome
  under its claim, as a standing host reports its own.

The checks never share a job with the identity token or the key
(actions-pool spec). Splitting run from sign costs one job start per
run. *Alternative:* one job per run with `id-token: write`. Any check
could then ask for the token and sign a pass for any key.

## Risks / Trade-offs

- [Contention on `runs/forge`] Twenty parallel sign jobs racing one
  ref retry several times. → The re-push is cheap and bounded; if it is
  not enough, signers' refs gain a per-host suffix under the same name.
- [Trusted roots] Verifying a keyless signature needs sigstore's
  trusted root, fetched by TUF at least once. → Cached on the host; a
  host that cannot fetch it reads forge results as unverified and says
  why, never as passes.
- [A round job's lifetime] An Actions job may be stopped at its time
  limit mid-round. → It is a lead that went quiet (72, 73): the next job
  or host takes over after the lease, and a landing it started settles
  from the remote.
- [Cost] Every run no standing host takes is a billed job. → Standing
  executors come first (107); the guide says how to keep the pool for
  the systems no box covers.

## Migration Plan

Per repository, once `peers` has landed: run `sluice init actions`, set
up the app's environment as it prints, and land one change that adds
the forge signer to `trust` and the pool to `[pools]`. Rollback: land a
policy without the pool; standing hosts go on as before.

## Open Questions

- The schedule's 15 minutes and the round job's 50 minutes are first
  guesses, to be tuned once the pool runs on sluice itself.
