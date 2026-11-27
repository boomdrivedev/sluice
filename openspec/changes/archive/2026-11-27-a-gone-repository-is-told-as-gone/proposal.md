## Why

On 2026-11-27, the gate failed #180 on
`a_gone_repository_leaves_the_subscription`. The test moves a
repository's git folder away while a round of the repository goes. The
subscription got an error, `does not appear to be a git repository`. It
did not get the overview that says the repository is gone. The test
passes when it runs alone.

The cause: a round that was in flight wrote its records after the git
folder went. That write made a new git folder at the same path. The new
folder holds only records, and no repository. `sluice host` found a
repository gone only when its git folder was not there. So it read the
repository as there, and each read of it failed. A subscriber, such as
the herdr plugin, got an error with no end, and the subscription kept
the folder.

## What Changes

- A repository is gone when its git folder holds no repository: the
  folder has no `HEAD` file or no `objects` folder, as git reads a git
  folder. One function says this. Each part of `sluice host` that asks
  whether a repository is gone calls it: the worker, the look, the
  scheduler and the index.
- A look that finds its repository gone says so. The scheduler then
  sends the overview that says the repository is gone. It holds the
  repository no longer, as when a worker finds it gone.

## Capabilities

### Modified Capabilities

- `resident-subscriptions`: a repository is gone when its git folder
  holds no repository, also when a round wrote in that folder after it
  went.

## Impact

- `crates/sluice/src/profiles/mod.rs`: `repository_gone`.
- `crates/sluice/src/host_process/tasks.rs`: the `gone` field of
  `Looked`, and the scheduler's end of a repository that a look found
  gone.
- `crates/sluice/src/host_process/index.rs` and `mod.rs`: the look, the
  test for sluice and the index ask `repository_gone`.
- Tests: `crates/sluice/tests/host_process.rs`, and the unit tests of
  `tasks.rs` and `profiles/mod.rs`.
