# Design

## Context

sluice starts three kinds of process detached today, each in a session of its own:

- A command starts the host process: `host_process/client.rs:Client::start`.
- A host process starts each run's supervisor: `runs.rs:Runs::start`. The supervisor runs the combination's checks in its own process group.
- A hand-over of a host process that is no daemon starts its successor: `host_process/mod.rs:hand_over`.

A run continues after its host process ends, and the next host process reads its result. A stopped daemon leaves its runs going, and its test asserts it. The launchd agent sets `AbandonProcessGroup` true, and the systemd unit sets `KillMode` `process`.

The tests start the binary with a state folder and scratch folders of their own. A host process ends at its next tick after its state folder is gone. Nothing makes a test fail when a process outlives it. A detached process takes no part in its test's process group, and its output goes to the host process's log. So a runner that watches a test's output does not see it.

This change gives each worker an owner that ends it, and adds the guard that proves it. It builds on the tokio edge of `sluice-host-runs-on-a-tokio-edge`. See the proposal for why.

## Goals / Non-Goals

**Goals:**

- Each worker of `sluice host` ends when its owner ends, unless a hand-over passes it on.
- Each test that leaves a process fails, in each Rust crate and in each shell test task.
- No process of a test runs after the test's guard is done.

**Non-Goals:**

- A session's checks and a preview's command as workers of `sluice host`. Today `sluice exec` and `sluice preview` start them. The bolt's unit `sluice-host-does-every-change` moves them.
- A command that starts the host process. It still starts it in a session of its own, as `clients-never-fetch-or-start-a-tool@8aa9c57f19b2` allows on a bare install. The guard of D6 finds that host process by its state folder.
- A new limit. `[runs] max` stays the limit of checks, session runs and previews, by the user's answer of 2026-11-21. git and credential workers run outside it.
- A rule for the plugins' own code. Only their tests change.

## Decisions

### D1. Each worker in a process group of its own, through process-wrap

`sluice host` starts each worker through process-wrap's `ProcessGroup` on tokio's `Command`, with `KillOnDrop`. A run's supervisor is one worker, and the checks are in its group. The supervisor no longer calls `setsid`. weir-bounded keeps the groups, the limits and the grace of remote calls, as now. Before a worker does its work, `sluice host` writes its process group, its pid and its start time to the host's state, under `host/workers/`. When the worker ends, `sluice host` removes the record. (`workers-end-with-their-owner@292e869158d6`)

Not taken: tokio's `kill_on_drop` alone. It ends the direct child only, and git and the checks start children of their own.

### D2. A stop ends the workers

A stop of `sluice host`, by a signal or by a state folder that is gone, cancels the root token of the tokio edge. Each task finishes its round. `sluice host` then ends each worker's process group, `SIGTERM` first and `SIGKILL` after the grace that weir-bounded gives, and removes its record. A run that ended so has no outcome. It blames no one, and its combination is open for the next host process. It counts toward the runs in a row as a run whose claim lapsed does. So a stop or a crash that repeats ends at a bound. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

Not taken: a run that outlives its owner. The accepted decision rules it out.

### D3. Every hand-over starts afresh in place

Each host process hands over as a daemon does now. It cancels its tasks, waits for them, lets its lock and socket go, and replaces its image with the newer program by `execve`. The pid stays the same, so each worker stays its child, and launchd and systemd see the same job. The new image reads `host/workers/` and waits on each worker by its pid. A take-up of `[telemetry]` uses the same path. The start lock is carried across the `execve`, as the daemon does now. An older command still uses a newer host process, so no client breaks. (`sluices-runs-end-with-its-host-process@e814b716f3b1`, `nothing-detached@f807c7f5fcea`)

Not taken: a successor of another pid that adopts the runs. A process cannot take another process's children, so the runs would have no parent that owns them.

### D4. A sweep at start

A `sluice host` that starts reads `host/workers/` before it claims a run or starts a worker. A record can name a pid that still runs with the recorded start time, under no live host process. For each such record, it ends the process group and gives the run no outcome. A record whose pid is gone, or runs with another start time, is removed with nothing ended. (`workers-end-with-their-owner@292e869158d6`, `nothing-detached@f807c7f5fcea`)

On Linux, systemd's cgroup also ends every worker of a unit that stops. On macOS, a worker in its own group is outside launchd's reach, so the sweep covers a `SIGKILL` there.

### D5. The services end the job, and a new package is a hand-over

The launchd agent sets `AbandonProcessGroup` false, and the systemd unit sets `KillMode` `control-group`. The service runs sluice by the user's profile's `bin/sluice`, so a new package changes no service and no restart ends the runs. The user's activation asks the running host process to hand over to the profile's program, where the program differs. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

Not taken: a restart of the service on a new package. With the job's group ended at a stop, a restart would end the runs.


### D6. A guard that finds a test's processes by its folders

The guard holds the test's state folder and scratch folders. When the test ends, it lists the user's processes with their environment and their working folder. A process belongs to the test where its `XDG_STATE_HOME` is the test's state folder. It also belongs to the test where its working folder is in one of the test's scratch folders. The host process keeps the environment of the command that started it, so the guard finds it. A run's checks work in a place in the test's repository, so the guard finds them too. (`a-leaked-child-fails-its-test@9a82a5147ba6`)

The guard lists processes with the sysinfo crate. sysinfo reads a process's environment and working folder on macOS and on Linux for the user's own processes.

Not taken: a variable that the guard sets and that each process inherits. A run's checks get a run's environment, so the variable would not reach them unless sluice kept it for tests. Not taken: the process group or the parent. A detached process has neither of the test's.

### D7. Stop the host processes first, then judge

At the test's end, the guard first sends `SIGTERM` to each `sluice host` of the test and waits for it to exit, up to ten seconds. A test that ends with its host process running is normal. A test that ends with a run, a supervisor or a git command still running is a leak. The guard then lists the test's processes again. It fails the test where any is left, and ends each of them, `SIGTERM` first and `SIGKILL` after two seconds.

The guard reports the failure after it ends the processes, so a failed test leaves nothing behind.

Not taken: a test that must stop its own host process. Each test would then repeat the same stop, and a test that panics would skip it.

### D8. The guard is part of the test's setup

`OnDisk` in the test support holds a guard. Each test that builds an `OnDisk` gets the guard with no other line. A test that starts sluice without `OnDisk` builds a guard of its own from its folders. The guard's check runs in `Drop`. Where the test already panicked, the guard ends the processes and does not panic again.

### D9. cargo-nextest for the Rust crates

The `test`, `tools` and `sandbox` tasks of the five crates run `cargo nextest run` with the same filters. A nextest profile at the root sets `leak-timeout` to `{ period = "500ms", result = "fail" }`. nextest runs each test in a process of its own, so a test that leaves a process that holds its output fails. The devenv shell pins cargo-nextest, as it pins the tree's other tools.

Not taken: the guard alone. It does not see a process that a test starts outside its folders, such as a plain `sleep`.

### D10. One shell wrapper for the shell test tasks

A script, `tests/leaks.sh` at the root, runs a test command with a new scratch folder and state folder in its environment. When the command ends, the script lists the processes of those folders by their environment and their working folder. It fails where one is left, and ends it. The plugins' shell test tasks and the check-e2e tier run through it.

## Risks / Trade-offs

- [`execve` in place with many threads.] → The host process cancels its tasks and waits for the tracker first, as the daemon does now. Only the thread that calls `execve` remains.
- [A sweep that ends a group which another process now holds.] → The sweep acts only where the pid still runs with the recorded start time.
- [A stop now costs the runs that were going.] → A hand-over keeps them. Only a stop, a crash or a removed state folder ends them, and their combinations are tested again.
- [A test that leaves a run now fails.] → The change's tasks run each suite one at a time. Each failing test then waits for its runs to end, or withdraws its submission, before it ends.
- [The guard's list of processes costs time at each test's end.] → It lists the processes once, or twice where a host process was stopped. sysinfo reads only the processes' environment and working folder.
- [nextest runs each test in a process of its own, so a test that shared state through statics in one process may act differently.] → The suites already run each test file as a binary of its own. The change runs each suite under nextest and fixes what fails.

## Migration Plan

- The first command of this version has the older host process hand over in place. Runs that an older host process started detached keep going until they end. The new host process reads their results as before, and owns each run that it starts.
- A host with the daemon's service takes the new module. Its first switch keeps the service's program path stable from then on.
- The change replaces `cargo test` with `cargo nextest run` in the crates' tasks. A person who runs `cargo test` by hand still can. Only the tasks and `sluice exec` use nextest.
