## Why

On 2026-11-26, sluice's own gate failed three times when `cli:test` and
`cli:tools` ran at one time. Each test passes when it runs alone. In
each case a read of sluice's state did not get what the test expected:

- `a_daemon_takes_up_a_changed_telemetry_table_in_place` (#164): a
  `sluice status --json --all` had no submissions list. The status
  failed while a round landed a submission.
- `a_status_says_a_merged_request_landed_once` (#163): a second status
  showed a merged pull request again. The first status said that it
  landed and let it go.

The runner and Flume read `sluice status` too. On a busy host, a status
that fails or that shows a request again misleads them.

Two reads are the cause:

1. sluice reads a branch's records in the host's own files with no
   lock. It reads the version, then it lists the files and reads each
   one. A write can end while the read lists and reads. Then a file
   that the read listed is gone, and the read fails. Or the read gets
   some files of one version and some of the next.
2. A process reads a followed pull request from GitHub in three steps.
   It claims the request under the lock of the follows. It reads GitHub
   with no lock. Then it puts the request back. A status can let the
   request go while the process reads GitHub. Then the process puts it
   back, and the next status shows it again.

## What Changes

- A read of a branch's records gives one version of the records whole,
  also while a round writes them. Where a write ends while the read
  lists and reads the files, the read lists and reads them again (131).
- What a read of a followed pull request gets from GitHub goes only
  into the follow that the read claimed. A request that a status or a
  wait let go during the read stays let go (137, design D3).

## Capabilities

### Modified Capabilities

- `resident-reads`: a read gives one version of a branch's records,
  whole.
- `request-intake-host`: a request that a status let go stays let go
  while another read of it ends.

## Impact

- `crates/sluice/src/profiles/local_files.rs`: `read` and
  `read_all_finished` read one version whole.
- `crates/sluice/src/profiles/github/pulls.rs`: `read_followed` keeps
  its read only into the follow that it claimed.
- `crates/sluice/tests/local_files.rs` and the tests of
  `crates/sluice/src/profiles/github/pulls.rs`: a test for each read.

Out of scope: a waiting submit that does not show the failed release of
its checkout (`landed.rs`). That is the output of a wait, not of a
status.
