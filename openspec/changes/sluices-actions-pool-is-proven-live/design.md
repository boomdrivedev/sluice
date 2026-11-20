## Context

The GitHub Actions pool (record 0119; the change `actions-pool`, built
and archived by `a-runs-life-is-written-first-and-actions-runs-its-checks`)
is on `main` and in sluice 0.97.0 and 0.97.1:

- `sluice init actions` writes `sluice.yml` and `sluice-pr.yml` from
  `crates/sluice/src/profiles/github/workflows/`, naming the release that
  wrote them, and appends `[signers.forge]` and `[pools.forge]`.
- The lead claims a run under `pool/<pool>/<n>` before it dispatches
  `sluice.yml` for it (core.65), reads it running from its run job's
  start, and reads a workflow run that ended with nothing recorded as a
  lapsed claim (88, 77).
- The run job installs the release for its runner's system, runs
  `sluice job run`, and uploads its report and its proof, holding
  `contents: read` and nothing else (core.62). The sign job, in the
  `sluice` environment, runs `sluice job sign`, `actions/attest` and
  `sluice job record`, which adds the bundle to `refs/sluice/runs/forge`
  and writes the run ended and closed.
- A host verifies a `github` signer's record with
  `crates/sluice/src/profiles/github/keyless.rs` against the roots
  sluice carries, by `https://github.com/<owner>/<repo>/<workflow>@<ref>`
  and GitHub's issuer, calling no service (78, 141), and the records
  profile's `verify` (`crates/sluice/src/profiles/remote_refs.rs`) turns
  its statement into the result lines the gate reads.

The bundles the tests read today are another project's: a conda
package's attestation from `prefix-dev/sigstore-example`, a release
attestation of GitHub's own instance, and one of Sigstore's staging
instance. None holds a run's statement. The verify of the change that
built the pool named the gap and left it to a trial.

## Goals / Non-Goals

**Goals:**

- A queue whose policy names a GitHub Actions pool lands a submission
  from runs on GitHub's Linux and macOS runners, each from a released
  sluice and signed by the workflow's own identity.
- What that trial signed is kept, and every gate reads it as every host
  reads a pool's signed run, so no release is cut from a tree that
  cannot.

**Non-Goals:**

- Fixing here what the trial finds wrong in a release (D6).
- A pull request's head run, a private repository's bundle, the
  CodeBuild pool's trial, a run job running a repository's own steps,
  and the round job (the proposal's Out of scope).

## Decisions

### D1. ops runs the trial by hand, on a release, before this change's code

Record 0119's amendment gives the trial to boomdrive-ops, by hand, once
a release holds `sluice job`, and the app's Actions permission to the
user. Its first task is the code stage's first: where the stage's prompt
names no trial's repository, the code agent stops short with
`flume needs --to ops`, saying what D2 asks of the trial and what the
user is asked, and goes on once ops answers with the repository. Every
later task reads what the trial made.

*Not taken:* the code stage running the trial itself, which would make a
public repository and handle the app's key from a session's place;
dispatching a build of this branch, which no release would ever run and
no host would ever install.

### D2. What the trial is

**Its repository.** A public scratch repository of ops's making, whose
owner and name the user gives on the card that asks for the app's
permission, since a public repository is theirs to name. Its bundles
then go through the public-good instance, with a log entry, the path a
public repository's sign job takes (the forge signer requirement of
run-signing).

**Its contents, on `main`.** A moon workspace of one project with one
task, tagged with the tag its gate requires, which prints the system it
runs on and passes; a toolchain the run job's Nix install provides,
such as a flake's dev shell holding moon named as
`toolchain = ["nix", "print-dev-env"]`; and `sluice init actions`'
workflows and tables. Its policy keeps `main` on `origin`, names
`linux-x64` and `macos-arm64` in `[systems]` and in `main`'s `systems`,
and trusts two signers: the leading host's, with `systems = []`, so the
host leads and takes no run (core.60; record 0119's amendment for the
CodeBuild pool), and `forge`, whose pool names `ubuntu-24.04` and
`macos-15`.

**Its setup.** sluice's app given Actions read and write by the user
and installed on the repository; the `sluice` environment deployable
from `main` alone, holding `SLUICE_APP_KEY` and `SLUICE_APP_ID`; the
leading host on the release, its `[forge]` naming the app.

**It passes when**, on that release:

1. `sluice forge check main` reads every line ok.
2. One submission to `main`, whose work the check reads, so it takes
   the chain rather than landing beside it (core.39), opens a
   combination whose runs, one per system, are each written `claimed`
   under `pool/forge/<n>` before one dispatch of `sluice.yml` on its
   runner (core.65).
3. Each run job's log gives its token `contents: read` and no other
   permission, and names no environment (core.62).
4. Each run reads `running` from its run job's start; each sign job adds
   one bundle under `refs/sluice/runs/forge` and writes its run ended
   and closed.
5. The submission lands on `main`, its verdict read from those two
   signed runs.

What runs every host reading the records is D4's, since a kept record
read with no key and no service is that reading on any host that runs
the gate.

*Not taken:* `sluice-pr.yml` and `sign-head` in the same trial, which
would ask the trial's `main` to take pull requests, a way of landing it
has no use for (proposal, Out of scope); a private repository, whose
bundles go through GitHub's own instance, which `github.sigstore.json`
already stands for.

### D3. The trial is kept under `crates/sluice/tests/trials/actions/`

- `linux-x64.sigstore.json` and `macos-arm64.sigstore.json`: the two
  records the sign jobs added to `refs/sluice/runs/forge`, at the paths
  `sluice job record` gave them, `<combination>/<system>/<digest>.json`,
  copied byte for byte.
- `policy.toml`: the trial's `.config/sluice.toml` at the revision its
  gate was read at.
- `trial.toml`: the repository, `<owner>/<name>`; the release; that
  revision; and, for each run, its system, its runner and its record's
  file. A test reads the claim and the combination from the records
  themselves.

`treefmt.nix` excludes `crates/sluice/tests/trials/**/*.sigstore.json`,
kept as the sign jobs wrote them (core.21), beside the recorded bundles'
exclude. The two TOML files are taplo's, as every TOML file of the tree
is.

`tests/trials/<pool kind>/` is one place for every pool's trial: the
CodeBuild pool's first run on a scratch account keeps its envelope under
`tests/trials/codebuild/` in the same form, so both pools' trials are
read alike.

*Not taken:* a git bundle of the trial's repository, binary, holding far
more than the gate reads, and read by nobody in a review; the records
fetched from the trial's repository at test time, which calls a service
and breaks once the repository goes; the bundles beside the recorded
ones under `tests/profiles/forge/`, which are other projects'
signatures read for the verifier alone.

### D4. Two unit tests of the records profile read the kept trial

In `crates/sluice/src/profiles/remote_refs.rs`'s tests, beside
`a_github_signers_record_counts_only_from_its_own_workflow`, each record
read by `include_str!`:

- **`the_trials_runs_count_for_its_gate`.** The kept policy parsed with
  `Policy::parse`; the identity built from its `[signers.forge] github`
  and `trial.toml`'s repository, as the records profile builds it for a
  branch on that remote; each record read through `verify("forge", …)`.
  Each gives a pass of every check the policy's `main` requires, of kind
  ran, on its own system, `linux-x64` from the one and `macos-arm64`
  from the other, its signer `forge`, its host the run's claim
  `pool/forge/<n>`, its branch `main`, both of one combination, each
  statement read as a run's (`Statement::checked`); and the policy's `main`
  rules count each line, `Rules::trusts_signed("forge", confined,
  system)` true, as `sluice_core::verdict::read` asks of every result.
- **`the_trials_runs_count_only_from_its_signers_workflow`.** The same
  records under the workflow on `refs/pull/1/merge`, and under the same
  workflow and ref in another repository, are refused, the error naming
  the identity the certificate carried, the trial's workflow on `main`.

Both run in `cli:test`, which `#check-fast` runs, so every bolt's gate
and `main`'s run them, and a release is cut only from a tree that
passed them (core.55). A later sigstore crate whose carried roots no
longer verify the trial, a records profile that reads its statement
otherwise, or a gate that stops counting its signer fails them.

*Not taken:* a test of the `tools` feature reading the records through
a bare remote and a fetch, which `#check-tools` alone runs, on `main`'s
gate and no bolt's, and which tests the fetch the forge's tests already
cover; a test through `sluice status` on a clone, which reads the state
the lead wrote rather than the signatures.

### D5. The kept trial is never edited to make a test pass

The kept records are what a pool signed; hosts hold such records for
good (109). A change whose test fails on them makes sluice unable to
read records its pools wrote, and is wrong unless it reads them as
before. A change to what a pool's sign job writes keeps reading the
records signed before it (core.40's run statements under `run/v1`), and
a trial on the release that carries it keeps its records beside these,
under a name of their own.

*Not taken:* replacing the kept files whenever what a pool signs
changes, which would let a release stop reading every record a pool
signed before it.

### D6. What the trial finds goes to `main`

The trial runs what a release ships, and a release's tag names a commit
cut from `main` (core.55), so a defect the trial meets cannot be fixed on
this bolt and tried before the bolt lands. ops sends each to `main` as a
fix, with the trial's words, and runs the trial again on the release
that carries it. This change keeps a trial that passed, and record
0119's amendment names each finding and where it went.

*Not taken:* holding the trial until this bolt lands and is released,
which would land tests that read no kept trial.

### D7. Record 0119 gains the trial's amendment

Dated when this lands: the repository, the release, the runners, each
run's time from its claim to its close, what `sluice forge check` read,
each run job's token as GitHub gave it, what the trial found and where
each finding went (D6), what is kept and the two tests that read it
(D3, D4), and D5, each with the path not taken. No clause of the
statement and no rule changes: the trial shows 78, 86, 87, 88, 107 and
141, and rules core.22, core.62 and core.65, holding as written.

## Looking across the family

- **The CodeBuild pool** keeps its trial in the same form (D3), its
  envelope read by the records profile's `verify` for a `kms` signer as
  D4 reads a `github` signer's.
- **timber**'s key-service form (timber C.7, timber record 0017) is the
  CodeBuild trial's to prove, not this one's.
- **The family's checks** move to a pool, GitHub Actions first, on the
  user's answer of 2026-11-16; the bolt's other unit,
  `a-pools-run-job-runs-the-repositorys-own-steps`, builds on a pool
  this trial proved.

## Risks / Trade-offs

- [The verifier refuses a real bundle] `actions/attest` may write a log
  entry of a kind the pinned `sigstore-verify` does not read, which no
  test has met, since none held a run's bundle. → The trial finds it, and
  it goes to `main` (D6).
- [A runner the release has no build for] `macos-15` is arm64, which
  `aarch64-darwin` serves; an Intel macOS runner has no release build and
  its run job says so. → The trial names `macos-15`.
- [The release's address] The workflows fetch the release from
  `agentplot/sluice`, which GitHub redirects to `boomdrivedev/sluice`. →
  The trial runs through the redirect; the tree names `agentplot/sluice`
  throughout, which is a move of its own.
- [A root rotates] A later crate's carried roots keep a rotated key for
  the time it signed, so the kept trial still verifies. → Where one does
  not, D4's tests fail, and the release that would have stopped reading
  every pool's record is not cut.

## Migration Plan

ops runs the trial (D1, D2) and answers the code stage's ask with the
repository. The code stage keeps it (D3), adds the tests (D4) and the
record's amendment (D7). Nothing on a host changes. Rollback: the tests
and the kept files removed together.

## Open Questions

None.
