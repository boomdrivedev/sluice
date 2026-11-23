# Proposal

## Why

sluice starts its host process, the host process's successor and each run's supervisor in sessions of their own. A run continues after the host process that started it ends. On 2026-11-20, one `target/debug/sluice host` ran for 11 hours after nothing used it, and no test failed for it.

This change gives each of sluice's processes an owner that ends it, and proves it with a test that fails when a process is left. The guarantee and its test land together.

Claims that this change makes hold:

- `workers-end-with-their-owner@292e869158d6`
- `sluices-runs-end-with-its-host-process@e814b716f3b1`
- `nothing-detached@f807c7f5fcea`
- `a-leaked-child-fails-its-test@9a82a5147ba6`

Claims that it keeps holding:

- `workers-within-their-tools-limit@b48467b4ed47`
- `one-worker-limit-per-tool@e651e72ce246`

## What Changes

- Each worker is a child of `sluice host` in a process group of its own, recorded in the host's state.
- A stop of `sluice host` ends its workers' groups. A run that ended with its owner has no outcome.
- Each hand-over and each settings take-up starts the next host process in place, with the same pid, so the runs stay its children. An older command still uses a newer host process.
- A host process that starts ends the groups that a killed host process left.
- The daemon's services end the job's process group. A new package is a hand-over by `sluice hand-over`, not a restart of the service.
- A daemon that takes the host's place asks the host process to end, and its runs end with it.
- `[runs] max` stays the one limit of sluice's checks, session runs and previews, by the user's answer of 2026-11-21. git and credential workers run outside it.
- A guard in the test support fails each test that leaves a process, and ends that process.
- The test tasks of each Rust crate run under cargo-nextest, which reports a test that leaves a process that holds its output.
- The shell test tasks of the plugins and the check-e2e tier run under one script that fails where a process of the test is left.

Out of scope:

- The tokio edge. The bolt's unit `sluice-host-runs-on-a-tokio-edge` lands first, and this change starts its workers from it.
- A session's checks and a preview's command in `sluice host`. The bolt's unit `sluice-host-does-every-change` moves them.
- The check that each gate tool runs at its pinned version. The bolt's unit `the-gate-checks-each-tools-pin` adds it. This change pins cargo-nextest where the tree pins its other tools.

## Capabilities

### New Capabilities

- `resident-workers`: each worker as a child of `sluice host` in its own process group, its end with its owner, a hand-over in place, and the sweep at start.
- `test-children`: a test fails when it leaves a process that continues to run, in each crate and in each shell test task.

### Modified Capabilities

- `host-process`: a run ends with its host process, and a hand-over starts the next host process in place.
- `host-daemon`: a stopped daemon ends its runs.
- `settings-take-up`: a take-up that cannot be done while the process runs starts afresh in place.
- `daemon-service`: the services end the job's process group, and a new package is a hand-over.

## Impact

- The runs and the host process of the `sluice` crate: workers in process groups, and a record of each group. Also a sweep at start, and a hand-over in place.
- The test support of the `sluice` crate: a guard, which the tests that start sluice use.
- The project files of the five crates: `cargo nextest run` in place of `cargo test`. A nextest configuration at the root.
- The devenv shell: cargo-nextest at a pinned version.
- The plugins' shell tests and the check-e2e tier: one wrapper script.
- The flake's nix modules: `AbandonProcessGroup` false and `KillMode` `control-group`.
- New crate dependencies: libc, for the record that a worker writes before its exec, and sysinfo, a dev-dependency to list processes with their environment and working folder.
- A new command, `sluice hand-over`, which a service's activation runs in place of a restart.
- The core: a lost run that names no check is tested again.
