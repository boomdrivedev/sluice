# Design

## Context

The host process lives in the `host_process` module of the `sluice` crate. It runs on threads:

- `host_process/mod.rs:serve` accepts connections in a loop that sleeps 10 ms between polls. Every three seconds it checks the state folder, a daemon's stop flag, the settings file and the index.
- A connection gets a thread of its own.
- `host_process/tasks.rs:Tasks::look` starts a thread for each look at an idle repository.
- Each repository with work has a task, a thread that loops over its passes.
- Only `sluice daemon` catches `SIGTERM` and `SIGINT`. It reads them from a flag at the next three-second check.

sluice uses tokio today only to run the AWS SDK, on a runtime with one thread. See the proposal for why.

## Goals / Non-Goals

**Goals:**

- The host process waits on events and timers, on one clock that a test can pause.
- A change to a repository's files starts a reading of it. The three-second look ends.
- Later units can add a subscription, a forge interval and workers on this edge without a second rewrite.

**Non-Goals:**

- Async code in the core, the seams or the fakes.
- Async code in a client. A command stays synchronous: it connects, asks and prints.
- A change to the protocol on the socket.
- The weir crate that will hold the resident's skeleton for each tool. This change builds the skeleton in sluice first.

## Decisions

### D1. A tokio runtime in the host process alone

`sluice host` and `sluice daemon` build a tokio runtime with many threads and a bounded pool for blocking work. No other command builds it. The edge holds the socket, the connections, the signals, the timers, the file events and the repository tasks. The core keeps its seams.

The core runs in two places:

- Each look is a call of the synchronous core through `spawn_blocking`, on the pool for blocking work.
- Each repository with work in play has a worker: a `std::thread` of its own, which the repository's task starts (`tasks.rs:Scheduler::bind`). The thread enters the runtime (`Handle::enter`), so it reads tokio's clock. It holds the repository's binding while work is in play or a follower holds the repository, and it runs each pass and each step of the repository. The task lets the worker go when the repository has no work in play and no follower.

The worker is a thread of its own for two reasons. The binding holds values that stay on one thread, so one thread must keep it from one pass to the next. And tokio moves a paused clock only while no task of the pool for blocking work runs. A round on a pool thread would stop a paused clock for the whole round, and a test could not move the clock past a timer while a round waits. A thread of its own does not stop the clock, so the clock moves while a round waits. Its waits that sleep in a loop move to the edge, which decides when to call again. This follows `tokio-streams-before-rxrust@0b9fc11cda8b`: the residents use tokio only at their edge.

Not taken: async seams in the core. That changes each seam method, each fake and the conformance suite, and buys nothing for git and file work. Not taken: threads with less polling. The polls, the stop flags and the threads for each connection stay.

### D2. Tasks and stops by TaskTracker and CancellationToken

tokio-util's `TaskTracker` holds every task. A `CancellationToken` tree holds the stops. It has one root for the process, a child for each repository, and a child for each connection. A stop, a hand-over and a take-up that starts afresh cancel the root. Each repository's task finishes the round that it is in, and the process waits on the tracker. `tokio::signal::unix` gives `SIGTERM` and `SIGINT` to every host process, not only to a daemon. The flag `stopping` and the function `crate::stopped` leave the host process.

Not taken: an actor crate such as ractor or kameo. The host process keeps nothing that the records do not hold, so a restart inside the process gives nothing. Not taken: `tokio-graceful-shutdown`. Its subsystems name what a token tree already holds.

### D3. One `watch` channel for each repository's reading

Each repository has a task with an inbox and a `watch` channel of its readings. A trigger marks a reading due. The task starts a reading only where none is in flight. When it ends, the task sends the reading on the channel and starts one more where a trigger came during it. A reader of the channel gets the newest reading only. The reading marks of today, `Tasks::begin_reading` and `Tasks::end_reading`, become this task's state.

### D4. File events by notify-debouncer-full

One debouncer watches, for each repository that the host process holds:

- the git common folder's `refs` and `packed-refs`
- each worktree's `HEAD` under the git common folder's `worktrees`
- sluice's records folder in the host's files

The debounce time is 200 ms. An event marks the repository's reading due. A repository that the index adds is watched at once, and one that the index drops is no longer watched.

The minute's backstop stays: a reading of each idle repository at least once a minute, for a change that no event reported. This keeps the rule that a remote's records are asked for once a minute where nothing is in play.

Not taken: watchman. It is a second resident on the host. Not taken: futures-time's debounce. Its timers do not run on tokio's paused clock.

### D5. Timers on tokio's clock

Each timer of the host process is tokio's. That includes the backstop, the round's pause, the room's waits and the time limits of a hand-over. It also includes the tick that a one-shot read takes as current. The core's `Clock` seam in the host process reads tokio's clock. A test builds the runtime with one thread and `start_paused`, and moves the clock with `tokio::time::advance`.

Not taken: a clock of sluice's own for the edge. tokio's paused clock already drives every tokio timer.

### D6. The socket on `tokio::net::UnixListener`

The listener is tokio's, bound at the path that `Paths` gives now. Each connection is a task that reads lines with tokio-util's `LinesCodec`, at the same JSON lines and the same requests. The start lock, `host/version` and the hand-over's carried descriptor stay as they are. A daemon that starts afresh in place builds the listener from the socket that it carried.

## Risks / Trade-offs

- [FSEvents on macOS joins events and can report them late.] → The debounce takes the join. The minute's backstop catches a change that no event reports.
- [inotify on Linux has a limit on watches for each user.] → The debouncer watches only refs, `packed-refs`, the worktrees' `HEAD` and the records folder, not the working copies. Where the limit refuses a watch, the host process says so once in its log and reads that repository at the minute's backstop.
- [A long round holds its repository's worker thread.] → Each repository with work in play has its own thread, so a round on one repository never holds another repository's round, its task or a thread of the pool for blocking work. The pool, bounded at 256 threads, runs only the looks and the other short calls of the core. A repository with no work in play has no worker thread.
- [`fork` and `execve` from a process with many threads.] → A hand-over in place cancels every task and waits for the tracker before `execve`, as the daemon does today.

## Migration Plan

Nothing on a host moves. The protocol and the files stay as they are. The first command of the new version has the older host process hand over, as for any upgrade.
