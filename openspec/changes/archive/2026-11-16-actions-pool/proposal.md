# Proposal

## Why

The `peers` change puts a branch's queue on its remote and lets the
operator's own hosts claim, run and sign its checks with SSH keys. A
submission still lands only while one of those hosts is up, and a
check on a system no host is waits until the branch's `timeout`. The
statement in agentplot/blueprints, `design/sluice/requirements.md`,
has GitHub Actions as a pool after the standing executors: a job per
run on the runner its system names, a job that advances a branch when
no host does, and results signed keylessly by the workflow on the
branch, so an agent can submit and close its laptop with every host
off (88, 107, 111, 113; C.2 launched, C.8). `peers` built the pool's
side of the check seam and its dispatch; this change builds the rest
of the path and the guide that makes it usable.

## What Changes

- **Keyless signatures, verified.** A `github` signer's run records
  are sigstore bundles from `actions/attest`. sluice verifies them on
  every host: a public-good bundle with the `sigstore` crate, a bundle
  of GitHub's own instance (a private repository's) by its own checks
  of the certificate chain, the RFC 3161 timestamp and the envelope's
  signature. Both trusted roots are fetched by TUF and cached on the
  host. The certificate's workflow, ref, repository and issuer must
  match `[signers.<name>] github` (78, 79).
- **The workflows.** `sluice.yml` runs rounds, runs and signing in
  separate jobs: a round job leads a branch when woken or on the
  schedule; a run job runs the checks with no key and no identity
  token; a sign job runs no code of the combination, keeps only keys
  the tree bears out and signs them. `sluice-pr.yml` runs a pull
  request's head with no secrets (D7 and D9 of `peers`).
- **`sluice job`**: `round`, `run`, `sign` and `record`, the commands
  the workflows call.
- **`sluice init actions`** writes both workflows, adds
  `[signers.forge]` and `[pools.forge]` when absent, and prints what to
  set on GitHub; `sluice forge check` reports the workflow, the
  permission to dispatch it, the environment's branch rule and the
  forge signer.
- **Pull requests.** A trusted run on a pull request's head counts
  toward its combination by key and system (113, S40), and shows in
  the checks per system that `pull-request-reports` posts;
  `pull_requests.checks` names other systems' checks only.
- **Guides.** `guides/github-actions.md`: the app and its environment,
  `sluice init actions`, the signers, submit and close the laptop,
  reading the checks on a pull request, the pool's cost and how
  standing executors come first. The commands reference, the pull
  requests guide, the setting-up skill and the README follow.

Out of scope, as in `peers`: signing the landing verdict, a pool on a
forge other than GitHub, reusing a key proven in another repository,
`sluice exec --remote`.

## Capabilities

### New Capabilities
- `actions-pool`: GitHub Actions as a pool: the workflows `sluice init
  actions` writes, one job per run on its system's runner, a job that
  never shares its identity with the checks, rounds run in a job, the
  wake and the schedule.

### Modified Capabilities
- `run-signing`: a `github` signer signs keylessly, and sluice verifies
  its bundles.
- `forge-identity`: `sluice forge check` covers the pool's workflow,
  the app's permission to dispatch it, the environment and the forge
  signer.
- `pull-request-intake`: runs on a request's head count toward its
  combination; `checks` names other systems' checks.

## Impact

- Depends on `peers` landing first: its records on the remote, claims,
  SSH signers, the pool's launched side and the GitHub dispatch.
- `crates/sluice`: keyless verification in the GitHub profile (the
  `sigstore` crate, and x509 and RFC 3161 checks for GitHub's
  instance); workflow templates under
  `crates/sluice/src/profiles/github/workflows/`; `sluice init
  actions`, `sluice job`.
- The gate: actionlint covers the templates (`treefmt.nix`); recorded
  bundles are test fixtures excluded from formatting by name.
- A hand trial on a scratch GitHub repository, which the gate cannot
  reach.
- Site, README and the setting-up skill, as listed under guides.
