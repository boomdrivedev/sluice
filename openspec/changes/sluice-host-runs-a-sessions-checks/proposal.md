# Proposal

## Why

After `sluice-host-owns-its-workers`, `sluice host` owns the workers of a combination's checks. But a session's checks, a preview's command and the equip steps still start from the command:

- `sluice exec` reads the plan, equips the worktree, asks `sluice host` for room by `hold`, and starts the run's supervisor in a session of its own. So the run continues with no owner after the command ends.
- `sluice preview` starts its command's supervisor in a session of its own, in the same way.
- `sluice equip` finds the worktree's top with git and runs the toolchain and the task runner from the command.
- A worktree's `sluice release` runs the place's release command from the command.
- Each of these commands binds the repository in its own process, and asks `sluice host` for the results by `results`.

This change has `sluice host` run each of them as its worker. A client sends its folder, its arguments and its caller's run variables, and prints the output as it comes.

Claims that this change makes hold:

- `clients-never-fetch-or-start-a-tool@8aa9c57f19b2`, for the checks, the toolchain, the task runner and the release command. The forge and the token stay outside this change.
- `workers-end-with-their-owner@292e869158d6`, `nothing-detached@f807c7f5fcea` and `sluices-runs-end-with-its-host-process@84c331c3e64a`, for a session's checks and a preview's command.
- `residents-do-the-network-and-git-work@a2d68cf95ffe`, for `sluice exec`, `sluice preview`, `sluice equip` and a worktree's release, but for the upkeep of the branch protection's hook that each command does.

Claims that this change keeps holding: `runs-max-counts-checks-only@2409f7ec2f4f`, `workers-within-their-tools-limit@205585d82c6b`, `names-use-the-domains-terms@343c9fac7fc5`.

## What Changes

- A session's checks, a preview's command, `sluice equip`'s steps and a worktree's release command run in `sluice host`, as its workers. The command sends its folder, its arguments and its caller's session, and prints the output as it comes.
- The output of a session's checks and of the equip steps reaches the client as they write it. The client exits with the code that `sluice host` answers.
- A client that ends, or that closes its connection, ends its session's run. `sluice host` gives the run's slot back and records the run as stopped. A run that waits for a slot stops waiting.
- A session's checks end with `sluice host`. The client says that sluice's host process ended, and exits 3.
- A preview's command stays a worker of `sluice host` after its client ends, until `sluice release` or a failed command ends it.
- A run of a session or a preview takes its environment's kept variables from its caller, as it does now, sent with the request.
- `sluice host` takes a slot of `[runs] max` for a session's checks from its own room, in the order of the run bound.
- The start of a run's supervisor in a session of its own goes.
- **BREAKING**: the request `hold` goes. `results` and `serve` go where no command sends them after this change. `sluice host` keeps no request for an older command. A command and its `sluice host` are the same version, by the user's ruling of 2026-11-22.

Out of scope:

- The change commands, `sluice previews` and `sluice step`. The change `sluice-host-makes-each-change` moves them.
- The git hook and the upkeep of the hook that each command does. The change `the-git-hook-asks-sluice-host` moves them.
- `sluice forge check`, `sluice pool check`, `sluice init` and `sluice credential`. The bolt's unit `sluice-host-is-the-only-github-client` moves them.
- The pools' jobs. A pool's job runs on a machine that the pool owns, with no resident, so `sluice job` keeps its one-shot form.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `resident-workers`: a session's checks, a preview's command, the equip steps and a worktree's release command are workers of `sluice host`. A session's checks run in `sluice host`, take a slot of its bound, stream their output to the client, and end with their client or with `sluice host`. A run's kept variables come from its caller. A client starts no other tool.

## Impact

- The protocol of `sluice host`: the new requests `exec`, `preview`, `equip` and `release` of a worktree, each with `run_env`, and the streamed answers `stdout` and `stderr` before `output`. `hold` goes. `results` and `serve` go where no command sends them.
- `main.rs`: `sluice exec`, `sluice preview`, `sluice equip` and a worktree's `sluice release` are sent to `sluice host` before the command binds a repository.
- `exec.rs`, `preview.rs`, `equip.rs`, `runs.rs:Runs::start`: the run's supervisor starts only as a worker of `sluice host`.
- `profiles/checks.rs:environment`: a run's kept variables come from the request. `profiles/checks.rs:Socket` and its `hold` go.
- Record 0093 amended with this change's decisions, each with Before and Now.
- The rule on a check's process group rewritten, with Before and Now: a session's run that `sluice exec` starts in a session of its own is no longer allowed.
- The README, the site's page on the host process and the using-sluice skill.
- Tests: a test of these commands runs the command with no toolchain and no task runner on its `PATH`, and with a `git` that records each call.
