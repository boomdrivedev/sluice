## Why

On Linux, `host_process::the_host_process_keeps_its_repositories_across_a_new_binding`
(`cli:test`) fails in four runs of five. The host's settings change, and
`sluice host` does not take the change up within 30 seconds. The gate
runs the test on a macOS host, and it passes there.

The cause is a loop. `sluice host` watches the refs and the records of
each repository that it holds. On Linux, notify asks inotify for each
open of a file, and `sluice host`'s own read of a repository opens its
refs and records. Each open is an event. `sluice host` read the event as
a change, and read the repository again, which opened the files again.
In the test, `sluice host` opened the files of one repository about
1,200 times each second. notify's inotify loop then read events with no end, and the
event of the settings file did not get through. On macOS, FSEvents
reports no read, so no loop starts.

A Linux host that holds a repository has this loop too. It reads that
repository with no end, and it can miss a change to its settings.

## What Changes

- `sluice host`'s watch drops each event that only says a file was
  opened, read, or closed after a read. A write, a close after a write,
  a create, a removal and a rename are changes as before.

## Capabilities

### Modified Capabilities

- `host-events`: a read of a repository's files starts no reading.

## Impact

- `crates/sluice/src/host_process/watch.rs`: `reads_only`, and the
  handler of `Stream::new`.
