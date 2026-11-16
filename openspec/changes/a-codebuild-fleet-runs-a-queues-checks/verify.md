# Verify: a-codebuild-fleet-runs-a-queues-checks

Verified on `change/a-codebuild-fleet-runs-a-queues-checks` at
`e0d8501cd`, against its base `bolt/sluice-is-refactored-and-runs-on-pools`
at `44e33d4e1`, on 2026-11-16. This is the second verify. The first, at
`c2d80fe9e`, found the work short (`4958da1ef`): the plugin's skills named
no `sluice pool`. The code stage added task 8.7 and did it in
`e0d8501cd`.

**Verdict: the work falls short.** The bolt's gate passes at
`e0d8501`, every task is ticked, and every requirement and scenario of
the delta specs is met by the code and the test it names. But the
change cannot be archived as it is written: once `openspec archive`
merges its delta specs into `openspec/specs/`, `root:specs` fails on
three requirements over openspec's length and on the new capability's
placeholder Purpose (see "The archive"). The change is not archived and
goes back to its code stage.

## The whole checks

`sluice exec bolt/sluice-is-refactored-and-runs-on-pools`, run once,
exited 0. It ran the bolt's gate, `#check-fast`, as `.config/sluice.toml`
names it, over the checks `e0d8501` affects:

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
  was ignored; `every_command_is_named_in_the_skills` in
  `crates/sluice/tests/plugin_docs.rs`, the first verify's failure,
  among them, and every binary of `cli:test` after it ran. The herdr
  plugin's suites passed 138 each, 3 skipped. `site:prose` gave 70
  suggestions, no warning and no error.
- sluice recorded the 28 results under their keys for `e0d8501`.

`#check-tools` and `#check-e2e` are main's, run when the bolt merges onto
main. The checks the tasks name outside the bolt's gate were each run
once:

- Task 2.2, `sluice exec bolt/sluice-is-refactored-and-runs-on-pools --
  cargo test -p sluice --features tools --test remote_refs a_kms_record`:
  exited 0, its three tests passed
  (`a_kms_record_verifies_by_the_listed_half_alone`,
  `a_kms_record_by_another_key_counts_for_nothing`,
  `a_kms_record_is_never_a_hosts`).
- Task 8.1, `sluice exec bolt/sluice-is-refactored-and-runs-on-pools --
  moon run site:build`: exited 0, 144 pages built and its link check
  found every internal link valid.

## Tasks

Every task from 1.1 to 8.7 is ticked. Every test the tasks and the delta
specs name is defined in the file they name, and each the bolt's gate
runs passed there by name: the policy's five, `kms.rs`' three, the SDK's
two, the read token's, the pool's five in `codebuild/mod.rs`,
`a_pool_is_reached_by_its_kind`, `an_aws_profile_in_the_settings_is_the_one_used`,
the template's four, the three `run_life` tests of tasks 4.3, 4.4 and
4.6, and in `tests/cli.rs` the two `job_run_` tests, the sign and record
builds', the three `init_codebuild_` and the three `pool_check_` tests.
`openspec validate a-codebuild-fleet-runs-a-queues-checks --strict`
passes.

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
  sign build took within `[peers] alive`, naming the rule. The rendered
  template's lead role starts the run projects alone, denied by `Null`
  on each keyed override and on any variable but the five; the key's
  policy lets the sign role alone `kms:Sign` under `ECDSA_SHA_256` and
  `DIGEST`; the rule matches `SUCCEEDED`, `FAILED`, `FAULT` and
  `TIMED_OUT` and passes `SLUICE_RUN_BUILD` from the event's build; the
  run buildspec removes every container and the build user's sluice
  scratch and state before `sluice job run`; the sign buildspec runs
  only `job sign --build` and `job record --build`. `job sign` reads the
  claim and branch from the build CodeBuild answers and keeps the
  hand-over only where its SHA-256 is the one CodeBuild recorded.
  `sluice pool check` names a branch no host can lead and a key the
  policy does not list, failing, and says without failing where a
  branch on a fleet does not test side by side.
- **run-signing.** A `kms` signer is read by its key's ARN and a P-256
  public half and refused otherwise; its envelope's `keyid` is the ARN
  and its signature KMS's over the encoding's SHA-256, sent as a digest;
  every host verifies it by the listed half alone, S normalized low, and
  one by another key reads as missing, never failed; a host signing as a
  `kms` signer is listed nowhere and leads and claims nothing; a signer
  with `systems = []` claims no run and counts for nothing.
- **run-claims.** The policy accepts a `codebuild` pool beside `github`,
  each paired with its own kind of signer, and refuses another pairing
  naming the pool, its kind and its signer's.
- **place-release.** `sluice job run` releases its checkout before it
  exits, whatever its run came to and when stopped, and a release that
  fails is said in its output and its `why`, the report standing; the
  run buildspec starts a reused instance clean.

Record 0119 carries the build's amendment for the CodeBuild pool, and
`docs/architecture/core.md` core.63 and the details of core.22 and
core.62. The setting-up skill walks a CodeBuild fleet's setup through
`sluice pool check <branch>` (task 8.7), and README's Get started names
`sluice init codebuild`, read through by hand.

## The archive

`openspec archive a-codebuild-fleet-runs-a-queues-checks --yes` merged
the delta specs, creating `codebuild-pool` and updating
`place-release`, `run-claims` and `run-signing`. `openspec validate
--specs --strict --no-interactive`, `root:specs`' command, then failed
under the release `devenv.nix` pins, 1.14.3, as under the host's
1.17.0: 81 of 84 specs passed, and

- `spec/codebuild-pool`: its Purpose is the placeholder `openspec
  archive` writes for a new capability (`TBD - created by archiving
  change …`);
- `spec/run-claims`, requirement 6, "The policy lists pools in order":
  its text is 562 characters, over openspec's 500 (355 at the base);
- `spec/run-signing`, requirement 4, "Run records are signed
  statements", 583 characters (439 at the base), and requirement 9, "A
  shared branch's run is taken only by a host whose signature its gate
  would count", 502 (493 at the base).

The three are this change's MODIFIED requirements, whose text the delta
specs give whole. `openspec validate --changes --strict`, `root:changes`'
command, does not weigh a requirement's length, so the gate passed with
the change open. Before this archive every spec passed (83 of 83), so
the archive was undone, nothing of it committed, and `openspec/specs/`
stands as at the base.

## What the code stage must do

1. Bring the text of the three MODIFIED requirements in
   `specs/run-claims/spec.md` and `specs/run-signing/spec.md` to 500
   characters or fewer, moving what they add into their scenarios or a
   requirement of its own, so that once archived `openspec validate
   --specs --strict` passes. The behaviour they state is met as it
   stands; only their wording is over.
2. Whoever archives the change then writes `codebuild-pool`'s Purpose in
   `openspec/specs/codebuild-pool/spec.md`, as each archive that made a
   capability has.

## Outside this repository

The design (D7) has C.6 of `design/sluice/requirements.md` amended in
boomdrivedev/blueprints before this change lands (core.12). The
amendment is `96e5575` on the branch `design/sluice-kms-signer` in the
host's clone; GitHub's `boomdrivedev/blueprints` holds neither the
branch nor the commit, its `main` at `3a5c19d`, as of this verify. It is
the orchestrator's to push, as the design says.
