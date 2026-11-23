# Proposal

## Why

After `sluice-host-answers-every-read`, `sluice host` answers each read. But the branch protection still works in other processes:

- The git hook binds the repository in its own process, reads the branch's ref, the policy and the records, judges the write, and writes each refused write in the records under the branch's lock.
- Each command puts back the hook in its own process, by its own path, and runs git to find the hooks' folder.
- A fetch of sluice's own whose packing runs the hook passes because the hook reads the refs as that fetch leaves them, with no lock (record 0110).

This change has the hook ask `sluice host`, which judges each transaction from the state that it keeps and writes each refused write. `sluice host` puts back the hook, and its own git commands pass the hook by a variable that names it.

Claims that this change makes hold:

- `reads-go-through-the-resident@69134e5825d7`, for the git hook.
- `residents-do-the-network-and-git-work@a2d68cf95ffe` and `clients-never-fetch-or-start-a-tool@8aa9c57f19b2`, for the hook and for the upkeep of the hook.

Claims that this change keeps holding: `one-read-in-flight@46e5af6765be`, `names-use-the-domains-terms@343c9fac7fc5`.

## What Changes

- The git hook asks `sluice host` whether to let each transaction go. It sends the transaction, its folder, `GIT_DIR` and git's command line, and exits with the code that `sluice host` answers. It reads no file of the repository and no record.
- `sluice host` judges the transaction from the state that it keeps, at once, with no wait for a round or a reading. It writes each refused write under the branch's lock.
- Where no `sluice host` answers, the hook starts one, as a client does. Where `sluice host` does not answer within ten seconds, the hook refuses the write. Its words say that sluice's host process did not answer, and name `sluice protect --remove`. The user decided this on 2026-11-23.
- Each git command that `sluice host` runs carries the variable `SLUICE_HOST_PROCESS`, which names the pid and the start time of `sluice host`. The hook lets that command's transaction go with no ask. A check, a command of a session, a preview's command, a release command and an equip step do not carry it.
- `sluice host` puts back the branch protection's hook in each repository that it binds for a request, by its own path. No command puts back the hook in its own process.
- **BREAKING**: a hook that an older sluice installed is put back by `sluice host` at its first request in the repository. `sluice host` keeps no request for an older hook. A command and its `sluice host` are the same version, by the user's ruling of 2026-11-22.

Out of scope:

- The change commands, `sluice protect` among them, and `sluice previews` and `sluice step`. The change `sluice-host-makes-each-change` moves them.
- `sluice exec`, `sluice preview`, `sluice equip` and a worktree's release. The change `sluice-host-runs-a-sessions-checks` moves them.
- `sluice forge check`, `sluice pool check`, `sluice init` and `sluice credential`. The bolt's unit `sluice-host-is-the-only-github-client` moves them.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `branch-protection`: the hook asks `sluice host`, and refuses the write where `sluice host` does not answer in time. `sluice host` judges the transaction and writes its refusal. The git commands of `sluice host` pass by the variable that names it. `sluice host` puts back the hook.

## Impact

- The protocol of `sluice host`: the new request `reference-transaction`, answered with `output`.
- `protect.rs`: the hook sends the transaction to `sluice host`. `judge`, `pruned` and `record_protected` run in `sluice host`. `reinstall` runs in `sluice host`.
- `main.rs`: `protection` and its call for each command go. The hook (`main.rs:202-211`, `788-829`) binds no repository.
- Each git worker of `sluice host` gets `SLUICE_HOST_PROCESS`. Each check, session command, preview command, release command and equip step has it taken out of its environment.
- Records amended, each with Before and Now:
  - 0110, for a fetch of `sluice host` whose packing runs the hook
  - 0093, for the hook and its upkeep
- Rules rewritten, each with Before and Now:
  - the rule that `sluice host` alone reads a repository's records, for the hook and for a command that puts back the hook
  - the rule on a write that the protection refused, for the hook's own write of it
  - the rule on sluice's one retry policy, for a run of sluice under sluice's own fetch.
- The README, the site's page on branch protection and the using-sluice skill.
- Tests: a test of the hook runs git in a repository whose hook names the sluice under test, with `sluice host` started with a state folder of its own.
