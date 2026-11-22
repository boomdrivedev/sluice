# Proposal

## Why

A one-shot `sluice status`, `sluice overview`, the home view and a forecast ask `sluice host` for a current reading, and then read the records and the branches in their own process. A command that cannot ask `sluice host` reads the results itself. A watch and a wait read the document that `sluice host` writes to a file, and a wait reads its submission's record itself. So ten statuses at one time make ten reads of one thing, and another tool, such as `flume host`, cannot get a whole status or overview from the socket.

This change has `sluice host` answer each read from the state that it keeps. A client sends its folder and prints the answer. A subscription serves many folders on one connection.

Claims that this change makes hold:

- `reads-go-through-the-resident@69134e5825d7`
- `one-read-in-flight@e99acda774d0`
- `flume-has-a-resident@49876bc5c73c`, sluice's part: `flume host` gets each status and overview from the socket of `sluice host`.

## What Changes

- `sluice host` answers a status, an overview, the home view, a forecast, `sluice inbox` and a wait's record. The answer is the output that the command prints, and its exit code.
- A client sends its folder. `sluice host` finds the repository, the worktree and the head from it. The read commands run no git and read no record and no result.
- Where `sluice host` has no current reading within the read's bound, it answers from the state that it last kept and says when it read that state. Where no `sluice host` answers, the command exits 3. A command no longer reads the records itself.
- Many asks of one read that come while that read is in flight share it. The newest result replaces the older results.
- A new request, `subscribe`, serves many folders on one connection. The client adds and removes folders on the same connection. A slow client gets only the newest document of each folder.
- `sluice overview --json --watch --stdin` is the client of `subscribe`. Each line of its input adds or removes a folder. Each line of its output is one folder's newest overview, with the folder named.
- `sluice overview --json --watch` and `sluice wait` use `subscribe`. They read no file of `sluice host`.
- `sluice run` follows a subscription, and prints each pass's report that the subscription sends.
- `sluice host` no longer answers `read`, `follow` or `room`, since no command sends them after this change. It writes no file under `readers/`: no reader's document, no reader's version, lease or lock, and no driver's lock. No command reads such a file.
- `sluice host` no longer leaves a repository to an older `sluice run` that holds the repository's driver lock.
- `sluice host` answers a request that it does not know with an error. The error names the version of `sluice host` and says to run that sluice. A newer command still has an older `sluice host` hand over before it asks.

Out of scope:

- The changes: a submit, an amend, a withdrawal, a set-aside, `sluice confirm`, a release, the branch commands and `sluice protect`. The bolt's unit `sluice-host-does-every-change` moves them, and with them the git hook, `sluice previews`, `sluice exec` and its forecast.
- How often `sluice host` asks the forge. A status that follows the request that carries a submission asks the forge from `sluice host` as it asks it today. The bolt's unit `sluice-host-is-the-only-github-client` bounds it to one forge reading for each repository in each interval.
- The plugins. The herdr watcher and the board change to one subscription each in the bolt's unit `the-plugins-hold-one-subscription-each`.

## Capabilities

### New Capabilities

- `resident-reads`: `sluice host` answers each read from the state that it keeps, a client sends its folder and reads nothing, a read past its bound, and a request that `sluice host` does not know.
- `resident-subscriptions`: one subscription serves many folders as a watch, a wait or `sluice run`, and many triggers of one read make one read in flight.

### Modified Capabilities

- `overview-reader`: a one-shot read is answered by `sluice host`. A read past its bound is answered from the last state, not by the command. A wait takes its submission and its record from `sluice host`. The reader's document is kept in `sluice host` and sent on subscriptions, with no file. No older reader's lease, lock or version is read or taken.
- `forecast`: `sluice host` forms a forecast that has no current reading.
- `host-process`: a command that needs results gets them only from `sluice host`. An older command gets an error for a request that `sluice host` does not know. No older `sluice run` keeps a repository from `sluice host`.

## Impact

- The protocol of `sluice host`: the new requests `status`, `overview`, `home`, `forecast`, `inbox` and `subscribe`, and the new answers `output`, `document` and, on a subscription, `pass`. The requests `read`, `follow` and `room` and the answers `read` and `room` go. `serve`, `step`, `pass`, `hold`, `results` and `hand-over` stay.
- The `sluice` crate: the read commands are sent to `sluice host` before the command binds a repository. `sluice host` resolves a folder with a git worker, and answers each read on a thread that reads the repository as the host holds it, beside the repository's worker.
- `sluice overview`: a new flag, `--stdin`, which needs `--watch`.
- `sluice wait`, a waiting `sluice submit`, `wt submit` and `sluice run`: they follow a subscription.
- `reader.rs` and `host_process/tasks.rs`: the files under `readers/` go, with `reader::current`, `reader::Follow` and the host process's look for an older `sluice run`'s driver lock.
- The site's pages on the host's state and settings: the files under `readers/` that go.
- `profiles/records.rs:results_for`: no read of the results in the command's own process.
- The record of the host process: an amendment with this change's decisions, each with Before and Now.
