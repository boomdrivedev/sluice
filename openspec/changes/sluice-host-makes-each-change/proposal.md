# Proposal

## Why

After `sluice-host-answers-every-read`, `sluice host` answers each read. But each change is still made by the command:

- A submit binds the repository in its own process. It reads the worktree's head and writes the submission. A submit to a branch that takes pull requests pushes the branch and opens the request from its own process. A wait on a pull request and a group's wait read the forge from the command.
- An amend, a withdrawal, `sluice confirm`, a release of a checkout or a preview, the branch commands and `sluice protect` bind the repository and write the records in the command.
- `sluice push` pushes a held landing from the command, and only on the operator's word.
- `sluice set-aside` writes a set-aside from the command, and each branch reads the set-asides beside the results.
- `sluice step` binds the repository to name it in its request, and the command tells `sluice host` of each change by `serve`.

This change has `sluice host` make each change. A client sends its folder and its arguments, and prints the answer. By the user's answers of 2026-11-23, `sluice push` and `sluice set-aside` go, with no request in their place.

Claims that this change makes hold:

- `residents-do-the-network-and-git-work@a2d68cf95ffe`, for each change command. Three parts stay outside this change:
  - the four commands that call GitHub or AWS: `sluice forge check`, `sluice pool check`, `sluice init` and `sluice credential`
  - the commands that run a session's checks, a preview, an equip and a worktree's release, which the change `sluice-host-runs-a-sessions-checks` moves
  - the upkeep of the branch protection's hook that each command does, which the change `the-git-hook-asks-sluice-host` moves.
- `reads-go-through-the-resident@69134e5825d7`, for `sluice previews` and a wait on a pull request.

Claims that this change keeps holding: `runs-max-counts-checks-only@2409f7ec2f4f`, `one-read-in-flight@46e5af6765be`, `names-use-the-domains-terms@343c9fac7fc5`.

## What Changes

- `sluice host` makes each change: a submit, an amend, a withdrawal, `sluice confirm`, a release of a checkout or a preview, `sluice branch create`, `accept` and `delete`, and `sluice protect`. Each command sends its folder, its arguments and its caller's session to `sluice host`, and prints the answer. The command runs no git for the change, reads no forge, and writes no record.
- `sluice host` writes a submit's submission. It reads the worktree's head from the folder that the submit sends.
- `sluice host` makes a pull request's submit: it pushes the branch, opens or updates the request, and marks it ready. A wait on a pull request and a group's wait follow `sluice host`, as a wait on a submission does.
- `sluice host` answers `sluice previews` and `sluice step` from the folder that the command sends.
- **BREAKING**: `sluice push` goes. `sluice host` pushes a held landing again by itself at each round of its branch, and shows the forge's latest words while the landing is held. The refusal is logged once for each distinct words of the forge.
- **BREAKING**: `sluice set-aside` goes, with its records and its reads. A fail that the host caused is a fault of sluice, which sluice fixes so that such a run records no outcome, as records 0031, 0043 and 0072 do. The kind of signed record that record 0077 kept for a set-aside goes with it. The Claude Code plugin's guard no longer refuses a set-aside, and a flaky failure's report names no step of the operator's.
- **BREAKING**: the request `serve` goes from each command that this change moves. `step` and `pass` name the folder in place of the repository. `sluice host` keeps no request for an older command. A command and its `sluice host` are the same version, by the user's ruling of 2026-11-22.

Out of scope:

- `sluice forge check`, `sluice pool check`, `sluice init` and `sluice credential`. They call GitHub or AWS from the command. The bolt's unit `sluice-host-is-the-only-github-client` moves them.
- `sluice exec`, `sluice preview`, `sluice equip` and a worktree's release. The change `sluice-host-runs-a-sessions-checks` moves them.
- The git hook and the upkeep of the hook. The change `the-git-hook-asks-sluice-host` moves them.
- The token. `sluice host` gets a token as it does now. The bolt's unit `sluice-host-gets-its-tokens-from-the-runners-socket` changes how.
- A service that owns `sluice host`. The bolt's unit `a-service-owns-sluice-host-by-its-socket` adds it.
- The pools' jobs. A pool's job runs on a machine that the pool owns, with no resident, so `sluice job` keeps its one-shot form.

## Capabilities

### New Capabilities

- `resident-changes`: `sluice host` makes each change. A change command sends its folder, its arguments and its caller's session, and prints the answer. `sluice host` pushes and opens a pull request's submit, and follows a wait on a pull request and a group's wait.

### Modified Capabilities

- `host-process`: `sluice host` writes a submit's submission.
- `resident-reads`: `sluice host` answers `sluice previews`.
- `refused-landing`: `sluice host` pushes a held landing again at each round, and `sluice push` goes.
- `run-outcome`: no result is set aside. A flaky failure names the result that it contradicts and no step of the operator's.
- `plugin-guard`: the guard has no set-aside to refuse.
- `submit-hints`: a way on names no step of the operator's.
- `verdict-report`: a report reads no set-aside and lists none.

## Impact

- The protocol of `sluice host`: the new requests `submit`, `amend`, `withdraw`, `confirm`, `release` of a checkout or a preview, `branch`, `protect` and `previews`. `wait` also follows a pull request and a group. `step` and `pass` name a folder. `serve` goes where no command sends it after this change.
- `main.rs`: each change command is sent to `sluice host` before the command binds a repository, as each read is.
- `lib.rs`: `Command::Push` and `Command::SetAside` go. A round pushes a held landing again (`perform_landing` in `sluice-core/src/step.rs`).
- The records: `set-aside.jsonl`, the ref `refs/sluice/set-aside` and each read of them go (`profiles/local_files.rs`, `profiles/remote_refs.rs`, `profiles/records.rs`, `sluice-core/src/records.rs`, `verdict_view.rs`, `sluice-seams`, `sluice-fakes`). An earlier release's set-asides stay in the records, read by nothing.
- The Claude Code plugin: `plugins/sluice/scripts/guard.sh` and its tests, and the using-sluice skill's references `verdicts.md` and `exec.md`.
- The statement in boomdrivedev/blueprints changes first, by the rule that a clause that no longer holds is changed in the statement first: clauses 116 and S44 (the set-aside), 152 (a held landing pushed again only on the operator's word), and C.6 (sluice's own kind for a set-aside).
- Records amended, each with Before and Now:
  - 0124: a held landing is pushed again at each round, and `sluice push` goes.
  - 0077: sluice defines no kind of its own, since the set-aside goes.
  - 0031, 0043 and 0044: their parts on the set-aside, its file, its hint and the report's read of it go.
  - 0093: `sluice host` makes each change, and `serve` goes.
- Rules rewritten, each with Before and Now:
  - the rule that a landing the forge refuses is held and never pushed again by itself
  - the rule that each check run is recorded before any verdict is read, for its parts on a set-aside
  - the rule that sluice runs no readiness command, for its part on the operator's set-aside
  - the rule that each signed record is of a catalogued kind, for sluice's own kind.
- The site's pages `reference/commands.md`, `reference/state.md`, `reference/configuration.md` and `architecture/rules.md`, and the README.
- Tests: a change test runs its command with a `git` on its `PATH` that records each call, after `sluice host` started with git and the test's forge.
