# Design

## Context

After `sluice-host-runs-on-a-tokio-edge`, `sluice-host-owns-its-workers` and `sluice-host-answers-every-read`, `sluice host` runs on a tokio edge, owns the workers of a combination's checks and answers each read. A read command sends its folder and prints the `output` that `sluice host` answers. This design cites the decisions of `sluice-host-answers-every-read` as the reads' D1 to D10.

The gap, at main 87bb89a3b:

| Where | What the client does |
|---|---|
| `lib.rs:2821`, `exec.rs:304-660` | `sluice exec` reads the plan, equips the worktree (the toolchain and the task runner), asks `hold`, and starts the run's supervisor. |
| `runs.rs:302-330` | Outside `sluice host`, the supervisor starts in a session of its own by `setsid`, so a session's run has no owner. |
| `lib.rs:2847`, `preview.rs:165-300` | `sluice preview` makes the preview, asks `hold`, and starts its command's supervisor in a session of its own. |
| `lib.rs:2839`, `main.rs:780-787` | `sluice equip` finds the worktree's top with git and runs the equip steps. |
| `lib.rs:2888-2920`, `lib.rs:7543` | A worktree's `sluice release` runs its release command, frees the place and sends `serve`. |
| `main.rs:291-317` | Each of these commands binds the repository. |
| `profiles/checks.rs:418`, `host_process/client.rs:495` | A session's run and a preview ask `sluice host` for room by `hold`. |
| `profiles/records.rs:1163`, `host_process/client.rs:446` | A command that binds the repository asks `sluice host` for the results by `results`. |

## Goals / Non-Goals

**Goals:**

- A client starts no check, toolchain, task runner or release command (`clients-never-fetch-or-start-a-tool@8aa9c57f19b2`).
- A session's checks and a preview's command are workers of `sluice host`, and end with it (`workers-end-with-their-owner@292e869158d6`, `nothing-detached@f807c7f5fcea`, `sluices-runs-end-with-its-host-process@84c331c3e64a`).
- Each command prints what it printed before, in text and in JSON, with the same exit codes.

**Non-Goals:**

- The change commands, `sluice previews` and `sluice step`.
- The git hook, and the upkeep of the hook that each command does. These commands still run the git of that upkeep.
- An older command at a newer `sluice host`. By the user's ruling of 2026-11-22, a command and its `sluice host` are the same version.

## Decisions

### D1. A session's checks, a preview's command and an equip run in sluice host

The protocol gets `exec`, `preview`, `equip` and `release` of a worktree. Each names the folder, the command's arguments, the caller's session and the caller's run variables (D2). `sluice host` resolves the folder as it resolves a read's folder (the reads' D2), and runs the command's code on a thread of the blocking pool:

1. It reads the plan, equips the working copy and forms the forecast, as `sluice exec` does now.
2. It takes a slot of `[runs] max` from its own room (D3).
3. It starts the run's supervisor as a worker (`runs.rs:Runs::start` with an owner), in a process group of its own, recorded in `host/workers/`.
4. It reads the run's log as it grows, as `exec.rs:follow` does now, and sends each new part to the client as a `stdout` line. Under `--json`, the part goes in a `stderr` line, as the command wrote it to standard error.
5. It ends with `output`: the summary and the exit code.

```json
{"stdout":"running cli:test ...\n"}
{"stderr":"waiting: the host is at its bound"}
{"output":{"stdout":"passed: 14 tasks\n","stderr":[],"code":0}}
```

The client writes each line as it comes.

`equip` sends each step's output in the same way, and finds the worktree's top from the folder. `release` of a worktree runs the place's release command as a worker, frees the place, and answers `output`. After the call, `sluice host` tells the repository's scheduler of the release, in place of the `serve` request that the command sent.

A preview's command hands off a stack that stays until its release (preview 98). So `preview` answers `output` once the command started, and the run stays a worker of `sluice host` after the connection closes. Only `sluice release` or a failed command ends it, as now.

No pseudo-terminal: the checks write to the run's log, as now, and the client prints what the log holds.

Before: `sluice exec` and `sluice preview` started the run's supervisor in a session of its own, and the run had no owner after the command ended. Now: `sluice host` starts it as a worker, and the run ends with `sluice host`, or with its client for a session's run.

Not taken: the session's checks as children of the client, which asks `sluice host` for room. That keeps a worker that its resident does not own (`workers-end-with-their-owner@292e869158d6`).

Not taken: a pseudo-terminal for the checks. The checks write to the run's log now, and no check reads its terminal. A pseudo-terminal adds a second path for the output for no reader that needs it.

### D2. A run's kept variables come from its caller

A run's environment holds the variables of `profiles/checks.rs:KEEP`, the `LC_*` variables and, for a run with a container engine, the `DOCKER_*` variables, read from the environment of the process that starts the run (`profiles/checks.rs:environment`). That process is now `sluice host`, whose environment is the one of the first command that started it. So `exec`, `preview`, `equip` and `release` send those variables of the caller's environment as `run_env`, and the call reads them from the request in place of its own environment. `sluice host` takes no other variable of the caller.

Before: a session's run took `PATH`, `HOME` and the other kept variables from the `sluice exec` that started it. Now: it takes them from the `run_env` of the request, which holds the same values.

### D3. The bound

A session's run and a preview's command take a slot of `[runs] max`, as now. An equip step and a release command take none (`runs-max-counts-checks-only@2409f7ec2f4f`). The order of the run bound does not change: a combination's run starts first, taking turns with the sessions' runs, and sessions' and previews' runs start in the order that they came to wait. While a run waits, `sluice host` sends the `waiting:` lines that the command printed.

The waits are now in one process, so `hold` goes, and a stopped client's wait leaves the count at once.

### D4. A session's run ends with its client and with sluice host

The client's Ctrl-C closes the connection. When a connection that holds a session's run closes before `output`, `sluice host` ends the run's process group, gives its slot back, and records the run as stopped, as `exec.rs:follow` does now on Ctrl-C. A run that waits for its slot stops waiting when its connection closes.

When `sluice host` ends for a reason other than a hand-over, it ends each session's run, as it ends each worker. The client reads the closed connection, says that sluice's host process ended, and exits 3.

### D5. The protocol keeps only what a command of this release sends

`hold` goes: `sluice host` holds the room of each session's run (D3). This change removes each read of `results` and each send of `serve` by a command that it moves. Where no command sends one of them after this change, it goes from the protocol. `sluice host` answers a request that no command of its release sends with the error of `resident-reads`, which names both versions.

## Risks / Trade-offs

- [The output of a session's checks passes through `sluice host`.] → `sluice host` reads the run's log as `sluice exec` read it, and sends only the new part. A slow client holds only its own connection's task.
- [A run's kept variables in a request.] → They are the variables that a run takes now, and only they.
- [A `sluice host` that ends takes each session's run with it.] → A hand-over in place keeps them (`sluice-host-owns-its-workers`). Any other end of `sluice host` is a fault that the client reports with exit 3.

## Migration Plan

Land the change with the commands and `sluice host` of one version. The first command of the new version has the older `sluice host` hand over in place. A session's run that an older `sluice exec` started in a session of its own runs on until it ends.

Rollback: stop `sluice host`, which ends its runs, and run the older sluice.

## Open Questions

None.
