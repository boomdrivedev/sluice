## Why

On GitHub's Linux job,
`pull_requests::a_wait_reads_its_pull_request_once_in_a_quarter_of_the_interval`
(`cli:tools`) fails each time with `0 reads`. The gate runs it on a
macOS host, and it passes there.

The test lets a wait follow its pull request for one minute. The wait
skips its pauses, and its clock moves three seconds at each pause. The
test expects one to four reads of the request from GitHub, one in each
quarter of the host's interval (136).

`sluice host` runs each step of the wait. Where `sluice host` passes only
when asked, a pass is at the time of the command that asks it. This is
design D14 of the change that made the host process. A wait's step is not: it
reads the real clock. The wait's minute goes by in about three seconds
of the real clock. So `sluice host` reads the request again only where
it kept no read of it. On Linux, the `sluice host` of the test's earlier
commands still runs, and it read the request before the wait began. So
the wait reads it zero times. On macOS, that `sluice host` ended before
the wait, and a new one read the request one time.

## What Changes

- Each step that a wait asks of `sluice host` carries the wait's time, in
  the field `at` of its ask. The field is required. Every client is built
  from this tree, so no step comes without it.
- Where `sluice host` passes only when asked, it runs the step at that
  time. Elsewhere it runs the step at its own time.

## Capabilities

### Modified Capabilities

- `host-process`: a wait's step is at the wait's time where `sluice host`
  passes only when asked.

## Impact

- `crates/sluice/src/host_process/protocol.rs`: `WaitAsk::at`.
- `crates/sluice/src/lib.rs`: `wait_ask` sets `at` from the wait's clock
  for each step.
- `crates/sluice/src/host_process/reads.rs`: the step's clock.
- `crates/sluice/tests/cli.rs`: `status_and_overview_show_a_hold_while_it_lasts`.
  Its clock moves at each read, and each step reads it one more time.
  So the times that it expects are later.
