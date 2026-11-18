## Why

A pool's machines keep nothing between runs, so what a run's checks
built is gone when its job ends. timber 135 has a host push the cache
entries of the tasks it ran, and in a pool "in the job that ran the
task, as a step of the repository's own workflow after the queue's run
step, which the queue's template leaves room for and never names, with
a credential of the job's own that writes cache entries and nothing
else in the store". timber's guide, *The run job's push*
(`site/src/content/docs/guides/self-hosting.mdx` in boomdrivedev/timber),
writes that step for sluice's Actions pool. The user's answer of
2026-11-16 on their card moves the family's checks to a pool, GitHub
Actions first, so the family's own runs need it.

What sluice holds on `bolt/sluices-pools-run-a-repositorys-own-steps`
leaves no such room:

- **The checkout is gone when `sluice job run` exits.** It releases the
  checkout it made before it exits (rule core.63), so a step after it
  finds no checkout and none of what the checks built
  (`crates/sluice/src/job.rs`).
- **A step of the repository's own makes the workflow differ.**
  `sluice init actions` leaves a workflow that differs from what it
  writes unless `--force`, which drops the step, and `sluice forge
  check` fails a workflow that is not byte for byte as init writes it
  (`crates/sluice/src/init.rs`, `crates/sluice/src/profiles/github/check.rs`).
  `sluice init codebuild` and `sluice pool check` do the same with the
  run build's buildspec (`crates/sluice/src/pool.rs`).
- **The run job and the run build hold no credential the repository
  could give them.** The run job's permissions are `contents: read`
  alone, and rule core.62 rules out any secret in it; the run build's
  role may write its hand-over and its logs alone, and the template
  takes no policy of the repository's.
- **timber's guide puts its step where it cannot run.** It adds the step
  after `sluice job run`, in `${{ github.workspace }}`, which is not the
  checkout the checks ran in.

Statement in boomdrivedev/blueprints: sluice's 79, 88, 109, 113, C.3's
launched row and C.8; timber's 135, 139 and C.1 (GitHub's registry).
Rules core.1, core.10, core.16, core.26, core.62, core.63; record 0119.

## What Changes

- **Each run job and run build leaves room for the repository's own
  steps after the checks.** `sluice.yml`'s and `sluice-pr.yml`'s run
  jobs, and each run project's buildspec in `sluice init codebuild`'s
  template, carry two `# sluice:` lines between which the repository
  writes its own steps or commands. sluice writes nothing between them,
  names no tool there, and reads nothing of them but that they stay
  within the run job's steps or the buildspec's commands.
- **They run in the checkout the checks ran in, before it is
  released.** `sluice job run` keeps its checkout for the job's later
  steps, naming it in a file, and the job names it to them as
  `SLUICE_CHECKOUT`. A new `sluice job release` releases it after those
  steps, whatever they came to, and the hand-over is uploaded after it.
  `sluice job run` stopped still releases at once.
- **Setting up keeps them, and reading the setup back accepts them.**
  `sluice init actions` and `sluice init codebuild` read a file that
  differs only between its `# sluice:` lines as sluice writes it, and
  `--force` carries those lines over. `sluice forge check` and `sluice
  pool check` read a workflow or a buildspec with them emptied. A line
  there that would leave the run job's steps or the buildspec's commands
  fails every one of them, so no step the repository adds can give the
  run job an environment, an identity or a permission.
- **The repository gives its steps their credential.** On GitHub
  Actions, its own steps name its own secrets, and the run job's token
  may also write its packages, `packages: write`, where the repository
  adds it, for a store on GitHub's registry; no other permission, no
  environment and no identity token. On CodeBuild, the template takes
  `RunPolicies`, managed policies the repository gives the run role,
  and the run role denies itself the signing key, the app's key, every
  CodeBuild action and every IAM action, so no policy it is given
  reaches sluice's keys.
- **The guides say how.** The pools guide shows where a team's own
  steps go on each pool, the checkout they run in and the credential
  they hold, naming no tool; the commands reference gives `sluice job
  release` and `job run --checkout`.

## Capabilities

### New Capabilities

- `repository-steps`: the room each pool's run job and run build leave
  for the repository's own steps after the checks, the checkout those
  steps run in, how setting up keeps them and how a reading of the
  setup accepts them (timber 135; record 0119; 88, C.8).

### Modified Capabilities

- `actions-pool`: `sluice init actions` reads a workflow that differs
  only in the repository's own steps and the run job's `packages: write`
  as it writes it; a run job holds no environment, identity or secret of
  sluice's, its permissions `contents: read` and the repository's
  `packages: write` (79, 109; core.62).
- `codebuild-pool`: the run build's role takes the repository's own
  policies and denies itself sluice's keys and every build; `sluice
  init codebuild` keeps the repository's own commands; `sluice pool
  check` reads a run project's buildspec with them emptied (core.62).
- `place-release`: a pool's job releases its checkout by `sluice job
  release`, after the repository's own steps, and `sluice job run`
  releases it at once only where it is stopped (core.10, core.63; 123).

## Impact

- **Code.** A new `crates/sluice/src/own_steps.rs`, the reading of the
  `# sluice:` lines, which no profile owns, so both profiles call it
  (core.2). `crates/sluice/src/job.rs` and `lib.rs` (`job run
  --checkout`, `job release`). `profiles/github/workflows.rs` and its two
  templates; `init.rs`; `profiles/github/check.rs` and `profiles/mod.rs`
  (forge check's reading). `profiles/codebuild/template.rs` and
  `templates/stack.yml`; `pool.rs`.
- **Tests.** Unit tests in `own_steps.rs`, `workflows.rs` and
  `template.rs`; `crates/sluice/tests/cli.rs` for `sluice job`, `sluice
  init actions`, `sluice init codebuild` and `sluice pool check`;
  `crates/sluice/tests/profiles/forge.rs` for `sluice forge check`.
- **Docs.** `site/src/content/docs/guides/pools.md`; the commands
  reference; the setting-up skill. Record 0119 gains this change's
  amendment; rule core.62's and core.63's details change (design D8).
- **Family.** timber's guide must move its step between the `# sluice:`
  lines and run it in `${{ env.SLUICE_CHECKOUT }}`; timber C.1's
  "pool's token" on GitHub's registry is the run job's `packages:
  write` (design, Looking across the family). Both are timber's to
  change, and the orchestrator is told.

## Out of scope

- **sluice pushing anything itself.** sluice names no store and no tool
  in its templates or its code (timber 135; core.1, core.26); a team's
  push is its own step.
- **A store on GitHub's registry from a CodeBuild pool.** GitHub gives
  no token that writes only one repository's cache packages outside
  Actions (timber C.1); a CodeBuild pool's steps reach a store their
  role's policies reach.
- **Ending what the checks left running before the repository's
  steps.** It stays until `sluice job release`, as on a host until a
  place's release (core.10); the credential's own scope bounds what a
  check that outlives the run can do with it (timber 135).
- **sluice's own repository on a pool.** This repository names no pool
  yet; moving its checks to one is the change that follows the user's
  answer of 2026-11-16.
