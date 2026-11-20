# Verify: a-pools-run-job-runs-the-repositorys-own-steps

Verified on 2026-11-18 at `5a733f03a` on
`change/a-pools-run-job-runs-the-repositorys-own-steps`, cut from
`bolt/sluices-pools-run-a-repositorys-own-steps` (`15b5cf1`), its base
and its gate (S66).

## The whole checks

`sluice exec bolt/sluices-pools-run-a-repositorys-own-steps`, sluice
0.95.1, ran the bolt's gate, `#check-fast`, once: the checks `5a733f0`
affects against the bolt. **Passed: 10 tasks, exit 0**, 10 results
recorded under their keys for `5a733f0`.

| Task | Outcome |
|---|---|
| `plugin-sluice:validate` | passed |
| `nix:typecheck` | passed |
| `root:changes` | passed |
| `core:test` | passed |
| `fakes:test` | passed |
| `conformance:test` | passed |
| `seams:test` | passed |
| `root:lint` | passed |
| `root:format` | passed |
| `cli:test` | passed |

moon warned ten times that it could not pack a task's outputs into
`.moon/cache/outputs` in the place; that warning decides nothing, and
every result was recorded.

`site:build`, which tasks 1.1 and 6.1 name and which is in no gate tier
(core.21), was run once on its own,
`sluice exec bolt/sluices-pools-run-a-repositorys-own-steps -- moon run site:build`:
147 pages built, every internal link valid.

`openspec validate a-pools-run-job-runs-the-repositorys-own-steps --strict`:
valid. `root:changes` archives the change into a scratch copy of the
specs and validates them (core.61), and passed.

## Tasks

Every task in `tasks.md`, 1.1 to 6.2, is ticked, and each holds:

- **1.1** Record 0119 carries the amendment of 2026-11-18, D1 to D6 each
  with the path not taken; its D2 also says how `sluice job run` finds
  its checkout's folder and that a failed release keeps its file. core.62's and core.63's details say what D8 asks.
  `site:build` passes.
- **2.1** `crates/sluice/src/own_steps.rs` reads by text and by parse,
  carries rooms by key, takes the caller's one further line, and names
  no tool or forge (core.1, core.2). `yaml-rust2` is a dependency.
  `a_room_is_read_emptied_and_carried` and
  `a_line_that_leaves_the_room_is_named` pass.
- **3.1** `sluice job run --checkout <file>` keeps the checkout and
  writes its path and place name; stopped, it releases at once and
  writes no file (`job.rs`, `lib.rs`); the help says so.
  `job_run_keeps_its_checkout_for_the_jobs_own_steps` and
  `job_run_stopped_releases_its_checkout` pass.
- **3.2** `sluice job release --checkout <file> [--out <dir>]` releases
  through the checkout seam, removes the file, says a failed release in
  its output and `<dir>/why` and exits 1 keeping the file, and exits 0
  with nothing kept. The commands reference gives both commands.
  `job_release_releases_the_kept_checkout` and
  `job_release_with_nothing_kept_releases_nothing` pass.
- **4.1** Both workflows' run jobs keep, name, leave the room, release
  `if: always()` and upload, in that order; permissions stay
  `contents: read`; the headers say so.
  `each_run_job_leaves_room_for_the_repositorys_own_steps` and
  `the_run_job_holds_no_identity_and_the_sign_job_runs_no_check` pass;
  `root:format` passes.
- **4.2** `workflows::read` and `carry` key the room by job and take
  `packages: write` after `contents: read` in the run job alone;
  `sluice init actions` reads, carries on `--force`, and refuses a room
  that leaves with exit 1. The four `init_actions_` tests the task
  names pass.
- **4.3** `sluice forge check` reads `sluice.yml` by that reading.
  `forge_check_reads_a_workflow_with_the_repositorys_own_steps`,
  `forge_check_names_own_steps_that_leave_the_run_job` and
  `forge_check_names_a_pools_missing_workflow` pass.
- **5.1** The run buildspec removes the old checkout file, runs
  `sluice job run --checkout` last in its build phase, exports `PATH` and
  `SLUICE_CHECKOUT` in `post_build`, leaves the room, and releases in
  `finally`. `RunPolicies` is a `CommaDelimitedList`, empty by default,
  given under `HasRunPolicies`. The run role denies `kms:*` on the
  signing key, `secretsmanager:*` on the app's key, and `codebuild:*` and
  `iam:*`. The four `template.rs` tests the task names pass;
  `root:format` and `root:lint` pass.
- **5.2** `template::read` and `carry` key each room by system;
  `sluice init codebuild` carries on `--force`, says a dropped system's
  room, refuses a room that leaves, and names `RunPolicies` in its deploy
  command. The three `init_codebuild_` tests the task names pass.
- **5.3** `sluice pool check` reads each run project's buildspec by
  `template::read_buildspec`, and the sign project's byte for byte.
  `pool_check_reads_a_buildspec_with_the_repositorys_own_commands` and
  `pool_check_names_own_commands_that_leave_the_buildspec` pass.
- **6.1** The pools guide says, under each pool, where a team's own steps
  go, the checkout they run in, the credential they hold, forks' lack of
  secrets, `--force` keeping them and moving earlier steps, and that the
  job releases after them. It names no tool of the family (core.26).
  `site:build` passes.
- **6.2** The setting-up skill says where each pool leaves room and links
  the guide. `plugin-sluice:validate` passes.

## Specs

Each requirement and scenario of the four delta specs is met by the code
and passes the test it names:

- **repository-steps** (new): the room in both workflows and the
  buildspec; the kept checkout named in the file and as
  `SLUICE_CHECKOUT`; setting up keeping the room, carried by job and by
  system, `linux-arm64`'s room left empty; forge check and pool check
  reading it; a room that leaves (`environment:`, a swallowing quoted
  string, `artifacts:` at the top level) named and refused.
- **actions-pool** (modified): init leaves a workflow that differs,
  except for the room and `packages: write`, unless `--force`; a run job
  with no environment, `id-token` or secret; `packages: write` kept and
  `id-token: write` in its place differing.
- **codebuild-pool** (modified): the run role's denies and
  `RunPolicies`, set and empty; the read token unchanged
  (`a_reading_token_asks_to_read_one_repository`); init writing and
  leaving the template; pool check reading the buildspec with the room
  aside, and naming a key the policy does not list.
- **place-release** (modified): the kept checkout, its release after the
  room, the failed release said in `why` and exiting 1, nothing kept,
  a stopped job releasing at once with no file, the templates releasing
  last, and the reused instance's first command removing the checkout
  file.

## Observations

These hold nothing back.

- `Places::path`, the place profile's new `Checkout::path`, has no test
  of its own against the real profile. The `job_` tests read it through
  `FakeCheckout`, so the file's first line is checked against
  `/fake/places/<name>`, not a folder on disk. `Places::path` reads the
  same record field (`made.path`) that `release` and `gone` already read.
- The repository-steps spec says the kept checkout "still holds what
  they wrote". The test proves that the place stands unreleased and the
  report is kept. It does not read a file the checks wrote in the
  checkout, since the checkout is a fake.
