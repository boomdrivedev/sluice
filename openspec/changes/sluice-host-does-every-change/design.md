# Design

## Context

After `sluice-host-runs-on-a-tokio-edge`, `sluice-host-owns-its-workers` and `sluice-host-answers-every-read`, `sluice host` runs on a tokio edge, owns its workers and answers each read. A read command sends its folder and prints the `output` that `sluice host` answers (`main.rs:read`, `host_process/reads.rs:answer`). This design cites the decisions of `sluice-host-answers-every-read` as the reads' D1 to D10.

Each command that is not a read still falls through to `main.rs`, which binds the repository in the command's own process (`Bound::open_with`, `main.rs:291`) and runs the command's code there (`lib.rs:dispatch`). The gap, at main 87bb89a3b:

| Where | What the client does |
|---|---|
| `main.rs:136-153` | Each command installs or puts back the hook: `protect::install` and `reinstall` run `git` (`protect.rs:48`) and write the hook's files. |
| `main.rs:202-211`, `main.rs:788-829`, `protect.rs:400-505` | The hook binds the repository (`Bound::guarding`), reads the branch's ref, the policy and the records, and writes each refused write under the branch's lock (`record_protected`). |
| `main.rs:291-317` | Each command that is not a read, a watch or a wait binds the repository. |
| `lib.rs:1982-2168` | A submit writes its submission (`lib.rs:submit` at 3368, `submit_group` at 3512). |
| `lib.rs:6093`, `lib.rs:6590` | A pull request's submit pushes the branch and opens, updates and marks the request from the command (`submit_pull`, `submit_pull_group`). |
| `lib.rs:2213-2290` | A wait on a pull request, and a wait on a branch that takes pull requests, read the forge and follow the request from the command. |
| `lib.rs:2628`, `2714`, `2774`, `2323` | An amend, a withdrawal, a set-aside and `sluice confirm` write the records. |
| `lib.rs:2863`, `lib.rs:7481` | `sluice push` pushes a held landing. |
| `lib.rs:2868-2920`, `lib.rs:7543` | A release frees a checkout, a preview or a worktree's place, and a worktree's release runs its release command. |
| `lib.rs:2929-2978` | `sluice protect` writes the hook and reads the worktrees. |
| `lib.rs:2979-2993` | `sluice branch create`, `accept` and `delete` make, move or remove a branch and push it. |
| `lib.rs:2361`, `main.rs:291` | `sluice step` binds the repository to send its `step` request. |
| `lib.rs:2862` | `sluice previews` reads the previews' records. |
| `lib.rs:2821`, `exec.rs:304-660` | `sluice exec` reads the plan, equips the worktree (the toolchain and the task runner), asks `hold`, and starts the run's supervisor. |
| `runs.rs:302-330` | Outside `sluice host`, the supervisor starts in a session of its own by `setsid`, so a session's run has no owner. |
| `lib.rs:2847`, `preview.rs:165-300` | `sluice preview` makes the preview, asks `hold`, and starts its command's supervisor in a session of its own. |
| `lib.rs:2839`, `main.rs:780-787` | `sluice equip` finds the worktree's top with git and runs the equip steps. |
| `lib.rs:1974-1980`, `host_process/client.rs:417`, `446`, `495` | The command sends `serve`, `results` and `hold`, each naming a repository that the command resolved. |
| `main.rs:124-131`, `main.rs:175-200`, `lib.rs:2330-2360`, `lib.rs:2994-3011` | `sluice credential`, `sluice forge check`, `sluice init` and `sluice pool check` call GitHub or AWS, or mint a token. Out of scope: the bolt's unit `sluice-host-is-the-only-github-client`. |

## Goals / Non-Goals

**Goals:**

- `sluice host` makes each change, and the client runs no git, reads no forge and writes no record (`residents-do-the-network-and-git-work@a2d68cf95ffe`).
- A client starts no other tool. It starts only `sluice host`, where none answers (`clients-never-fetch-or-start-a-tool@8aa9c57f19b2`).
- A session's checks and a preview's command are workers of `sluice host`, and end with it (`workers-end-with-their-owner@292e869158d6`, `nothing-detached@f807c7f5fcea`, `sluices-runs-end-with-its-host-process@84c331c3e64a`).
- The hook reads no state. `sluice host` judges each transaction (`reads-go-through-the-resident@69134e5825d7`).
- Each command prints what it printed before, in text and in JSON, with the same exit codes.

**Non-Goals:**

- The four commands that call GitHub or AWS: `sluice forge check`, `sluice pool check`, `sluice init` and `sluice credential`.
- A service that owns `sluice host`.
- A bound on the forge reads. A wait on a pull request asks the forge from `sluice host` at each step, as the command asks it now. The bolt's unit `sluice-host-is-the-only-github-client` bounds it to one forge reading for each repository in each interval (`one-forge-read-per-repository-per-interval@c331a8d50756`).
- An older command at a newer `sluice host`. By the user's ruling of 2026-11-22, sluice has no public clients during the reset, so a command and its `sluice host` are the same version, and no request is kept for an older command.

## Decisions

### D1. One request for each change, answered with its output

The protocol gets a request for each change: `submit`, `amend`, `withdraw`, `set-aside`, `confirm`, `push`, `release`, `branch` and `protect`, and the read `previews`. Each names the client's folder, the command's own arguments and the caller's session. `sluice host` answers with `output`, as it answers a read (the reads' D1): the bytes on standard output, the lines on standard error, and the exit code.

```json
{"ask":"withdraw","folder":"/w/app/feat","caller":{"SLUICE_AGENT":"feat"},"id":4,"by":null,"json":true,"branch":null}
{"output":{"stdout":"{\"id\":4,\"state\":\"withdrawn\", ...}\n","stderr":[],"code":0}}
```

`sluice host` resolves the folder as it resolves a read's folder (the reads' D2). It runs the command's code in one `spawn_blocking` call on a task of the `TaskTracker`. The call binds the repository as the host holds it, takes the branch's lock where the command took it, runs the command's code into a buffer, and drops the binding. After the call, `sluice host` tells the repository's scheduler that a change was made, in place of the `serve` request that the command sent.

A change is not joined with another ask. Two withdrawals of one submission are two calls, and the second answers as the command answered a second withdrawal.

`main.rs` sends each change command before it binds a repository, in the same place where it sends a read. Where no `sluice host` answers, the command exits 3 and says so, as a read does.

**A change names its caller.** Before: a submit took the agent's name, the pane and `TRACEPARENT` from its own environment, and the worktree driver read the session from it. Now: each change sends `caller`, the variables of `caller::SESSION` and `TRACEPARENT`. The call runs with them as the caller's environment (`caller::as_caller`). So a submission records the caller's agent and pane, and its trace names the caller's span.

Not taken: one request that runs any command's arguments in `sluice host`. A typed request says which changes `sluice host` makes (the reads' D1).

### D2. A submit, a pull request's submit and their waits

`submit` names the folder, the worktrees of the act, the branch, `--head`, `--after`, `--priority`, the subjects, `--no-wait`, `--status` and `--json`. `sluice host` reads each worktree's head from its folder, writes the submission and answers with the submission's number in `output`. A waiting submit then sends `wait` for that number, as it does now after its own write.

For a branch that takes pull requests, `sluice host` pushes the branch with a git worker, opens or updates the request, marks it ready and takes it in. The push asks `sluice credential` for its token, as it does now in the command. `wait` also follows a pull request and a group's submissions: `sluice host` takes each step, and reads the forge for it. The client reads only what `sluice host` sends on the subscription (the reads' D8).

Before: the command pushed, opened the request and followed it on the forge. Now: `sluice host` does each, and the command follows `sluice host`.

### D3. A session's checks, a preview's command and an equip run in sluice host

The protocol gets `exec`, `preview` and `equip`, and `release` frees a worktree's place. Each names the folder, the command's arguments, the caller's session and the caller's run variables (D4). `sluice host` runs the command's code on a thread of the blocking pool:

1. It reads the plan, equips the working copy and forms the forecast, as `sluice exec` does now.
2. It asks its own room for a slot of `[runs] max`, in the order of the run bound, with no `hold` request. While it waits, it sends `waiting:` lines as the command printed them.
3. It starts the run's supervisor as a worker (`runs.rs:Runs::start` with an owner), in a process group of its own, recorded in `host/workers/`.
4. It reads the run's log as it grows, as `exec.rs:follow` does now, and sends each new part to the client as a `stdout` line. Under `--json`, the part goes in a `stderr` line, as the command wrote it to standard error.
5. It ends with `output`: the summary and the exit code.

```json
{"stdout":"running cli:test ...\n"}
{"stderr":"waiting: the host is at its bound"}
{"output":{"stdout":"passed: 14 tasks\n","stderr":[],"code":0}}
```

The client writes each line as it comes. Its Ctrl-C closes the connection. When a connection that holds a run closes before `output`, `sluice host` ends the run's process group, gives its slot back, and records the run as stopped, as `exec.rs:follow` does now on Ctrl-C. A run that waits for its slot stops waiting when its connection closes.

`equip` sends each step's output in the same way. `release` of a worktree runs the place's release command as a worker and answers `output`.

A preview's command hands off a stack that stays until its release (preview 98). So `preview` answers `output` once the command started, and the run stays a worker of `sluice host` after the connection closes. Only `sluice release` or a failed command ends it, as now.

No pseudo-terminal: the checks write to the run's log, as now, and the client prints what the log holds.

Before: `sluice exec` and `sluice preview` started the run's supervisor in a session of its own, and the run had no owner after the command ended. Now: `sluice host` starts it as a worker, and the run ends with `sluice host`, or with its client for a session's run.

Not taken: the session's checks as children of the client, which asks `sluice host` for room. That keeps a worker that its resident does not own (`workers-end-with-their-owner@292e869158d6`).

Not taken: a pseudo-terminal for the checks. The checks write to the run's log now, and no check reads its terminal. A pseudo-terminal adds a second path for the output for no reader that needs it.

### D4. A run's kept variables come from its caller

A run's environment holds the variables of `profiles/checks.rs:KEEP`, the `LC_*` variables and, for a run with a container engine, the `DOCKER_*` variables, read from the environment of the process that starts the run (`profiles/checks.rs:environment`). That process is now `sluice host`, whose environment is the one of the first command that started it. So `exec`, `preview`, `equip` and `release` send those variables of the caller's environment as `run_env`, and the call reads them from the request in place of its own environment.

Before: a session's run took `PATH`, `HOME` and the other kept variables from the `sluice exec` that started it. Now: it takes them from the `run_env` of the request, which is the same values.

### D5. The git hook asks sluice host

The hook reads the transaction on its standard input, the folder that it runs in, `GIT_DIR`, and the command line of the git that runs it (`protect.rs:caller`). It sends them as `reference-transaction`. It reads no file of the repository and no record.

`sluice host` resolves the repository that git names (branch-protection: the protection judges the repository git names). It judges each line by the branch's ref on the host and the policy that it keeps, as `protect.rs:judge` does now, with git's packing let go first (record 0110). It writes each refused write under the branch's lock, as `protect.rs:hook` does now: the first refusal of a cause is a `protected` line, and each later one is counted. It answers `output` with the refusal's words and the code, 0 or 1.

The connection's task answers `reference-transaction` before any read or change, from the state that the host keeps. It never waits for a round or a reading. So a round that holds the repository does not hold a commit.

Where no `sluice host` answers, the hook starts one, as a client does. Where `sluice host` does not answer within the hook's bound, the hook refuses the transaction, and says that `sluice host` did not answer and that `sluice protect --remove` takes the protection out. The hook's bound is ten seconds: it covers the start of `sluice host` on a host where none ran, and a hand-over's wait for the rounds to end.

Before: the hook bound the repository, read the branch's state and wrote the refused write itself. Now: it asks `sluice host`, which judges and writes.

### D6. The git commands of sluice host carry its name

`sluice host` gives each git command that its workers run the variable `SLUICE_HOST_PROCESS`, `<pid> <start>`: the pid of `sluice host` and its start time in seconds. The hook lets a transaction go with no ask where that pid runs and started at that time. A value whose pid does not run, or started at another time, is no name: the hook asks `sluice host`.

`sluice host` takes the variable out of the environment of each check, each command of a session, each preview's command, each release command and each equip step. So a write to `main` by a check is judged.

This keeps record 0110: a fetch of `sluice host` whose packing runs the hook waits on no ask of the hook. It also keeps the hook from asking `sluice host` about a write that `sluice host` decided.

The guard is not a security boundary (`protect.rs`): a person can remove the hook. The pid and the start time keep a value that an earlier `sluice host` gave from passing a later write.

### D7. sluice host puts back the hook

`sluice host` puts back the branch protection's hook each time it binds a repository for a request, where sluice's hook stands for another sluice, as each command did. It installs the hook where a submit, a wait or a step names the repository and none stands. The hook names the executable of `sluice host`. `sluice protect --remove` takes the hook out, and a later request does not put it back.

Before: each command put back the hook in its own process, by its own path. Now: `sluice host` puts it back by its own path. A newer command has `sluice host` hand over first, so the hook names the newer sluice.

### D8. The protocol keeps only what a command of this release sends

`serve`, `hold` and `results` go: `sluice host` tells itself of each change (D1), holds the room of each session's run (D3) and reads the results in its own calls. `step` and `pass` name the folder, which `sluice host` resolves, in place of a repository that the command resolved with git. `sluice host` answers a request that it does not know with the error of `resident-reads`, which names both versions.

### D9. The bound

A session's run and a preview's command take a slot of `[runs] max`, as now. A git command, a credential command, an equip step and a release command take none (`runs-max-counts-checks-only@2409f7ec2f4f`). The order of the run bound does not change: a combination's run starts first, taking turns with the sessions' runs, and sessions' and previews' runs start in the order that they came to wait. The waits are now in one process, so a stopped client's wait leaves the count at once.

## Risks / Trade-offs

- [Each commit in each worktree asks `sluice host`.] → The connection's task answers from the kept state, with no round and no reading in the way. The transaction names a branch that the host does not protect in most commits, and `sluice host` answers it at once.
- [A hook that refuses while `sluice host` does not answer stops each commit on the host.] → The hook starts `sluice host` where none runs, and waits ten seconds. Its words name `sluice protect --remove`. See the open question.
- [The output of a session's checks passes through `sluice host`.] → `sluice host` reads the run's log as `sluice exec` read it, and sends only the new part. A slow client holds only its own connection's task.
- [A run's kept variables in a request.] → They are the variables that a run takes now, and only they. `sluice host` takes no other variable of the caller.
- [Many commands depend on `sluice host`.] → A command that cannot reach it exits 3 and names the socket, as a read does.

## Migration Plan

1. Land the change with the commands and `sluice host` of one version. The first command of the new version has the older `sluice host` hand over in place.
2. Each repository's hook names the newer sluice once `sluice host` binds the repository for its first request.

Rollback: stop `sluice host`, which ends its runs, and run the older sluice. Its first command puts back the hook for itself.

## Open Questions

- Where `sluice host` does not answer within the hook's bound, the hook refuses the write (D5), as the change `sluice-host-is-the-resident` has it. The other way is today's way for a transaction that the hook cannot read: the write goes, said once. A refusal keeps a protected branch safe from a person's write, but stops each commit on the host while `sluice host` does not answer. The user decides which.
