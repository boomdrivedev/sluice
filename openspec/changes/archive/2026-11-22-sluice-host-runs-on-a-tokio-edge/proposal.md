# Proposal

## Why

sluice's host process runs on threads and polls. Its accept loop sleeps 10 ms between polls. It looks at each idle repository every three seconds to find a change, and it reads a stop from a flag. Each later unit of the bolt needs a host process that waits on events. Those units add a subscription, one reading in flight, a forge reading on an interval, and workers that end with their owner.

Claims that this change makes hold:

- `tokio-streams-before-rxrust@0b9fc11cda8b`

## What Changes

- The host process runs a tokio runtime at its edge. The socket, the connections, the timers, the signals, the file events and each repository's task are tokio tasks.
- The core and its seams stay synchronous. The host process calls the core on tokio's pool for blocking work.
- The accept loop waits for a connection. The 10 ms poll ends.
- Each repository's reading has a `watch` channel. A trigger that comes while a reading is in flight makes one more reading after it.
- notify-debouncer-full watches each repository's refs and sluice's records in the host's files. A change there starts a reading.
- The look at each idle repository every three seconds ends. An idle repository is read on a file event, a command, a follower's opening, or the minute's backstop.
- `SIGTERM` and `SIGINT` stop each host process as a hand-over does, not only a daemon.
- Each wait of the host process is on tokio's clock, which a test can pause and move.

Out of scope:

- Workers in process groups that end with their owner. The bolt's unit `sluice-host-owns-its-workers` does that, on this edge.
- Reads that a client makes itself. The bolt's unit `sluice-host-answers-every-read` moves them into the host process.
- The forge reading's interval for each repository. The bolt's unit `sluice-host-is-the-only-github-client` adds it, on this edge.
- The protocol on the socket. It stays as it is, request for request.

## Capabilities

### New Capabilities

- `host-events`: the host process waits on events and timers, and a change to a repository's files starts a reading of it.

### Modified Capabilities

None.

## Impact

- The host process of the `sluice` crate: the accept loop, the connections, the looks, the repository tasks, the stop and the take-up of settings.
- The `sluice` crate's dependencies: tokio's runtime, net, signal, sync and time features, tokio-util, tokio-stream and notify-debouncer-full.
- The host process's tests: they can run on a paused clock.
