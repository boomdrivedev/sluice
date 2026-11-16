# Verify: a-codebuild-fleet-runs-a-queues-checks

Verified on `change/a-codebuild-fleet-runs-a-queues-checks` at `c2d80fe9e`,
against its base `bolt/sluice-is-refactored-and-runs-on-pools` at
`44e33d4e1`, on 2026-11-16.

**Verdict: the work falls short.** The bolt's gate fails on one test:
the plugin's skills name no `sluice pool`, the command this change adds.
Every task is ticked and every requirement and scenario read holds, but
the gate does not pass, so the change is not archived and goes back to
its code stage.

## The whole checks

`sluice exec bolt/sluice-is-refactored-and-runs-on-pools`, run once,
exited 1. It ran the bolt's gate, `#check-fast`, as `.config/sluice.toml`
names it, over the checks `c2d80fe` affects, and recorded the 28 results
under their keys for `c2d80fe`.

- 27 of 28 tasks passed: `root:format`, `root:lint`, `root:adrs`,
  `root:changes`, `root:specs`, `root:versions`, `root:marketplace`,
  `root:openspec-claude`, `site:prose`, `site:rules`, `seams:test`,
  `core:test`, `fakes:test`, `conformance:test`, `nix:lint`,
  `nix:typecheck`, `nix:test`, the plugins' `plugin-sluice:validate`,
  `plugin-sluice:hooks`, `plugin-sluice:lay-out`, `plugin-board:*` and
  `plugin-herdr:*` among them.
- **`cli:test` failed.** `crates/sluice/tests/plugin_docs.rs`,
  `every_command_is_named_in_the_skills`:

  ```
  thread 'every_command_is_named_in_the_skills' panicked at crates/sluice/tests/plugin_docs.rs:43:5:
  commands no skill names (as `sluice <command>`): ["pool"]
  test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
  error: test failed, to rerun pass `-p sluice --test plugin_docs`
  ```

  `sluice pool check` (task 7.1) is a command a person runs, and ADR 0023
  has every such command named in the plugin's skills. The setting-up
  skill names `sluice init codebuild` (task 8.4) and nowhere `sluice
  pool`. Task 8.4's own checks, `plugin-sluice:validate` and
  `site:prose`, do not read for it; `cli:test` does.
- Of the tests that ran, 1365 passed and that one failed. cargo stopped
  at the failing binary, so `cli:test`'s binaries after `plugin_docs`
  did not run in this gate: `previews`, `protect`, `reader_tick`,
  `runs`, `sessions`, `stack`, `telemetry`, `telemetry_logs` and `usage`.
  Every test the change names in `tests/cli.rs`, its library's unit
  tests and `sluice-conformance` ran and passed, among them
  `job_run_releases_its_checkout_before_it_exits`,
  `job_run_stopped_releases_its_checkout`,
  `job_sign_keeps_a_hand_over_only_as_codebuild_recorded_it`,
  `job_record_build_ends_the_run_with_what_the_key_signed`, the three
  `init_codebuild_` and three `pool_check_` tests, and the three
  `run_life` tests of tasks 4.3, 4.4 and 4.6.

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
  moon run site:build`: exited 0, the site built and its
  link check found every internal link valid.

## Tasks

Every task from 1.1 to 8.6 is ticked. Every test the tasks and the delta
specs name is defined in the file they name. `openspec validate
a-codebuild-fleet-runs-a-queues-checks --strict` passes, and each
MODIFIED requirement's name is one `openspec/specs/` holds.

## Requirements and scenarios

Read against the code and the tests they name:

- **codebuild-pool.** The four template tests read the rendered stack
  as each scenario says: the lead role starts the run projects alone,
  denied by `Null` on each keyed override and on any variable but the
  five, with no `kms:` or `secretsmanager:` action and the sign project
  only read; the run role names no `kms:`, `secretsmanager:` or
  `codebuild:` action and its buildspec runs `sluice job run` and
  nothing that signs; the key's policy and `SignWithTheKey` let the sign
  role alone `kms:Sign`, with `ECDSA_SHA_256` and `DIGEST`; the rule
  matches the four ends and passes `SLUICE_RUN_BUILD` from the event's
  build; and the run buildspec removes every container and the build
  user's sluice scratch and state before `sluice job run`, images kept.
  `CodeBuild::launch` makes one `StartBuild` with the five variables and
  the claim as its idempotency token, and the SDK's `start_build` sets
  nothing else; `job` reads `None`, `Queued`, `Running` since the queued
  phase's end, and `Ended` recorded or with why, as D5's table has it.
  The CLI tests of the sign and record builds, `sluice init codebuild`
  and `sluice pool check` assert what their scenarios say, and the
  `run_life` tests lead `main` from a host whose signer may claim
  nothing with every run claimed by `pool/codebuild/<n>`, lapse a pool's
  job with the pool's why, nobody blamed, and log a refused pool read
  once with its end, the run read as it stood.
- **run-signing.** `a_kms_record_verifies_by_the_listed_half_alone`
  verifies a high-S envelope by the listed half with no AWS reached;
  `a_kms_record_by_another_key_counts_for_nothing` leaves it out, missing
  and never failed, its reason naming `[signers.codebuild]` and the key;
  `a_kms_record_is_never_a_hosts` has a host signing as a `kms` signer
  verify as none of the branch's. The policy, `kms.rs` and record tests
  of the remaining scenarios are where they say.
- **run-claims.** The policy accepts a `codebuild` pool beside `github`,
  each paired with its own kind of signer, and refuses another pairing
  naming the pool, its kind and its signer's.
- **place-release.** `sluice job run` releases its checkout before it
  exits, whatever its run came to and when stopped, and a release that
  fails is said in its output and its `why`, the report standing.

The AWS crates are at the versions timber's `Cargo.toml` pins
(`aws-config` 1.13, `aws-sdk-kms` 1.126, `aws-smithy-http-client` 1.5 on
`rustls-ring`). Record 0119 carries the build's amendment, and
`docs/architecture/core.md` core.63 and the details of core.22 and
core.62.

## Outside this repository

The design (D7) has C.6 of `design/sluice/requirements.md` amended in
boomdrivedev/blueprints before this change lands (core.12). The
amendment is `96e5575` on that repository's branch
`design/sluice-kms-signer` in the host's clone; GitHub's
`boomdrivedev/blueprints` holds neither the branch nor the commit on
its `main` (`3a5c19d`), as of this verify.

## What the code stage must do

1. Name `sluice pool check` in the plugin's skills, where the setting-up
   skill offers the CodeBuild pool and `sluice init codebuild`, so
   `every_command_is_named_in_the_skills` passes (ADR 0023; task 8.4).
2. Run the bolt's gate again, so the test binaries cargo did not reach
   after `plugin_docs` run.
