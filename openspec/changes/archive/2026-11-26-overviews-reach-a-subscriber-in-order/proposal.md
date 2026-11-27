## Why

On 2026-11-27, sluice's own gate failed #171 on one test only:
`a_slow_subscriber_gets_the_newest_overview`. The test saw the third
overview first. Then it read the second overview. The test passes when
it runs alone.

`sluice host` reads a repository's overview after each reading that
ends while a client follows the repository. Each read is a task of its
own, and the tasks end in any order. Each task sends its overview when
it ends. On a busy host, a read that began first can end last. Then its
older overview replaces the newer one. herdr's sluice plugin follows
these overviews, so it can show an older state after a newer one.

## What Changes

- Each overview read carries the moment when its read began. The reads
  of one repository's overview do not overlap, so these moments put the
  overviews in the order of the state that each read.
- `sluice host` does not send an overview whose read began before the
  read of the overview that it last sent. A client gets the overviews
  of a folder in the order of their state, and a slow client gets the
  newest.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `resident-subscriptions`: a client gets a folder's overviews in the
  order of their state, also where the reads end in another order.

## Impact

- `crates/sluice/src/host_process/joined.rs`: an answer carries the
  moment when its call began, and `ask_dated` gives it.
- `crates/sluice/src/host_process/reads.rs`: `document` gives that
  moment with the overview.
- `crates/sluice/src/host_process/tasks.rs`: `send_document` does not
  send an overview whose read began before the read of the last one
  sent, and a test for it.
