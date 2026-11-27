## Why

Once the tests of #186 passed on GitHub's Linux job, two more tests
failed there. The gate runs both on a macOS host, and they pass there.

1. `peers::a_host_whose_own_main_is_refused_counts_the_remotes_signers`
   (`cli:tools`). The second host lands the first host's submission.
   Then the test reads the submission's state on the first host one
   time, and it expects `landed`. The first host's `sluice host` answers
   from its last read of the remote. That read can come before the
   landing (129, rule core.31). On GitHub it said `queued`.
   `a_submission_lands_by_another_host` reads the first host the
   same way.
2. `watch: source exits` (`plugin-herdr:test-dash`). The test kills the
   watcher's source and waits for a new one. Its condition reads the
   running sources two times. The killed source runs as a zombie until
   the watcher reaps it, and the next source starts later. So the first
   read can find the zombie, and the second read can find nothing. The
   condition then holds, and the test takes no source as the new one.

## What Changes

- A test host of `crates/sluice/tests/peers.rs` gets `reads`. It reads
  a submission's state until it is the one expected, for one minute at
  most. The two tests read the first host with it.
- `watch: source exits` reads the running sources one time for each
  look, and keeps that read as the new source.

## Capabilities

### Modified Capabilities

- `test-children`: a test waits on a state that another process
  changes, and reads it one time for each look.

## Impact

- `crates/sluice/tests/peers.rs`: `Host::reads`, and two tests.
- `plugins/herdr/tests/run.sh`: `watch: source exits`.
