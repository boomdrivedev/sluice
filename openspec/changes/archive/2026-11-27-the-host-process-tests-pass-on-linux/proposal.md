## Why

On GitHub's Linux job, two tests of `cli:test` fail. The gate runs them
on a macOS host, and they pass there.

1. `host_process::a_daemon_starting_afresh_in_place_keeps_the_hosts_lock`
   fails each time. The client asks the daemon to start afresh in place.
   The new image starts, but it does not answer within 70 seconds. The
   new image waits in `Watch::settings`, on notify's inotify loop, for a
   watch to be added. That loop does not add it. While the daemon starts
   afresh, the test opens the host's `lock` file again and again, to ask
   for the lock. On Linux, each open of a file in the host's folder is an
   inotify event. notify reads events until none is left, and only then
   adds a watch. The opens do not stop, so the watch is never added. On
   macOS, FSEvents joins the events, and the watch is added.
2. `host_process::a_repository_gone_as_a_round_writes_in_it_is_gone`
   fails in two runs of three. Its subscription gets the error of the
   repository's last read, and no document that says the repository is
   gone. On Linux, the event of the git folder's move comes at once. The
   scan of the index then finds the repository gone, and takes it out of
   the index. That lets go of the repository's channel of documents, and
   no document says that the repository is gone. A later look starts a
   new scheduler on a new channel. That scheduler says that the
   repository is gone, but no subscriber follows the new channel. On
   macOS, the scheduler's own read finds the repository gone first, and
   says so on the channel that the subscriber follows.

## What Changes

- The scan of the index can find a repository gone. Then it sends the
  document that says so on the repository's channel. Only after that does
  it let the channel go.
- The test of a daemon that starts afresh asks for the lock on one
  descriptor that it opens one time. A lock that a process takes makes
  no file event.

## Capabilities

### Modified Capabilities

- `resident-subscriptions`: a repository that the scan of the index
  finds gone is said gone to its subscribers.
- `test-children`: a test does not open files again and again in a
  folder that `sluice host` watches.

## Out of scope

- **notify's inotify loop.** It adds no watch while events keep coming.
  So on Linux, a folder with file events that do not stop holds back each
  new watch of `sluice host`. That is a change for notify's own
  project, `notify-rs/notify`.

## Impact

- `crates/sluice/src/host_process/tasks.rs`: `Tasks::forget` and
  `send_gone`.
- `crates/sluice/tests/host_process.rs`: `keeps_the_hosts_lock`.
