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

### D1. Each worker in a process group of its own, recorded before it works

`sluice host` starts each worker with std's `Command` in a process group of its own. A run's supervisor is one worker, and the checks are a group that the supervisor starts. The supervisor no longer calls `setsid`. weir-bounded keeps the limits and the grace of remote calls, as now. `crate::workers` makes a command a worker: between its fork and its exec, the child writes its record to `host/workers/<pid>`, with the owner's pid and start time. A write that fails stops the start, so no work runs unrecorded. A run's supervisor records the run's checks and watch for the same owner, by the variable `SLUICE_WORKERS`. When a child worker ends, `sluice host` removes its record. A record of a worker that is not its child goes at the next prune. (`workers-end-with-their-owner@292e869158d6`)

Not taken: the process-wrap crate. std's process group and the record before the exec do the same with no new crate. Not taken: tokio's `kill_on_drop` alone. It ends the direct child only, and git and the checks start children of their own.

### D2. A stop ends the workers

A stop of `sluice host`, by a signal or by a state folder that is gone, cancels the root token of the tokio edge. Each task finishes its round. `sluice host` then sends `SIGTERM` to each child worker's group, and gives it 15 seconds. A run's supervisor that is told to stop ends its checks and writes no result. Then `sluice host` ends each group that a record still names, `SIGTERM` first and `SIGKILL` after the grace that weir-bounded gives, and removes its record. A run that ended so has no outcome. It blames no one, and its combination is open for the next host process. It counts toward the runs in a row as a run whose claim lapsed does. So a stop or a crash that repeats ends at a bound. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

Not taken: a run that outlives its owner. The accepted decision rules it out.

### D3. Every hand-over starts afresh in place

Each host process hands over as a daemon does now. It cancels its tasks, waits for them, lets its lock go, and replaces its image with the newer program by `execve`. The pid stays the same, so each worker stays its child, and launchd and systemd see the same job. The new image reads `host/workers/` and waits on each worker by its pid. A take-up of `[telemetry]` uses the same path. The start lock and the socket are carried across the `execve`, as the daemon does now. A newer command waits for a host process of another pid, or of the same pid with another `since`. Where the process that it asked ends, as an older sluice's does, it starts its own. A host process still writes to `SLUICE_READY_FD` where an older sluice's host process hands over to it. An older command still uses a newer host process, so no client breaks. (`sluices-runs-end-with-its-host-process@e814b716f3b1`, `nothing-detached@f807c7f5fcea`, `never-break-an-existing-client@ed293965fc5c`)

A daemon is another process, so it cannot take a host process's runs. A daemon that starts while a host process that is no daemon holds the host sends `hand-over` with `ends`. That host process ends, and its runs end with it. The daemon tests their combinations again. A host process of an older sluice reads the request as a `hand-over`, and ends as before. A host process with a running child worker is not idle, since its workers would end with it.

Not taken: a successor of another pid that adopts the runs. A process cannot take another process's children, so the runs would have no parent that owns them.

### D4. A sweep at start

A `sluice host` that starts reads `host/workers/` before it claims a run or starts a worker. A record is of a live worker only where its pid runs and started within two seconds of the record's write. For each live record of another owner, it ends the process group, and the run has no outcome. It waits again on each live child that an earlier image of its own process started. A record whose pid is gone, or runs with another start time, is removed with nothing ended. (`workers-end-with-their-owner@292e869158d6`, `nothing-detached@f807c7f5fcea`)

On Linux, systemd's cgroup also ends every worker of a unit that stops. On macOS, a worker in its own group is outside launchd's reach, so the sweep covers a `SIGKILL` there.

### D5. The services end the job, and a new package is a hand-over

The launchd agent sets `AbandonProcessGroup` false, and the systemd unit sets `KillMode` `control-group`. The service runs the system profile's `bin/sluice`, where the module installs its package, so a new package changes no service and no restart ends the runs. `hello`'s answer names the executable that the host process runs. `sluice hand-over` has a host process that runs another sluice start afresh as this one, and starts none. The user's activation runs it: on nix-darwin after the link, and on NixOS where the unit runs. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

Not taken: a restart of the service on a new package. With the job's group ended at a stop, a restart would end the runs.


### D6. A guard that finds a test's processes by its folders

The guard holds the test's state folder and scratch folders. When the test ends, it lists the user's processes with their environment and their working folder. A process belongs to the test where its `XDG_STATE_HOME` is the test's state folder. It also belongs to the test where its working folder is in one of the test's scratch folders. The host process keeps the environment of the command that started it, so the guard finds it. A run's checks work in a place in the test's repository, so the guard finds them too. (`a-leaked-child-fails-its-test@9a82a5147ba6`)

The guard lists processes with the sysinfo crate. sysinfo reads a process's environment and working folder on macOS and on Linux for the user's own processes.

Not taken: a variable that the guard sets and that each process inherits. A run's checks get a run's environment, so the variable would not reach them unless sluice kept it for tests. Not taken: the process group or the parent. A detached process has neither of the test's.

### D7. Stop the host processes first, then judge

At the test's end, the guard first notes the test's processes. Then it sends `SIGTERM` to each `sluice host` of the test and waits for it to exit, up to thirty seconds. A test that ends with its host process running is normal, and so is a git or credential worker that the host's stop ends. A test that ends with a run, a supervisor or another process still running is a leak. A process that ends by itself within one second, such as the far side of a git fetch that just ended, does not continue to run, and is no leak. The guard then lists the test's processes again. It fails the test where any was left, and ends each of them, `SIGTERM` first and `SIGKILL` after two seconds.

The guard reports the failure after it ends the processes, so a failed test leaves nothing behind.

Not taken: a test that must stop its own host process. Each test would then repeat the same stop, and a test that panics would skip it.

### D8. The guard is part of the test's setup

`OnDisk` in the test support holds a guard of all its folders, and each `Scratch` holds a guard of its own folder. So each test that builds an `OnDisk` or a scratch folder gets the guard with no other line. The guard's check runs in `Drop`. Where the test already panicked, the guard ends the processes and does not panic again.

### D9. cargo-nextest for the Rust crates

The `test`, `tools` and `sandbox` tasks of the five crates run `cargo nextest run` with the same filters. A nextest profile at the root sets `leak-timeout` to `{ period = "500ms", result = "pass" }`. nextest runs each test in a process of its own, and reports a test that leaves a process that holds its output as leaky. The guard fails a test that leaves a process (D6). The devenv shell pins cargo-nextest, as it pins the tree's other tools.

nextest's report does not fail a test, because on macOS it also names a test that left nothing. When nextest starts two test processes at one time, the second can inherit the first's output pipe at a high descriptor. The second test, and the commands that it starts, then hold the first test's output until the second test ends, 15 to 40 seconds later in the peers tests. A longer period does not prevent this: the other test can run for any time.

Not taken: nextest's report as a failure. The gate would fail tests that left nothing.

Not taken: one test at a time, which prevents the inherited pipe. The tools tier alone took 14 minutes with its tests in parallel.

Not taken: the guard alone, with no report. The report names a process that a test starts outside its folders, such as a plain `sleep`, which the guard does not see.

### D10. One shell wrapper for the shell test tasks

A script, `tests/leaks.sh` at the root, runs a test command in a process group of its own, with a new scratch folder and state folder in its environment. When the command ends, it stops each `sluice host` that a state folder in the scratch folder names. Then it lists what is left of the command's process group, and each worker that a state folder records. It fails where one is left, and ends it. It reads processes with pgrep alone, since the gate's sandbox refuses `ps`, which is set-user-ID on macOS. The plugins' shell test tasks and the check-e2e tier run through it. `root:leaks-cases` runs it on a test that leaves a watcher and on one that leaves nothing.

Not taken: a listing by environment and working folder, as the guard does. The sandbox refuses `ps`, and `lsof` takes minutes on a host with many processes.

### D11. A lost run that names no check is tested again

A run that is given no check by name, on a branch that names no required check, ends with no outcome when its owner stops. The core named no check of such a run with no outcome, so it read the run as passed. Now such a run has every check of the run with no outcome, and its follow-up is given every check again. A follow-up's pass decides the run. (`sluices-runs-end-with-its-host-process@e814b716f3b1`)

## Risks / Trade-offs

- [`execve` in place with many threads.] → The host process cancels its tasks and waits for the tracker first, as the daemon does now. Only the thread that calls `execve` remains.
- [A sweep that ends a group which another process now holds.] → The sweep acts only where the pid still runs with the recorded start time.
- [A stop now costs the runs that were going.] → A hand-over keeps them. Only a stop, a crash or a removed state folder ends them, and their combinations are tested again.
- [A test that leaves a run now fails.] → The change's tasks run each suite one at a time. Each failing test then waits for its runs to end, or withdraws its submission, before it ends.
- [The guard's list of processes costs time at each test's end.] → It lists the processes once, or twice where a host process was stopped. sysinfo reads only the processes' environment and working folder.
- [nextest runs each test in a process of its own, so a test that shared state through statics in one process may act differently.] → The suites already run each test file as a binary of its own. The change runs each suite under nextest and fixes what fails.

## Migration Plan

- The first command of this version has the older host process hand over. A host process that is no daemon ends, as it did, and the command starts its own. An older daemon starts afresh in place, as it did. Runs that an older host process started detached keep going until they end. The new host process reads their results as before, and owns each run that it starts.
- A host with the daemon's service takes the new module. Its first switch restarts the service once, since the program path changes to the system profile's. The path stays the same from then on, and later switches are hand-overs.
- The change replaces `cargo test` with `cargo nextest run` in the crates' tasks. A person who runs `cargo test` by hand still can. Only the tasks and `sluice exec` use nextest.
