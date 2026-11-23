# Design

## Context

After `sluice-host-runs-on-a-tokio-edge`, `sluice-host-owns-its-workers` and `sluice-host-answers-every-read`, `sluice host` runs on a tokio edge, owns its workers and answers each read. A read command sends its folder and prints the `output` that `sluice host` answers (`main.rs:read`, `host_process/reads.rs:answer`). This design cites the decisions of `sluice-host-answers-every-read` as the reads' D1 to D10.

Each change command still falls through to `main.rs`, which binds the repository in the command's own process (`Bound::open_with`, `main.rs:291`) and runs the command's code there (`lib.rs:dispatch`). The gap, at main 87bb89a3b:

| Where | What the client does |
|---|---|
| `main.rs:291-317` | Each command that is not a read, a watch or a wait binds the repository. |
| `lib.rs:1982-2168` | A submit writes its submission (`lib.rs:submit` at 3368, `submit_group` at 3512). |
| `lib.rs:6093`, `lib.rs:6590` | A pull request's submit pushes the branch and opens, updates and marks the request from the command (`submit_pull`, `submit_pull_group`). |
| `lib.rs:2213-2290` | A wait on a pull request, and a wait on a branch that takes pull requests, read the forge and follow the request from the command. |
| `lib.rs:2628`, `2714`, `2323` | An amend, a withdrawal and `sluice confirm` write the records. |
| `lib.rs:2774` | `sluice set-aside` writes a set-aside. |
| `lib.rs:2863`, `lib.rs:7481` | `sluice push` pushes a held landing. |
| `lib.rs:2868-2887` | A release frees a checkout or a preview. |
| `lib.rs:2929-2978` | `sluice protect` writes the hook and reads the worktrees. |
| `lib.rs:2979-2993` | `sluice branch create`, `accept` and `delete` make, move or remove a branch and push it. |
| `lib.rs:2361`, `main.rs:291` | `sluice step` binds the repository to send its `step` request. |
| `lib.rs:2862` | `sluice previews` reads the previews' records. |
| `lib.rs:1979`, `host_process/client.rs:417` | The command sends `serve`, naming a repository that the command resolved. |

## Goals / Non-Goals

**Goals:**

- `sluice host` makes each change, and the change command runs no git for its change, reads no forge and writes no record (`residents-do-the-network-and-git-work@a2d68cf95ffe`).
- `sluice push` and `sluice set-aside` go, by the user's answers of 2026-11-23.
- Each command that stays prints what it printed before, in text and in JSON, with the same exit codes.

**Non-Goals:**

- The four commands that call GitHub or AWS: `sluice forge check`, `sluice pool check`, `sluice init` and `sluice credential`.
- `sluice exec`, `sluice preview`, `sluice equip` and a worktree's release, and the `hold` and `results` requests that they send.
- The git hook, and the upkeep of the hook that each command does. A change command still runs the git of that upkeep.
- A service that owns `sluice host`.
- A bound on the forge reads. A wait on a pull request asks the forge from `sluice host` at each step, as the command asks it now. The bolt's unit `sluice-host-is-the-only-github-client` bounds it to one forge reading for each repository in each interval (`one-forge-read-per-repository-per-interval@c331a8d50756`).
- An older command at a newer `sluice host`. By the user's ruling of 2026-11-22, a command and its `sluice host` are the same version.

## Decisions

### D1. One request for each change, answered with its output

The protocol gets a request for each change: `submit`, `amend`, `withdraw`, `confirm`, `release` of a checkout or a preview, `branch` and `protect`, and the read `previews`. Each names the client's folder, the command's own arguments and the caller's session. `sluice host` answers with `output`, as it answers a read (the reads' D1): the bytes on standard output, the lines on standard error, and the exit code.

```json
{"ask":"withdraw","folder":"/w/app/feat","caller":{"SLUICE_AGENT":"feat"},"id":4,"by":null,"json":true,"branch":null}
{"output":{"stdout":"{\"id\":4,\"state\":\"withdrawn\", ...}\n","stderr":[],"code":0}}
```

`sluice host` resolves the folder as it resolves a read's folder (the reads' D2). It runs the command's code in one `spawn_blocking` call on a task of the `TaskTracker`. The call binds the repository as the host holds it, takes the branch's lock where the command took it, runs the command's code into a buffer, and drops the binding. After the call, `sluice host` tells the repository's scheduler that a change was made, in place of the `serve` request that the command sent.

A change is not joined with another ask. Two withdrawals of one submission are two calls, and the second answers as the command answered a second withdrawal.

`main.rs` sends each change command before it binds a repository, in the same place where it sends a read. Where no `sluice host` answers, the command exits 3 and says so, as a read does.

`sluice protect` is a change: `sluice host` writes the hook, which names the executable of `sluice host`, and reads the worktrees for its report.

**A change names its caller.** Before: a submit took the agent's name, the pane and `TRACEPARENT` from its own environment, and the worktree driver read the session from it. Now: each change sends `caller`, the variables of `caller::SESSION` and `TRACEPARENT`. The call runs with them as the caller's environment (`caller::as_caller`). So a submission records the caller's agent and pane, and its trace names the caller's span.

Not taken: one request that runs any command's arguments in `sluice host`. A typed request says which changes `sluice host` makes (the reads' D1).

### D2. A submit, a pull request's submit and their waits

`submit` names the folder, the worktrees of the act, the branch, `--head`, `--after`, `--priority`, the subjects, `--no-wait`, `--status` and `--json`. `sluice host` reads each worktree's head from its folder, writes the submission and answers with the submission's number in `output`. A waiting submit then sends `wait` for that number, as it does now after its own write.

For a branch that takes pull requests, `sluice host` pushes the branch with a git worker, opens or updates the request, marks it ready and takes it in. The push asks `sluice credential` for its token, as it does now in the command. `wait` also follows a pull request and a group's submissions: `sluice host` takes each step, and reads the forge for it. The client reads only what `sluice host` sends on the subscription (the reads' D8).

Before: the command pushed, opened the request and followed it on the forge. Now: `sluice host` does each, and the command follows `sluice host`.

### D3. A held landing is pushed again at each round

Decided by the user on 2026-11-23. A round of a branch with a held landing pushes the landing again, as the same landing, before it forms anything (`perform_landing`). Where the forge takes it, the round lands it as the landing it formed, with no run. Where the forge refuses it with the words that it gave before, the round writes nothing new. Where the forge gives other words, the round writes them on the held landing and logs one more `forge_refused` line. The status, the inbox and a wait read the latest words from the held landing, as they read the words now.

A branch whose one piece of work in play is a held landing takes its rounds at the pace of a repository with nothing in play: once a minute at most (the rule on an idle repository's remote). So a held landing is pushed once a minute, and not at each pass of three seconds.

Each round's push is a new call under sluice's one retry policy, and not a try of the push that the forge refused. A push whose answer was not read is settled by reading the branch, as now.

Before: no round pushed a held landing again, and the operator ran `sluice push` once the forge's rule changed. Now: each round pushes it, and `sluice push` goes.

Not taken: a push again only after the forge's rule changes. sluice cannot read a forge's push rules, so it cannot know when they change.

### D4. The set-aside goes

Decided by the user on 2026-11-23. `sluice set-aside` goes, with `Records::set_aside`, `set-aside.jsonl`, the remote ref `refs/sluice/set-aside`, and each read of them by a verdict, a report or a status. A set-aside that an earlier release wrote stays where it is, and nothing reads it. A key whose fail was set aside then reads as its results say: a fail and a pass read flaky.

A fail that the host caused is a fault of sluice. sluice is fixed so that such a run records no outcome, as record 0031 does for a check that a run did not judge, record 0043 for a check that its time limit ended, and record 0072 for a check that failed on what its executor refused.

A flaky failure's report names the result that it contradicts and when it was recorded. It says that the same head lands once a real fix changes what the check reads, and that a fail the host caused is a fault of sluice. It names no step of the operator's.

Record 0077's kind of sluice's own, which stood only for a set-aside, goes: each record that a signer signs is of a kind in in-toto's catalogue.

Before: the operator set a fail aside, and no branch read it after that. Now: no result is set aside, and sluice records no outcome for a run that the host caused to fail.

### D5. The protocol keeps only what a command of this release sends

`step` and `pass` name the folder, which `sluice host` resolves, in place of a repository that the command resolved with git. `sluice host` answers a request that no command of its release sends with the error of `resident-reads`, which names both versions. This change removes each send of `serve` by a command that it moves. Where no command sends `serve` after this change, `serve` goes from the protocol.

### D6. The bound

A git command and a credential command of a change take no slot of `[runs] max` (`runs-max-counts-checks-only@2409f7ec2f4f`). A change does not wait for room.

## Risks / Trade-offs

- [A held landing is pushed against the forge's answer once a minute.] → It is one push a minute for each held landing, and the forge's words are logged once for each distinct words. A branch whose submitter withdraws or replaces the work stops the pushes at once.
- [A set-aside that an earlier release wrote is no longer read.] → A key that it held passed reads flaky until a real fix changes what the check reads. The fail that it set aside was a fault of the host, which sluice now fixes at its cause.
- [Many commands depend on `sluice host`.] → A command that cannot reach it exits 3 and names the socket, as a read does.

## Migration Plan

1. Change the statement's clauses 116, S44, 152 and C.6 in boomdrivedev/blueprints first.
2. Land the change with the commands and `sluice host` of one version. The first command of the new version has the older `sluice host` hand over in place.

Rollback: stop `sluice host` and run the older sluice. Its set-asides and `sluice push` come back with it. A set-aside written before this change is still in the records.

## Open Questions

None.
