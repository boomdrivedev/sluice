## Why

The GitHub Actions pool is built, and has never run. Record 0119's
amendment of 2026-11-16, on what the build of the pool chose, says so:
no workflow run has signed a run's statement, every check of the build
runs with no credential, and the workflow installs the release that
wrote it. It leaves the pool's first live run to a trial of its own:
once a release holds `sluice job`, boomdrive-ops tries the pool by hand
on a scratch repository, the user granting sluice's app its Actions
permission, and the bundle its sign job makes is recorded for the test
that reads a pool's signed run for a gate (the planner, 2026-11-16).

sluice 0.97.0, released 2026-11-17, holds `sluice job`, and 0.97.1 after
it. Its releases ship `aarch64-darwin`, `aarch64-linux` and
`x86_64-linux` builds, which the workflow's run job installs on GitHub's
Linux and macOS runners.

What the tree holds today proves less than the specs say. run-signing's
"A forge job's result" cites a test that reads a statement out of an
unsigned envelope, and "A sign job on main" one that verifies a conda
package's attestation from another project's workflow
(`tests/profiles/forge/public-good.sigstore.json`). No test reads a
statement sluice's own sign job signed, through the records and the
gate. The verify of the change that built the pool says the same, and
leaves the bundle and the live trial to this one.

The user's answer of 2026-11-16, on their card, moves the family's
checks to a pool, GitHub Actions first. A pool that has never run live
is where that starts.

## What Changes

- **The trial runs on a release, before this change's code.** ops runs
  it by hand on a public scratch repository whose `main` names a GitHub
  Actions pool for `linux-x64` and `macos-arm64`, with the release
  installed on the leading host and in both of the workflow's jobs. One
  submission lands on that `main` from a signed run on each system:
  `ubuntu-24.04` and `macos-15`, each run claimed before its dispatch,
  running from its run job's start, signed by its sign job with the
  workflow's own identity, and ended and closed in the records. The user
  is asked for the repository's owner and name, since it is public, on
  the card that asks for the app's Actions permission.
- **What the trial made is kept.** The two records its sign jobs added
  to `refs/sluice/runs/forge`, byte for byte, the policy at the revision
  it ran under, and where and with what it ran, under
  `crates/sluice/tests/trials/actions/`.
- **sluice's own tests hold each later release to it.** Two of the
  records profile's unit tests read the kept records as every host reads
  a pool's signed run: verified against the roots sluice carries, by the
  signer the trial's policy names in the repository it ran in, calling
  no service, each check the trial's `main` requires reading a pass on
  each system that its gate counts; and the same records under any other
  workflow's identity counting for nothing. They run in every gate,
  each bolt's and `main`'s, so no release is cut from a tree that cannot
  read what the pool signed.
- **run-signing's scenarios cite what proves them.** "A forge job's
  result" and "A sign job on main" cite the trial's test.
- **Record 0119 gains the trial's amendment.** Where the trial ran, on
  which release, how long each run took, what it found and where each
  finding went, what is kept, and that the kept trial is never edited to
  make a test pass.

## Out of scope

- **A defect the trial meets is fixed on `main`, not here.** The trial
  runs what a release ships, and a release is cut from `main` alone
  (core.55). ops sends such a fix to `main`, and tries again on the
  release that carries it. This change keeps a trial that passed.
- **A pull request's head run on the pool** (113): `sluice-pr.yml` and
  the sign job's `sign-head` are not tried, since the trial's `main`
  takes no pull requests. Their scenarios keep the tests they cite, until
  the first branch that takes pull requests through a pool tries them.
- **A private repository's bundle,** from GitHub's own instance: the
  trial's repository is public. "A private repository's bundle" keeps
  `github.sigstore.json`.
- **The CodeBuild pool's trial,** on a scratch account, is its own
  (record 0119's amendment for the CodeBuild pool). It keeps its
  envelope in the same form, under `tests/trials/codebuild/`.
- **A pool's run job running a repository's own steps** is the other
  unit of this change's bolt, `a-pools-run-job-runs-the-repositorys-own-steps`.
- **The round job,** a workflow that runs a branch's rounds with every
  host off, is `a-github-action-runs-a-queue-whole`'s.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `actions-pool`: the pool's trial, a signed run on each of `linux-x64`
  and `macos-arm64` from a released sluice, is kept, and sluice's tests
  read it as the gate reads a pool's signed run.
- `run-signing`: "A forge job's result" and "A sign job on main" cite
  the trial's test.

## Impact

- **Tests.** `crates/sluice/src/profiles/remote_refs.rs` gains two unit
  tests, which the `cli:test` task runs, tagged `check-fast`.
- **Fixtures.** `crates/sluice/tests/trials/actions/`: two Sigstore
  bundles as the sign jobs wrote them, the trial's policy, and
  `trial.toml`. `treefmt.nix` excludes the bundles by name, with why
  (core.21).
- **Records.** Record 0119 gains an amendment. No clause of the
  statement and no rule changes: the trial proves 78, 86, 87, 88, 107
  and 141, and rules core.22, core.62 and core.65, as written.
- **Code.** None outside the tests, unless the trial finds a defect,
  which goes to `main` (Out of scope).
- **Live.** A public scratch repository on GitHub, sluice's app given
  Actions read and write and installed on it, and its `sluice`
  environment. Nothing reads the repository once the trial is kept, and it may be
  removed.
