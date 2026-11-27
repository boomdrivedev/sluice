## Why

On 2026-11-27, submission 172 failed in sluice's own gate only on
`an_upgrade_keeps_its_runs_as_children`. The test found that the
parent of the check's supervisor was not `sluice host`. The test passed
when it ran alone and in a full run of `cli:test`.

The fault is in the test, not in the hand-over. A hand-over in place
replaces the image of `sluice host` by `execve`. The pid stays the
same, and each child stays a child. The run's supervisor is a direct
child of `sluice host`. It starts the check's shell in a process group
of its own.

The test found the check by its command line. The shell of the check
forks a copy of itself to run `sleep` ten times each second. Until the
copy replaces its image, it has the command line of the shell. Under
load, the copy lives longer, and the test found it. The parent of the
copy is the shell, so the test read the supervisor as the parent of the
supervisor. Pid 48334 was the supervisor, and 45669 was `sluice host`.

## What Changes

- A test of sluice finds a run's check by its command line and by its
  process group. The check is the process that leads its group. A copy
  that the shell forked is in the group of the shell and does not lead
  it.
- A test holds a forked copy of a check's shell, so that each read
  finds two processes with the command line. Before this change, the
  test named the copy.

## Capabilities

### New Capabilities

### Modified Capabilities

- `test-children`: a test names a run's check by the leader of its
  process group.

## Impact

- `crates/sluice/tests/host_process.rs`: `matching` finds each process
  with the command line of the held check. `check_of` names the one
  that leads its process group.
- Test: `a_checks_forked_copy_is_not_the_check`.
- No change to `sluice host`, the hand-over or the supervisor.
