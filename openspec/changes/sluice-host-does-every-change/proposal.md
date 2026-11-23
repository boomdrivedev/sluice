# Proposal

## Why

After `sluice-host-answers-every-read`, `sluice host` answers each read. But each change is still made by the command:

- A submit binds the repository in its own process. It reads the worktree's head and writes the submission. A submit to a branch that takes pull requests pushes the branch and opens the request from its own process. A wait on a pull request and a group's wait read the forge from the command.
- An amend, a withdrawal, a set-aside, `sluice confirm`, `sluice push`, a release, the branch commands and `sluice protect` bind the repository and write the records in the command.
- `sluice exec` and `sluice preview` start a run's supervisor from the command, in a session of its own, so the run continues with no owner after the command ends. `sluice exec`, `sluice equip` and a worktree's release run the toolchain, the task runner and the release command from the command.
- The git hook binds the repository in its own process, judges the write from the files and writes the refused write in the records.
- Each command puts back the branch protection's hook in its own process.

This change has `sluice host` do the work of each change. A client sends its folder and its arguments, and prints the answer.

Claims that this change makes hold:

- `residents-do-the-network-and-git-work@a2d68cf95ffe`, for each command but four: `sluice forge check`, `sluice pool check`, `sluice init` and `sluice credential`. The bolt's unit `sluice-host-is-the-only-github-client` moves those four.
- `clients-never-fetch-or-start-a-tool@8aa9c57f19b2`, but for the forge and the token. The bolt's units `sluice-host-is-the-only-github-client` and `sluice-host-gets-its-tokens-from-the-runners-socket` make those parts hold.
- `workers-end-with-their-owner@292e869158d6`, `nothing-detached@f807c7f5fcea` and `sluices-runs-end-with-its-host-process@84c331c3e64a`, for a session's checks and a preview's command.
- `reads-go-through-the-resident@69134e5825d7`, for `sluice previews` and the git hook.

Claims that this change keeps holding: `workers-within-their-tools-limit@205585d82c6b`, `runs-max-counts-checks-only@2409f7ec2f4f`, `names-use-the-domains-terms@343c9fac7fc5`.

## What Changes

- `sluice host` makes each change: a submit, an amend, a withdrawal, a set-aside, `sluice confirm`, `sluice push`, a release, `sluice branch create`, `accept` and `delete`, and `sluice protect`. Each command sends its folder, its arguments and its caller's session to `sluice host`, and prints the answer. The command runs no git, reads no forge, and writes no record.
- `sluice host` writes a submit's submission. It reads the worktree's head from the folder that the submit sends.
- `sluice host` makes a pull request's submit: it pushes the branch, opens or updates the request, and marks it ready. A wait on a pull request and a group's wait follow `sluice host`, as a wait on a submission does.
- `sluice host` answers `sluice previews` and a step's request from the folder that the command sends.
- A session's checks, a preview's command, `sluice equip`'s steps and a worktree's release command run in `sluice host`, as its workers. Their output reaches the client as they run. A client that ends, or that closes its connection, ends its run.
- A run of a session or a preview takes its environment's kept variables from its caller, as it does now, sent with the request.
- The git hook asks `sluice host` whether to let each transaction go. `sluice host` judges it from the state that it keeps, and writes each refused write. Where no `sluice host` answers within the hook's bound, the hook refuses the write and says why.
- Each git command that `sluice host` runs carries the variable `SLUICE_HOST_PROCESS`, which names `sluice host`. The hook lets that command's transaction go with no ask. A check, a command of a session, a preview's command and a release command do not carry it.
- `sluice host` puts back the branch protection's hook in each repository that it binds for a request, as each command did. No command writes the hook.
- **BREAKING**: the requests `serve`, `hold` and `results` go, because no command sends them after this change. `step` and `pass` name the folder in place of the repository. `sluice host` keeps no request for an older command. A command and its `sluice host` are the same version, by the user's ruling of 2026-11-22.

Out of scope:

- `sluice forge check`, `sluice pool check`, `sluice init` and `sluice credential`. They call GitHub or AWS from the command. The bolt's unit `sluice-host-is-the-only-github-client` moves them.
- The token. `sluice host` gets a token as it does now. The bolt's unit `sluice-host-gets-its-tokens-from-the-runners-socket` changes how.
- A service that owns `sluice host`. A client still starts `sluice host` on each host where none answers, as now. The bolt's unit `a-service-owns-sluice-host-by-its-socket` changes that.
- The pools' jobs. A pool's job runs on a machine that the pool owns, with no resident, so `sluice job` keeps its one-shot form.

## Capabilities

### New Capabilities

- `resident-changes`: `sluice host` makes each change. A change command sends its folder, its arguments and its caller's session, and prints the answer. `sluice host` pushes and opens a pull request's submit, and follows a wait on a pull request and a group's wait. A client starts no other tool.

### Modified Capabilities

- `host-process`: `sluice host` writes a submit's submission.
- `resident-workers`: a session's checks, a preview's command and `sluice equip`'s steps are workers of `sluice host`. A session's checks run in `sluice host`, and their output reaches the client as they run. They end with their client.
- `resident-reads`: `sluice host` answers `sluice previews`.
- `branch-protection`: the hook asks `sluice host`. The git commands of `sluice host` pass by the variable that names it. `sluice host` puts back the hook.

## Impact

- The protocol of `sluice host`: the new requests `submit`, `amend`, `withdraw`, `set-aside`, `confirm`, `push`, `release`, `branch`, `protect`, `previews`, `exec`, `preview`, `equip` and `reference-transaction`, and the streamed answers `stdout` and `stderr` before `output`. `wait` also follows a pull request and a group. `serve`, `hold` and `results` go. `step` and `pass` name a folder.
- `main.rs`: each change command is sent to `sluice host` before the command binds a repository, as each read is. Only `sluice host`, `sluice daemon`, `sluice hand-over`, `sluice supervise`, `sluice job` and the four commands of the bolt's unit `sluice-host-is-the-only-github-client` bind a repository or read a forge.
- `exec.rs`, `preview.rs`, `equip.rs`, `runs.rs:Runs::start`: the run's supervisor starts only as a worker of `sluice host`. The start in a session of its own goes.
- `profiles/checks.rs:environment`: a run's kept variables come from the request.
- `protect.rs`: the hook sends the transaction to `sluice host`. `sluice host` judges it and writes each refused write.
- Tests: a change test runs its command with no git on its `PATH`, and with a forge address that refuses each call, after `sluice host` started with git and the test's forge.
- The record of the host process and the rules: an amendment with this change's decisions, each with Before and Now. The rules on the git hook, a run's process group, the reads of `sluice host` and refused writes change.
