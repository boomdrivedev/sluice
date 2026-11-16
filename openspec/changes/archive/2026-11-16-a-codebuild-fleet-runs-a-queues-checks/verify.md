# Verify: a-codebuild-fleet-runs-a-queues-checks

Verified on `change/a-codebuild-fleet-runs-a-queues-checks` at
`e24d39efd`, against its base `bolt/sluice-is-refactored-and-runs-on-pools`
at `44e33d4e1`, on 2026-11-16.

**Verdict: the work holds.** The bolt's gate passes at `e24d39e`, all
29 tasks are ticked, every requirement and scenario of the delta specs
is met by the code and the test it names, and the change archives into
specs that `openspec validate --specs --strict` passes, 84 of 84. The
change is archived in the commit after this report's.

## The whole checks

`sluice exec bolt/sluice-is-refactored-and-runs-on-pools`, run once,
exited 0. It ran the bolt's gate, `#check-fast`, as `.config/sluice.toml`
names it, over the checks `e24d39e` affects:

- 28 tasks passed: `root:format`, `root:lint`, `root:adrs`,
  `root:changes`, `root:specs`, `root:versions`, `root:marketplace`,
  `root:openspec-claude`, `site:prose`, `site:rules`, `seams:test`,
  `core:test`, `fakes:test`, `conformance:test`, `cli:test`,
  `nix:lint`, `nix:typecheck`, `nix:test`, `plugin-sluice:validate`,
  `plugin-sluice:hooks`, `plugin-sluice:lay-out`,
  `plugin-board:engine-types`, `plugin-board:typecheck`,
  `plugin-board:validate`, `plugin-board:test`,
  `plugin-herdr:manifest`, `plugin-herdr:test-sh` and
  `plugin-herdr:test-dash`.
- Across 74 Rust test binaries, 1438 tests passed, none failed and none
  was ignored. The herdr plugin's suites passed 138 each, 3 skipped.
  `site:prose` gave 0 errors, 0 warnings and 70 suggestions.
- sluice recorded the 28 results under their keys for `e24d39e`.
- moon warned, for each of the 28 tasks, that it could not package the
  task's outputs into an archive under `.moon/cache/outputs`, which in
  this place links to the repository's shared `moon-outputs`; and that
  it skipped the Rust toolchain's dependency install with no internet
  connection, as the executor's `network = "localhost"` has it. Neither
  is a check's outcome: every task passed and its result was recorded.

`#check-tools` and `#check-e2e` are main's, run when the bolt merges onto
main. The checks the tasks name outside the bolt's gate were each run
once:

- Task 2.2, `sluice exec bolt/sluice-is-refactored-and-runs-on-pools --
  cargo test -p sluice --features tools --test remote_refs a_kms_record`:
  exited 0, its three tests passed.
- Task 8.1, `sluice exec bolt/sluice-is-refactored-and-runs-on-pools --
  moon run site:build`: exited 0, 144 pages built and its link check
  found every internal link valid.
- Task 8.8, under `sluice exec bolt/sluice-is-refactored-and-runs-on-pools
  -- sh …`: `openspec/` copied to the run's scratch, `openspec archive
  a-codebuild-fleet-runs-a-queues-checks --yes` there under the pinned
  openspec 1.14.3, codebuild-pool's Purpose written in place of the
  archive's placeholder, then `openspec validate --specs --strict
  --no-interactive`: 84 passed, 0 failed.

## Tasks

Every task from 1.1 to 8.8 is ticked, none open. Each of the 40 tests
the tasks and the delta specs name is defined in the file they name, and
each the bolt's gate runs passed there by name. `openspec validate
a-codebuild-fleet-runs-a-queues-checks --strict` passes.

## Requirements and scenarios

Read against the code and the tests they name:

- **codebuild-pool.** `CodeBuild::launch` buys the reading token, makes
  one `StartBuild` of the system's run project with the five variables
  and the claim as its idempotency token, and keeps the build's id; the
  SDK's `start_build` sets the variables and the token and no other
  override, and a host with no app starts nothing, naming `[forge]`.
  `CodeBuild::job` reads `None`, `Queued` while submitted or queued,
  `Running` from the queued phase's end, `Ended` recorded once the sign
  build succeeded, and `Ended` with why for a stopped run build or one no
  sign build took within `[peers] alive`, naming the rule; the lead's
  lapse writes the pool's why. A pool's refused read is logged once per
  cause and host and lapses nothing. The rendered template's lead role
  starts the run projects alone, denied by `Null` on each keyed override
  and on any variable but the five; the key's policy lets the sign role
  alone `kms:Sign` under `ECDSA_SHA_256` and `DIGEST`; the rule matches
  `SUCCEEDED`, `FAILED`, `FAULT` and `TIMED_OUT` and passes
  `SLUICE_RUN_BUILD` from the event's build; the sign buildspec runs
  only `job sign --build` and `job record --build`. `job sign` reads the
  claim and branch from the build CodeBuild answers and keeps the
  hand-over only where its SHA-256 is the one CodeBuild recorded.
  `[aws] profile` is the profile the pool's calls use. `sluice init
  codebuild` writes the template and leaves a differing one unless
  `--force`, and `--stack` appends the signer and pool. `sluice pool
  check` names a branch no host can lead and a key the policy does not
  list, failing, and says without failing where a branch on a fleet does
  not test side by side. A gate trusting a pool's signer beside a lead's
  signer with `systems = []` has every run claimed by the pool.
- **run-signing.** A `kms` signer is read by its key's ARN and a P-256
  public half and refused otherwise; it signs through KMS with
  `ECDSA_SHA_256` and `DIGEST` over the encoding's SHA-256, the
  envelope's `keyid` the ARN; every host verifies it by the listed half
  alone, S normalized low, and one by another key reads as missing,
  never failed; a host signing as a `kms` signer is listed nowhere and
  leads and claims nothing; a signer with `systems = []` claims no run
  and counts for nothing.
- **run-claims.** The policy accepts a `codebuild` pool beside `github`,
  refusing a runner for a system its signer may not claim; a pool signs
  as its kind's signer, `github` with `github` and `codebuild` with
  `kms`, and another pairing is refused naming the pool, its kind and
  its signer's (`a_pool_signs_as_its_kinds_signer`).
- **place-release.** `sluice job run` releases its checkout before it
  exits, whatever its run came to and when stopped, and a release that
  fails is said in its output and its `why`, the report standing; the
  run buildspec ends every container and removes the build user's
  sluice scratch and state before `sluice job run`.

Record 0119 carries the build's amendment for the CodeBuild pool, and
`docs/architecture/core.md` core.63 and the details of core.22 and
core.62. The setting-up skill walks a CodeBuild fleet's setup through
`sluice pool check <branch>`, and README's Get started names `sluice
init codebuild`, read through by hand.

## The archive

`openspec archive a-codebuild-fleet-runs-a-queues-checks --yes` creates
`codebuild-pool` and updates `place-release`, `run-claims` and
`run-signing`. The archive's commit writes `codebuild-pool`'s Purpose in
place of the placeholder `openspec archive` gives a new capability.

## Outside this repository

The design (D7) has C.6 of `design/sluice/requirements.md` amended in
boomdrivedev/blueprints before this change lands (core.12). The
amendment is `96e5575` on the branch `design/sluice-kms-signer` in the
host's clone; GitHub's `boomdrivedev/blueprints` holds neither, its
`main` at `3a5c19d`, as of this verify. It is the orchestrator's to push,
as the design says.
