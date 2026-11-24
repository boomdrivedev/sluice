# Design

## Context

After `sluice-host-runs-on-a-tokio-edge`, `sluice-host-owns-its-workers` and `sluice-host-answers-every-read`, `sluice host` runs on a tokio edge, owns its workers and answers each read. This design cites the decisions of `sluice-host-answers-every-read` as the reads' D1 to D10.

The gap, at main 87bb89a3b:

| Where | What the client does |
|---|---|
| `main.rs:136-153` | Each command installs or puts back the hook: `protect::install` and `reinstall` run `git` (`protect.rs:48`) and write the hook's files. |
| `main.rs:202-211`, `main.rs:788-829`, `protect.rs:400-505` | The hook binds the repository (`Bound::guarding`), reads the branch's ref, the policy and the records, and writes each refused write under the branch's lock (`record_protected`). |
| `protect.rs:pruned` | A transaction under sluice's own fetch passes because the hook reads the refs as that fetch leaves them (record 0110). |

## Goals / Non-Goals

**Goals:**

- The hook reads no state. `sluice host` judges each transaction (`reads-go-through-the-resident@69134e5825d7`).
- No command runs git to put back the hook (`residents-do-the-network-and-git-work@a2d68cf95ffe`).
- A commit on a branch that sluice does not protect ends at once, while `sluice host` runs a round.
- The refusal's words stay as the hook gave them before.

**Non-Goals:**

- `sluice protect`, which installs the hook. It is a change command.
- An older hook at a newer `sluice host`. By the user's ruling of 2026-11-22, a command and its `sluice host` are the same version, and `sluice host` puts back the hook at its first request.

## Decisions

### D1. The git hook asks sluice host

The hook reads the transaction on its standard input, the folder that it runs in, `GIT_DIR`, and the command line of the git that runs it (`protect.rs:caller`). It sends them as `reference-transaction`. It reads no file of the repository and no record.

`sluice host` resolves the repository that git names (branch-protection: the protection judges the repository git names). It judges each line by the branch's ref on the host and the policy that it keeps, as `protect.rs:judge` does now, with git's packing let go first (record 0110). It writes each refused write under the branch's lock, as `protect.rs:hook` does now: the first refusal of a cause is a `protected` line, and each later one is counted. It answers `output` with the refusal's words and the code, 0 or 1.

The connection's task answers `reference-transaction` before any read or change, from the state that the host keeps. It never waits for a round or a reading. So a round that holds the repository does not hold a commit.

Before: the hook bound the repository, read the branch's state and wrote the refused write itself. Now: it asks `sluice host`, which judges and writes.

### D2. Where sluice host does not answer, the hook refuses

Decided by the user on 2026-11-23. Where no `sluice host` answers, the hook starts one, as a client does. Where `sluice host` does not answer within the hook's bound, the hook refuses the transaction. Its words say that sluice's host process did not answer, and that `sluice protect --remove` takes the protection out. The hook's bound is ten seconds: it covers the start of `sluice host` on a host where none ran, and a hand-over's wait for the rounds to end.

A policy or settings that `sluice host` cannot read still pass the transaction, said once, as now.

Before: a transaction that the hook could not read for a reason of its own passed, said once. Now: a transaction that `sluice host` does not judge in time is refused.

Not taken: the write goes, said once. Commits then keep working while `sluice host` does not answer, but a person's write to a protected branch gets through in that time.

### D3. The git commands of sluice host carry its name

`sluice host` gives each git command that its workers run the variable `SLUICE_HOST_PROCESS`, `<pid> <start>`: the pid of `sluice host` and its start time in seconds. The hook lets a transaction go with no ask where that pid runs and started at that time. A value whose pid does not run, or started at another time, is no name: the hook asks `sluice host`.

`sluice host` takes the variable out of the environment of each check, each command of a session, each preview's command, each release command and each equip step. So a write to `main` by a check is judged.

This keeps record 0110: a fetch of `sluice host` whose packing runs the hook waits on no ask of the hook. It also keeps the hook from asking `sluice host` about a write that `sluice host` decided.

The guard is not a security boundary (`protect.rs`): a person can remove the hook. The pid and the start time keep a value that an earlier `sluice host` gave from passing a later write.

Before: a transaction under sluice's own fetch passed because the hook read the refs as that fetch left them. Now: it passes because its git carries the name of a live `sluice host`.

### D4. sluice host puts back the hook

`sluice host` puts back the branch protection's hook each time it binds a repository for a request, where sluice's hook stands for another sluice, as each command did. It installs the hook where a submit, a wait or a step names the repository and none stands. The hook names the executable of `sluice host`. `sluice protect --remove` takes the hook out, and a later request does not put it back. `main.rs:protection` goes.

Before: each command put back the hook in its own process, by its own path. Now: `sluice host` puts it back by its own path. A newer command has `sluice host` hand over first, so the hook names the newer sluice.

## Risks / Trade-offs

- [Each commit in each worktree asks `sluice host`.] → The connection's task answers from the kept state, with no round and no reading in the way. The transaction names a branch that the host does not protect in most commits, and `sluice host` answers it at once.
- [A hook that refuses while `sluice host` does not answer stops each commit on the host.] → The hook starts `sluice host` where none runs, and waits ten seconds. Its words name `sluice protect --remove`.

## Migration Plan

Land the change with the commands and `sluice host` of one version. The first command of the new version has the older `sluice host` hand over in place, and `sluice host` puts back each repository's hook at its first request there.

Rollback: stop `sluice host` and run the older sluice. Its first command puts back the hook for itself.

## Open Questions

None.
